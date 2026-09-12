#!/usr/bin/env python3
"""Resolve Warcraft III base object data beneath the original map overrides.

The map object files are deltas over Blizzard SLK/profile data. This script
combines the locally extracted Warcraft III 2.0.x data set selected by W3I with
those deltas and emits searchable plaintext tables.

W3P-protected maps can contain repeated modifications for the same field. We
never discard that evidence: the full output records every candidate, the
normal last-write interpretation, and a conservative recovery heuristic used
only by the convenience catalogs.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import math
import re
import struct
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


CATEGORY_FILES = {
    "units": "obj-units.json",
    "items": "obj-items.json",
    "abilities": "obj-abilities.json",
    "buffs": "obj-buffs.json",
    "destructables": "obj-destructables.json",
    "doodads": "obj-doodads.json",
}

METADATA_FILES = {
    "units": "unitmetadata.slk",
    "items": "unitmetadata.slk",
    "abilities": "abilitymetadata.slk",
    "buffs": "abilitybuffmetadata.slk",
    "destructables": "destructablemetadata.slk",
    "doodads": "doodadmetadata.slk",
}

SLK_ALIASES = {
    "doodaddata": "doodads",
}

PATH_CELL_WORLD_UNITS = 32


def split_semicolon_record(line: str) -> list[str]:
    fields: list[str] = []
    current: list[str] = []
    quoted = False
    i = 0
    while i < len(line):
        ch = line[i]
        if ch == '"':
            current.append(ch)
            if quoted and i + 1 < len(line) and line[i + 1] == '"':
                current.append('"')
                i += 2
                continue
            quoted = not quoted
        elif ch == ";" and not quoted:
            fields.append("".join(current))
            current = []
        else:
            current.append(ch)
        i += 1
    fields.append("".join(current))
    return fields


def parse_scalar(token: str) -> Any:
    if not token.startswith("K"):
        return None
    value = token[1:]
    if value.startswith('"') and value.endswith('"') and len(value) >= 2:
        return value[1:-1].replace('""', '"')
    upper = value.upper()
    if upper == "TRUE":
        return True
    if upper == "FALSE":
        return False
    if re.fullmatch(r"[-+]?\d+", value):
        try:
            return int(value)
        except ValueError:
            pass
    if re.fullmatch(r"[-+]?(?:\d+\.\d*|\d*\.\d+)(?:[Ee][-+]?\d+)?", value) or re.fullmatch(
        r"[-+]?\d+[Ee][-+]?\d+", value
    ):
        try:
            return float(value)
        except ValueError:
            pass
    return value


def parse_slk(path: Path) -> dict[str, dict[str, Any]]:
    """Parse the subset of SYLK used by Warcraft object tables."""
    cells: dict[tuple[int, int], Any] = {}
    x = 0
    y = 0
    text = path.read_text(encoding="utf-8-sig", errors="replace")
    for raw_line in text.splitlines():
        if not raw_line.startswith("C;"):
            continue
        value_marker: str | None = None
        for token in split_semicolon_record(raw_line)[1:]:
            if token.startswith("X") and token[1:].isdigit():
                x = int(token[1:])
            elif token.startswith("Y") and token[1:].isdigit():
                y = int(token[1:])
            elif token.startswith("K"):
                value_marker = token
        if value_marker is not None and x > 0 and y > 0:
            cells[(y, x)] = parse_scalar(value_marker)

    headers: dict[int, str] = {}
    max_x = max((cx for (cy, cx) in cells if cy == 1), default=0)
    for cx in range(1, max_x + 1):
        value = cells.get((1, cx))
        if value is not None:
            headers[cx] = str(value)
    if 1 not in headers:
        raise ValueError(f"{path}: missing first-column header")

    rows: dict[str, dict[str, Any]] = {}
    max_y = max((cy for cy, _ in cells), default=1)
    for cy in range(2, max_y + 1):
        key = cells.get((cy, 1))
        if key is None:
            continue
        row: dict[str, Any] = {}
        for cx, name in headers.items():
            if (cy, cx) in cells:
                row[name] = cells[(cy, cx)]
        rows[str(key)] = row
    return rows


def parse_profile(path: Path) -> dict[str, dict[str, str]]:
    sections: dict[str, dict[str, str]] = {}
    section: str | None = None
    text = path.read_text(encoding="utf-8-sig", errors="replace")
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if not line or line.startswith("//") or line.startswith(";"):
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip()
            sections.setdefault(section, {})
            continue
        if section is None or "=" not in raw_line:
            continue
        key, value = raw_line.split("=", 1)
        sections[section][key.strip()] = value.strip()
    return sections


def merge_profiles(destination: dict[str, dict[str, str]], source: dict[str, dict[str, str]]) -> None:
    for section, fields in source.items():
        destination.setdefault(section, {}).update(fields)


def ci_get(mapping: dict[str, Any], key: str) -> Any:
    wanted = key.casefold()
    for current, value in mapping.items():
        if current.casefold() == wanted:
            return value
    return None


def profile_value(profile: dict[str, dict[str, str]], section: str, field: str) -> Any:
    fields = profile.get(section)
    if fields is None:
        return None
    # W3I game_data_set_version=0 corresponds to the custom V0 data set.
    variant = ci_get(fields, f"{field}:custom,V0")
    if variant is not None:
        return variant
    return ci_get(fields, field)


def parse_csv_list(value: Any) -> list[str]:
    if not isinstance(value, str):
        return [str(value)]
    try:
        return next(csv.reader([value], skipinitialspace=False))
    except (csv.Error, StopIteration):
        return [value]


def selected_index(value: Any, index: int) -> Any:
    if index < 0 or not isinstance(value, str):
        return value
    values = parse_csv_list(value)
    return values[index] if index < len(values) else None


def split_object_key(key: str, table: str) -> tuple[str, str | None]:
    if table == "custom" and ":" in key:
        new_id, base_id = key.split(":", 1)
        return new_id, base_id
    return key, None


def stable_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def same_value(a: Any, b: Any) -> bool:
    if isinstance(a, float) and isinstance(b, float):
        return math.isclose(a, b, rel_tol=0.0, abs_tol=1e-6)
    return a == b


def all_same(values: list[Any]) -> bool:
    return all(same_value(values[0], value) for value in values[1:]) if values else True


@dataclass(frozen=True)
class MapCandidate:
    index: int
    value: Any
    value_type: str


def choose_recovered(candidates: list[MapCandidate]) -> tuple[Any, str]:
    """Choose a convenience value without hiding W3P ambiguity.

    Repeated identical values are harmless. The protection observed in this
    map appends numeric 0/1 sentinels after plausible authored values for HP,
    armor, damage and range. Recover those narrowly. Conflicting strings are
    not safely inferable, so use loader-style last write and flag ambiguity.
    """
    if not candidates:
        return None, "base"
    values = [candidate.value for candidate in candidates]
    if all_same(values):
        return values[0], "map-identical-duplicates" if len(values) > 1 else "map"
    first = values[0]
    later = values[1:]
    if isinstance(first, (int, float)) and not isinstance(first, bool):
        if abs(float(first)) > 1.0 and later and all(
            isinstance(value, (int, float)) and not isinstance(value, bool) and float(value) in (0.0, 1.0)
            for value in later
        ):
            return first, "w3p-recovered-first-numeric-sentinel"
    if isinstance(first, str):
        return values[-1], "ambiguous-string-last-write"
    return values[-1], "ambiguous-last-write"


def resolve_string_key(value: Any, world_strings: dict[str, str]) -> Any:
    if not isinstance(value, str):
        return value
    seen: set[str] = set()
    current = value
    while current in world_strings and current not in seen:
        seen.add(current)
        current = world_strings[current]
    return current


def world_edit_strings(path: Path) -> dict[str, str]:
    sections = parse_profile(path)
    values: dict[str, str] = {}
    for fields in sections.values():
        values.update(fields)
    return values


def load_slks(source_root: Path) -> tuple[dict[str, dict[str, dict[str, Any]]], dict[str, str]]:
    base_units = source_root / "base" / "units"
    custom_units = source_root / "custom_v0" / "units"
    base_doodads = source_root / "base" / "doodads"

    tables: dict[str, dict[str, dict[str, Any]]] = {}
    table_sources: dict[str, str] = {}

    # Prefer custom_v0 rows for tables provided by that data set, then fill
    # missing rawcodes from the base table. Metadata is always from base.
    all_paths = list(base_units.glob("*.slk")) + list(base_doodads.glob("*.slk"))
    for path in all_paths:
        stem = path.stem.casefold()
        tables[stem] = parse_slk(path)
        table_sources[stem] = f"base/{path.parent.name}/{path.name}"

    for path in custom_units.glob("*.slk"):
        stem = path.stem.casefold()
        custom_rows = parse_slk(path)
        if stem in tables:
            merged = dict(tables[stem])
            merged.update(custom_rows)
            tables[stem] = merged
        else:
            tables[stem] = custom_rows
        table_sources[stem] = f"custom_v0/units/{path.name} (+ base fallback)"

    return tables, table_sources


def load_profiles(source_root: Path) -> dict[str, dict[str, str]]:
    profile: dict[str, dict[str, str]] = {}

    # Base non-localized object profiles and skins.
    for directory in (source_root / "base" / "units", source_root / "base" / "doodads"):
        for path in sorted(directory.glob("*.txt")):
            merge_profiles(profile, parse_profile(path))

    # The map explicitly selects custom V0. Its function/profile data takes
    # precedence over generic base profile data.
    for path in sorted((source_root / "custom_v0" / "units").glob("*.txt")):
        merge_profiles(profile, parse_profile(path))

    # Localized labels/tooltips take precedence last. profile_value() itself
    # prefers :custom,V0 variants where the same field provides variants.
    for directory in (source_root / "enus" / "units", source_root / "enus" / "doodads"):
        for path in sorted(directory.glob("*.txt")):
            merge_profiles(profile, parse_profile(path))
    return profile


def table_row(tables: dict[str, dict[str, dict[str, Any]]], table_name: str, rawcode: str) -> dict[str, Any] | None:
    stem = SLK_ALIASES.get(table_name.casefold(), table_name.casefold())
    table = tables.get(stem)
    if table is None:
        return None
    return table.get(rawcode)


def row_ci_get(row: dict[str, Any] | None, field: str) -> Any:
    if row is None:
        return None
    return ci_get(row, field)


def csv_rawcodes(value: Any) -> set[str]:
    if value in (None, "", "_"):
        return set()
    return {part.strip() for part in str(value).split(",") if part.strip()}


def metadata_applies(meta: dict[str, Any], base_rawcode: str, source_row: dict[str, Any] | None) -> bool:
    specific = csv_rawcodes(ci_get(meta, "useSpecific"))
    excluded = csv_rawcodes(ci_get(meta, "notSpecific"))
    identities = {base_rawcode}
    code = row_ci_get(source_row, "code")
    if isinstance(code, str) and code:
        identities.add(code)
    if specific and not (specific & identities):
        return False
    if excluded and (excluded & identities):
        return False
    return True


def source_column_for_metadata(meta: dict[str, Any], level: int, column: int) -> str:
    field = str(ci_get(meta, "field") or "")
    repeat = int(ci_get(meta, "repeat") or 0)
    data_pointer = column
    if data_pointer == 0:
        data_pointer = int(ci_get(meta, "data") or 0)
    result = field
    if data_pointer:
        result += chr(ord("a") + data_pointer - 1)
    if repeat and level:
        result += str(level)
    return result


def metadata_levels(meta: dict[str, Any], base_levels: int) -> list[tuple[int, int]]:
    repeat = int(ci_get(meta, "repeat") or 0)
    data = int(ci_get(meta, "data") or 0)
    if not repeat:
        return [(0, data)]
    levels = max(base_levels, 1)
    return [(level, data) for level in range(1, levels + 1)]


def base_value_for_metadata(
    meta: dict[str, Any],
    base_rawcode: str,
    level: int,
    column: int,
    tables: dict[str, dict[str, dict[str, Any]]],
    profile: dict[str, dict[str, str]],
) -> tuple[Any, str, str]:
    slk = str(ci_get(meta, "slk") or "")
    field = str(ci_get(meta, "field") or "")
    raw_index = ci_get(meta, "index")
    index = int(raw_index) if raw_index is not None else -1
    if slk.casefold() == "profile":
        raw = profile_value(profile, base_rawcode, field)
        repeat = int(ci_get(meta, "repeat") or 0)
        if repeat and level > 0 and isinstance(raw, str):
            values = parse_csv_list(raw)
            value = values[level - 1] if level - 1 < len(values) else (values[-1] if len(values) == 1 else None)
        else:
            value = selected_index(raw, index)
        return value, "Profile", field

    source_column = source_column_for_metadata(meta, level, column)
    row = table_row(tables, slk, base_rawcode)
    value = row_ci_get(row, source_column)
    if value is None and level > 0:
        # Some old tables store a scalar despite metadata being repeat-capable.
        value = row_ci_get(row, field)
    if index >= 0:
        value = selected_index(value, index)
    return value, slk, source_column


def effective_base_levels(category: str, base_rawcode: str, tables: dict[str, dict[str, dict[str, Any]]]) -> int:
    if category != "abilities":
        return 1
    row = table_row(tables, "AbilityData", base_rawcode)
    levels = row_ci_get(row, "levels")
    try:
        return max(1, int(levels))
    except (TypeError, ValueError):
        return 1


def parse_tga_pathing(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    if len(data) < 18:
        raise ValueError(f"{path}: truncated TGA")
    id_length, color_map_type, image_type = struct.unpack_from("<BBB", data, 0)
    color_map_length = struct.unpack_from("<H", data, 5)[0]
    color_map_depth = data[7]
    width, height = struct.unpack_from("<HH", data, 12)
    pixel_depth = data[16]
    descriptor = data[17]
    if color_map_type != 0 or image_type != 2 or pixel_depth not in (24, 32):
        raise ValueError(f"{path}: unsupported pathing TGA layout")
    color_map_bytes = ((color_map_depth + 7) // 8) * color_map_length
    offset = 18 + id_length + color_map_bytes
    bytes_per_pixel = pixel_depth // 8
    expected = width * height * bytes_per_pixel
    pixels = data[offset : offset + expected]
    if len(pixels) != expected:
        raise ValueError(f"{path}: truncated TGA pixel data")

    top_origin = bool(descriptor & 0x20)
    right_origin = bool(descriptor & 0x10)
    rows: list[list[int]] = []
    for output_y in range(height):
        source_y = output_y if top_origin else height - 1 - output_y
        row: list[int] = []
        for output_x in range(width):
            source_x = width - 1 - output_x if right_origin else output_x
            pixel_offset = (source_y * width + source_x) * bytes_per_pixel
            blue = pixels[pixel_offset]
            green = pixels[pixel_offset + 1]
            red = pixels[pixel_offset + 2]
            flags = (1 if red else 0) | (2 if green else 0) | (4 if blue else 0)
            row.append(flags)
        rows.append(row)

    counts = {
        "unwalkable": sum(1 for row in rows for value in row if value & 1),
        "unflyable": sum(1 for row in rows for value in row if value & 2),
        "unbuildable": sum(1 for row in rows for value in row if value & 4),
    }
    return {
        "name": path.name,
        "width_cells": width,
        "height_cells": height,
        "width_world_units": width * PATH_CELL_WORLD_UNITS,
        "height_world_units": height * PATH_CELL_WORLD_UNITS,
        "cell_world_units": PATH_CELL_WORLD_UNITS,
        "flag_bits": {"1": "unwalkable", "2": "unflyable", "4": "unbuildable"},
        "counts": counts,
        "hex_rows": ["".join(format(value, "x") for value in row) for row in rows],
        "sha256": hashlib.sha256(data).hexdigest(),
    }


def normalize_pathing_name(value: Any) -> str | None:
    if not isinstance(value, str):
        return None
    cleaned = value.replace("/", "\\").strip()
    if not cleaned or cleaned.casefold() in {"_", "none"}:
        return None
    return cleaned.split("\\")[-1].casefold()


def parse_build_info(path: Path) -> dict[str, Any]:
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    if len(lines) < 2:
        return {}
    headers = [part.split("!", 1)[0] for part in lines[0].split("|")]
    values = lines[1].split("|")
    return {header: values[index] if index < len(values) else "" for index, header in enumerate(headers)}


def repaired_object_data(path: Path) -> dict[str, dict[str, list[dict[str, Any]]]]:
    return json.loads(path.read_text(encoding="utf-8"))


def canonical_map_candidates(modifications: list[dict[str, Any]]) -> dict[tuple[str, int, int], list[MapCandidate]]:
    result: dict[tuple[str, int, int], list[MapCandidate]] = defaultdict(list)
    for index, mod in enumerate(modifications):
        key = (str(mod["id"]).rstrip("\0"), int(mod.get("level", 0)), int(mod.get("column", 0)))
        result[key].append(MapCandidate(index=index, value=mod.get("value"), value_type=str(mod.get("type", ""))))
    return dict(result)


def source_table_key(meta: dict[str, Any]) -> str:
    return str(ci_get(meta, "slk") or "")


def metadata_display(meta: dict[str, Any], strings: dict[str, str]) -> str:
    value = ci_get(meta, "displayName")
    if value is None:
        return ""
    return str(resolve_string_key(value, strings))


def load_metadata(
    category: str,
    source_root: Path,
    tables: dict[str, dict[str, dict[str, Any]]],
) -> dict[str, dict[str, Any]]:
    filename = METADATA_FILES[category]
    if category == "doodads":
        path = source_root / "base" / "doodads" / filename
    else:
        path = source_root / "base" / "units" / filename
    stem = path.stem.casefold()
    if stem in tables:
        return tables[stem]
    return parse_slk(path)


def canonical_key_order(key: tuple[str, int, int]) -> tuple[str, int, int]:
    return key


def value_as_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bool):
        return "1" if value else "0"
    if isinstance(value, float):
        return f"{value:.9g}"
    return str(value)


def numeric(value: Any) -> float | None:
    if isinstance(value, bool):
        return float(int(value))
    if isinstance(value, (int, float)):
        return float(value)
    if isinstance(value, str):
        try:
            return float(value)
        except ValueError:
            return None
    return None


def rawcode_list(value: Any) -> list[str]:
    if not isinstance(value, str) or not value.strip():
        return []
    return [part.strip() for part in value.split(",") if part.strip()]


def build_field_resolver(rows: list[dict[str, Any]]) -> dict[tuple[str, str, int, int], dict[str, Any]]:
    return {
        (row["category"], row["rawcode"], int(row["level"]), int(row["column"])): row
        for row in rows
    }


def field_lookup(rows_by_object: dict[tuple[str, str], dict[tuple[str, int, int], dict[str, Any]]], category: str, rawcode: str, field_id: str, level: int = 0, column: int = 0) -> Any:
    row = rows_by_object.get((category, rawcode), {}).get((field_id, level, column))
    return row.get("recovered_value") if row else None


def raw_source_value(tables: dict[str, dict[str, dict[str, Any]]], table: str, rawcode: str, field: str) -> Any:
    return row_ci_get(table_row(tables, table, rawcode), field)


def write_tsv(path: Path, header: list[str], rows: Iterable[Iterable[Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as handle:
        writer = csv.writer(handle, delimiter="\t", lineterminator="\n")
        writer.writerow(header)
        writer.writerows(rows)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", type=Path, default=Path(".wc3-base/source"))
    parser.add_argument("--map-extracted", type=Path, default=Path("docs/original_map/extracted"))
    parser.add_argument("--output", type=Path, default=Path("docs/original_map/extracted/resolved"))
    args = parser.parse_args()

    source_root = args.base.resolve()
    map_root = args.map_extracted.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)

    tables, table_sources = load_slks(source_root)
    profile = load_profiles(source_root)
    editor_strings = world_edit_strings(source_root / "enus" / "ui" / "worldeditstrings.txt")
    editor_strings.update(world_edit_strings(source_root / "enus" / "ui" / "worldeditgamestrings.txt"))
    build_info = parse_build_info(source_root / "install" / ".build.info")
    map_info = json.loads((map_root / "map-info.json").read_text(encoding="utf-8"))
    selected_data_set = int(map_info.get("game_data_set_version", 0))
    if selected_data_set != 0:
        raise ValueError(f"this extraction cache contains custom_v0, but W3I selects data set {selected_data_set}")

    pathing_textures: dict[str, dict[str, Any]] = {}
    for path in sorted((source_root / "base" / "pathtextures").glob("*.tga")):
        decoded = parse_tga_pathing(path)
        pathing_textures[path.name.casefold()] = decoded
    (output / "pathing-textures.json").write_text(
        json.dumps({name: pathing_textures[name] for name in sorted(pathing_textures)}, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    full_rows: list[dict[str, Any]] = []
    rows_by_object: dict[tuple[str, str], dict[tuple[str, int, int], dict[str, Any]]] = defaultdict(dict)
    object_records: list[dict[str, Any]] = []
    conflict_rows: list[list[Any]] = []
    unresolved_base_objects: list[dict[str, str]] = []
    unknown_map_fields: Counter[str] = Counter()

    for category, filename in CATEGORY_FILES.items():
        object_path = map_root / filename
        if not object_path.exists():
            continue
        objects_data = repaired_object_data(object_path)
        metadata = load_metadata(category, source_root, tables)

        for table, objects in objects_data.items():
            for object_key, modifications in objects.items():
                rawcode, parent_rawcode = split_object_key(object_key, table)
                base_rawcode = parent_rawcode or rawcode
                candidates = canonical_map_candidates(modifications)
                base_levels = effective_base_levels(category, base_rawcode, tables)

                primary_source_table = {
                    "units": "UnitData",
                    "items": "ItemData",
                    "abilities": "AbilityData",
                    "buffs": "AbilityBuffData",
                    "destructables": "DestructableData",
                    "doodads": "DoodadData",
                }[category]
                source_row = table_row(tables, primary_source_table, base_rawcode)
                profile_row = profile.get(base_rawcode)
                if source_row is None and profile_row is None:
                    unresolved_base_objects.append({"category": category, "rawcode": rawcode, "base_rawcode": base_rawcode})

                canonical_keys: set[tuple[str, int, int]] = set(candidates)
                base_fields: dict[tuple[str, int, int], tuple[Any, dict[str, Any], str, str]] = {}
                for field_id, meta in metadata.items():
                    if not metadata_applies(meta, base_rawcode, source_row):
                        continue
                    for level, column in metadata_levels(meta, base_levels):
                        value, source_table, source_field = base_value_for_metadata(
                            meta, base_rawcode, level, column, tables, profile
                        )
                        value = resolve_string_key(value, editor_strings)
                        if value is None:
                            continue
                        key = (field_id, level, column)
                        base_fields[key] = (value, meta, source_table, source_field)
                        canonical_keys.add(key)

                for key in candidates:
                    if key[0] not in metadata:
                        unknown_map_fields[key[0]] += 1

                for field_id, level, column in sorted(canonical_keys, key=canonical_key_order):
                    meta = metadata.get(field_id, {})
                    base_entry = base_fields.get((field_id, level, column))
                    base_value = base_entry[0] if base_entry else None
                    source_table = base_entry[2] if base_entry else source_table_key(meta)
                    source_field = base_entry[3] if base_entry else source_column_for_metadata(meta, level, column) if meta else ""
                    map_values = candidates.get((field_id, level, column), [])
                    last_value = map_values[-1].value if map_values else base_value
                    recovered_map, recovery_reason = choose_recovered(map_values)
                    recovered_value = recovered_map if map_values else base_value
                    if not map_values:
                        recovery_reason = "base"
                    conflict = len(map_values) > 1 and not all_same([candidate.value for candidate in map_values])
                    if conflict:
                        conflict_rows.append([
                            category,
                            table,
                            rawcode,
                            base_rawcode,
                            field_id,
                            level,
                            column,
                            stable_json(base_value),
                            stable_json([candidate.value for candidate in map_values]),
                            stable_json(last_value),
                            stable_json(recovered_value),
                            recovery_reason,
                        ])

                    row = {
                        "category": category,
                        "table": table,
                        "rawcode": rawcode,
                        "base_rawcode": base_rawcode,
                        "field_id": field_id,
                        "level": level,
                        "column": column,
                        "field_name": str(ci_get(meta, "field") or "") if meta else "",
                        "display_name": metadata_display(meta, editor_strings) if meta else "",
                        "value_type": str(ci_get(meta, "type") or "") if meta else (map_values[0].value_type if map_values else ""),
                        "source_table": source_table,
                        "source_field": source_field,
                        "base_value": base_value,
                        "map_values": [candidate.value for candidate in map_values],
                        "map_indexes": [candidate.index for candidate in map_values],
                        "last_write_value": last_value,
                        "recovered_value": recovered_value,
                        "selection": recovery_reason,
                        "protection_conflict": conflict,
                    }
                    full_rows.append(row)
                    rows_by_object[(category, rawcode)][(field_id, level, column)] = row

                object_records.append({
                    "category": category,
                    "table": table,
                    "rawcode": rawcode,
                    "base_rawcode": base_rawcode,
                    "modification_count": len(modifications),
                    "canonical_modification_count": len(candidates),
                    "duplicate_modification_count": len(modifications) - len(candidates),
                    "protection_conflict_count": sum(
                        1 for values in candidates.values() if len(values) > 1 and not all_same([candidate.value for candidate in values])
                    ),
                })

    write_tsv(
        output / "object-fields.tsv",
        [
            "category", "table", "rawcode", "base_rawcode", "field_id", "level", "column",
            "field_name", "display_name", "value_type", "source_table", "source_field",
            "base_value_json", "map_values_json", "map_indexes", "last_write_value_json",
            "recovered_value_json", "selection", "protection_conflict",
        ],
        (
            [
                row["category"], row["table"], row["rawcode"], row["base_rawcode"], row["field_id"],
                row["level"], row["column"], row["field_name"], row["display_name"], row["value_type"],
                row["source_table"], row["source_field"], stable_json(row["base_value"]),
                stable_json(row["map_values"]), ",".join(str(index) for index in row["map_indexes"]),
                stable_json(row["last_write_value"]), stable_json(row["recovered_value"]), row["selection"],
                int(row["protection_conflict"]),
            ]
            for row in full_rows
        ),
    )

    write_tsv(
        output / "protection-conflicts.tsv",
        [
            "category", "table", "rawcode", "base_rawcode", "field_id", "level", "column",
            "base_value_json", "map_values_json", "last_write_value_json", "recovered_value_json", "selection",
        ],
        conflict_rows,
    )

    name_fields = {
        "units": "unam",
        "items": "unam",
        "abilities": "anam",
        "buffs": "ftip",
        "destructables": "bnam",
        "doodads": "dnam",
    }
    for record in object_records:
        record["name"] = field_lookup(
            rows_by_object, record["category"], record["rawcode"], name_fields[record["category"]]
        )

    write_tsv(
        output / "objects.tsv",
        [
            "category", "table", "rawcode", "base_rawcode", "name", "modification_count", "canonical_modification_count",
            "duplicate_modification_count", "protection_conflict_count",
        ],
        ([record[column] for column in [
            "category", "table", "rawcode", "base_rawcode", "name", "modification_count", "canonical_modification_count",
            "duplicate_modification_count", "protection_conflict_count",
        ]] for record in object_records),
    )

    # Compact normalized unit view. The raw resolved table above remains the
    # complete source of truth; this table is intentionally ergonomic.
    unit_header = [
        "table", "rawcode", "base_rawcode", "name", "tip", "ubertip", "race", "is_building",
        "build_time", "gold_cost", "lumber_cost", "hp", "hp_regen", "hp_regen_type", "mana_max",
        "mana_start", "mana_regen", "armor", "armor_type", "move_type", "move_speed", "collision",
        "acquisition_range", "abilities", "classifications", "pathing_texture",
        "attack1_enabled", "attack1_type", "attack1_weapon_type", "attack1_range", "attack1_cooldown",
        "attack1_dice", "attack1_sides", "attack1_bonus", "attack1_min", "attack1_max", "attack1_avg",
        "attack1_dps", "attack1_targets", "attack1_full_aoe", "attack1_half_aoe", "attack1_quarter_aoe",
        "attack1_half_factor", "attack1_quarter_factor", "attack1_splash_targets", "attack1_projectile_speed",
        "attack2_enabled", "attack2_type", "attack2_weapon_type", "attack2_range", "attack2_cooldown",
        "attack2_dice", "attack2_sides", "attack2_bonus", "attack2_min", "attack2_max", "attack2_avg",
        "attack2_dps", "attack2_targets", "protection_conflict_fields",
    ]

    # Raw editor field IDs are stable and resolved through UnitMetaData.slk.
    U = {
        "name": "unam", "tip": "utip", "ubertip": "utub", "race": "urac", "build_time": "ubld",
        "gold": "ugol", "lumber": "ulum", "hp": "uhpm", "hp_regen": "uhpr", "hp_regen_type": "uhrt",
        "mana_max": "umpm", "mana_start": "umpi", "mana_regen": "umpr", "armor": "udef", "armor_type": "udty",
        "move_type": "umvt", "move_speed": "umvs", "collision": "ucol", "acquire": "uacq", "abilities": "uabi",
        "classifications": "utyp", "pathing": "upat", "attacks_enabled": "uaen",
    }

    def f(rawcode: str, field_id: str, level: int = 0, column: int = 0) -> Any:
        return field_lookup(rows_by_object, "units", rawcode, field_id, level, column)

    def source_unit(rawcode: str, base_rawcode: str, table: str, field: str) -> Any:
        # Source-only fields have no map rawcode counterpart; expose them as a
        # fallback/context value from the selected base rawcode.
        return raw_source_value(tables, table, base_rawcode, field)

    unit_rows: list[list[Any]] = []
    building_rows: list[list[Any]] = []
    used_pathing: set[str] = set()
    missing_pathing: set[str] = set()

    unit_objects = [record for record in object_records if record["category"] == "units"]
    for record in unit_objects:
        rawcode = record["rawcode"]
        base_rawcode = record["base_rawcode"]
        conflicts = [
            row["field_id"] for row in full_rows
            if row["category"] == "units" and row["rawcode"] == rawcode and row["protection_conflict"]
        ]
        attacks_enabled = f(rawcode, U["attacks_enabled"])
        if attacks_enabled is None:
            attacks_enabled = source_unit(rawcode, base_rawcode, "UnitWeapons", "weapsOn")
        try:
            attack_bits = int(attacks_enabled or 0)
        except (TypeError, ValueError):
            attack_bits = 0

        attack_values: dict[int, dict[str, Any]] = {}
        for number in (1, 2):
            suffix = str(number)
            ids = {
                "type": f"ua{suffix}t", "weapon": f"ua{suffix}w", "range": f"ua{suffix}r", "cooldown": f"ua{suffix}c",
                "dice": f"ua{suffix}d", "sides": f"ua{suffix}s", "bonus": f"ua{suffix}b", "targets": f"ua{suffix}g",
                "full": f"ua{suffix}f", "half": f"ua{suffix}h", "quarter": f"ua{suffix}q",
                "projectile_speed": f"ua{suffix}z",
            }
            values = {name: f(rawcode, field_id) for name, field_id in ids.items()}
            # Half/quarter factors use historical non-uniform raw IDs.
            values["half_factor"] = f(rawcode, "uhd1" if number == 1 else "uhd2")
            values["quarter_factor"] = f(rawcode, "uqd1" if number == 1 else "uqd2")
            values["splash_targets"] = f(rawcode, "ua1p" if number == 1 else "ua2p")
            dice = numeric(values["dice"])
            sides = numeric(values["sides"])
            bonus = numeric(values["bonus"])
            cooldown = numeric(values["cooldown"])
            if dice is not None and sides is not None and bonus is not None:
                minimum = bonus + dice
                maximum = bonus + dice * sides
                average = (minimum + maximum) / 2.0
            else:
                minimum = maximum = average = None
            dps = average / cooldown if average is not None and cooldown and cooldown > 0 else None
            values.update({"min": minimum, "max": maximum, "avg": average, "dps": dps})
            attack_values[number] = values

        is_building = source_unit(rawcode, base_rawcode, "UnitBalance", "isbldg")
        pathing_texture = f(rawcode, U["pathing"])
        normalized_path = normalize_pathing_name(pathing_texture)
        if normalized_path:
            if normalized_path in pathing_textures:
                used_pathing.add(normalized_path)
            else:
                missing_pathing.add(normalized_path)

        row = [
            record["table"], rawcode, base_rawcode, f(rawcode, U["name"]), f(rawcode, U["tip"]), f(rawcode, U["ubertip"]),
            f(rawcode, U["race"]), is_building, f(rawcode, U["build_time"]), f(rawcode, U["gold"]), f(rawcode, U["lumber"]),
            f(rawcode, U["hp"]), f(rawcode, U["hp_regen"]), f(rawcode, U["hp_regen_type"]), f(rawcode, U["mana_max"]),
            f(rawcode, U["mana_start"]), f(rawcode, U["mana_regen"]), f(rawcode, U["armor"]), f(rawcode, U["armor_type"]),
            f(rawcode, U["move_type"]), f(rawcode, U["move_speed"]), f(rawcode, U["collision"]), f(rawcode, U["acquire"]),
            f(rawcode, U["abilities"]), f(rawcode, U["classifications"]), pathing_texture,
            bool(attack_bits & 1),
            attack_values[1]["type"], attack_values[1]["weapon"], attack_values[1]["range"], attack_values[1]["cooldown"],
            attack_values[1]["dice"], attack_values[1]["sides"], attack_values[1]["bonus"], attack_values[1]["min"],
            attack_values[1]["max"], attack_values[1]["avg"], attack_values[1]["dps"], attack_values[1]["targets"],
            attack_values[1]["full"], attack_values[1]["half"], attack_values[1]["quarter"], attack_values[1]["half_factor"],
            attack_values[1]["quarter_factor"], attack_values[1]["splash_targets"], attack_values[1]["projectile_speed"],
            bool(attack_bits & 2),
            attack_values[2]["type"], attack_values[2]["weapon"], attack_values[2]["range"], attack_values[2]["cooldown"],
            attack_values[2]["dice"], attack_values[2]["sides"], attack_values[2]["bonus"], attack_values[2]["min"],
            attack_values[2]["max"], attack_values[2]["avg"], attack_values[2]["dps"], attack_values[2]["targets"],
            ",".join(sorted(set(conflicts))),
        ]
        unit_rows.append(row)

        building_flag = bool(is_building) if isinstance(is_building, bool) else numeric(is_building) not in (None, 0.0)
        if building_flag or normalized_path:
            texture = pathing_textures.get(normalized_path or "")
            building_rows.append([
                record["table"], rawcode, base_rawcode, f(rawcode, U["name"]), f(rawcode, U["tip"]), f(rawcode, U["ubertip"]),
                f(rawcode, U["hp"]), f(rawcode, U["armor"]), f(rawcode, U["armor_type"]), f(rawcode, U["collision"]),
                f(rawcode, U["abilities"]), f(rawcode, U["gold"]), f(rawcode, U["lumber"]), f(rawcode, U["build_time"]),
                pathing_texture or "", texture["width_cells"] if texture else "", texture["height_cells"] if texture else "",
                texture["width_world_units"] if texture else "", texture["height_world_units"] if texture else "",
                texture["counts"]["unwalkable"] if texture else "", texture["counts"]["unflyable"] if texture else "",
                texture["counts"]["unbuildable"] if texture else "", "/".join(texture["hex_rows"]) if texture else "",
                ",".join(sorted(set(conflicts))),
            ])

    write_tsv(output / "units.tsv", unit_header, unit_rows)
    write_tsv(
        output / "buildings.tsv",
        [
            "table", "rawcode", "base_rawcode", "name", "tip", "ubertip", "hp", "armor", "armor_type", "collision",
            "abilities", "gold_cost", "lumber_cost", "build_time", "pathing_texture", "footprint_width_cells",
            "footprint_height_cells", "footprint_width_world_units", "footprint_height_world_units", "unwalkable_cells",
            "unflyable_cells", "unbuildable_cells", "footprint_hex_rows", "protection_conflict_fields",
        ],
        building_rows,
    )

    # Join placed map doodads/destructibles to their inherited object data and
    # exact pathing masks. This turns war3map.doo into usable static-layout
    # evidence instead of only a list of rawcodes and coordinates.
    placed_rows: list[list[Any]] = []
    placed_pathing_types: set[str] = set()
    unresolved_placed_types: set[str] = set()
    missing_placed_pathing: set[str] = set()
    doodad_data = json.loads((map_root / "doodads.json").read_text(encoding="utf-8"))

    def placed_definition(rawcode: str) -> tuple[str, Any, Any]:
        if ("doodads", rawcode) in rows_by_object:
            return (
                "doodad",
                field_lookup(rows_by_object, "doodads", rawcode, "dnam"),
                field_lookup(rows_by_object, "doodads", rawcode, "dptx"),
            )
        if ("destructables", rawcode) in rows_by_object:
            return (
                "destructable",
                field_lookup(rows_by_object, "destructables", rawcode, "bnam"),
                field_lookup(rows_by_object, "destructables", rawcode, "bptx"),
            )
        doodad_row = table_row(tables, "DoodadData", rawcode)
        if doodad_row:
            name = resolve_string_key(row_ci_get(doodad_row, "Name"), editor_strings)
            return "doodad", name, row_ci_get(doodad_row, "pathTex")
        destructable_row = table_row(tables, "DestructableData", rawcode)
        if destructable_row:
            name = resolve_string_key(row_ci_get(destructable_row, "Name"), editor_strings)
            return "destructable", name, row_ci_get(destructable_row, "pathTex")
        return "unknown", "", ""

    for index, placement in enumerate(doodad_data.get("regular", [])):
        rawcode = str(placement.get("type", ""))
        kind, name, pathing_texture = placed_definition(rawcode)
        if kind == "unknown":
            unresolved_placed_types.add(rawcode)
        normalized_path = normalize_pathing_name(pathing_texture)
        texture = pathing_textures.get(normalized_path or "")
        if normalized_path:
            placed_pathing_types.add(rawcode)
            if texture is None:
                missing_placed_pathing.add(normalized_path)
        position = placement.get("position", [None, None, None])
        scale = placement.get("scale", [None, None, None])
        flags = placement.get("flags", {})
        placed_rows.append([
            index,
            placement.get("id", ""),
            rawcode,
            kind,
            name,
            position[0] if len(position) > 0 else "",
            position[1] if len(position) > 1 else "",
            position[2] if len(position) > 2 else "",
            placement.get("angle", ""),
            scale[0] if len(scale) > 0 else "",
            scale[1] if len(scale) > 1 else "",
            scale[2] if len(scale) > 2 else "",
            int(bool(flags.get("visible"))),
            int(bool(flags.get("solid"))),
            int(bool(flags.get("fixedZ"))),
            placement.get("variation", ""),
            placement.get("life", ""),
            pathing_texture or "",
            texture["width_cells"] if texture else "",
            texture["height_cells"] if texture else "",
            texture["width_world_units"] if texture else "",
            texture["height_world_units"] if texture else "",
            texture["counts"]["unwalkable"] if texture else "",
            texture["counts"]["unflyable"] if texture else "",
            texture["counts"]["unbuildable"] if texture else "",
            "/".join(texture["hex_rows"]) if texture else "",
        ])

    write_tsv(
        output / "placed-doodads.tsv",
        [
            "placement_index", "editor_id", "rawcode", "object_kind", "name", "x", "y", "z", "angle_degrees",
            "scale_x", "scale_y", "scale_z", "visible", "solid", "fixed_z", "variation", "life",
            "pathing_texture", "footprint_width_cells", "footprint_height_cells", "footprint_width_world_units",
            "footprint_height_world_units", "unwalkable_cells", "unflyable_cells", "unbuildable_cells", "footprint_hex_rows",
        ],
        placed_rows,
    )

    # Ability convenience view: one row per resolved ability level. The full
    # object-fields table carries all DataA..I and ability-specific raw fields.
    ability_rows: list[list[Any]] = []
    ability_objects = [record for record in object_records if record["category"] == "abilities"]
    for record in ability_objects:
        rawcode = record["rawcode"]
        base_rawcode = record["base_rawcode"]
        levels = field_lookup(rows_by_object, "abilities", rawcode, "alev", 0, 0)
        if levels is None:
            levels = effective_base_levels("abilities", base_rawcode, tables)
        try:
            level_count = max(1, int(levels))
        except (TypeError, ValueError):
            level_count = 1
        for level in range(1, level_count + 1):
            def af(field_id: str) -> Any:
                value = field_lookup(rows_by_object, "abilities", rawcode, field_id, level, 0)
                if value is None:
                    value = field_lookup(rows_by_object, "abilities", rawcode, field_id, 0, 0)
                return value

            data: dict[str, Any] = {}
            labeled_data: dict[str, Any] = {}
            object_fields = rows_by_object.get(("abilities", rawcode), {})
            for (field_id, row_level, column), field_row in object_fields.items():
                if row_level not in (0, level):
                    continue
                if field_row["field_name"].casefold() == "data":
                    letter = chr(ord("A") + max(column, 1) - 1)
                    data[f"Data{letter}"] = field_row["recovered_value"]
                    label = field_row["display_name"] or field_id
                    if label in labeled_data:
                        label = f"{label} [{field_id}]"
                    labeled_data[label] = field_row["recovered_value"]
            ability_rows.append([
                record["table"], rawcode, base_rawcode, level, af("anam"), af("atp1"), af("aub1"), af("amcs"), af("acdn"),
                af("aran"), af("aare"), af("atar"), af("abuf"), stable_json(data), stable_json(labeled_data),
            ])

    write_tsv(
        output / "abilities.tsv",
        [
            "table", "rawcode", "base_rawcode", "level", "name", "tip", "ubertip", "mana_cost", "cooldown", "range",
            "area", "targets", "buffs", "data_fields_json", "data_fields_labeled_json",
        ],
        ability_rows,
    )

    def protected_static_value(rawcode: str, level: int, field: str) -> tuple[Any, str]:
        field_id = {"cooldown": "acdn", "mana_cost": "amcs"}[field]
        object_fields = rows_by_object.get(("abilities", rawcode), {})
        row = object_fields.get((field_id, level, 0)) or object_fields.get((field_id, 0, 0))
        if row is None:
            return None, ""
        return row["recovered_value"], str(row["selection"])

    def protected_comparison(runtime_value: str, static_value: Any) -> str:
        if static_value is None:
            return "static-missing"
        runtime_number = numeric(runtime_value)
        static_number = numeric(static_value)
        if runtime_number is not None and static_number is not None:
            return "static-match" if math.isclose(runtime_number, static_number, rel_tol=0.0, abs_tol=1e-6) else "static-differs"
        return "static-match" if str(runtime_value) == str(static_value) else "static-differs"

    protected_rows: list[list[Any]] = []
    protected_comparisons: Counter[str] = Counter()
    protected_path = map_root / "script" / "protected-ability-fields.tsv"
    if protected_path.exists():
        with protected_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                rawcode = row["rawcode"]
                level = int(row["level"])
                static_value, static_selection = protected_static_value(rawcode, level, row["field"])
                comparison = protected_comparison(row["runtime_value"], static_value)
                protected_comparisons[comparison] += 1
                protected_rows.append([
                    rawcode,
                    row["rawcode_integer"],
                    row["names"],
                    row["level_index"],
                    level,
                    row["field"],
                    row["runtime_value"],
                    value_as_text(static_value),
                    static_selection,
                    comparison,
                    row["jass_add_restore"],
                    row["source_function"],
                    row["byte_offset"],
                ])
    write_tsv(
        output / "protected-ability-fields.tsv",
        [
            "rawcode", "rawcode_integer", "names", "level_index", "level", "field", "runtime_value",
            "static_resolved_value", "static_selection", "comparison", "jass_add_restore", "source_function",
            "byte_offset",
        ],
        protected_rows,
    )

    protected_jass_rows: list[list[Any]] = []
    protected_jass_comparisons: Counter[str] = Counter()
    protected_jass_path = map_root / "script" / "protected-ability-jass-add-restores.tsv"
    if protected_jass_path.exists():
        with protected_jass_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                rawcode = row["rawcode"]
                level = int(row["level"])
                static_value, static_selection = protected_static_value(rawcode, level, row["field"])
                comparison = protected_comparison(row["runtime_value"], static_value)
                protected_jass_comparisons[comparison] += 1
                protected_jass_rows.append([
                    rawcode,
                    row["rawcode_integer"],
                    row["names"],
                    row["level_index"],
                    level,
                    row["field"],
                    row["runtime_value"],
                    value_as_text(static_value),
                    static_selection,
                    comparison,
                    row["canonical_relation"],
                    row["source_function"],
                    row["byte_offset"],
                ])
    write_tsv(
        output / "protected-ability-jass-add-restores.tsv",
        [
            "rawcode", "rawcode_integer", "names", "level_index", "level", "field", "runtime_value",
            "static_resolved_value", "static_selection", "comparison", "canonical_relation", "source_function",
            "byte_offset",
        ],
        protected_jass_rows,
    )

    effective_unit_rows: list[list[Any]] = []
    effective_unit_comparisons: dict[str, Counter[str]] = {
        field: Counter() for field in ("hp", "armor", "dps", "attack_range", "move_speed")
    }
    effective_unit_vs_unitstat_comparisons: dict[str, Counter[str]] = {
        field: Counter() for field in ("hp", "armor", "dps", "attack_range", "move_speed")
    }
    effective_unit_path = map_root / "script" / "effective-unit-stats.tsv"
    static_units: dict[str, dict[str, str]] = {}
    with (output / "units.tsv").open(encoding="utf-8", newline="") as handle:
        static_units = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    protected_unit_rows: list[list[Any]] = []
    protected_unit_applied: dict[str, dict[str, Any]] = {}
    protected_unit_path = map_root / "script" / "protected-unit-stats.tsv"
    protected_unit_override_counts: Counter[str] = Counter()

    def protected_unit_overlay(row: dict[str, str], override_field: str, static_field: str) -> tuple[str, str]:
        override = row.get(override_field, "")
        static = static_units.get(row["rawcode"], {}).get(static_field, "")
        return (override if override != "" else static), ("protected-runtime" if override != "" else "static-resolved")

    if protected_unit_path.exists():
        with protected_unit_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                static = static_units.get(row["rawcode"])
                if static is None:
                    raise ValueError(f"protected UnitStat row has no static unit definition: {row['rawcode']}")
                field_pairs = [
                    ("hp", "hp"),
                    ("armor", "armor"),
                    ("move_speed", "move_speed"),
                    ("attack1_base_damage", "attack1_bonus"),
                    ("attack1_dice_number", "attack1_dice"),
                    ("attack1_dice_sides", "attack1_sides"),
                    ("attack1_cooldown", "attack1_cooldown"),
                    ("attack1_range", "attack1_range"),
                    ("attack2_base_damage", "attack2_bonus"),
                    ("attack2_dice_number", "attack2_dice"),
                    ("attack2_dice_sides", "attack2_sides"),
                    ("attack2_cooldown", "attack2_cooldown"),
                    ("attack2_range", "attack2_range"),
                ]
                applied: dict[str, str] = {}
                sources: dict[str, str] = {}
                override_fields: list[str] = []
                for override_field, static_field in field_pairs:
                    applied[override_field], sources[override_field] = protected_unit_overlay(row, override_field, static_field)
                    if row.get(override_field, "") != "":
                        protected_unit_override_counts[override_field] += 1
                        override_fields.append(override_field)
                if row.get("defense_type", "") != "":
                    protected_unit_override_counts["defense_type"] += 1
                    override_fields.append("defense_type")

                def attack_derived(number: int) -> tuple[Any, Any, Any, Any]:
                    base = numeric(applied[f"attack{number}_base_damage"])
                    dice = numeric(applied[f"attack{number}_dice_number"])
                    sides = numeric(applied[f"attack{number}_dice_sides"])
                    cooldown = numeric(applied[f"attack{number}_cooldown"])
                    if None in (base, dice, sides):
                        return None, None, None, None
                    minimum = base + dice
                    maximum = base + dice * sides
                    average = base + dice * (sides + 1.0) / 2.0
                    dps = average / cooldown if cooldown and cooldown > 0 else None
                    return minimum, maximum, average, dps

                attack1_min, attack1_max, attack1_avg, attack1_dps = attack_derived(1)
                attack2_min, attack2_max, attack2_avg, attack2_dps = attack_derived(2)
                protected_unit_applied[row["rawcode"]] = {
                    "hp": applied["hp"],
                    "armor": applied["armor"],
                    "move_speed": applied["move_speed"],
                    "attack_range": applied["attack1_range"],
                    "dps": value_as_text(attack1_dps),
                }
                protected_unit_rows.append([
                    row["rawcode"], row["rawcode_integer"], row["names"], row["source_fingerprint"],
                    ",".join(override_fields), row["override_field_count"],
                    static["hp"], row["hp"], applied["hp"], sources["hp"],
                    static["armor"], row["armor"], applied["armor"], sources["armor"],
                    static["armor_type"], row["defense_type"],
                    static["move_speed"], row["move_speed"], applied["move_speed"], sources["move_speed"],
                    static["attack1_bonus"], row["attack1_base_damage"], applied["attack1_base_damage"], sources["attack1_base_damage"],
                    static["attack1_dice"], row["attack1_dice_number"], applied["attack1_dice_number"], sources["attack1_dice_number"],
                    static["attack1_sides"], row["attack1_dice_sides"], applied["attack1_dice_sides"], sources["attack1_dice_sides"],
                    static["attack1_cooldown"], row["attack1_cooldown"], applied["attack1_cooldown"], sources["attack1_cooldown"],
                    static["attack1_range"], row["attack1_range"], applied["attack1_range"], sources["attack1_range"],
                    attack1_min, attack1_max, attack1_avg, attack1_dps,
                    static["attack2_bonus"], row["attack2_base_damage"], applied["attack2_base_damage"], sources["attack2_base_damage"],
                    static["attack2_dice"], row["attack2_dice_number"], applied["attack2_dice_number"], sources["attack2_dice_number"],
                    static["attack2_sides"], row["attack2_dice_sides"], applied["attack2_dice_sides"], sources["attack2_dice_sides"],
                    static["attack2_cooldown"], row["attack2_cooldown"], applied["attack2_cooldown"], sources["attack2_cooldown"],
                    static["attack2_range"], row["attack2_range"], applied["attack2_range"], sources["attack2_range"],
                    attack2_min, attack2_max, attack2_avg, attack2_dps,
                    row["encoded_values_json"], row["source_function"], row["byte_offset"],
                ])
    write_tsv(
        output / "protected-unit-stats.tsv",
        [
            "rawcode", "rawcode_integer", "names", "source_fingerprint", "override_fields", "override_field_count",
            "static_hp", "override_hp", "unitstat_hp", "hp_source",
            "static_armor", "override_armor", "unitstat_armor", "armor_source",
            "static_armor_type", "override_defense_type",
            "static_move_speed", "override_move_speed", "unitstat_move_speed", "move_speed_source",
            "static_attack1_base_damage", "override_attack1_base_damage", "unitstat_attack1_base_damage", "attack1_base_damage_source",
            "static_attack1_dice_number", "override_attack1_dice_number", "unitstat_attack1_dice_number", "attack1_dice_number_source",
            "static_attack1_dice_sides", "override_attack1_dice_sides", "unitstat_attack1_dice_sides", "attack1_dice_sides_source",
            "static_attack1_cooldown", "override_attack1_cooldown", "unitstat_attack1_cooldown", "attack1_cooldown_source",
            "static_attack1_range", "override_attack1_range", "unitstat_attack1_range", "attack1_range_source",
            "unitstat_attack1_min", "unitstat_attack1_max", "unitstat_attack1_avg", "unitstat_attack1_dps",
            "static_attack2_base_damage", "override_attack2_base_damage", "unitstat_attack2_base_damage", "attack2_base_damage_source",
            "static_attack2_dice_number", "override_attack2_dice_number", "unitstat_attack2_dice_number", "attack2_dice_number_source",
            "static_attack2_dice_sides", "override_attack2_dice_sides", "unitstat_attack2_dice_sides", "attack2_dice_sides_source",
            "static_attack2_cooldown", "override_attack2_cooldown", "unitstat_attack2_cooldown", "attack2_cooldown_source",
            "static_attack2_range", "override_attack2_range", "unitstat_attack2_range", "attack2_range_source",
            "unitstat_attack2_min", "unitstat_attack2_max", "unitstat_attack2_avg", "unitstat_attack2_dps",
            "encoded_values_json", "source_function", "byte_offset",
        ],
        protected_unit_rows,
    )

    def effective_comparison(runtime_value: str, static_value: str, *, tolerance: float = 1e-6) -> str:
        runtime_number = numeric(runtime_value)
        static_number = numeric(static_value)
        if runtime_number is None or static_number is None:
            return "static-missing"
        return "static-match" if math.isclose(runtime_number, static_number, rel_tol=0.0, abs_tol=tolerance) else "static-differs"

    if effective_unit_path.exists():
        with effective_unit_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                static = static_units.get(row["unit_rawcode"], {})
                static_values = {
                    "hp": static.get("hp", ""),
                    "armor": static.get("armor", ""),
                    "dps": static.get("attack1_dps", ""),
                    "attack_range": static.get("attack1_range", ""),
                    "move_speed": static.get("move_speed", ""),
                }
                comparisons = {
                    "hp": effective_comparison(row["hp"], static_values["hp"]),
                    "armor": effective_comparison(row["armor"], static_values["armor"]),
                    # xO stores DPS at hundredths precision, so treat the
                    # corresponding quantization band as an exact catalog match.
                    "dps": effective_comparison(row["dps"], static_values["dps"], tolerance=0.011),
                    "attack_range": effective_comparison(row["attack_range"], static_values["attack_range"]),
                    "move_speed": effective_comparison(row["move_speed"], static_values["move_speed"]),
                }
                unitstat_values = protected_unit_applied.get(row["unit_rawcode"], {})
                unitstat_comparisons: dict[str, str] = {}
                for field in ("hp", "armor", "dps", "attack_range", "move_speed"):
                    candidate = value_as_text(unitstat_values.get(field))
                    tolerance = 0.011 if field == "dps" else 1e-6
                    comparison = effective_comparison(row[field], candidate, tolerance=tolerance)
                    unitstat_comparisons[field] = comparison.replace("static-", "unitstat-")
                for field, comparison in comparisons.items():
                    effective_unit_comparisons[field][comparison] += 1
                for field, comparison in unitstat_comparisons.items():
                    effective_unit_vs_unitstat_comparisons[field][comparison] += 1
                effective_unit_rows.append([
                    row["building_rawcode"],
                    row["building_names"],
                    row["unit_rawcode"],
                    row["unit_names"],
                    row["hp"],
                    static_values["hp"],
                    comparisons["hp"],
                    value_as_text(unitstat_values.get("hp")),
                    unitstat_comparisons["hp"],
                    row["armor"],
                    static_values["armor"],
                    comparisons["armor"],
                    value_as_text(unitstat_values.get("armor")),
                    unitstat_comparisons["armor"],
                    row["dps"],
                    static_values["dps"],
                    comparisons["dps"],
                    value_as_text(unitstat_values.get("dps")),
                    unitstat_comparisons["dps"],
                    row["attack_range"],
                    static_values["attack_range"],
                    comparisons["attack_range"],
                    value_as_text(unitstat_values.get("attack_range")),
                    unitstat_comparisons["attack_range"],
                    row["move_speed"],
                    static_values["move_speed"],
                    comparisons["move_speed"],
                    value_as_text(unitstat_values.get("move_speed")),
                    unitstat_comparisons["move_speed"],
                    row["spawns_per_cycle"],
                    row["can_hit_air"],
                    row["source_function"],
                    row["byte_offset"],
                ])
    write_tsv(
        output / "effective-unit-stats.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names",
            "effective_hp", "static_hp", "hp_comparison", "unitstat_hp", "hp_vs_unitstat",
            "effective_armor", "static_armor", "armor_comparison", "unitstat_armor", "armor_vs_unitstat",
            "effective_dps", "static_attack1_dps", "dps_comparison", "unitstat_attack1_dps", "dps_vs_unitstat",
            "effective_attack_range", "static_attack1_range", "attack_range_comparison", "unitstat_attack1_range", "attack_range_vs_unitstat",
            "effective_move_speed", "static_move_speed", "move_speed_comparison", "unitstat_move_speed", "move_speed_vs_unitstat",
            "spawns_per_cycle", "can_hit_air", "source_function", "byte_offset",
        ],
        effective_unit_rows,
    )

    # Base source rows for every map object's inheritance anchor. These expose
    # computed/non-editor SLK columns such as realHP, min/max damage and DPS.
    source_rows: list[list[Any]] = []
    category_tables = {
        "units": ["UnitData", "UnitBalance", "UnitAbilities", "UnitUI", "UnitWeapons"],
        "items": ["ItemData"],
        "abilities": ["AbilityData"],
        "buffs": ["AbilityBuffData"],
        "destructables": ["DestructableData"],
        "doodads": ["DoodadData"],
    }
    for record in object_records:
        category = record["category"]
        base_rawcode = record["base_rawcode"]
        for table_name in category_tables[category]:
            source = table_row(tables, table_name, base_rawcode)
            if not source:
                continue
            for field_name, value in sorted(source.items(), key=lambda item: item[0].casefold()):
                source_rows.append([
                    category, record["table"], record["rawcode"], base_rawcode, table_name, field_name,
                    stable_json(value), table_sources.get(SLK_ALIASES.get(table_name.casefold(), table_name.casefold()), ""),
                ])
    write_tsv(
        output / "base-source-fields.tsv",
        ["category", "table", "rawcode", "base_rawcode", "source_table", "source_field", "base_value_json", "source_file"],
        source_rows,
    )

    source_manifest: list[dict[str, Any]] = []
    for path in sorted(source_root.rglob("*")):
        if not path.is_file():
            continue
        data = path.read_bytes()
        source_manifest.append({
            "path": path.relative_to(source_root).as_posix(),
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
    (output / "base-data-manifest.json").write_text(
        json.dumps(source_manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    summary = {
        "warcraft_install_build": {
            "version": build_info.get("Version"),
            "build_key": build_info.get("Build Key"),
            "product": build_info.get("Product"),
        },
        "selected_game_data_set": {
            "w3i_index": selected_data_set,
            "casc_overlay": "war3.w3mod:_balance\\custom_v0.w3mod",
        },
        "map_objects": dict(Counter(record["category"] for record in object_records)),
        "resolved_field_rows": len(full_rows),
        "base_source_field_rows": len(source_rows),
        "protection_conflicts": len(conflict_rows),
        "objects_with_protection_conflicts": len({(row[0], row[2]) for row in conflict_rows}),
        "base_data_files": len(source_manifest),
        "pathing_textures_extracted": len(pathing_textures),
        "pathing_textures_used_by_map_units": len(used_pathing),
        "missing_pathing_textures": sorted(missing_pathing),
        "placed_doodads": len(placed_rows),
        "placed_object_types_with_pathing": len(placed_pathing_types),
        "unresolved_placed_object_types": sorted(unresolved_placed_types),
        "missing_placed_pathing_textures": sorted(missing_placed_pathing),
        "unresolved_base_objects": unresolved_base_objects,
        "unknown_map_field_ids": dict(sorted(unknown_map_fields.items())),
        "protected_ability_runtime_fields": len(protected_rows),
        "protected_ability_runtime_field_comparisons": dict(sorted(protected_comparisons.items())),
        "protected_ability_jass_add_fields": len(protected_jass_rows),
        "protected_ability_jass_add_field_comparisons": dict(sorted(protected_jass_comparisons.items())),
        "protected_unit_stat_rows": len(protected_unit_rows),
        "protected_unit_stat_override_assignments": sum(protected_unit_override_counts.values()),
        "protected_unit_stat_override_counts": dict(sorted(protected_unit_override_counts.items())),
        "effective_unit_stat_rows": len(effective_unit_rows),
        "effective_unit_stat_comparisons": {
            field: dict(sorted(counts.items())) for field, counts in effective_unit_comparisons.items()
        },
        "effective_unit_stat_vs_unitstat_comparisons": {
            field: dict(sorted(counts.items())) for field, counts in effective_unit_vs_unitstat_comparisons.items()
        },
        "notes": [
            "object-fields.tsv preserves base, every map candidate, last-write and recovered values",
            "recovered values use a narrow W3P numeric-sentinel heuristic; ambiguous strings retain last-write semantics",
            "pathing texture pixels are 32 world units; bits 1/2/4 mean unwalkable/unflyable/unbuildable",
            "base-source-fields.tsv exposes selected custom_v0/base SLK values before map overrides, including computed columns",
            "protected-ability-fields.tsv compares the protected Lua runtime table against static resolved cooldown/mana values without overwriting either source",
            "protected-unit-stats.tsv applies the exactly decoded jP UnitStat overrides on top of static resolved unit fields while preserving static, override, source and encoded-row provenance; further scripted modifiers may still change live values",
            "effective-unit-stats.tsv compares the generated xO building-to-unit effective stat catalog against static unit object data; DPS comparison allows 0.011 for hundredths quantization",
        ],
    }
    (output / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"resolved {len(object_records)} map objects into {len(full_rows)} field rows")
    print(f"wrote {len(unit_rows)} unit rows, {len(building_rows)} building rows, {len(ability_rows)} ability-level rows")
    print(f"recorded {len(conflict_rows)} conflicting repeated map fields")
    if unresolved_base_objects:
        print(f"warning: {len(unresolved_base_objects)} objects have no base source row/profile")
    if missing_pathing:
        print(f"warning: {len(missing_pathing)} referenced unit/building pathing textures were not found")
    if unresolved_placed_types:
        print(f"warning: {len(unresolved_placed_types)} placed doodad rawcodes have no base definition")
    if missing_placed_pathing:
        print(f"warning: {len(missing_placed_pathing)} placed doodad pathing textures were not found")


if __name__ == "__main__":
    main()
