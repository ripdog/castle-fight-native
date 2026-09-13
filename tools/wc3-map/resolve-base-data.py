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
WURST_GENERATED_FIELD_ID = "wurs"
WURST_GENERATED_MARKER = 42
# Canonical Warcraft order IDs for the three base-order strings used by every
# scripted unit-spell ability in this map. These are engine constants (also
# exposed by Wurst Stdlib Orders.wurst), so they remain stable independently of
# W3P's encrypted registry expression.
CANONICAL_ORDER_IDS = {
    "absorb": 852529,
    "heal": 852063,
    "parasite": 852601,
}


@dataclass(frozen=True)
class GameDataSelection:
    data_set_index: int
    game_data_version: int
    overlay_dir: str
    profile_variant: str
    label: str
    casc_overlay: str


def select_game_data(map_info: dict[str, Any]) -> GameDataSelection:
    """Resolve W3I's game-data enum to the installed balance overlay.

    WC3MapTranslator exposes the W3I enum as Default=0, Custom101=1,
    MeleeLatestPatch=2. The map's separate game-data version selects ROC=0 or
    TFT=1. In current Warcraft III CASC data, TFT Default corresponds to the
    custom_v1 overlay/profile variant; ROC Default and explicit Custom101 use
    custom_v0; MeleeLatestPatch uses melee_v0.
    """
    data_set = int(map_info.get("game_data_set_version", 0))
    game_version = int((map_info.get("game_data_version") or {}).get("raw", 1))
    if data_set == 0:
        if game_version == 0:
            return GameDataSelection(0, 0, "custom_v0", "custom,V0", "Default (ROC)", "war3.w3mod:_balance\\custom_v0.w3mod")
        if game_version == 1:
            return GameDataSelection(0, 1, "custom_v1", "custom,V1", "Default (TFT)", "war3.w3mod:_balance\\custom_v1.w3mod")
        raise ValueError(f"unsupported W3I game data version {game_version} for Default data set")
    if data_set == 1:
        return GameDataSelection(1, game_version, "custom_v0", "custom,V0", "Custom (1.01)", "war3.w3mod:_balance\\custom_v0.w3mod")
    if data_set == 2:
        return GameDataSelection(2, game_version, "melee_v0", "melee,V0", "Melee (Latest Patch)", "war3.w3mod:_balance\\melee_v0.w3mod")
    raise ValueError(f"unsupported W3I game data set {data_set}")


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


def profile_value(
    profile: dict[str, dict[str, str]],
    section: str,
    field: str,
    variant_name: str | None = None,
) -> Any:
    fields = profile.get(section)
    if fields is None:
        return None
    if variant_name:
        variant = ci_get(fields, f"{field}:{variant_name}")
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
        if len(values) >= 3 and all_same(values[-2:]):
            return values[-1], "map-stable-final-write"
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


def load_slks(
    source_root: Path,
    selection: GameDataSelection,
) -> tuple[dict[str, dict[str, dict[str, Any]]], dict[str, str]]:
    base_units = source_root / "base" / "units"
    selected_units = source_root / selection.overlay_dir / "units"
    base_doodads = source_root / "base" / "doodads"

    tables: dict[str, dict[str, dict[str, Any]]] = {}
    table_sources: dict[str, str] = {}

    # Prefer the W3I-selected balance overlay for data tables, then fill
    # missing rawcodes from the generic base table. Metadata stays generic.
    all_paths = list(base_units.glob("*.slk")) + list(base_doodads.glob("*.slk"))
    for path in all_paths:
        stem = path.stem.casefold()
        tables[stem] = parse_slk(path)
        table_sources[stem] = f"base/{path.parent.name}/{path.name}"

    if not selected_units.exists():
        raise ValueError(f"selected Warcraft data overlay is missing from cache: {selected_units}")
    for path in selected_units.glob("*.slk"):
        stem = path.stem.casefold()
        selected_rows = parse_slk(path)
        if stem in tables:
            merged = dict(tables[stem])
            merged.update(selected_rows)
            tables[stem] = merged
        else:
            tables[stem] = selected_rows
        table_sources[stem] = f"{selection.overlay_dir}/units/{path.name} (+ base fallback)"

    return tables, table_sources


def load_profiles(source_root: Path, selection: GameDataSelection) -> dict[str, dict[str, str]]:
    profile: dict[str, dict[str, str]] = {}

    # Base non-localized object profiles and skins.
    for directory in (source_root / "base" / "units", source_root / "base" / "doodads"):
        for path in sorted(directory.glob("*.txt")):
            merge_profiles(profile, parse_profile(path))

    # Selected balance function/profile data takes precedence over generic
    # base profile data.
    for path in sorted((source_root / selection.overlay_dir / "units").glob("*.txt")):
        merge_profiles(profile, parse_profile(path))

    # Localized labels/tooltips take precedence last. profile_value() chooses
    # the W3I-selected :custom/:melee variant where present.
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
    profile_variant: str,
) -> tuple[Any, str, str]:
    slk = str(ci_get(meta, "slk") or "")
    field = str(ci_get(meta, "field") or "")
    raw_index = ci_get(meta, "index")
    index = int(raw_index) if raw_index is not None else -1
    if slk.casefold() == "profile":
        raw = profile_value(profile, base_rawcode, field, profile_variant)
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


def integer_rawcode(integer_id: int) -> str:
    return integer_id.to_bytes(4, "big").decode("latin1")


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

    map_info = json.loads((map_root / "map-info.json").read_text(encoding="utf-8"))
    data_selection = select_game_data(map_info)
    tables, table_sources = load_slks(source_root, data_selection)
    profile = load_profiles(source_root, data_selection)
    editor_strings = world_edit_strings(source_root / "enus" / "ui" / "worldeditstrings.txt")
    editor_strings.update(world_edit_strings(source_root / "enus" / "ui" / "worldeditgamestrings.txt"))
    build_info = parse_build_info(source_root / "install" / ".build.info")
    base_misc = parse_profile(source_root / "base" / "units" / "miscdata.txt").get("Misc", {})
    map_misc_path = map_root / "war3mapMisc.txt"
    map_misc = parse_profile(map_misc_path).get("Misc", {}) if map_misc_path.exists() else {}

    protected_unit_conflict_fields = {
        "uhpm": "hp",
        "udef": "armor",
        "ua1b": "attack1_base_damage",
        "ua1r": "attack1_range",
        "ua1s": "attack1_dice_sides",
    }
    protected_unit_conflict_runtime: dict[str, dict[str, str]] = {}
    protected_unit_conflict_path = map_root / "script" / "protected-unit-stats.tsv"
    if protected_unit_conflict_path.exists():
        with protected_unit_conflict_path.open(encoding="utf-8", newline="") as handle:
            protected_unit_conflict_runtime = {
                row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")
            }

    def effective_misc_value(key: str) -> tuple[str, str, str]:
        base_value = ci_get(base_misc, key)
        map_value = ci_get(map_misc, key)
        effective = map_value if map_value is not None else base_value
        if effective is None:
            raise ValueError(f"missing Warcraft misc constant: {key}")
        return str(base_value or ""), str(map_value or ""), str(effective)

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
        # Wurst's compile-time object API adds this non-WC3 metadata field to
        # every newly created object definition. Its compiler source defines
        # GENERATED_BY_WURST = 42 and later uses the marker to identify/remove
        # generated objects; it is provenance, not a gameplay/editor field.
        metadata[WURST_GENERATED_FIELD_ID] = {
            "field": "GENERATED_BY_WURST",
            "displayName": "Wurst generated-object marker",
            "type": "int",
            "slk": "WurstCompiler",
        }

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
                            meta, base_rawcode, level, column, tables, profile, data_selection.profile_variant
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
                    if conflict and category == "units" and field_id in protected_unit_conflict_fields:
                        runtime_row = protected_unit_conflict_runtime.get(rawcode)
                        runtime_column = protected_unit_conflict_fields[field_id]
                        runtime_value = runtime_row.get(runtime_column, "") if runtime_row is not None else ""
                        runtime_number = numeric(runtime_value)
                        if runtime_number is not None:
                            matching_values = [
                                candidate.value for candidate in map_values
                                if numeric(candidate.value) is not None
                                and math.isclose(float(numeric(candidate.value)), runtime_number, rel_tol=0.0, abs_tol=1e-9)
                            ]
                            if not matching_values:
                                raise ValueError(
                                    f"protected UnitStat runtime value {rawcode}.{runtime_column}={runtime_value} "
                                    f"matches none of conflicting map candidates {[candidate.value for candidate in map_values]}"
                                )
                            recovered_value = matching_values[0]
                            recovery_reason = "w3p-runtime-unitstat-confirmed"
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

    wurst_marker_rows = [row for row in full_rows if row["field_id"] == WURST_GENERATED_FIELD_ID]
    invalid_wurst_markers = [
        (row["category"], row["rawcode"], row["recovered_value"])
        for row in wurst_marker_rows
        if row["recovered_value"] != WURST_GENERATED_MARKER
    ]
    if invalid_wurst_markers:
        raise ValueError(f"unexpected Wurst generated-object marker values: {invalid_wurst_markers[:10]}")

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

    # Compact normalized item view. Keep all authored map items, including
    # helper/result items that are not directly sold by the Castle shop.
    item_fields = {
        "name": "unam", "description": "ides", "tip": "utip", "ubertip": "utub",
        "gold_cost": "igol", "lumber_cost": "ilum", "class": "icla", "level": "ilev",
        "old_level": "ilvo", "uses": "iuse", "stack_max": "ista", "stock_max": "isto",
        "stock_initial": "isit", "stock_start": "isst", "stock_regen": "istr", "abilities": "iabi",
        "cooldown_group": "icid", "usable": "iusa", "perishable": "iper", "powerup": "ipow",
        "droppable": "idro", "drop_on_death": "idrp", "pawnable": "ipaw", "sellable": "isel",
    }
    item_records = [record for record in object_records if record["category"] == "items"]
    item_rows: list[list[Any]] = []
    items_by_rawcode: dict[str, dict[str, Any]] = {}
    for record in item_records:
        rawcode = str(record["rawcode"])
        values = {
            name: value_as_text(field_lookup(rows_by_object, "items", rawcode, field_id))
            for name, field_id in item_fields.items()
        }
        items_by_rawcode[rawcode] = {
            "table": record["table"],
            "rawcode": rawcode,
            "base_rawcode": record["base_rawcode"],
            **values,
            "protection_conflict_fields": record["protection_conflict_count"],
        }
        item_rows.append([
            record["table"], rawcode, record["base_rawcode"],
            *[values[name] for name in item_fields],
            record["protection_conflict_count"],
        ])
    write_tsv(
        output / "items.tsv",
        ["table", "rawcode", "base_rawcode", *item_fields.keys(), "protection_conflict_fields"],
        item_rows,
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
    defense_type_names = {
        0: "small",
        1: "medium",
        2: "large",
        3: "fort",
        4: "normal",
        5: "hero",
        6: "divine",
        7: "none",
    }

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
                    "armor_type": (
                        defense_type_names[int(row["defense_type"])]
                        if row.get("defense_type", "") != ""
                        else static["armor_type"]
                    ),
                    "move_speed": applied["move_speed"],
                    "attack1_base_damage": applied["attack1_base_damage"],
                    "attack1_dice_number": applied["attack1_dice_number"],
                    "attack1_dice_sides": applied["attack1_dice_sides"],
                    "attack1_cooldown": applied["attack1_cooldown"],
                    "attack1_range": applied["attack1_range"],
                    "attack1_min": value_as_text(attack1_min),
                    "attack1_max": value_as_text(attack1_max),
                    "attack1_avg": value_as_text(attack1_avg),
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

    # Normalize each production unit into explicit weapon profiles. xO is a
    # useful one-number building/AI summary, but it necessarily flattens units
    # with multiple attacks or conditional weapon switches. This table keeps
    # the decoded UnitStat attack primitives beside static targeting/type data
    # and structurally decoded War Club (Agra) attack-index switching.
    protected_unit_catalog: dict[str, dict[str, str]] = {}
    with (output / "protected-unit-stats.tsv").open(encoding="utf-8", newline="") as handle:
        protected_unit_catalog = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    level_one_abilities: dict[str, dict[str, str]] = {}
    with (output / "abilities.tsv").open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            if row["level"] == "1":
                level_one_abilities[row["rawcode"]] = row

    production_attack_rows: list[list[Any]] = []
    production_attack_profiles = 0
    conditional_attack_profiles = 0
    two_profile_sum_patterns = 0
    if effective_unit_path.exists():
        with effective_unit_path.open(encoding="utf-8", newline="") as handle:
            for catalog in csv.DictReader(handle, delimiter="\t"):
                unit = static_units.get(catalog["unit_rawcode"])
                protected = protected_unit_catalog.get(catalog["unit_rawcode"])
                if unit is None or protected is None:
                    raise ValueError(f"production unit has no resolved attack source: {catalog['unit_rawcode']}")

                attack_switches: list[dict[str, Any]] = []
                for ability_rawcode in (part for part in unit["abilities"].split(",") if part):
                    ability = level_one_abilities.get(ability_rawcode)
                    if ability is None or ability["base_rawcode"] != "Agra":
                        continue
                    labeled = json.loads(ability["data_fields_labeled_json"] or "{}")
                    attack_switches.append({
                        "rawcode": ability_rawcode,
                        "name": ability["name"],
                        "disabled": int(labeled.get("Disabled Attack Index", -1)),
                        "enabled": int(labeled.get("Enabled Attack Index", -1)),
                        "maximum_attacks": labeled.get("Maximum Attacks"),
                    })

                known_ranges: list[float] = []
                for attack_index in (1, 2):
                    attack_range = numeric(protected[f"unitstat_attack{attack_index}_range"])
                    if attack_range is not None:
                        known_ranges.append(attack_range)
                xo_range = numeric(catalog["attack_range"])
                sum_pattern = (
                    len(known_ranges) == 2
                    and xo_range is not None
                    and math.isclose(xo_range, sum(known_ranges) - 1.0, rel_tol=0.0, abs_tol=1e-6)
                )
                if sum_pattern:
                    two_profile_sum_patterns += 1

                for attack_index in (1, 2):
                    prefix = f"attack{attack_index}"
                    zero_index = attack_index - 1
                    enabling = [switch for switch in attack_switches if switch["enabled"] == zero_index]
                    disabling = [switch for switch in attack_switches if switch["disabled"] == zero_index]
                    default_enabled = unit[f"{prefix}_enabled"] == "True"
                    activation = "default" if default_enabled else ("conditional" if enabling else "unavailable")
                    if activation != "unavailable":
                        production_attack_profiles += 1
                    if enabling:
                        conditional_attack_profiles += 1

                    range_value = protected[f"unitstat_{prefix}_range"]
                    dps_value = protected[f"unitstat_{prefix}_dps"]
                    range_relation = effective_comparison(catalog["attack_range"], range_value).replace("static-", "xo-")
                    dps_relation = effective_comparison(catalog["dps"], dps_value, tolerance=0.011).replace("static-", "xo-")
                    if sum_pattern and range_relation == "xo-differs":
                        range_relation = "xo-two-profile-sum-minus-one"

                    production_attack_rows.append([
                        catalog["building_rawcode"], catalog["building_names"],
                        catalog["unit_rawcode"], catalog["unit_names"], attack_index,
                        activation, int(default_enabled),
                        ",".join(switch["rawcode"] for switch in enabling),
                        ",".join(switch["name"] for switch in enabling),
                        ",".join(value_as_text(switch["maximum_attacks"]) for switch in enabling),
                        ",".join(switch["rawcode"] for switch in disabling),
                        unit[f"{prefix}_type"], unit[f"{prefix}_weapon_type"], unit[f"{prefix}_targets"],
                        protected[f"unitstat_{prefix}_base_damage"], protected[f"{prefix}_base_damage_source"],
                        protected[f"unitstat_{prefix}_dice_number"], protected[f"{prefix}_dice_number_source"],
                        protected[f"unitstat_{prefix}_dice_sides"], protected[f"{prefix}_dice_sides_source"],
                        protected[f"unitstat_{prefix}_cooldown"], protected[f"{prefix}_cooldown_source"],
                        range_value, protected[f"{prefix}_range_source"],
                        protected[f"unitstat_{prefix}_min"], protected[f"unitstat_{prefix}_max"],
                        protected[f"unitstat_{prefix}_avg"], dps_value,
                        catalog["dps"], dps_relation, catalog["attack_range"], range_relation,
                    ])
    write_tsv(
        output / "production-unit-attacks.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names", "attack_index",
            "activation", "default_enabled", "enabled_by_ability", "enabled_by_ability_name", "conditional_max_attacks",
            "disabled_by_ability", "attack_type", "weapon_type", "targets",
            "base_damage", "base_damage_source", "dice_number", "dice_number_source", "dice_sides", "dice_sides_source",
            "cooldown", "cooldown_source", "range", "range_source", "min_damage", "max_damage", "avg_damage", "dps",
            "xo_effective_dps", "xo_dps_relation", "xo_effective_range", "xo_range_relation",
        ],
        production_attack_rows,
    )

    # Normalize every ability initially attached to a production unit. Most are
    # map objects, but a few are unmodified Blizzard utility abilities (Ghost,
    # Locust, Invulnerable); keep those links explicitly instead of dropping
    # rawcodes that do not appear in the map's ability-object delta table.
    ability_levels: dict[str, list[dict[str, str]]] = defaultdict(list)
    with (output / "abilities.tsv").open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            ability_levels[row["rawcode"]].append(row)

    protected_ability_values: dict[tuple[str, int, str], str] = {}
    with (output / "protected-ability-fields.tsv").open(encoding="utf-8", newline="") as handle:
        for row in csv.DictReader(handle, delimiter="\t"):
            protected_ability_values[(row["rawcode"], int(row["level"]), row["field"])] = row["runtime_value"]

    script_rawcode_summary: dict[str, dict[str, str]] = {}
    script_rawcode_summary_path = map_root / "script" / "rawcode-summary.tsv"
    if script_rawcode_summary_path.exists():
        with script_rawcode_summary_path.open(encoding="utf-8", newline="") as handle:
            script_rawcode_summary = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    ability_metadata = load_metadata("abilities", source_root, tables)

    def inherited_ability_level_one(rawcode: str) -> dict[str, str] | None:
        if table_row(tables, "AbilityData", rawcode) is None:
            return None

        def base_field(field_id: str) -> str:
            meta = ability_metadata.get(field_id)
            if meta is None:
                return ""
            value, _, _ = base_value_for_metadata(
                meta, rawcode, 1, 1, tables, profile, data_selection.profile_variant
            )
            value = resolve_string_key(value, editor_strings)
            return value_as_text(value)

        levels_value = base_field("alev")
        return {
            "table": "inherited",
            "rawcode": rawcode,
            "base_rawcode": rawcode,
            "level": "1",
            "level_count": levels_value or "1",
            "name": base_field("anam"),
            "tip": base_field("atp1"),
            "ubertip": base_field("aub1"),
            "mana_cost": base_field("amcs"),
            "cooldown": base_field("acdn"),
            "range": base_field("aran"),
            "area": base_field("aare"),
            "targets": base_field("atar"),
            "buffs": base_field("abuf"),
            "data_fields_json": "{}",
            "data_fields_labeled_json": "{}",
        }

    production_ability_rows: list[list[Any]] = []
    production_ability_unique: set[str] = set()
    production_ability_inherited_links = 0
    production_ability_runtime_field_links = 0
    if effective_unit_path.exists():
        with effective_unit_path.open(encoding="utf-8", newline="") as handle:
            for catalog in csv.DictReader(handle, delimiter="\t"):
                unit = static_units.get(catalog["unit_rawcode"])
                if unit is None:
                    raise ValueError(f"production unit has no resolved unit definition: {catalog['unit_rawcode']}")
                ability_rawcodes = [part.strip() for part in unit["abilities"].split(",") if part.strip() and part.strip() != "_"]
                for slot, ability_rawcode in enumerate(ability_rawcodes, start=1):
                    definitions = ability_levels.get(ability_rawcode, [])
                    definition = next((row for row in definitions if row["level"] == "1"), None)
                    definition_source = "map-resolved"
                    if definition is None:
                        definition = inherited_ability_level_one(ability_rawcode)
                        definition_source = "inherited-base"
                        production_ability_inherited_links += 1
                    if definition is None:
                        raise ValueError(
                            f"production unit ability has no map or inherited definition: {catalog['unit_rawcode']} {ability_rawcode}"
                        )

                    level_count = len(definitions) if definitions else int(definition.get("level_count", "1") or 1)
                    static_mana = definition["mana_cost"]
                    static_cooldown = definition["cooldown"]
                    runtime_mana = protected_ability_values.get((ability_rawcode, 1, "mana_cost"))
                    runtime_cooldown = protected_ability_values.get((ability_rawcode, 1, "cooldown"))
                    if runtime_mana is not None or runtime_cooldown is not None:
                        production_ability_runtime_field_links += 1
                    effective_mana = runtime_mana if runtime_mana is not None else static_mana
                    effective_cooldown = runtime_cooldown if runtime_cooldown is not None else static_cooldown
                    mana_source = "protected-runtime" if runtime_mana is not None else "static-resolved"
                    cooldown_source = "protected-runtime" if runtime_cooldown is not None else "static-resolved"

                    script = script_rawcode_summary.get(ability_rawcode, {})
                    production_ability_unique.add(ability_rawcode)
                    production_ability_rows.append([
                        catalog["building_rawcode"], catalog["building_names"],
                        catalog["unit_rawcode"], catalog["unit_names"], slot,
                        ability_rawcode, definition_source, definition["base_rawcode"], 1, level_count,
                        definition["name"], definition["tip"], definition["ubertip"],
                        static_mana, effective_mana, mana_source,
                        static_cooldown, effective_cooldown, cooldown_source,
                        definition["range"], definition["area"], definition["targets"], definition["buffs"],
                        definition["data_fields_json"], definition["data_fields_labeled_json"],
                        script.get("reference_count", "0"), script.get("function_count", "0"), script.get("functions", ""),
                    ])
    write_tsv(
        output / "production-unit-abilities.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names", "ability_slot",
            "ability_rawcode", "definition_source", "base_rawcode", "initial_level", "level_count",
            "name", "tip", "ubertip",
            "static_mana_cost", "effective_mana_cost", "mana_cost_source",
            "static_cooldown", "effective_cooldown", "cooldown_source",
            "range", "area", "targets", "buffs", "data_fields_json", "data_fields_labeled_json",
            "script_reference_count", "script_function_count", "script_functions",
        ],
        production_ability_rows,
    )

    unit_spell_rows: list[list[Any]] = []
    unit_spell_by_pair: dict[tuple[str, str], dict[str, str]] = {}
    unit_spell_production_rows = 0
    unit_spell_registration_path = map_root / "script" / "unit-spell-registrations.tsv"
    production_source_by_unit: dict[str, dict[str, str]] = {}
    if effective_unit_path.exists():
        with effective_unit_path.open(encoding="utf-8", newline="") as handle:
            production_source_by_unit = {
                row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")
            }
    production_special_rows: list[list[Any]] = []
    production_special_path = map_root / "script" / "production-unit-special-mechanics.tsv"
    if production_special_path.exists():
        with production_special_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                unit_rawcode = mechanic["unit_rawcode"]
                unit = static_units.get(unit_rawcode)
                if unit is None:
                    raise ValueError(f"special production-unit mechanic has no resolved unit: {unit_rawcode}")
                production = production_source_by_unit.get(unit_rawcode)
                if production is None:
                    raise ValueError(f"special production-unit mechanic is not linked to a production building: {unit_rawcode}")
                parameters = json.loads(mechanic["parameters_json"])

                if unit_rawcode == "e00F":
                    war_club = next((row for row in ability_levels.get("A0BC", []) if row["level"] == "1"), None)
                    if war_club is None:
                        raise ValueError("Mountain Giant special mechanic is missing A0BC object data")
                    war_club_fields = json.loads(war_club["data_fields_labeled_json"])
                    if war_club_fields.get("Maximum Attacks") != 10:
                        raise ValueError(f"Mountain Giant War Club maximum attacks changed: {war_club_fields}")
                    parameters["war_club_object_data"] = war_club_fields
                elif unit_rawcode == "n03I":
                    remnant_activation = next((row for row in ability_levels.get("A0HD", []) if row["level"] == "1"), None)
                    if remnant_activation is None:
                        raise ValueError("Echofoot Mystic special mechanic is missing A0HD object data")
                    parameters["remnant_activation_object_data"] = json.loads(
                        remnant_activation["data_fields_labeled_json"]
                    )
                    protected_cooldown = protected_ability_values.get(("A0H4", 1, "cooldown"))
                    if protected_cooldown is None:
                        raise ValueError("Echofoot Mystic special mechanic is missing protected A0H4 cooldown")
                    parameters["protected_ability_cooldown_seconds"] = numeric(protected_cooldown)
                    parameters["script_proc_cooldown_overrides_protected_ability_cooldown"] = (
                        numeric(protected_cooldown) != numeric(parameters["runtime_cooldown_seconds"])
                    )
                elif unit_rawcode == "h03A":
                    defend = next((row for row in ability_levels.get("A03G", []) if row["level"] == "1"), None)
                    if defend is None:
                        raise ValueError("Defender special mechanic is missing A03G object data")
                    parameters["defend_object_data"] = json.loads(defend["data_fields_labeled_json"])
                elif mechanic["mechanic_kind"] == "damage-triggered-feral-rage-and-hibernation":
                    damage_rawcode = integer_rawcode(int(parameters["feral_rage_damage_ability_id"]))
                    speed_rawcode = integer_rawcode(int(parameters["feral_rage_attack_speed_ability_id"]))
                    regen_rawcode = integer_rawcode(int(parameters["hibernate_regen_ability_id"]))
                    damage_ability = next((row for row in ability_levels.get(damage_rawcode, []) if row["level"] == "1"), None)
                    speed_ability = next((row for row in ability_levels.get(speed_rawcode, []) if row["level"] == "1"), None)
                    regen_ability = next((row for row in ability_levels.get(regen_rawcode, []) if row["level"] == "1"), None)
                    if damage_ability is None or speed_ability is None or regen_ability is None:
                        raise ValueError(f"Bear runtime mechanic is missing linked ability data: {unit_rawcode}")
                    damage_fields = json.loads(damage_ability["data_fields_labeled_json"])
                    speed_fields = json.loads(speed_ability["data_fields_labeled_json"])
                    regen_fields = json.loads(regen_ability["data_fields_labeled_json"])
                    expected_bonus = 0.2 if unit_rawcode == "n029" else 0.3
                    if (
                        numeric(damage_fields.get("Damage Increase (%)")) != expected_bonus
                        or numeric(speed_fields.get("Attack Speed Increase (%)")) != expected_bonus
                    ):
                        raise ValueError(f"Bear Feral Rage object data changed: {unit_rawcode}")
                    damage_duration = numeric(field_lookup(rows_by_object, "abilities", damage_rawcode, "adur", 1, 0))
                    speed_duration = numeric(field_lookup(rows_by_object, "abilities", speed_rawcode, "adur", 1, 0))
                    if damage_duration != speed_duration:
                        raise ValueError(f"Bear Feral Rage buff durations diverged: {unit_rawcode}")
                    parameters["feral_rage_damage_object_data"] = damage_fields
                    parameters["feral_rage_attack_speed_object_data"] = speed_fields
                    parameters["feral_rage_buff_duration_seconds"] = damage_duration
                    parameters["hibernate_regen_object_data"] = regen_fields
                    extra_id = parameters.get("hibernate_extra_sleep_ability_id")
                    if extra_id is not None:
                        extra_rawcode = integer_rawcode(int(extra_id))
                        extra_ability = next((row for row in ability_levels.get(extra_rawcode, []) if row["level"] == "1"), None)
                        if extra_ability is None:
                            raise ValueError(f"Ancient Bear sleep bonus ability missing: {extra_rawcode}")
                        parameters["hibernate_extra_sleep_object_data"] = json.loads(
                            extra_ability["data_fields_labeled_json"]
                        )
                elif mechanic["mechanic_kind"] == "damage-triggered-auto-fan-of-knives":
                    razor = next((row for row in ability_levels.get("A0GL", []) if row["level"] == "1"), None)
                    if razor is None:
                        raise ValueError("Razormane special mechanic is missing A0GL object data")
                    parameters["ability_object_data"] = json.loads(razor["data_fields_labeled_json"])
                    parameters["effective_mana_cost"] = numeric(
                        protected_ability_values.get(("A0GL", 1, "mana_cost"), razor["mana_cost"])
                    )
                    parameters["effective_cooldown_seconds"] = numeric(
                        protected_ability_values.get(("A0GL", 1, "cooldown"), razor["cooldown"])
                    )
                    parameters["area"] = numeric(razor["area"])
                    parameters["targets"] = razor["targets"]
                elif mechanic["mechanic_kind"] == "damage-to-mana-kaboom-charge":
                    kaboom = next((row for row in ability_levels.get("A0EN", []) if row["level"] == "1"), None)
                    if kaboom is None:
                        raise ValueError("Greater Wind special mechanic is missing A0EN object data")
                    parameters["kaboom_object_data"] = json.loads(kaboom["data_fields_labeled_json"])
                    parameters["kaboom_targets"] = kaboom["targets"]
                elif mechanic["mechanic_kind"] == "attack-stacking-corrosion":
                    stack_levels = []
                    for level in (1, 2, 3):
                        stack = next((row for row in ability_levels.get("A0C9", []) if row["level"] == str(level)), None)
                        if stack is None:
                            raise ValueError(f"Emerald Dragon corrosion stack level missing: {level}")
                        stack_fields = json.loads(stack["data_fields_labeled_json"])
                        spell_list = [part.strip() for part in str(stack_fields.get("Spell List", "")).split(",") if part.strip()]
                        if len(spell_list) != 2:
                            raise ValueError(f"Emerald Dragon corrosion spell list changed at level {level}: {stack_fields}")
                        armor = next((row for row in ability_levels.get(spell_list[0], []) if row["level"] == "1"), None)
                        if armor is None:
                            raise ValueError(f"Emerald Dragon armor state missing: {spell_list[0]}")
                        armor_fields = json.loads(armor["data_fields_labeled_json"])
                        stack_levels.append({
                            "level": level,
                            "spell_list": spell_list,
                            "armor_ability_rawcode": spell_list[0],
                            "defense_bonus": numeric(armor_fields.get("Defense Bonus")),
                        })
                    if [row["defense_bonus"] for row in stack_levels] != [-2, -4, -6]:
                        raise ValueError(f"Emerald Dragon corrosion armor states changed: {stack_levels}")
                    parameters["stack_levels"] = stack_levels
                elif mechanic["mechanic_kind"] == "attack-proc-native-mirror-image":
                    mirror = next((row for row in ability_levels.get("A0CU", []) if row["level"] == "1"), None)
                    if mirror is None:
                        raise ValueError("Greater Water special mechanic is missing A0CU object data")
                    parameters["mirror_image_object_data"] = json.loads(mirror["data_fields_labeled_json"])
                    parameters["mirror_image_duration_seconds"] = numeric(
                        field_lookup(rows_by_object, "abilities", "A0CU", "adur", 1, 0)
                    )
                    parameters["mirror_image_effective_mana_cost"] = numeric(
                        protected_ability_values.get(("A0CU", 1, "mana_cost"), mirror["mana_cost"])
                    )
                    parameters["mirror_image_cooldown_seconds"] = numeric(
                        protected_ability_values.get(("A0CU", 1, "cooldown"), mirror["cooldown"])
                    )
                elif mechanic["mechanic_kind"] == "source-damage-mastery-over-death":
                    decay = next((row for row in ability_levels.get("A086", []) if row["level"] == "1"), None)
                    if decay is None:
                        raise ValueError("Lich King special mechanic is missing A086 object data")
                    decay_fields = json.loads(decay["data_fields_labeled_json"])
                    if (
                        numeric(decay_fields.get("Max Life Drained per Second (%)")) != 0.08
                        or numeric(decay_fields.get("Building Reduction")) != 0.1
                    ):
                        raise ValueError(f"Lich King Death and Decay fields changed: {decay_fields}")
                    parameters["death_and_decay_object_data"] = decay_fields
                    parameters["death_and_decay_duration_seconds"] = numeric(
                        field_lookup(rows_by_object, "abilities", "A086", "adur", 1, 0)
                    )
                    parameters["death_and_decay_area"] = numeric(decay["area"])
                    parameters["death_and_decay_targets"] = decay["targets"]
                    parameters["death_and_decay_effective_mana_cost"] = numeric(
                        protected_ability_values.get(("A086", 1, "mana_cost"), decay["mana_cost"])
                    )
                    parameters["death_and_decay_effective_cooldown_seconds"] = numeric(
                        protected_ability_values.get(("A086", 1, "cooldown"), decay["cooldown"])
                    )
                elif mechanic["mechanic_kind"] == "source-damage-stacking-blood-corrosion":
                    stack_levels = []
                    for level in range(1, 6):
                        stack = next((row for row in ability_levels.get("A0GY", []) if row["level"] == str(level)), None)
                        if stack is None:
                            raise ValueError(f"Vampire Lord Blood Corrosion level missing: {level}")
                        fields = json.loads(stack["data_fields_labeled_json"])
                        stack_levels.append({
                            "level": level,
                            "defense_bonus": numeric(fields.get("Defense Bonus")),
                        })
                    if [row["defense_bonus"] for row in stack_levels] != [-2, -4, -6, -8, -10]:
                        raise ValueError(f"Vampire Lord Blood Corrosion states changed: {stack_levels}")
                    parameters["stack_levels"] = stack_levels
                elif mechanic["mechanic_kind"] == "source-damage-health-scaled-aftershock":
                    object_level = numeric(field_lookup(rows_by_object, "units", unit_rawcode, "ulev", 0, 0))
                    if object_level != numeric(parameters["unit_level"]):
                        raise ValueError(
                            f"Earth Elemental runtime level changed: {unit_rawcode} object={object_level} "
                            f"script={parameters['unit_level']}"
                        )
                    expected_max = 50 * int(parameters["unit_level"])
                    if numeric(parameters["maximum_bonus_damage_at_full_hp"]) != expected_max:
                        raise ValueError(f"Earth Elemental Aftershock maximum changed: {unit_rawcode}")
                    parameters["object_unit_level"] = object_level
                elif mechanic["mechanic_kind"] == "target-damage-melee-thunderbolt-retaliation":
                    level = int(parameters["unit_level"])
                    object_level = numeric(field_lookup(rows_by_object, "units", unit_rawcode, "ulev", 0, 0))
                    if object_level != level:
                        raise ValueError(
                            f"Lightning Elemental runtime level changed: {unit_rawcode} object={object_level} script={level}"
                        )
                    thunderbolt = next(
                        (row for row in ability_levels.get("A0D5", []) if row["level"] == str(level)),
                        None,
                    )
                    if thunderbolt is None:
                        raise ValueError(f"Lightning retaliation ability level missing: A0D5 level {level}")
                    thunderbolt_fields = json.loads(thunderbolt["data_fields_labeled_json"])
                    expected_damage = 25 * level
                    if numeric(thunderbolt_fields.get("Damage")) != expected_damage:
                        raise ValueError(f"Lightning retaliation damage changed at level {level}: {thunderbolt_fields}")
                    parameters["object_unit_level"] = object_level
                    parameters["thunderbolt_object_data"] = thunderbolt_fields
                    parameters["thunderbolt_duration_seconds"] = numeric(
                        field_lookup(rows_by_object, "abilities", "A0D5", "adur", level, 0)
                    )
                    parameters["thunderbolt_range"] = numeric(thunderbolt["range"])
                    parameters["thunderbolt_targets"] = thunderbolt["targets"]
                    parameters["thunderbolt_effective_mana_cost"] = numeric(
                        protected_ability_values.get(("A0D5", level, "mana_cost"), thunderbolt["mana_cost"])
                    )
                    parameters["thunderbolt_effective_cooldown_seconds"] = numeric(
                        protected_ability_values.get(("A0D5", level, "cooldown"), thunderbolt["cooldown"])
                    )
                elif mechanic["mechanic_kind"] == "attack-damage-flying-target-multiplier":
                    if unit_rawcode != "n03U":
                        raise ValueError(f"Riptide air-bonus mechanic attached to unexpected unit: {unit_rawcode}")
                    if "45% extra damage to air units" not in unit["ubertip"]:
                        raise ValueError("Winged Riptide Serpent tooltip air-bonus text changed")
                    parameters["unit_tooltip"] = unit["ubertip"]
                elif mechanic["mechanic_kind"] == "damage-triggered-persistent-low-hp-attack-bonus":
                    levels: list[dict[str, Any]] = []
                    for level in (1, 2, 3):
                        bonus = next((row for row in ability_levels.get("A0HR", []) if row["level"] == str(level)), None)
                        if bonus is None:
                            raise ValueError(f"Forest Troll hidden damage-bonus level missing: {level}")
                        fields = json.loads(bonus["data_fields_labeled_json"])
                        levels.append({"level": level, "attack_bonus": numeric(fields.get("Attack Bonus"))})
                    if [row["attack_bonus"] for row in levels] != [30, 60, 90]:
                        raise ValueError(f"Forest Troll hidden damage-bonus levels changed: {levels}")
                    parameters["resolved_bonus_levels"] = levels
                    for threshold in parameters["thresholds"]:
                        level = int(threshold["ability_level"])
                        threshold["attack_bonus"] = levels[level - 1]["attack_bonus"]
                    parameters["bonus_ability_rawcode"] = "A0HR"
                    parameters["bonus_ability_name"] = ability_levels["A0HR"][0]["name"]
                    parameters["unit_tooltip"] = unit["ubertip"]
                    parameters["mechanic_absent_from_unit_tooltip"] = "Troll Damage Bonus" not in unit["ubertip"]
                elif mechanic["mechanic_kind"] == "attack-proc-ground-whirlwind-aoe":
                    marker = next((row for row in ability_levels.get("A0HG", []) if row["level"] == "1"), None)
                    if marker is None:
                        raise ValueError("Ironpaw Whirlwind marker A0HG is missing")
                    if numeric(parameters["damage"]) != 175 or "150 damage" not in unit["ubertip"]:
                        raise ValueError("Ironpaw Whirlwind runtime/tooltip damage evidence changed")
                    parameters["marker_ability_rawcode"] = "A0HG"
                    parameters["marker_ability_name"] = marker["name"]
                    parameters["marker_ability_object_data"] = json.loads(marker["data_fields_labeled_json"])
                    parameters["unit_tooltip"] = unit["ubertip"]
                elif mechanic["mechanic_kind"] == "summon-event-mana-reset":
                    parameters["static_object_mana_start"] = numeric(unit["mana_start"])
                    parameters["mana_max"] = numeric(unit["mana_max"])
                elif mechanic["mechanic_kind"] == "train-event-random-initial-mana":
                    mine_spell = next((row for row in ability_levels.get("A095", []) if row["level"] == "1"), None)
                    if mine_spell is None:
                        raise ValueError("Mine Layer train-time mana mechanic is missing A095 object data")
                    effective_cost = numeric(
                        protected_ability_values.get(("A095", 1, "mana_cost"), mine_spell["mana_cost"])
                    )
                    if effective_cost != 25:
                        raise ValueError(f"Mine Layer spell cost changed: {effective_cost}")
                    parameters["static_object_mana_start"] = numeric(unit["mana_start"])
                    parameters["mana_max"] = numeric(unit["mana_max"])
                    parameters["mine_spell_effective_mana_cost"] = effective_cost
                elif mechanic["mechanic_kind"] == "train-event-set-exploded-flag":
                    explosion = next((row for row in ability_levels.get("A0FZ", []) if row["level"] == "1"), None)
                    if explosion is None:
                        raise ValueError("Goblin Rocketeer exploded-flag mechanic is missing A0FZ object data")
                    explosion_fields = json.loads(explosion["data_fields_labeled_json"])
                    expected_explosion = {
                        "Full Damage Amount": 325,
                        "Full Damage Radius": 160,
                        "Partial Damage Amount": 175,
                        "Partial Damage Radius": 320,
                    }
                    if explosion_fields != expected_explosion:
                        raise ValueError(f"Goblin Rocketeer death explosion fields changed: {explosion_fields}")
                    parameters["death_explosion_ability_rawcode"] = "A0FZ"
                    parameters["death_explosion_base_rawcode"] = explosion["base_rawcode"]
                    parameters["death_explosion_object_data"] = explosion_fields
                    parameters["death_explosion_targets"] = explosion["targets"]
                elif mechanic["mechanic_kind"] == "kill-triggered-native-berserk":
                    berserk = next((row for row in ability_levels.get("A02I", []) if row["level"] == "1"), None)
                    if berserk is None:
                        raise ValueError("Troll-family kill mechanic is missing A02I object data")
                    berserk_fields = json.loads(berserk["data_fields_labeled_json"])
                    if (
                        numeric(berserk_fields.get("Attack Speed Increase")) != 2
                        or numeric(berserk_fields.get("Movement Speed Increase")) != 0.25
                        or numeric(berserk_fields.get("Damage Taken Increase")) != 0.1
                    ):
                        raise ValueError(f"Troll-family Berserk parameters changed: {berserk_fields}")
                    parameters["berserk_object_data"] = berserk_fields
                    parameters["berserk_duration_seconds"] = numeric(
                        field_lookup(rows_by_object, "abilities", "A02I", "adur", 1, 0)
                    )
                    parameters["berserk_effective_cooldown_seconds"] = numeric(
                        protected_ability_values.get(("A02I", 1, "cooldown"), berserk["cooldown"])
                    )
                    parameters["berserk_base_order"] = value_as_text(
                        field_lookup(rows_by_object, "abilities", "A02I", "aord", 0, 0)
                    )
                elif unit_rawcode == "u00F":
                    split = next((row for row in ability_levels.get("A0DY", []) if row["level"] == "1"), None)
                    if split is None:
                        raise ValueError("Greater Fire Elemental special mechanic is missing A0DY object data")
                    split_fields = json.loads(split["data_fields_labeled_json"])
                    split_child_rawcode = value_as_text(
                        field_lookup(rows_by_object, "abilities", "A0DY", "Hwe1", 1, 0)
                    )
                    if split_child_rawcode != "h030":
                        raise ValueError(f"Greater Fire Elemental split child changed: {split_child_rawcode}")
                    if split_fields.get("Split Attack Count") != 15 or split_fields.get("Generation Count") != 3:
                        raise ValueError(f"Greater Fire Elemental split parameters changed: {split_fields}")
                    parameters["native_split_base_ability_rawcode"] = split["base_rawcode"]
                    parameters["native_split_object_data"] = split_fields
                    parameters["native_split_summoned_unit_rawcode"] = split_child_rawcode
                    parameters["native_split_targets_allowed"] = split["targets"]
                    parameters["native_split_effective_cooldown"] = numeric(
                        protected_ability_values.get(("A0DY", 1, "cooldown"), split["cooldown"])
                    )
                    parameters["native_split_effective_mana_cost"] = numeric(
                        protected_ability_values.get(("A0DY", 1, "mana_cost"), split["mana_cost"])
                    )

                production_special_rows.append([
                    production["building_rawcode"], production["building_names"],
                    unit_rawcode, unit["name"], mechanic["mechanic_kind"], mechanic["trigger"],
                    mechanic["related_objects_json"], stable_json(parameters),
                    mechanic["source_functions"], mechanic["evidence_kind"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "production-unit-special-mechanics.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names",
            "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ],
        production_special_rows,
    )

    perk_mechanic_rows: list[list[Any]] = []
    perk_mechanics_path = map_root / "script" / "perk-mechanics.tsv"
    if perk_mechanics_path.exists():
        def perk_ability_snapshot(rawcode: str) -> dict[str, Any]:
            ability = next((row for row in ability_levels.get(rawcode, []) if row["level"] == "1"), None)
            if ability is None:
                raise ValueError(f"perk mechanic is missing ability level 1: {rawcode}")
            return {
                "rawcode": rawcode,
                "name": ability["name"],
                "tip": ability["tip"],
                "ubertip": ability["ubertip"],
                "base_rawcode": ability["base_rawcode"],
                "mana_cost": numeric(protected_ability_values.get((rawcode, 1, "mana_cost"), ability["mana_cost"])),
                "cooldown": numeric(protected_ability_values.get((rawcode, 1, "cooldown"), ability["cooldown"])),
                "range": numeric(ability["range"]),
                "area": numeric(ability["area"]),
                "targets": ability["targets"],
                "buffs": ability["buffs"],
                "object_data": json.loads(ability["data_fields_labeled_json"]),
            }

        def perk_item_snapshot(rawcode: str) -> dict[str, Any]:
            item = items_by_rawcode.get(rawcode)
            if item is None:
                raise ValueError(f"perk mechanic is missing item definition: {rawcode}")
            return {
                "rawcode": rawcode,
                "name": item["name"],
                "description": item["description"],
                "tip": item["tip"],
                "ubertip": item["ubertip"],
                "uses": numeric(item["uses"]),
                "stack_max": numeric(item["stack_max"]),
                "abilities": item["abilities"],
                "usable": item["usable"],
                "perishable": item["perishable"],
                "droppable": item["droppable"],
            }

        def perk_unit_snapshot(rawcode: str) -> dict[str, Any]:
            unit = static_units.get(rawcode)
            if unit is None:
                raise ValueError(f"perk mechanic is missing unit definition: {rawcode}")
            runtime = protected_unit_applied.get(rawcode, {})
            return {
                "rawcode": rawcode,
                "name": unit["name"],
                "hp": numeric(runtime.get("hp", unit["hp"])),
                "armor": numeric(runtime.get("armor", unit["armor"])),
                "armor_type": runtime.get("armor_type", unit["armor_type"]),
                "move_speed": numeric(runtime.get("move_speed", unit["move_speed"])),
                "attack1_min": numeric(runtime.get("attack1_min", unit["attack1_min"])),
                "attack1_max": numeric(runtime.get("attack1_max", unit["attack1_max"])),
                "attack1_dps": numeric(runtime.get("dps", unit["attack1_dps"])),
                "attack1_cooldown": numeric(runtime.get("attack1_cooldown", unit["attack1_cooldown"])),
                "attack1_range": numeric(runtime.get("attack1_range", unit["attack1_range"])),
                "attack1_type": unit["attack1_type"],
                "attack1_targets": unit["attack1_targets"],
                "abilities": unit["abilities"],
            }

        with perk_mechanics_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                parameters = json.loads(mechanic["parameters_json"])
                perk_id = mechanic["perk_id"]
                if perk_id == "perk_02":
                    parameters["portable_cloud_item"] = perk_item_snapshot("IcfS")
                    parameters["portable_cloud_effect_ability"] = perk_ability_snapshot("AcfS")
                elif perk_id == "perk_03":
                    parameters["bird_food_item"] = perk_item_snapshot("IM01")
                    parameters["bird_unit"] = perk_unit_snapshot("x002")
                elif perk_id == "perk_04":
                    parameters["towerless_item"] = perk_item_snapshot("IM02")
                    parameters["tiny_tower_unit"] = perk_unit_snapshot("h081")
                    parameters["tiny_tower_build_ability"] = perk_ability_snapshot("AM04")
                    parameters["initializer_related_multishot_ability"] = perk_ability_snapshot("AM05")
                elif perk_id == "perk_09":
                    parameters["toggle_abilities"] = [
                        perk_ability_snapshot("AM06"), perk_ability_snapshot("AM07")
                    ]
                elif perk_id == "perk_12":
                    parameters["armor_abilities"] = [
                        perk_ability_snapshot("AM08"), perk_ability_snapshot("AM09")
                    ]
                elif perk_id == "perk_14":
                    parameters["enchantment_abilities"] = [
                        perk_ability_snapshot("AM0a"), perk_ability_snapshot("AM0b"), perk_ability_snapshot("AM0c")
                    ]
                elif perk_id == "perk_17":
                    parameters["enchantment_abilities"] = [
                        perk_ability_snapshot("AM0d"), perk_ability_snapshot("AM0e")
                    ]

                perk_mechanic_rows.append([
                    mechanic["perk_id"], mechanic["perk_name"], mechanic["protected_registry_slot"],
                    mechanic["mechanic_kind"], mechanic["trigger"], mechanic["related_objects_json"],
                    stable_json(parameters), mechanic["source_functions"], mechanic["evidence_kind"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "perk-mechanics.tsv",
        [
            "perk_id", "perk_name", "protected_registry_slot", "mechanic_kind", "trigger",
            "related_objects_json", "parameters_json", "source_functions", "evidence_kind", "byte_offset",
        ],
        perk_mechanic_rows,
    )

    runtime_ai_rows: list[list[Any]] = []
    runtime_ai_path = map_root / "script" / "runtime-ai-mechanics.tsv"
    if runtime_ai_path.exists():
        with runtime_ai_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                system_id = mechanic["system_id"]
                parameters = json.loads(mechanic["parameters_json"])
                if system_id == "ai-rescue-strike-controller":
                    ability = next((row for row in ability_levels.get("A005", []) if row["level"] == "1"), None)
                    if ability is None:
                        raise ValueError("AI Rescue Strike runtime mechanic is missing A005 level 1")
                    effective_mana = numeric(protected_ability_values.get(("A005", 1, "mana_cost"), ability["mana_cost"]))
                    effective_cooldown = numeric(protected_ability_values.get(("A005", 1, "cooldown"), ability["cooldown"]))
                    ability_area = numeric(ability["area"])
                    ability_range = numeric(ability["range"])
                    if effective_mana != 0 or effective_cooldown != 60:
                        raise ValueError(
                            f"AI Rescue Strike protected ability fields changed: mana={effective_mana} cooldown={effective_cooldown}"
                        )
                    if ability_area != 700 or ability_range != 1000:
                        raise ValueError(
                            f"AI Rescue Strike object geometry changed: area={ability_area} range={ability_range}"
                        )
                    parameters["rescue_strike_ability"] = {
                        "rawcode": "A005",
                        "name": ability["name"],
                        "tip": ability["tip"],
                        "ubertip": ability["ubertip"],
                        "base_rawcode": ability["base_rawcode"],
                        "effective_mana_cost": effective_mana,
                        "effective_cooldown_seconds": effective_cooldown,
                        "static_mana_cost": numeric(ability["mana_cost"]),
                        "static_cooldown_seconds": numeric(ability["cooldown"]),
                        "range": ability_range,
                        "area": ability_area,
                        "targets": ability["targets"],
                        "buffs": ability["buffs"],
                        "object_data": json.loads(ability["data_fields_labeled_json"]),
                    }
                elif system_id != "ai-engagement-damage-signals":
                    raise ValueError(f"unrecognized runtime AI mechanic: {system_id}")

                runtime_ai_rows.append([
                    system_id, mechanic["mechanic_kind"], mechanic["trigger"],
                    mechanic["related_objects_json"], stable_json(parameters),
                    mechanic["source_functions"], mechanic["evidence_kind"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "runtime-ai-mechanics.tsv",
        [
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ],
        runtime_ai_rows,
    )

    runtime_system_rows: list[list[Any]] = []
    runtime_system_path = map_root / "script" / "runtime-system-mechanics.tsv"
    if runtime_system_path.exists():
        with runtime_system_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                system_id = mechanic["system_id"]
                parameters = json.loads(mechanic["parameters_json"])

                def ability_level_one(rawcode: str) -> dict[str, str]:
                    row = next((entry for entry in ability_levels.get(rawcode, []) if entry["level"] == "1"), None)
                    if row is None:
                        raise ValueError(f"runtime system {system_id} is missing ability level 1: {rawcode}")
                    return row

                if system_id == "power-plant-power-surge":
                    power_plant = static_units.get("h09T")
                    if power_plant is None:
                        raise ValueError("Power Plant runtime system is missing h09T")
                    armor_aura = ability_level_one("A09S")
                    mana_aura = ability_level_one("A09P")
                    damage_aura = ability_level_one("A09V")
                    hp_stack = next((row for row in ability_levels.get("A09M", []) if row["level"] == "4"), None)
                    armor_bonus = ability_level_one("A09Q")
                    damage_bonus = ability_level_one("A09R")
                    spell_resist = ability_level_one("A0HV")
                    if hp_stack is None:
                        raise ValueError("Power Armor HP stacking is missing A09M level 4")
                    armor_fields = json.loads(armor_aura["data_fields_labeled_json"])
                    mana_fields = json.loads(mana_aura["data_fields_labeled_json"])
                    tower_fields = json.loads(damage_aura["data_fields_labeled_json"])
                    hp_fields = json.loads(hp_stack["data_fields_labeled_json"])
                    spawn_armor_fields = json.loads(armor_bonus["data_fields_labeled_json"])
                    spawn_damage_fields = json.loads(damage_bonus["data_fields_labeled_json"])
                    spell_fields = json.loads(spell_resist["data_fields_labeled_json"])
                    if numeric(armor_fields.get("Armor Bonus")) != 2 or numeric(armor_aura["area"]) != 180:
                        raise ValueError(f"Power Plant armor aura changed: {armor_aura}")
                    if numeric(mana_fields.get("Mana Regeneration Increase")) != 0.2 or numeric(mana_aura["area"]) != 180:
                        raise ValueError(f"Power Plant mana aura changed: {mana_aura}")
                    if numeric(tower_fields.get("Attack Damage Increase")) != 0.25 or numeric(damage_aura["area"]) != 180:
                        raise ValueError(f"Power Plant tower aura changed: {damage_aura}")
                    if numeric(hp_fields.get("Max Life Gained")) != -150:
                        raise ValueError(f"Power Armor A09M level-4 HP trick changed: {hp_fields}")
                    if numeric(spawn_armor_fields.get("Defense Bonus")) != 4:
                        raise ValueError(f"Power Armor spawn armor bonus changed: {spawn_armor_fields}")
                    if numeric(spawn_damage_fields.get("Attack Damage Increase")) != 0.2:
                        raise ValueError(f"Power Armor spawn damage bonus changed: {spawn_damage_fields}")
                    if numeric(spell_fields.get("Damage Reduction")) != 0.15:
                        raise ValueError(f"Power Armor spawn spell resistance changed: {spell_fields}")
                    parameters["power_plant_name"] = power_plant["name"]
                    parameters["power_plant_tooltip"] = power_plant["ubertip"]
                    parameters["building_armor_aura_object_data"] = armor_fields
                    parameters["mana_regen_aura_object_data"] = mana_fields
                    parameters["tower_damage_aura_object_data"] = tower_fields
                    parameters["spawn_hp_stack_object_data_at_level4"] = hp_fields
                    parameters["spawn_hp_bonus_technique"] = "add-A09M,set-level-4(-150-max-life),remove-A09M"
                    parameters["spawn_armor_bonus_object_data"] = spawn_armor_fields
                    parameters["spawn_damage_bonus_object_data"] = spawn_damage_fields
                    parameters["spawn_spell_resist_object_data"] = spell_fields
                elif system_id == "heroic-shrine-companion-spawning":
                    shrine = static_units.get("h05G")
                    carrier = static_units.get("e00E")
                    weeper = static_units.get("n02K")
                    smiley = static_units.get("n02L")
                    if None in (shrine, carrier, weeper, smiley):
                        raise ValueError("Heroic Shrine runtime system is missing shrine/twins object data")
                    tooltip = str(shrine["ubertip"])
                    if "16" not in tooltip or "32" not in tooltip:
                        raise ValueError(f"Heroic Shrine tooltip no longer advertises 16-32%: {tooltip}")
                    if int(parameters["per_shrine_actual_probability_percent"]) != 17:
                        raise ValueError("Heroic Shrine runtime probability changed")
                    parameters["heroic_shrine_name"] = shrine["name"]
                    parameters["heroic_shrine_tooltip"] = tooltip
                    parameters["twins_carrier_name"] = carrier["name"]
                    parameters["twins_replacement_units"] = [
                        {"rawcode": "n02K", "name": weeper["name"]},
                        {"rawcode": "n02L", "name": smiley["name"]},
                    ]
                elif system_id == "golden-shrine-revival":
                    shrine = static_units.get("h059")
                    if shrine is None:
                        raise ValueError("Golden Shrine runtime system is missing h059")
                    tooltip = str(shrine["ubertip"])
                    if "20" not in tooltip or "40" not in tooltip:
                        raise ValueError(f"Golden Shrine tooltip no longer advertises 20-40%: {tooltip}")
                    parameters["golden_shrine_name"] = shrine["name"]
                    parameters["golden_shrine_tooltip"] = tooltip
                elif system_id == "blood-fiend-randomization":
                    carrier = static_units.get("n00L")
                    production = production_source_by_unit.get("n00L")
                    if carrier is None or production is None:
                        raise ValueError("Blood Fiend runtime system is missing production carrier/building")
                    if "A07J" not in rawcode_list(carrier["abilities"]):
                        raise ValueError("Blood Fiend carrier no longer has A07J randomization marker")
                    body_rows: list[dict[str, Any]] = []
                    body_signature: tuple[Any, ...] | None = None
                    for body in parameters["body_distribution"]:
                        rawcode = integer_rawcode(int(body["unit_id"]))
                        unit = static_units.get(rawcode)
                        protected = protected_unit_applied.get(rawcode)
                        if unit is None or protected is None:
                            raise ValueError(f"Blood Fiend body is missing protected runtime unit data: {rawcode}")
                        signature = (
                            numeric(protected["hp"]), numeric(protected["armor"]), numeric(protected["move_speed"]),
                            numeric(protected["attack1_base_damage"]), numeric(protected["attack1_dice_number"]),
                            numeric(protected["attack1_dice_sides"]), numeric(protected["attack1_cooldown"]),
                            numeric(protected["attack1_range"]),
                        )
                        if body_signature is None:
                            body_signature = signature
                        elif signature != body_signature:
                            raise ValueError(f"Blood Fiend body core combat stats diverged: {rawcode} {signature} != {body_signature}")
                        body_rows.append({
                            "rawcode": rawcode,
                            "name": unit["name"],
                            "probability_percent": body["probability_percent"],
                            "hp": numeric(protected["hp"]),
                            "armor": numeric(protected["armor"]),
                            "armor_type": protected["armor_type"],
                            "move_speed": numeric(protected["move_speed"]),
                            "attack1_base_damage": numeric(protected["attack1_base_damage"]),
                            "attack1_dice_number": numeric(protected["attack1_dice_number"]),
                            "attack1_dice_sides": numeric(protected["attack1_dice_sides"]),
                            "attack1_min": numeric(protected["attack1_min"]),
                            "attack1_max": numeric(protected["attack1_max"]),
                            "attack1_avg": numeric(protected["attack1_avg"]),
                            "attack1_cooldown": numeric(protected["attack1_cooldown"]),
                            "attack1_range": numeric(protected["attack1_range"]),
                            "attack1_dps": numeric(protected["dps"]),
                        })
                    resolved_groups: list[dict[str, Any]] = []
                    for group in parameters["trait_groups"]:
                        outcomes: list[dict[str, Any]] = []
                        for outcome in group["outcomes"]:
                            ability_id = outcome["ability_id"]
                            if ability_id is None:
                                outcomes.append({
                                    "ability_rawcode": None,
                                    "ability_name": None,
                                    "probability_percent": outcome["probability_percent"],
                                })
                                continue
                            ability_rawcode = integer_rawcode(int(ability_id))
                            ability = ability_level_one(ability_rawcode)
                            outcomes.append({
                                "ability_rawcode": ability_rawcode,
                                "ability_name": ability["name"],
                                "base_rawcode": ability["base_rawcode"],
                                "probability_percent": outcome["probability_percent"],
                                "object_data": json.loads(ability["data_fields_labeled_json"]),
                            })
                        if sum(float(outcome["probability_percent"]) for outcome in outcomes) != 100:
                            raise ValueError(f"Blood Fiend trait group probabilities no longer sum to 100: {group['group']}")
                        resolved_groups.append({"group": group["group"], "outcomes": outcomes})
                    if sum(float(row["probability_percent"]) for row in body_rows) != 100:
                        raise ValueError("Blood Fiend body probabilities no longer sum to 100")
                    parameters["production_building_rawcode"] = production["building_rawcode"]
                    parameters["production_building_names"] = production["building_names"]
                    parameters["production_carrier_rawcode"] = "n00L"
                    parameters["production_carrier_name"] = carrier["name"]
                    parameters["resolved_body_distribution"] = body_rows
                    parameters["resolved_trait_groups"] = resolved_groups
                elif system_id == "elemental-linker-death-heal":
                    linker = static_units.get("h063")
                    marker = ability_level_one("A0F5")
                    if linker is None:
                        raise ValueError("Elemental Linker runtime system is missing h063")
                    tooltip = str(linker["ubertip"])
                    if "17.5" not in tooltip or "70" not in tooltip:
                        raise ValueError(f"Elemental Linker tooltip no longer advertises 17.5/70%: {tooltip}")
                    if (
                        numeric(parameters["heal_per_owned_elemental_production_building"]) != 17
                        or numeric(parameters["maximum_heal"]) != 500
                        or numeric(parameters["heal_radius"]) != 350
                        or numeric(parameters["other_unit_type_heal_factor"]) != 0.2
                    ):
                        raise ValueError(f"Elemental Linker runtime formula changed: {parameters}")
                    bucket_path = map_root / "script" / "element-building-buckets.tsv"
                    if not bucket_path.exists():
                        raise ValueError("Elemental Linker runtime system is missing script element-building buckets")
                    with bucket_path.open(encoding="utf-8", newline="") as bucket_handle:
                        bucket_rows = list(csv.DictReader(bucket_handle, delimiter="\t"))
                    bucket_ids = sorted({int(row["bucket"]) for row in bucket_rows})
                    if bucket_ids != [1, 2, 3, 4, 5]:
                        raise ValueError(f"Elemental Linker building buckets changed: {bucket_ids}")
                    parameters["linker_name"] = linker["name"]
                    parameters["linker_tooltip"] = tooltip
                    parameters["elemental_marker_ability_rawcode"] = "A0F5"
                    parameters["elemental_marker_ability_name"] = marker["name"]
                    parameters["resolved_elemental_building_count"] = len(bucket_rows)
                    parameters["resolved_elemental_building_buckets"] = bucket_ids
                elif system_id == "treasure-box-income-multiplier":
                    treasure = static_units.get("h008")
                    if treasure is None:
                        raise ValueError("Treasure Box runtime system is missing h008")
                    tooltip = str(treasure["ubertip"])
                    if "25" not in tooltip or "15" not in tooltip:
                        raise ValueError(f"Treasure Box tooltip no longer advertises 25%/15%: {tooltip}")
                    expected_table = [0.0, 1.0, 1.85, 2.57, 3.18, 3.7, 4.14, 4.52, 4.84, 5.11]
                    expected_multipliers = [1.0, 1.25, 1.4625, 1.6425, 1.795, 1.925, 2.035, 2.13, 2.21, 2.2775]
                    if [numeric(value) for value in parameters["multiplier_table_indexes_0_through_9"]] != expected_table:
                        raise ValueError(f"Treasure Box multiplier table changed: {parameters}")
                    if [numeric(value) for value in parameters["multipliers_0_through_9"]] != expected_multipliers:
                        raise ValueError(f"Treasure Box derived multipliers changed: {parameters}")
                    if numeric(parameters["count_10_plus_marginal_multiplier_per_box"]) != 0.0625:
                        raise ValueError(f"Treasure Box linear-tail increment changed: {parameters}")
                    parameters["treasure_box_name"] = treasure["name"]
                    parameters["treasure_box_tooltip"] = tooltip
                elif system_id == "human-artillery-auto-bombardment":
                    artillery = static_units.get("h001")
                    protected = protected_unit_applied.get("h001")
                    burning_oil = ability_level_one("A02K")
                    if artillery is None or protected is None:
                        raise ValueError("Human Artillery runtime system is missing h001 protected stats")
                    if (
                        numeric(protected["hp"]) != 700
                        or numeric(protected["attack1_min"]) != 300
                        or numeric(protected["attack1_max"]) != 400
                        or numeric(protected["attack1_cooldown"]) != 15
                        or numeric(protected["attack1_range"]) != 99999
                    ):
                        raise ValueError(f"Human Artillery protected weapon profile changed: {protected}")
                    parameters["artillery_name"] = artillery["name"]
                    parameters["artillery_tooltip"] = artillery["ubertip"]
                    parameters["runtime_hp"] = numeric(protected["hp"])
                    parameters["attack_type"] = artillery["attack1_type"]
                    parameters["weapon_type"] = artillery["attack1_weapon_type"]
                    parameters["attack_min"] = numeric(protected["attack1_min"])
                    parameters["attack_max"] = numeric(protected["attack1_max"])
                    parameters["attack_average"] = numeric(protected["attack1_avg"])
                    parameters["native_attack_cooldown_seconds"] = numeric(protected["attack1_cooldown"])
                    parameters["attack_range"] = numeric(protected["attack1_range"])
                    parameters["full_aoe_radius"] = numeric(artillery["attack1_full_aoe"])
                    parameters["half_aoe_radius"] = numeric(artillery["attack1_half_aoe"])
                    parameters["quarter_aoe_radius"] = numeric(artillery["attack1_quarter_aoe"])
                    parameters["half_aoe_damage_factor"] = numeric(artillery["attack1_half_factor"])
                    parameters["quarter_aoe_damage_factor"] = numeric(artillery["attack1_quarter_factor"])
                    parameters["burning_oil_ability_rawcode"] = "A02K"
                    parameters["burning_oil_object_data"] = json.loads(burning_oil["data_fields_labeled_json"])
                elif system_id == "raise-dead-skeleton-randomization":
                    skeletons: list[dict[str, Any]] = []
                    for unit_id in parameters["skeleton_table_unit_ids"]:
                        rawcode = integer_rawcode(int(unit_id))
                        unit = static_units.get(rawcode)
                        protected = protected_unit_applied.get(rawcode)
                        if unit is None or protected is None:
                            raise ValueError(f"Raise Dead skeleton is missing protected runtime stats: {rawcode}")
                        skeletons.append({
                            "rawcode": rawcode,
                            "name": unit["name"],
                            "hp": numeric(protected["hp"]),
                            "armor": numeric(protected["armor"]),
                            "attack1_min": numeric(protected["attack1_min"]),
                            "attack1_max": numeric(protected["attack1_max"]),
                            "attack1_cooldown": numeric(protected["attack1_cooldown"]),
                            "attack1_range": numeric(protected["attack1_range"]),
                        })
                    damage_bonus = ability_level_one("A088")
                    armor_bonus = ability_level_one("A089")
                    damage_fields = json.loads(damage_bonus["data_fields_labeled_json"])
                    armor_fields = json.loads(armor_bonus["data_fields_labeled_json"])
                    if numeric(damage_fields.get("Attack Bonus")) != 12 or numeric(armor_fields.get("Defense Bonus")) != 3:
                        raise ValueError("Lich King summoned-skeleton bonus object data changed")
                    parameters["resolved_skeleton_table"] = skeletons
                    parameters["lich_king_bonus_damage_ability_rawcode"] = "A088"
                    parameters["lich_king_bonus_damage_object_data"] = damage_fields
                    parameters["lich_king_bonus_armor_ability_rawcode"] = "A089"
                    parameters["lich_king_bonus_armor_object_data"] = armor_fields
                elif system_id == "gjallarhorn-team-count-scaling":
                    gjallar = static_units.get("h010")
                    if gjallar is None:
                        raise ValueError("Gjallarhorn runtime counter system is missing h010")
                    levels: list[dict[str, Any]] = []
                    for level in range(1, 5):
                        buff = next((row for row in ability_levels.get("A016", []) if row["level"] == str(level)), None)
                        if buff is None:
                            raise ValueError(f"Gjallarhorn buff level missing: {level}")
                        fields = json.loads(buff["data_fields_labeled_json"])
                        levels.append({
                            "level": level,
                            "attack_speed_increase": numeric(fields.get("Attack Speed Increase (%)")),
                            "duration_seconds": numeric(field_lookup(rows_by_object, "abilities", "A016", "adur", level, 0)),
                        })
                    if [row["attack_speed_increase"] for row in levels] != [0.4, 0.45, 0.5, 0.55]:
                        raise ValueError(f"Gjallarhorn buff levels changed: {levels}")
                    parameters["gjallarhorn_name"] = gjallar["name"]
                    parameters["gjallarhorn_tooltip"] = gjallar["ubertip"]
                    parameters["resolved_effect_levels"] = levels
                elif system_id == "support-order-controller":
                    assassin = static_units.get("n01Z")
                    royal = static_units.get("n020")
                    gobbo = static_units.get("n01U")
                    if None in (assassin, royal, gobbo):
                        raise ValueError("Support-order controller is missing Assassin/Royal/Gobbo unit data")
                    normal_feedback = ability_level_one("A076")
                    royal_feedback = ability_level_one("A075")
                    normal_windwalk = ability_level_one("A077")
                    royal_windwalk = ability_level_one("A078")
                    normal_evasion = ability_level_one("A00U")
                    royal_evasion = ability_level_one("A03P")
                    gobbo_repair = ability_level_one("A09K")
                    normal_feedback_fields = json.loads(normal_feedback["data_fields_labeled_json"])
                    royal_feedback_fields = json.loads(royal_feedback["data_fields_labeled_json"])
                    normal_windwalk_fields = json.loads(normal_windwalk["data_fields_labeled_json"])
                    royal_windwalk_fields = json.loads(royal_windwalk["data_fields_labeled_json"])
                    normal_evasion_fields = json.loads(normal_evasion["data_fields_labeled_json"])
                    royal_evasion_fields = json.loads(royal_evasion["data_fields_labeled_json"])
                    gobbo_repair_fields = json.loads(gobbo_repair["data_fields_labeled_json"])
                    if (
                        numeric(normal_feedback_fields.get("Max Mana Drained - Units")) != 40
                        or numeric(normal_feedback_fields.get("Damage Ratio - Units (%)")) != 1.7
                        or numeric(royal_feedback_fields.get("Max Mana Drained - Units")) != 60
                        or numeric(royal_feedback_fields.get("Damage Ratio - Units (%)")) != 2.2
                    ):
                        raise ValueError("Assassin Feedback object data changed")
                    if (
                        numeric(normal_windwalk_fields.get("Backstab Damage")) != 150
                        or numeric(normal_windwalk_fields.get("Movement Speed Increase (%)")) != 0.3
                        or numeric(royal_windwalk_fields.get("Backstab Damage")) != 300
                        or numeric(royal_windwalk_fields.get("Movement Speed Increase (%)")) != 0.45
                    ):
                        raise ValueError("Assassin Wind Walk object data changed")
                    if numeric(normal_evasion_fields.get("Chance to Evade")) != 0.15 or numeric(royal_evasion_fields.get("Chance to Evade")) != 0.25:
                        raise ValueError("Assassin Evasion object data changed")
                    if numeric(gobbo_repair_fields.get("Repair Time Ratio")) != 0.45 or numeric(gobbo_repair_fields.get("Repair Cost Ratio")) != 0:
                        raise ValueError("Gobbo repair object data changed")
                    parameters["actors"] = [
                        {
                            "unit_rawcode": "n01Z",
                            "unit_name": assassin["name"],
                            "production_building_rawcode": production_source_by_unit["n01Z"]["building_rawcode"],
                            "production_building_names": production_source_by_unit["n01Z"]["building_names"],
                            "feedback_ability_rawcode": "A076",
                            "feedback_object_data": normal_feedback_fields,
                            "windwalk_ability_rawcode": "A077",
                            "windwalk_object_data": normal_windwalk_fields,
                            "evasion_ability_rawcode": "A00U",
                            "evasion_object_data": normal_evasion_fields,
                            "low_hp_priority_threshold": 150,
                        },
                        {
                            "unit_rawcode": "n020",
                            "unit_name": royal["name"],
                            "production_building_rawcode": production_source_by_unit["n020"]["building_rawcode"],
                            "production_building_names": production_source_by_unit["n020"]["building_names"],
                            "feedback_ability_rawcode": "A075",
                            "feedback_object_data": royal_feedback_fields,
                            "windwalk_ability_rawcode": "A078",
                            "windwalk_object_data": royal_windwalk_fields,
                            "evasion_ability_rawcode": "A03P",
                            "evasion_object_data": royal_evasion_fields,
                            "low_hp_priority_threshold": 300,
                        },
                        {
                            "unit_rawcode": "n01U",
                            "unit_name": gobbo["name"],
                            "production_building_rawcode": production_source_by_unit["n01U"]["building_rawcode"],
                            "production_building_names": production_source_by_unit["n01U"]["building_names"],
                            "repair_ability_rawcode": "A09K",
                            "repair_object_data": gobbo_repair_fields,
                            "timed_life_seconds": parameters["gobbo_timed_life_seconds"],
                        },
                    ]
                    parameters["assassin_tooltip"] = assassin["ubertip"]
                    parameters["royal_assassin_tooltip"] = royal["ubertip"]
                    parameters["gobbo_tooltip"] = gobbo["ubertip"]
                elif system_id == "global-idle-attack-reengage":
                    excluded: list[dict[str, Any]] = []
                    for rawcode in ("A07I", "A07M"):
                        ability = ability_level_one(rawcode)
                        excluded.append({
                            "rawcode": rawcode,
                            "name": ability["name"],
                            "base_rawcode": ability["base_rawcode"],
                            "object_data": json.loads(ability["data_fields_labeled_json"]),
                        })
                    parameters["resolved_excluded_abilities"] = excluded
                elif system_id == "builder-castle-blink":
                    blink = ability_level_one("A0-1")
                    blink_fields = json.loads(blink["data_fields_labeled_json"])
                    if blink["name"] != "Blink" or numeric(blink["range"]) != 10000:
                        raise ValueError(f"Builder Blink object data changed: {blink}")
                    if numeric(blink_fields.get("Target Type")) != 2 or numeric(blink_fields.get("Options")) != 1:
                        raise ValueError(f"Builder Blink channel fields changed: {blink_fields}")
                    parameters["ability_name"] = blink["name"]
                    parameters["ability_tooltip"] = blink["ubertip"]
                    parameters["ability_range"] = numeric(blink["range"])
                    parameters["ability_object_data"] = blink_fields
                elif system_id == "round-start-castle-protection":
                    if numeric(parameters["protection_duration_seconds"]) != 15:
                        raise ValueError(f"Castle protection duration changed: {parameters}")
                    parameters["round_time_formula"] = "VGb * 60 + UGb"
                elif system_id == "corrupted-eye-of-corruption-spell-vulnerability":
                    eye = static_units.get("h04T")
                    if eye is None:
                        raise ValueError("Eye of Corruption h04T is missing")
                    aura = ability_level_one("A02C")
                    aura_fields = json.loads(aura["data_fields_labeled_json"])
                    aura_buffs = rawcode_list(aura["buffs"])
                    if "A02C" not in rawcode_list(eye["abilities"]):
                        raise ValueError(f"Eye of Corruption lost A02C: {eye['abilities']}")
                    if aura_buffs != ["B00Q"]:
                        raise ValueError(f"Eye of Corruption A02C buff changed: {aura_buffs}")
                    if numeric(aura_fields.get("Armor Bonus")) != -6 or numeric(aura["area"]) != 99999:
                        raise ValueError(f"Eye of Corruption A02C aura fields changed: {aura}")
                    if numeric(parameters["damage_multiplier"]) != 1.12:
                        raise ValueError(f"Eye of Corruption runtime multiplier changed: {parameters}")
                    tooltip = eye["ubertip"]
                    if (
                        "|cffffff006|r less armor" not in tooltip
                        or "|cffffff0012|r% more spell damage" not in tooltip
                    ):
                        raise ValueError(f"Eye of Corruption tooltip semantics changed: {tooltip}")
                    parameters["source_building_rawcode"] = "h04T"
                    parameters["source_building_name"] = eye["name"]
                    parameters["source_building_tooltip"] = tooltip
                    parameters["aura_ability_rawcode"] = "A02C"
                    parameters["aura_ability_name"] = aura["name"]
                    parameters["aura_ability_base_rawcode"] = aura["base_rawcode"]
                    parameters["aura_area"] = numeric(aura["area"])
                    parameters["aura_targets"] = aura["targets"]
                    parameters["aura_buff_rawcodes"] = aura_buffs
                    parameters["aura_object_data"] = aura_fields
                    parameters["aura_armor_bonus"] = numeric(aura_fields.get("Armor Bonus"))
                    parameters["target_buff_rawcode"] = "B00Q"
                    parameters["tooltip_spell_damage_taken_bonus_percent"] = 12
                    parameters["tooltip_multiple_buildings_no_additional_benefit"] = (
                        "Multiples of this building offer no additional benefit." in tooltip
                    )
                elif system_id == "obelisk-of-light-cleansing-light":
                    obelisk = static_units.get("h005")
                    if obelisk is None:
                        raise ValueError("Obelisk of Light h005 is missing")
                    phoenix = ability_level_one("A000")
                    phoenix_fields = json.loads(phoenix["data_fields_labeled_json"])
                    if (
                        numeric(phoenix_fields.get("Initial Damage")) != 180
                        or numeric(phoenix_fields.get("Damage Per Second")) != 0
                        or numeric(phoenix["cooldown"]) != 5
                    ):
                        raise ValueError(f"Obelisk of Light A000 Phoenix Fire fields changed: {phoenix}")
                    removed: list[dict[str, Any]] = []
                    for ability_id in parameters["removed_persistent_ability_ids"]:
                        rawcode = integer_rawcode(int(ability_id))
                        ability = next((row for row in ability_levels.get(rawcode, []) if row["level"] == "1"), None)
                        removed.append({
                            "ability_id": int(ability_id),
                            "rawcode": rawcode,
                            "name": ability["name"] if ability is not None else "",
                        })
                    parameters["building_rawcode"] = "h005"
                    parameters["building_name"] = obelisk["name"]
                    parameters["building_tooltip"] = obelisk["ubertip"]
                    parameters["effect_ability_rawcode"] = "A000"
                    parameters["effect_ability_name"] = phoenix["name"]
                    parameters["effect_ability_base_rawcode"] = phoenix["base_rawcode"]
                    parameters["effect_initial_damage"] = numeric(phoenix_fields.get("Initial Damage"))
                    parameters["effect_damage_per_second"] = numeric(phoenix_fields.get("Damage Per Second"))
                    parameters["effect_cooldown_seconds"] = numeric(phoenix["cooldown"])
                    parameters["effect_area"] = numeric(phoenix["area"])
                    parameters["effect_targets"] = phoenix["targets"]
                    parameters["removed_persistent_abilities"] = removed
                elif system_id == "rescue-strike":
                    rescue = ability_level_one("A005")
                    effect = ability_level_one("A06E")
                    marker = static_units.get("h04X")
                    if marker is None:
                        raise ValueError("Rescue Strike marker unit h04X is missing")
                    runtime_cooldown = protected_ability_values.get(("A005", 1, "cooldown"))
                    runtime_mana = protected_ability_values.get(("A005", 1, "mana_cost"))
                    if numeric(runtime_cooldown) != 60 or numeric(runtime_mana) != 0:
                        raise ValueError(
                            f"Rescue Strike protected cooldown/mana changed: cooldown={runtime_cooldown} mana={runtime_mana}"
                        )
                    if numeric(rescue["area"]) != 700 or numeric(rescue["range"]) != 1000:
                        raise ValueError(f"Rescue Strike object range/area changed: {rescue}")
                    parameters["ability_rawcode"] = "A005"
                    parameters["ability_name"] = rescue["name"]
                    parameters["ability_tooltip"] = rescue["ubertip"]
                    parameters["static_object_cooldown_seconds"] = numeric(rescue["cooldown"])
                    parameters["effective_protected_cooldown_seconds"] = numeric(runtime_cooldown)
                    parameters["effective_protected_mana_cost"] = numeric(runtime_mana)
                    parameters["object_range"] = numeric(rescue["range"])
                    parameters["object_area"] = numeric(rescue["area"])
                    parameters["effect_ability_rawcode"] = "A06E"
                    parameters["effect_ability_name"] = effect["name"]
                    parameters["marker_unit_rawcode"] = "h04X"
                    parameters["marker_unit_name"] = marker["name"]
                elif system_id == "elemental-forge-weapon-cleave":
                    obelisk = static_units.get("h060")
                    if obelisk is None:
                        raise ValueError("Forge Weapon runtime system is missing Obelisk of Elements h060")
                    forge_marker = ability_level_one("A0F1")
                    dispatcher = ability_level_one("A03Q")
                    parameters["source_building_rawcode"] = "h060"
                    parameters["source_building_name"] = obelisk["name"]
                    parameters["source_building_tooltip"] = obelisk["ubertip"]
                    parameters["forge_weapon_marker_rawcode"] = "A0F1"
                    parameters["forge_weapon_marker_name"] = forge_marker["name"]
                    parameters["forge_weapon_marker_object_data"] = json.loads(forge_marker["data_fields_labeled_json"])
                    parameters["source_damage_dispatch_marker_rawcode"] = "A03Q"
                    parameters["source_damage_dispatch_marker_name"] = dispatcher["name"]
                elif system_id == "locust-harpy-structure-damage-penalty":
                    locust = static_units.get("u00G")
                    protected = protected_unit_applied.get("u00G")
                    if locust is None or protected is None:
                        raise ValueError("Locust Harpy runtime system is missing u00G protected data")
                    if numeric(protected["attack1_min"]) != 142 or numeric(protected["attack1_max"]) != 146:
                        raise ValueError(f"Locust Harpy protected attack changed: {protected}")
                    parameters["unit_rawcode"] = "u00G"
                    parameters["unit_name"] = locust["name"]
                    parameters["runtime_attack_min"] = numeric(protected["attack1_min"])
                    parameters["runtime_attack_max"] = numeric(protected["attack1_max"])
                    parameters["runtime_attack_average"] = numeric(protected["attack1_avg"])
                    parameters["runtime_attack_range"] = numeric(protected["attack1_range"])
                    parameters["runtime_attack_type"] = locust["attack1_type"]
                elif system_id == "celestial-chi-tower-impact-vision":
                    tower = static_units.get("h07V")
                    protected = protected_unit_applied.get("h07V")
                    if tower is None or protected is None:
                        raise ValueError("Celestial Chi Tower runtime system is missing h07V protected data")
                    if numeric(protected["attack1_range"]) != 20000 or numeric(protected["attack1_cooldown"]) != 7.5:
                        raise ValueError(f"Celestial Chi Tower protected weapon changed: {protected}")
                    parameters["tower_rawcode"] = "h07V"
                    parameters["tower_name"] = tower["name"]
                    parameters["tower_tooltip"] = tower["ubertip"]
                    parameters["runtime_hp"] = numeric(protected["hp"])
                    parameters["runtime_armor"] = numeric(protected["armor"])
                    parameters["runtime_attack_min"] = numeric(protected["attack1_min"])
                    parameters["runtime_attack_max"] = numeric(protected["attack1_max"])
                    parameters["runtime_attack_cooldown_seconds"] = numeric(protected["attack1_cooldown"])
                    parameters["runtime_attack_range"] = numeric(protected["attack1_range"])
                else:
                    raise ValueError(f"unrecognized runtime system mechanic: {system_id}")

                runtime_system_rows.append([
                    system_id, mechanic["mechanic_kind"], mechanic["trigger"],
                    mechanic["related_objects_json"], stable_json(parameters),
                    mechanic["source_functions"], mechanic["evidence_kind"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "runtime-system-mechanics.tsv",
        [
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ],
        runtime_system_rows,
    )

    # Closure audit for production-unit-specific branches in core runtime event
    # handlers. A new explicit production rawcode in these handlers must either
    # gain an importer-ready special row, already be covered by the scripted
    # unit-spell registry, or be one of the two strictly classified exceptions.
    special_kinds_by_unit: dict[str, set[str]] = defaultdict(set)
    for row in production_special_rows:
        special_kinds_by_unit[str(row[2])].add(str(row[4]))

    registered_unit_spell_units: set[str] = set()
    if unit_spell_registration_path.exists():
        with unit_spell_registration_path.open(encoding="utf-8", newline="") as handle:
            registered_unit_spell_units = {
                row["unit_rawcode"] for row in csv.DictReader(handle, delimiter="\t")
            }

    core_runtime_handler_functions = {
        "onUnitEnteredMap",
        "onSummonedUnit",
        "onUnitTrained",
        "fJ",
        "mJ",
        "checkForWrongOrder",
        "handleSourceDamageEffects",
        "handleTargetDamageEffects",
        "handleFanOfKnives",
        "applyVampireArmorReduction",
        "DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire",
        "hL",
        "DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack",
        "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood",
        "DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind",
        "EventListener_add_FireElemental_onEvent_add_FireElemental",
        "EventListener_add_doAfter_FixDefend_onEvent_add_doAfter_FixDefend",
    }
    explicit_runtime_functions_by_unit: dict[str, set[str]] = defaultdict(set)
    rawcode_reference_path = map_root / "script" / "rawcode-reference-sites.tsv"
    if rawcode_reference_path.exists():
        with rawcode_reference_path.open(encoding="utf-8", newline="") as handle:
            for reference in csv.DictReader(handle, delimiter="\t"):
                rawcode = reference["rawcode"]
                if (
                    reference["categories"] == "units"
                    and rawcode in production_source_by_unit
                    and reference["function"] in core_runtime_handler_functions
                ):
                    explicit_runtime_functions_by_unit[rawcode].add(reference["function"])

    # Marker-driven branches do not mention their production rawcodes in the
    # handler, so include those owners in the same audit explicitly.
    marker_requirements = {
        "A0DW": "source-damage-health-scaled-aftershock",
        "A0DX": "target-damage-melee-thunderbolt-retaliation",
        "A0HG": "attack-proc-ground-whirlwind-aoe",
    }
    marker_runtime_hooks_by_unit: dict[str, set[str]] = defaultdict(set)
    for unit_rawcode in production_source_by_unit:
        unit = static_units.get(unit_rawcode)
        if unit is None:
            continue
        attached = set(rawcode_list(unit["abilities"]))
        for marker_rawcode, required_kind in marker_requirements.items():
            if marker_rawcode not in attached:
                continue
            if required_kind not in special_kinds_by_unit.get(unit_rawcode, set()):
                raise ValueError(
                    f"marker-driven production runtime hook is not normalized: "
                    f"{unit_rawcode} {marker_rawcode} expected {required_kind}"
                )
            marker_runtime_hooks_by_unit[unit_rawcode].add(f"ability-marker:{marker_rawcode}")

    runtime_coverage_exceptions = {
        "h02Z": (
            "parent-special-endpoint",
            "Fire Elemental is the h02Z replacement endpoint already proven by Greater Fire Elemental's u00F split row",
        ),
        "n01B": (
            "verified-visual-only",
            "Shadow Drake's onUnitEnteredMap rawcode branch is strictly asserted in lua_index.py to only set vertex color 82,0,135,102",
        ),
    }
    runtime_coverage_rows: list[list[Any]] = []
    audited_runtime_units = sorted(set(explicit_runtime_functions_by_unit) | set(marker_runtime_hooks_by_unit))
    for unit_rawcode in audited_runtime_units:
        production = production_source_by_unit[unit_rawcode]
        unit = static_units[unit_rawcode]
        special_kinds = sorted(special_kinds_by_unit.get(unit_rawcode, set()))
        has_unit_spell = unit_rawcode in registered_unit_spell_units
        if special_kinds and has_unit_spell:
            status = "special-mechanic+unit-spell"
            note = "runtime branch has dedicated special normalization and scripted unit-spell coverage"
        elif special_kinds:
            status = "special-mechanic"
            note = "runtime branch has dedicated importer-ready special normalization"
        elif has_unit_spell:
            status = "unit-spell"
            note = "runtime branch is covered by the scripted unit-spell normalization"
        elif unit_rawcode in runtime_coverage_exceptions:
            status, note = runtime_coverage_exceptions[unit_rawcode]
        else:
            raise ValueError(
                f"uncovered production-unit runtime branch: {unit_rawcode} {unit['name']} "
                f"functions={sorted(explicit_runtime_functions_by_unit.get(unit_rawcode, set()))}"
            )
        runtime_coverage_rows.append([
            production["building_rawcode"], production["building_names"],
            unit_rawcode, unit["name"],
            ",".join(sorted(explicit_runtime_functions_by_unit.get(unit_rawcode, set()))),
            ",".join(sorted(marker_runtime_hooks_by_unit.get(unit_rawcode, set()))),
            ",".join(special_kinds), int(has_unit_spell), status, note,
        ])
    write_tsv(
        output / "production-unit-runtime-coverage.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names",
            "explicit_runtime_functions", "marker_runtime_hooks", "special_mechanic_kinds",
            "has_scripted_unit_spell", "coverage_status", "coverage_note",
        ],
        runtime_coverage_rows,
    )

    if unit_spell_registration_path.exists():
        with unit_spell_registration_path.open(encoding="utf-8", newline="") as handle:
            for registration in csv.DictReader(handle, delimiter="\t"):
                unit_rawcode = registration["unit_rawcode"]
                ability_rawcode = registration["ability_rawcode"]
                unit = static_units.get(unit_rawcode)
                if unit is None:
                    raise ValueError(f"scripted unit-spell registration has no resolved unit definition: {unit_rawcode}")
                definitions = ability_levels.get(ability_rawcode, [])
                definition = next((row for row in definitions if row["level"] == "1"), None)
                definition_source = "map-resolved"
                if definition is None:
                    definition = inherited_ability_level_one(ability_rawcode)
                    definition_source = "inherited-base"
                if definition is None:
                    raise ValueError(f"scripted unit-spell registration has no ability definition: {ability_rawcode}")
                runtime_mana = protected_ability_values.get((ability_rawcode, 1, "mana_cost"))
                runtime_cooldown = protected_ability_values.get((ability_rawcode, 1, "cooldown"))
                effective_mana = runtime_mana if runtime_mana is not None else definition["mana_cost"]
                effective_cooldown = runtime_cooldown if runtime_cooldown is not None else definition["cooldown"]
                production = production_source_by_unit.get(unit_rawcode)
                if production is not None:
                    unit_spell_production_rows += 1
                expected_rawcode = registration["expected_immediate_unit_rawcode"]
                expected_name = ""
                if expected_rawcode:
                    expected = static_units.get(expected_rawcode)
                    expected_name = expected["name"] if expected is not None else registration["expected_immediate_unit_names"]
                base_order = value_as_text(field_lookup(rows_by_object, "abilities", ability_rawcode, "aord", 0, 0))
                registered_order_id = registration["order_id"]
                if registered_order_id:
                    resolved_order_id = registered_order_id
                    resolved_order_source = "script-integer"
                else:
                    canonical_order_id = CANONICAL_ORDER_IDS.get(base_order)
                    resolved_order_id = str(canonical_order_id) if canonical_order_id is not None else ""
                    resolved_order_source = "wc3-canonical-base-order" if canonical_order_id is not None else "unresolved"
                unit_spell_by_pair[(unit_rawcode, ability_rawcode)] = {
                    "unit_name": unit["name"],
                    "production_building_rawcode": production["building_rawcode"] if production is not None else "",
                    "production_building_names": production["building_names"] if production is not None else "",
                    "ability_name": definition["name"],
                    "ability_tip": definition["tip"],
                    "ability_ubertip": definition["ubertip"],
                    "effective_mana_cost": value_as_text(effective_mana),
                    "effective_cooldown": value_as_text(effective_cooldown),
                    "target_mode": registration["target_mode"],
                    "target_mode_label": registration["target_mode_label"],
                    "base_order": base_order,
                }
                unit_spell_rows.append([
                    unit_rawcode, unit["name"],
                    production["building_rawcode"] if production is not None else "",
                    production["building_names"] if production is not None else "",
                    ability_rawcode, definition_source, definition["base_rawcode"], definition["name"], definition["tip"], definition["ubertip"],
                    definition["mana_cost"], effective_mana, "protected-runtime" if runtime_mana is not None else "static-resolved",
                    definition["cooldown"], effective_cooldown, "protected-runtime" if runtime_cooldown is not None else "static-resolved",
                    base_order, definition["range"], definition["area"], definition["targets"], definition["buffs"],
                    definition["data_fields_json"], definition["data_fields_labeled_json"],
                    registration["target_mode"], registration["target_mode_label"], registration["order_id"], registration["order_expression_kind"],
                    resolved_order_id, resolved_order_source, expected_rawcode, expected_name,
                    registration["handler_function"], registration["registration_function"], registration["evidence_kind"], registration["byte_offset"],
                ])
    write_tsv(
        output / "unit-spells.tsv",
        [
            "unit_rawcode", "unit_names", "production_building_rawcode", "production_building_names",
            "ability_rawcode", "definition_source", "base_rawcode", "ability_name", "ability_tip", "ability_ubertip",
            "static_mana_cost", "effective_mana_cost", "mana_cost_source",
            "static_cooldown", "effective_cooldown", "cooldown_source",
            "base_order", "range", "area", "targets", "buffs", "data_fields_json", "data_fields_labeled_json",
            "target_mode", "target_mode_label", "registered_order_id", "order_expression_kind",
            "resolved_order_id", "resolved_order_id_source", "expected_immediate_unit_rawcode", "expected_immediate_unit_name",
            "handler_function", "registration_function", "evidence_kind", "byte_offset",
        ],
        unit_spell_rows,
    )

    unit_spell_mechanic_rows: list[list[Any]] = []
    unit_spell_mechanics_path = map_root / "script" / "unit-spell-mechanics.tsv"
    if unit_spell_mechanics_path.exists():
        with unit_spell_mechanics_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                pair = (mechanic["unit_rawcode"], mechanic["ability_rawcode"])
                spell = unit_spell_by_pair.get(pair)
                if spell is None:
                    raise ValueError(f"unit-spell mechanic has no scripted registration: {pair}")
                reachable = json.loads(mechanic["reachable_map_rawcode_paths_json"])
                enriched_objects: list[dict[str, Any]] = []
                for effect in reachable:
                    rawcode = str(effect["rawcode"])
                    enriched: dict[str, Any] = dict(effect)
                    definitions = ability_levels.get(rawcode, [])
                    definition = next((row for row in definitions if row["level"] == "1"), None)
                    if definition is None:
                        definition = inherited_ability_level_one(rawcode)
                    if definition is not None:
                        enriched["ability_level1"] = {
                            "name": definition["name"],
                            "range": definition["range"],
                            "area": definition["area"],
                            "targets": definition["targets"],
                            "buffs": definition["buffs"],
                            "duration_normal": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "adur", 1, 0)),
                            "duration_hero": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "ahdu", 1, 0)),
                            "data_fields_labeled_json": definition["data_fields_labeled_json"],
                        }
                    unit_effect = static_units.get(rawcode)
                    if unit_effect is not None:
                        enriched["unit_object"] = {
                            "name": unit_effect["name"],
                            "abilities": unit_effect["abilities"],
                            "move_speed": unit_effect["move_speed"],
                            "attack1_type": unit_effect["attack1_type"],
                            "attack1_weapon_type": unit_effect["attack1_weapon_type"],
                            "attack1_targets": unit_effect["attack1_targets"],
                        }
                    enriched_objects.append(enriched)
                unit_spell_mechanic_rows.append([
                    mechanic["unit_rawcode"], spell["unit_name"],
                    spell["production_building_rawcode"], spell["production_building_names"],
                    mechanic["ability_rawcode"], spell["ability_name"], spell["ability_tip"], spell["ability_ubertip"],
                    spell["effective_mana_cost"], spell["effective_cooldown"],
                    spell["target_mode"], spell["target_mode_label"], spell["base_order"],
                    mechanic["mechanic_kind"], mechanic["direct_calls"], mechanic["helper_functions"],
                    mechanic["delayed_callback_functions"], mechanic["dynamic_callback_functions"], mechanic["scheduled_delays_json"],
                    mechanic["periodic_intervals_json"], mechanic["random_real_ranges_json"],
                    mechanic["direct_map_rawcodes"],
                    json.dumps(enriched_objects, separators=(",", ":"), sort_keys=True, ensure_ascii=False),
                    mechanic["semantic_effect_sites_json"], mechanic["source_numeric_literals_json"],
                    mechanic["handler_function"], mechanic["evidence_kind"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "unit-spell-mechanics.tsv",
        [
            "unit_rawcode", "unit_name", "production_building_rawcode", "production_building_names",
            "ability_rawcode", "ability_name", "ability_tip", "ability_ubertip",
            "effective_mana_cost", "effective_cooldown", "target_mode", "target_mode_label", "base_order",
            "mechanic_kind", "direct_calls", "helper_functions", "delayed_callback_functions", "dynamic_callback_functions",
            "scheduled_delays_json", "periodic_intervals_json", "random_real_ranges_json",
            "direct_map_rawcodes", "reachable_map_objects_json", "semantic_effect_sites_json", "source_numeric_literals_json",
            "handler_function", "evidence_kind", "byte_offset",
        ],
        unit_spell_mechanic_rows,
    )

    protected_filter_path = map_root / "script" / "protected-filter-bindings.tsv"
    protected_filter_rows: list[dict[str, str]] = []
    if protected_filter_path.exists():
        with protected_filter_path.open(encoding="utf-8", newline="") as handle:
            protected_filter_rows = list(csv.DictReader(handle, delimiter="\t"))
    protected_filters_by_symbol = {
        row["symbol"]: row for row in protected_filter_rows if row["resolution_status"] == "resolved"
    }

    element_bucket_path = map_root / "script" / "element-building-buckets.tsv"
    element_bucket_rows: list[dict[str, str]] = []
    if element_bucket_path.exists():
        with element_bucket_path.open(encoding="utf-8", newline="") as handle:
            element_bucket_rows = list(csv.DictReader(handle, delimiter="\t"))
    expected_element_buckets = {
        1: ("fire", {"h045", "h046"}),
        2: ("earth", {"h040", "h041"}),
        3: ("lightning", {"h042", "h044"}),
        4: ("water", {"h03X", "h03Y", "h03Z"}),
        5: ("wind", {"h04A", "h04C", "h04D"}),
    }
    element_buckets_by_number: dict[int, dict[str, Any]] = {}
    if element_bucket_rows:
        actual_by_bucket: dict[int, set[str]] = defaultdict(set)
        for row in element_bucket_rows:
            actual_by_bucket[int(row["bucket"])].add(row["building_rawcode"])
        for bucket, (element, expected_rawcodes) in expected_element_buckets.items():
            actual = actual_by_bucket.get(bucket, set())
            if actual != expected_rawcodes:
                raise ValueError(
                    f"Elemental building bucket {bucket}/{element} changed: expected={sorted(expected_rawcodes)} actual={sorted(actual)}"
                )
            element_buckets_by_number[bucket] = {
                "element": element,
                "building_rawcodes": sorted(actual),
            }
    write_tsv(
        output / "element-building-buckets.tsv",
        ["bucket", "element", "building_rawcode", "building_names", "source_function", "byte_offset"],
        [
            [
                row["bucket"], expected_element_buckets[int(row["bucket"])][0], row["building_rawcode"], row["building_names"],
                row["source_function"], row["byte_offset"],
            ]
            for row in element_bucket_rows
        ],
    )

    def integer_rawcode_text(integer_id: int) -> str:
        if integer_id < 0 or integer_id > 0xFFFFFFFF:
            raise ValueError(f"rawcode integer is outside u32 range: {integer_id}")
        return integer_id.to_bytes(4, "big").decode("latin1")

    def numeric_literals_by_function(mechanic: dict[str, str]) -> dict[str, list[str]]:
        return {
            str(row["function"]): [str(value) for value in row["literals"]]
            for row in json.loads(mechanic["source_numeric_literals_json"])
        }

    def require_literals(mechanic: dict[str, str], function_name: str, required: Iterable[str]) -> None:
        literals = numeric_literals_by_function(mechanic).get(function_name, [])
        missing = [value for value in required if value not in literals]
        if missing:
            raise ValueError(
                f"unit-spell semantic evidence changed for {mechanic['unit_rawcode']} {function_name}: "
                f"missing literals {missing}"
            )

    proxy_callee_argument_indexes = {
        "dummyCastTargetFrom": (1,),
        "dummyCastTargetFrom1": (1,),
        "dummyCastTargetWithVision": (1,),
        "dummyCastPointFrom": (1,),
        "dummyCastImmediateFrom": (1,),
        "dummyCastImmediateFrom1": (1,),
        "dummyCarrierWithAbility": (1,),
        "dummyCarrierCastImmediate": (1,),
        "dummyCarrierWithAbilities1": (1, 2, 3),
    }

    building_improvement_spawn_by_source: dict[str, dict[str, str]] = {}
    building_improvement_spawn_path = map_root / "script" / "building-improvement-spawn-mechanics.tsv"
    if building_improvement_spawn_path.exists():
        with building_improvement_spawn_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                source_rawcode = row["source_unit_rawcode"]
                if source_rawcode in building_improvement_spawn_by_source:
                    raise ValueError(f"duplicate building-improvement spawn mechanic source: {source_rawcode}")
                building_improvement_spawn_by_source[source_rawcode] = row

    unit_spell_semantic_rows: list[list[Any]] = []
    unit_spell_semantic_status_counts: Counter[str] = Counter()
    unit_spell_semantic_kind_counts: Counter[str] = Counter()
    for mechanic_row in unit_spell_mechanic_rows:
        mechanic = dict(zip([
            "unit_rawcode", "unit_name", "production_building_rawcode", "production_building_names",
            "ability_rawcode", "ability_name", "ability_tip", "ability_ubertip",
            "effective_mana_cost", "effective_cooldown", "target_mode", "target_mode_label", "base_order",
            "mechanic_kind", "direct_calls", "helper_functions", "delayed_callback_functions", "dynamic_callback_functions",
            "scheduled_delays_json", "periodic_intervals_json", "random_real_ranges_json",
            "direct_map_rawcodes", "reachable_map_objects_json", "semantic_effect_sites_json", "source_numeric_literals_json",
            "handler_function", "evidence_kind", "byte_offset",
        ], mechanic_row, strict=True))
        unit_rawcode = mechanic["unit_rawcode"]
        semantic_sites = json.loads(mechanic["semantic_effect_sites_json"])
        effect_rawcodes: list[str] = []
        delivery_sites: list[dict[str, Any]] = []
        for site in semantic_sites:
            indexes = proxy_callee_argument_indexes.get(str(site["callee"]))
            if indexes is None:
                continue
            arguments = list(site["arguments"])
            site_effects: list[str] = []
            for argument_index in indexes:
                if argument_index >= len(arguments):
                    continue
                argument = str(arguments[argument_index])
                if not argument.isdigit():
                    continue
                rawcode = integer_rawcode_text(int(argument))
                site_effects.append(rawcode)
                if rawcode not in effect_rawcodes:
                    effect_rawcodes.append(rawcode)
            if site_effects:
                delivery_sites.append({
                    "callee": site["callee"],
                    "function": site["function"],
                    "effect_rawcodes": site_effects,
                    "arguments": arguments,
                })

        semantic_kind = "unexpanded-script"
        normalization_status = "partial"
        parameters: dict[str, Any] = {
            "delivery_sites": delivery_sites,
        }
        source_functions: list[str] = [mechanic["handler_function"]]
        if effect_rawcodes:
            semantic_kind = "proxy-object-effect" if len(effect_rawcodes) == 1 else "proxy-object-effect-bundle"
            normalization_status = "object-effect-ready"

        if unit_rawcode == "e000":
            require_literals(mechanic, mechanic["handler_function"], ("1.2",))
            semantic_kind = "registered-object-effect-plus-resume"
            normalization_status = "object-effect-ready"
            effect_rawcodes = ["A0CG"]
            parameters.update({
                "primary_effect_ability_rawcode": "A0CG",
                "resume_attack_delay_seconds": 1.2,
                "effect_parameters_source": "registered-ability-object-data",
            })
            source_functions.append("CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities1")
        elif unit_rawcode == "e006":
            require_literals(mechanic, mechanic["handler_function"], ("1095328818", "0.6"))
            semantic_kind = "registered-heal-plus-permanent-armor"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A004", "AId2"]
            parameters.update({
                "primary_effect_ability_rawcode": "A004",
                "permanent_armor_ability_rawcode": "AId2",
                "permanent_armor_bonus": 2,
                "resume_attack_delay_seconds": 0.6,
            })
            source_functions.append("CallbackSingle_doAfter_RaceNatureAbilities_call_doAfter_RaceNatureAbilities2")
        elif unit_rawcode == "e009":
            require_literals(mechanic, mechanic["handler_function"], ("1095328819", "0.6"))
            keeper_callback = "CallbackSingle_doAfter_RaceNatureAbilities_call_doAfter_RaceNatureAbilities3"
            require_literals(mechanic, keeper_callback, ("852126", "1."))
            semantic_kind = "registered-heal-armor-plus-summon"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0BT", "AId3", "A0BU", "e00D"]
            parameters.update({
                "primary_effect_ability_rawcode": "A0BT",
                "permanent_armor_ability_rawcode": "AId3",
                "permanent_armor_bonus": 3,
                "summon_order_id": 852126,
                "summon_order_mapping": "AOsf-proven-by-shared-Naga-Siren-order",
                "summon_ability_rawcode": "A0BU",
                "summoned_unit_rawcode": "e00D",
                "summoned_unit_count": 2,
                "summoned_unit_duration_seconds": 45,
                "post_heal_delay_seconds": 0.6,
                "resume_attack_after_summon_seconds": 1,
            })
            source_functions.extend([
                keeper_callback,
                "CallbackSingle_doAfter_doAfter_RaceNatureAbilities_call_doAfter_doAfter_RaceNatureAbilities",
            ])
        elif unit_rawcode == "e00F":
            require_literals(mechanic, mechanic["handler_function"], ("852520", "0.6"))
            semantic_kind = "taunt-plus-resume"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0BI"]
            parameters.update({
                "immediate_order_id": 852520,
                "immediate_order_name": "taunt",
                "order_mapping_source": "warcraft-standard-order-id",
                "taunt_ability_rawcode": "A0BI",
                "taunt_base_ability_rawcode": "Atau",
                "taunt_area": 350,
                "resume_attack_delay_seconds": 0.6,
            })
        elif unit_rawcode == "h03B":
            require_literals(mechanic, mechanic["handler_function"], ("852066", "1."))
            semantic_kind = "registered-heal-plus-inner-fire"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A03K", "A03M"]
            parameters.update({
                "primary_effect_ability_rawcode": "A03K",
                "primary_heal": 25,
                "inner_fire_order_id": 852066,
                "inner_fire_order_mapping": "Ainf-proven-by-feralRage-dynamic-ability-cast",
                "inner_fire_ability_rawcode": "A03M",
                "armor_bonus": 6,
                "life_regen_per_second": 16,
                "buff_duration_seconds": 10,
                "resume_attack_delay_seconds": 1,
            })
            source_functions.extend([
                "feralRage",
                "CallbackSingle_doAfter_RaceHumanAbilities_call_doAfter_RaceHumanAbilities",
            ])
        elif unit_rawcode == "h03C":
            require_literals(mechanic, "churchSpell", ("852066", "0.405", "1."))
            paladin_callback = "CallbackSingle_doAfter_RaceHumanAbilities_call_doAfter_RaceHumanAbilities1"
            paladin_filter = "ForGroupCallback_forUnitsInRange_doAfter_RaceHumanAbilities_callback_forUnitsInRange_doAfter_RaceHumanAbilities"
            require_literals(mechanic, paladin_callback, ("900.", "852094", "1."))
            semantic_kind = "registered-heal-inner-fire-maxhp-resurrection"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A03K", "A03I", "A03L", "A03H"]
            parameters.update({
                "primary_effect_ability_rawcode": "A03K",
                "primary_heal": 25,
                "inner_fire_order_id": 852066,
                "inner_fire_order_mapping": "Ainf-proven-by-feralRage-dynamic-ability-cast",
                "inner_fire_ability_rawcode": "A03I",
                "armor_bonus": 9,
                "life_regen_per_second": 24,
                "buff_duration_seconds": 10,
                "permanent_max_hp_bonus_ability_rawcode": "A03L",
                "permanent_max_hp_bonus": 100,
                "resurrection_precheck_delay_seconds": 1,
                "resurrection_precheck_radius": 900,
                "resurrection_precheck_predicate": "ally;dead;non-mechanical",
                "resurrection_precheck_checks_wc3_can_raise": False,
                "resurrection_order_id": 852094,
                "resurrection_order_mapping": "resurrection-proven-by-Holy-Warrior-A0I4-carrier",
                "resurrection_ability_rawcode": "A03H",
                "resurrection_corpses_raised": 1,
                "resurrection_effect_uses_wc3_corpse_eligibility": True,
            })
            source_functions.extend(["churchSpell", "feralRage", paladin_callback, paladin_filter])
        elif unit_rawcode == "h03V":
            if set(element_buckets_by_number) != {1, 2, 3, 4, 5}:
                raise ValueError("Master of Elements semantics require all five Elemental building buckets")
            require_literals(mechanic, "onMasterSpell", ("0", "99", "50", "75.", "35.", "70."))
            require_literals(mechanic, "onLightningBolt", ("30.", "50.", "400.", "03"))
            require_literals(mechanic, "processMasterLightningBolts", ("20.", "30.", "50.", "400."))
            require_literals(mechanic, "spawnSnowMissileFanStep", ("26.", "400.", "04"))
            require_literals(mechanic, "processSnowMissiles", ("26.", "38.", "852226", "1.", "20."))
            require_literals(mechanic, "findMasterLightningHit", ("34.",))
            semantic_kind = "element-scaled-dual-projectile-system"
            sx_filter = protected_filters_by_symbol.get("SX")
            normalization_status = "script-native-ready" if sx_filter is not None else "partial"
            effect_rawcodes = ["h03H", "h068", "A043"]
            parameters.update({
                "branch_roll": {"min": 0, "max": 99, "lightning_if_less_than": 50, "frost_otherwise": True},
                "element_building_buckets": {
                    info["element"]: {"bucket": bucket, "building_rawcodes": info["building_rawcodes"]}
                    for bucket, info in sorted(element_buckets_by_number.items())
                },
                "lightning": {
                    "projectile_unit_rawcode": "h03H",
                    "projectile_count_formula": "2 + floor(lightning_building_count / 3)",
                    "split_budget_formula": "2 + floor(wind_building_count / 2)",
                    "damage_formula": "75 * min(1 + fire_building_count, 4)",
                    "damage_attack_type": "normal",
                    "damage_type": "lightning",
                    "initial_segment_budget": 400,
                    "segment_budget_decrement_per_tick": 20,
                    "tick_seconds": 0.03,
                    "movement_per_tick": 20,
                    "movement_speed_world_units_per_second": 20 / 0.03,
                    "hit_radius": 34,
                    "target_predicate": "alive-combat-sapper;enemy;not-previous-hit",
                    "initial_origin_forward_offset": 30,
                    "lane_spacing": 50,
                    "split_angle_degrees": 30,
                    "split_stub_budget": 50,
                    "state_machine_source": "processMasterLightningBolts",
                },
                "frost": {
                    "projectile_unit_rawcode": "h068",
                    "projectile_count_formula": "5 + 2 * water_building_count",
                    "damage_formula": "75 * min(1 + earth_building_count, 4)",
                    "frost_nova_level_formula": "clamp(floor(wind_building_count / 4), 1, 3)",
                    "fan_min_relative_angle_degrees": -35,
                    "fan_max_relative_angle_degrees": 35,
                    "spawn_interval_seconds": 0.04,
                    "tick_seconds": 0.03,
                    "movement_per_tick": 26,
                    "movement_speed_world_units_per_second": 26 / 0.03,
                    "initial_range_budget": 400,
                    "range_budget_decrement_per_tick": 20,
                    "maximum_travel_world_units": 520,
                    "hit_radius": 38,
                    "direct_damage_attack_type": "normal",
                    "direct_damage_type": "cold",
                    "stops_on_first_hit": True,
                    "frost_nova_ability_rawcode": "A043",
                    "frost_nova_order_id": 852226,
                    "frost_nova_order_name": "frostnova",
                    "frost_nova_dummy_lifetime_seconds": 1,
                    "frost_nova_only_if_target_survives_direct_hit": True,
                    "target_filter_symbol": "SX",
                    "target_filter_status": "resolved" if sx_filter is not None else "protected-global-filter-not-yet-resolved",
                    "target_filter_function": sx_filter["resolved_function"] if sx_filter is not None else "",
                    "target_predicate": sx_filter["predicate"] if sx_filter is not None else "",
                    "state_machine_source": "processSnowMissiles",
                },
                **({
                    "normalization_blocker": "",
                    "resolved_filter_evidence_kind": sx_filter["evidence_kind"],
                } if sx_filter is not None else {
                    "normalization_blocker": "resolve protected global Frost target filter SX",
                }),
            })
            source_functions.extend([
                "onMasterSpell", "onLightningBolt", "processMasterLightningBolts", "findMasterLightningHit",
                "spawnSnowMissileFanStep", "processSnowMissiles", "CallbackSingle_doAfter_MasterOfElements_call_doAfter_MasterOfElements",
            ])
        elif unit_rawcode == "n01W":
            require_literals(mechanic, mechanic["handler_function"], ("3.",))
            brood_callback = "CallbackSingle_doAfter_RaceNatureAbilities_call_doAfter_RaceNatureAbilities5"
            brood_callback2 = "CallbackSingle_doAfter_doAfter_RaceNatureAbilities_call_doAfter_doAfter_RaceNatureAbilities2"
            require_literals(mechanic, brood_callback, ("852212", "3."))
            require_literals(mechanic, brood_callback2, ("852602",))
            semantic_kind = "registered-infest-then-enable-autocasts"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0AV", "A0AS", "n00T"]
            parameters.update({
                "primary_effect_ability_rawcode": "A0AV",
                "primary_effect_damage_per_second": 20,
                "primary_effect_duration_seconds": 10,
                "primary_effect_spawn_count_on_kill": 2,
                "primary_effect_spawn_unit_rawcode": "n00T",
                "first_followup_delay_seconds": 3,
                "first_followup_order_id": 852212,
                "first_followup_order_name": "webon",
                "first_followup_ability_rawcode": "A0AS",
                "second_followup_delay_seconds": 3,
                "second_followup_order_id": 852602,
                "second_followup_order_name": "parasiteon",
                "second_followup_ability_rawcode": "A0AV",
                "order_mapping_source": "warcraft-standard-order-id",
                "resume_attack_after_second_followup": True,
            })
            source_functions.extend([brood_callback, brood_callback2])
        elif unit_rawcode == "n02Y":
            require_literals(mechanic, mechanic["handler_function"], ("0.1", "0.3"))
            ogre_callback = "CallbackSingle_doAfter_RaceDesertAbilities_call_doAfter_RaceDesertAbilities1"
            require_literals(mechanic, ogre_callback, ("852100", "0.1"))
            semantic_kind = "delayed-berserk-rage"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0GR"]
            parameters.update({
                "random_activation_delay_seconds": [0.1, 0.3],
                "berserk_order_id": 852100,
                "berserk_order_mapping": "Absk-proven-by-independent-Troll-Trapper-runtime-order",
                "berserk_ability_rawcode": "A0GR",
                "attack_speed_increase": 1.4,
                "movement_speed_increase": 0.15,
                "damage_taken_increase": 0.01,
                "duration_seconds": 6,
                "resume_attack_delay_seconds": 0.1,
            })
            source_functions.append(ogre_callback)
        elif unit_rawcode == "n02L":
            require_literals(
                mechanic,
                "CallbackSingle_doAfter_RaceChaosAbilities_call_doAfter_RaceChaosAbilities1",
                ("600.", "100."),
            )
            require_literals(mechanic, mechanic["handler_function"], ("700.", "3."))
            semantic_kind = "projectile-aoe-mana-burn"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0FS"]
            parameters.update({
                "impact_radius": 600,
                "max_mana_burn_per_target": 100,
                "bonus_damage_equals_mana_burn": True,
                "projectile_speed": 700,
                "carrier_timed_life_seconds": 3,
                "impact_ability_rawcode": "A0FS",
                "target_predicate": "alive-combat-sapper;enemy;ground;missing-Avul;missing-A08H",
            })
            source_functions.extend([
                "CallbackSingle_doAfter_RaceChaosAbilities_call_doAfter_RaceChaosAbilities1",
                "ForGroupCallback_forUnitsInRange_doAfter_RaceChaosAbilities_callback_forUnitsInRange_doAfter_RaceChaosAbilities",
            ])
        elif unit_rawcode == "z003":
            require_literals(
                mechanic,
                "CallbackSingle_doAfter_RaceMechAbilities_call_doAfter_RaceMechAbilities2",
                ("300.", "100."),
            )
            require_literals(mechanic, mechanic["handler_function"], ("700.", "3."))
            semantic_kind = "projectile-aoe-mana-burn"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A09E"]
            parameters.update({
                "impact_radius": 300,
                "max_mana_burn_per_target": 100,
                "bonus_damage_equals_mana_burn": True,
                "projectile_speed": 700,
                "carrier_timed_life_seconds": 3,
                "impact_ability_rawcode": "A09E",
                "target_predicate": "alive-combat-sapper;enemy;ground;missing-Avul;missing-A08H",
            })
            source_functions.extend([
                "CallbackSingle_doAfter_RaceMechAbilities_call_doAfter_RaceMechAbilities2",
                "ForGroupCallback_forUnitsInRange_doAfter_RaceMechAbilities_callback_forUnitsInRange_doAfter_RaceMechAbilities",
            ])
        elif unit_rawcode == "n032":
            callback = "CallbackSingle_doAfter_RaceDesertAbilities_call_doAfter_RaceDesertAbilities2"
            require_literals(mechanic, callback, ("75.",))
            semantic_kind = "delayed-life-steal"
            normalization_status = "script-native-ready"
            effect_rawcodes = []
            parameters.update({
                "maximum_life_stolen": 75,
                "damage_equals_life_stolen": True,
                "heal_caster_by_actual_damage": True,
                "damage_attack_type": "chaos",
                "damage_type": "normal",
                "random_delay_seconds": [0.1, 0.6],
            })
            source_functions.append(callback)
        elif unit_rawcode == "h06U":
            require_literals(mechanic, "mineLayerSpell", ("50.", "220.", "700.", "3."))
            semantic_kind = "random-offset-landmine-placement"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["n025"]
            parameters.update({
                "placement_distance_min": 50,
                "placement_distance_max": 220,
                "placement_angle_degrees": [0, 360],
                "carrier_speed": 700,
                "carrier_timed_life_seconds": 3,
                "carrier_unit_rawcode": "h09M",
                "landmine_unit_rawcode": "n025",
            })
            source_functions.extend([
                "mineLayerSpell",
                "CallbackSingle_doAfter_RaceMechAbilities_call_doAfter_RaceMechAbilities3",
            ])
        elif unit_rawcode == "e00C":
            require_literals(mechanic, "keeperSpell", ("144.", "160.", "218.", "120.", "3."))
            require_literals(mechanic, "spawnTreantFromWall", ("112.", "20."))
            semantic_kind = "five-tree-wall-to-treants"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["YTct", "e00D"]
            parameters.update({
                "tree_rawcode": "YTct",
                "tree_count": 5,
                "tree_radius": 120,
                "tree_angles_degrees": [0, 72, 144, 216, 288],
                "nearby_ground_clearance_radius": 144,
                "nearby_unit_displacement_radius": [160, 218],
                "transform_delay_seconds": 3,
                "treant_rawcode": "e00D",
                "treant_spawn_radius": 112,
                "treant_timed_life_seconds": 20,
            })
            source_functions.extend([
                "keeperSpell",
                "CallbackSingle_doAfter_RaceNatureAbilities_call_doAfter_RaceNatureAbilities4",
                "spawnTreantFromWall",
                "CallbackSingle_doAfter_doAfter_RaceNatureAbilities_call_doAfter_doAfter_RaceNatureAbilities1",
            ])
        elif unit_rawcode in {"n030", "n035"}:
            require_literals(mechanic, "spellSandCloud", ("5.7", "1."))
            require_literals(
                mechanic,
                "CallbackPeriodic_doPeriodically_RaceDesertAbilities_call_doPeriodically_RaceDesertAbilities",
                ("5",),
            )
            effect_rawcode = "A0HP" if unit_rawcode == "n035" else "A0GA"
            semantic_kind = "periodic-sandcloud-carrier"
            normalization_status = "script-native-ready"
            effect_rawcodes = [effect_rawcode]
            parameters.update({
                "effect_ability_rawcode": effect_rawcode,
                "carrier_lifetime_seconds": 5.7,
                "period_seconds": 1,
                "cast_iterations": 5,
            })
            source_functions.extend([
                "spellSandCloud",
                "CallbackPeriodic_doPeriodically_RaceDesertAbilities_call_doPeriodically_RaceDesertAbilities",
            ])
        elif unit_rawcode == "n01Y":
            require_literals(mechanic, "solarStrikeSpell", ("400.", "4", "3."))
            semantic_kind = "bounded-multi-target-proxy"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A08U"]
            parameters.update({
                "search_radius": 400,
                "max_targets": 4,
                "effect_ability_rawcode": "A08U",
                "dummy_lifetime_seconds": 3,
                "target_predicate": "alive-combat-sapper;enemy;flying;missing-Avul",
            })
            source_functions.extend([
                "solarStrikeSpell",
                "ForGroupCallback_forUnitsInRange_RaceElvenAbilities_callback_forUnitsInRange_RaceElvenAbilities",
            ])
        elif unit_rawcode == "n02G":
            require_literals(mechanic, "forestTrollTrapperSpell", ("800.", "5", "2."))
            semantic_kind = "bounded-multi-target-proxy"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0FC"]
            parameters.update({
                "search_radius": 800,
                "max_targets": 5,
                "effect_ability_rawcode": "A0FC",
                "dummy_lifetime_seconds": 2,
                "target_predicate": "alive-combat-sapper;enemy;flying",
            })
            source_functions.extend([
                "forestTrollTrapperSpell",
                "ForGroupCallback_forUnitsInRange_RaceOrcAbilities_callback_forUnitsInRange_RaceOrcAbilities",
            ])
        elif unit_rawcode == "n027":
            require_literals(mechanic, "annihilatorSpell", ("3", "20.", "10.", "10.", "2."))
            semantic_kind = "four-lane-cone-proxy"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0AF"]
            parameters.update({
                "lane_count": 4,
                "relative_angles_degrees": [-20, -10, 0, 10],
                "cast_origin_offset": 10,
                "effect_ability_rawcode": "A0AF",
                "dummy_lifetime_seconds": 2,
            })
            source_functions.append("annihilatorSpell")
        elif unit_rawcode == "e00J":
            require_literals(mechanic, "assassinSpell", ("300.", "3", "11."))
            semantic_kind = "bounded-multi-target-proxy"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A07Q"]
            parameters.update({
                "search_radius": 300,
                "max_targets": 3,
                "effect_ability_rawcode": "A07Q",
                "dummy_lifetime_seconds": 11,
                "selection_policy": "script-priority-list-then-fallback",
            })
            source_functions.append("assassinSpell")
        elif unit_rawcode == "h062":
            require_literals(mechanic, "improveSpecialBuilding", ("1.", "2"))
            cross_runtime = building_improvement_spawn_by_source.get("h062")
            if cross_runtime is None:
                raise ValueError("Mana Generator semantic is missing A0EL reachability audit evidence")
            if cross_runtime["mechanic_kind"] != "unreachable-production-spawn-branch-keyed-by-A0EL":
                raise ValueError(f"Mana Generator cross-runtime reachability classification changed: {cross_runtime['mechanic_kind']}")
            unreachable_spawn_code = json.loads(cross_runtime["parameters_json"])
            if (
                unreachable_spawn_code.get("improvement_ability_id") != 1093682508
                or unreachable_spawn_code.get("gameplay_reachable_from_legal_mana_generator_target") is not False
                or unreachable_spawn_code.get("mana_generator_rejects_production_buildings") is not True
                or unreachable_spawn_code.get("normal_unit_train_finish_uses_setup_unit") is not False
            ):
                raise ValueError(f"Mana Generator cross-runtime reachability evidence changed: {unreachable_spawn_code}")
            semantic_kind = "permanent-special-building-mana-regen-improvement"
            normalization_status = "script-native-ready"
            effect_rawcodes = ["A0EL"]
            parameters.update({
                "improvement_ability_rawcode": "A0EL",
                "normal_level": 1,
                "elemental_level": 2,
                "normal_mana_regen_bonus": 0.15,
                "elemental_mana_regen_bonus": 0.30,
                "elemental_marker_rawcode": "A0DS",
                "one_generator_per_target": True,
                "target_requires_mana": True,
                "target_requires_special_building": True,
                "target_rejects_production_buildings": True,
                "unreachable_spawn_code_audit": {
                    "mechanic_kind": cross_runtime["mechanic_kind"],
                    "gameplay_reachable": False,
                    "reason": "legal A0EL targets have no production spawn rawcode; setupUnit is only used by production companion-spawn paths",
                },
            })
            source_functions.append("improveSpecialBuilding")

        enriched_effects: list[dict[str, Any]] = []
        for rawcode in effect_rawcodes:
            enriched: dict[str, Any] = {"rawcode": rawcode}
            definitions = ability_levels.get(rawcode, [])
            definition = next((row for row in definitions if row["level"] == "1"), None)
            if definition is None:
                definition = inherited_ability_level_one(rawcode)
            if definition is not None:
                enriched["ability_level1"] = {
                    "name": definition["name"],
                    "range": definition["range"],
                    "area": definition["area"],
                    "targets": definition["targets"],
                    "buffs": definition["buffs"],
                    "duration_normal": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "adur", 1, 0)),
                    "duration_hero": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "ahdu", 1, 0)),
                    "data_fields_labeled_json": definition["data_fields_labeled_json"],
                }
            unit_effect = static_units.get(rawcode)
            if unit_effect is not None:
                enriched["unit_object"] = {
                    "name": unit_effect["name"],
                    "abilities": unit_effect["abilities"],
                    "move_speed": unit_effect["move_speed"],
                }
            enriched_effects.append(enriched)

        source_functions = list(dict.fromkeys(function for function in source_functions if function))
        unit_spell_semantic_status_counts[normalization_status] += 1
        unit_spell_semantic_kind_counts[semantic_kind] += 1
        unit_spell_semantic_rows.append([
            unit_rawcode, mechanic["unit_name"], mechanic["production_building_rawcode"], mechanic["production_building_names"],
            mechanic["ability_rawcode"], mechanic["ability_name"], mechanic["effective_mana_cost"], mechanic["effective_cooldown"],
            mechanic["target_mode"], mechanic["target_mode_label"], semantic_kind, normalization_status,
            ",".join(effect_rawcodes),
            json.dumps(enriched_effects, separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            json.dumps(parameters, separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            ",".join(source_functions), mechanic["evidence_kind"], mechanic["byte_offset"],
        ])
    write_tsv(
        output / "unit-spell-semantics.tsv",
        [
            "unit_rawcode", "unit_name", "production_building_rawcode", "production_building_names",
            "ability_rawcode", "ability_name", "effective_mana_cost", "effective_cooldown",
            "target_mode", "target_mode_label", "semantic_kind", "normalization_status",
            "effect_rawcodes", "effect_objects_json", "parameters_json", "source_functions", "evidence_kind", "byte_offset",
        ],
        unit_spell_semantic_rows,
    )
    write_tsv(
        output / "protected-filter-bindings.tsv",
        [
            "symbol", "initializer_function", "resolved_function", "predicate",
            "resolution_status", "evidence_kind", "byte_offset",
        ],
        [
            [
                row["symbol"], row["initializer_function"], row["resolved_function"], row["predicate"],
                row["resolution_status"], row["evidence_kind"], row["byte_offset"],
            ]
            for row in protected_filter_rows
        ],
    )

    def ability_object(rawcode: str) -> dict[str, Any]:
        levels: list[dict[str, Any]] = []
        for definition in sorted(ability_levels.get(rawcode, []), key=lambda value: int(value["level"])):
            level = int(definition["level"])
            levels.append({
                "level": level,
                "name": definition["name"],
                "mana_cost": protected_ability_values.get((rawcode, level, "mana_cost"), definition["mana_cost"]),
                "cooldown": protected_ability_values.get((rawcode, level, "cooldown"), definition["cooldown"]),
                "static_mana_cost": definition["mana_cost"],
                "static_cooldown": definition["cooldown"],
                "range": definition["range"],
                "area": definition["area"],
                "duration_normal": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "adur", level, 0)),
                "duration_hero": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "ahdu", level, 0)),
                "targets": definition["targets"],
                "buffs": definition["buffs"],
                "data_fields_labeled": json.loads(definition["data_fields_labeled_json"] or "{}"),
            })
        return {"rawcode": rawcode, "levels": levels}

    normalized_items: dict[str, dict[str, str]] = {}
    with (output / "items.tsv").open(encoding="utf-8", newline="") as handle:
        normalized_items = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    def compact_item_object(rawcode: str) -> dict[str, Any]:
        definition = normalized_items.get(rawcode, {})
        abilities = rawcode_list(definition.get("abilities", ""))
        return {
            "rawcode": rawcode,
            "name": definition.get("name", ""),
            "gold_cost": definition.get("gold_cost", ""),
            "lumber_cost": definition.get("lumber_cost", ""),
            "abilities": abilities,
            "ability_objects": [ability_object(ability) for ability in abilities],
            "tip": definition.get("tip", ""),
            "ubertip": definition.get("ubertip", ""),
        }

    castle_shop_path = map_root / "script" / "castle-shop-items.tsv"
    castle_shop_rows: list[dict[str, str]] = []
    if castle_shop_path.exists():
        with castle_shop_path.open(encoding="utf-8", newline="") as handle:
            castle_shop_rows = list(csv.DictReader(handle, delimiter="\t"))
    resolved_shop_rows: list[list[Any]] = []
    shop_item_by_trigger_ability: dict[str, dict[str, str]] = {}
    for row in castle_shop_rows:
        item_rawcode = row["item_rawcode"]
        item = normalized_items.get(item_rawcode, {})
        abilities = rawcode_list(item.get("abilities", ""))
        for ability in abilities:
            shop_item_by_trigger_ability.setdefault(ability, row)
        resolved_shop_rows.append([
            row["slot"], item_rawcode, row["item_rawcode_integer"], item.get("name", row["item_name"]), row["script_item_value"],
            item.get("gold_cost", ""), item.get("lumber_cost", ""), item.get("class", ""), item.get("uses", ""),
            item.get("stack_max", ""), item.get("stock_max", ""), item.get("stock_initial", ""), item.get("stock_start", ""),
            item.get("stock_regen", ""), item.get("cooldown_group", ""), item.get("usable", ""), item.get("perishable", ""),
            item.get("powerup", ""), item.get("abilities", ""),
            json.dumps([ability_object(ability) for ability in abilities], separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            item.get("description", ""), item.get("tip", ""), item.get("ubertip", ""), row["source_function"], row["byte_offset"],
        ])
    write_tsv(
        output / "castle-shop-items.tsv",
        [
            "slot", "item_rawcode", "item_rawcode_integer", "item_name", "script_item_value",
            "gold_cost", "lumber_cost", "class", "uses", "stack_max", "stock_max", "stock_initial", "stock_start", "stock_regen",
            "cooldown_group", "usable", "perishable", "powerup", "ability_rawcodes", "ability_objects_json",
            "description", "tip", "ubertip", "source_function", "byte_offset",
        ],
        resolved_shop_rows,
    )

    item_mechanics_path = map_root / "script" / "item-mechanics.tsv"
    item_mechanic_rows: list[dict[str, str]] = []
    if item_mechanics_path.exists():
        with item_mechanics_path.open(encoding="utf-8", newline="") as handle:
            item_mechanic_rows = list(csv.DictReader(handle, delimiter="\t"))
    item_spell_path = map_root / "script" / "item-spell-mechanics.tsv"
    item_spell_rows: list[dict[str, str]] = []
    if item_spell_path.exists():
        with item_spell_path.open(encoding="utf-8", newline="") as handle:
            item_spell_rows = list(csv.DictReader(handle, delimiter="\t"))

    resolved_item_mechanics: list[list[Any]] = []

    def related_items(parameters: dict[str, Any]) -> tuple[str, str]:
        rawcodes = list(dict.fromkeys(
            str(value)
            for key, value in parameters.items()
            if key.endswith("item_rawcode") and isinstance(value, str) and value
        ))
        return (
            ",".join(rawcodes),
            json.dumps([compact_item_object(rawcode) for rawcode in rawcodes], separators=(",", ":"), sort_keys=True, ensure_ascii=False),
        )

    for row in item_mechanic_rows:
        effects = rawcode_list(row["linked_ability_rawcodes"])
        parameters = json.loads(row["parameters_json"] or "{}")
        related_rawcodes, related_json = related_items(parameters)
        resolved_item_mechanics.append([
            row["item_rawcode"], row["item_rawcode_integer"], row["item_name"], row["mechanic_kind"],
            "", row["linked_ability_rawcodes"], "[]",
            json.dumps([ability_object(rawcode) for rawcode in effects], separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            related_rawcodes, related_json, row["parameters_json"], row["source_functions"], row["evidence_kind"],
        ])

    for row in item_spell_rows:
        shop_item = shop_item_by_trigger_ability.get(row["trigger_ability_rawcode"])
        if shop_item is None:
            continue
        effects = rawcode_list(row["effect_ability_rawcodes"])
        parameters = json.loads(row["parameters_json"] or "{}")
        related_rawcodes, related_json = related_items(parameters)
        resolved_item_mechanics.append([
            shop_item["item_rawcode"], shop_item["item_rawcode_integer"], shop_item["item_name"], row["mechanic_kind"],
            row["trigger_ability_rawcode"], row["effect_ability_rawcodes"],
            json.dumps([ability_object(row["trigger_ability_rawcode"])], separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            json.dumps([ability_object(rawcode) for rawcode in effects], separators=(",", ":"), sort_keys=True, ensure_ascii=False),
            related_rawcodes, related_json, row["parameters_json"], row["source_functions"], row["evidence_kind"],
        ])

    write_tsv(
        output / "item-mechanics.tsv",
        [
            "item_rawcode", "item_rawcode_integer", "item_name", "mechanic_kind",
            "trigger_ability_rawcodes", "effect_ability_rawcodes", "trigger_ability_objects_json", "effect_ability_objects_json",
            "related_item_rawcodes", "related_item_objects_json", "parameters_json", "source_functions", "evidence_kind",
        ],
        resolved_item_mechanics,
    )

    # Join the map's generated UnitObjectMeta table to its complete race
    # partition and authored upgrade graph. This is the preferred native import
    # view for Castle Fight building/production definitions: spawn time and
    # costs come from the metadata consumed and validated by CFBuilding_setup,
    # not from generic Warcraft construction-time fields or tooltip parsing.
    resolved_buildings: dict[str, dict[str, str]] = {}
    with (output / "buildings.tsv").open(encoding="utf-8", newline="") as handle:
        resolved_buildings = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    unit_object_metadata_path = map_root / "script" / "unit-object-metadata.tsv"
    race_buildings_path = map_root / "script" / "race-buildings.tsv"
    building_upgrades_path = map_root / "script" / "building-upgrades.tsv"
    race_semantics_path = map_root / "script" / "race-building-semantics.tsv"
    metadata_rows: list[dict[str, str]] = []
    race_by_building: dict[str, dict[str, str]] = {}
    semantics_by_building: dict[str, dict[str, str]] = {}
    upgrades_from: dict[str, list[str]] = defaultdict(list)
    upgrades_to: dict[str, list[str]] = defaultdict(list)
    xo_buildings: set[str] = set()

    if unit_object_metadata_path.exists():
        with unit_object_metadata_path.open(encoding="utf-8", newline="") as handle:
            metadata_rows = list(csv.DictReader(handle, delimiter="\t"))
    if race_buildings_path.exists():
        with race_buildings_path.open(encoding="utf-8", newline="") as handle:
            race_by_building = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
    if building_upgrades_path.exists():
        with building_upgrades_path.open(encoding="utf-8", newline="") as handle:
            for row in csv.DictReader(handle, delimiter="\t"):
                upgrades_to[row["source_building_rawcode"]].append(row["target_building_rawcode"])
                upgrades_from[row["target_building_rawcode"]].append(row["source_building_rawcode"])
    if race_semantics_path.exists():
        with race_semantics_path.open(encoding="utf-8", newline="") as handle:
            semantics_by_building = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
    if effective_unit_path.exists():
        with effective_unit_path.open(encoding="utf-8", newline="") as handle:
            xo_buildings = {row["building_rawcode"] for row in csv.DictReader(handle, delimiter="\t")}

    metadata_buildings = {row["building_rawcode"] for row in metadata_rows}
    if metadata_rows and set(race_by_building) != metadata_buildings:
        raise ValueError("generated race catalogs do not exactly partition UnitObjectMeta buildings")
    if metadata_rows and set(semantics_by_building) != metadata_buildings:
        raise ValueError("race wrapper semantics do not exactly cover UnitObjectMeta buildings")

    building_spell_rows: list[list[Any]] = []
    building_spell_by_pair: dict[tuple[str, str], dict[str, str]] = {}
    building_spell_registration_path = map_root / "script" / "building-spell-registrations.tsv"
    if building_spell_registration_path.exists():
        with building_spell_registration_path.open(encoding="utf-8", newline="") as handle:
            for registration in csv.DictReader(handle, delimiter="\t"):
                building_rawcode = registration["building_rawcode"]
                race = race_by_building.get(building_rawcode)
                if race is None:
                    raise ValueError(f"scripted building spell references building outside race catalog: {building_rawcode}")
                ability_rawcode = registration["ability_rawcode"]
                definitions = ability_levels.get(ability_rawcode, [])
                definition = next((row for row in definitions if row["level"] == "1"), None)
                definition_source = "map-resolved"
                if definition is None:
                    definition = inherited_ability_level_one(ability_rawcode)
                    definition_source = "inherited-base"
                if definition is None:
                    raise ValueError(f"scripted building spell ability has no resolved definition: {ability_rawcode}")
                runtime_mana = protected_ability_values.get((ability_rawcode, 1, "mana_cost"))
                runtime_cooldown = protected_ability_values.get((ability_rawcode, 1, "cooldown"))
                static_mana = definition["mana_cost"]
                static_cooldown = definition["cooldown"]
                effective_mana = runtime_mana if runtime_mana is not None else static_mana
                effective_wc3_cooldown = runtime_cooldown if runtime_cooldown is not None else static_cooldown
                building_unit = static_units.get(building_rawcode)
                if building_unit is None:
                    raise ValueError(f"scripted building spell has no static unit definition: {building_rawcode}")
                mana_regen = numeric(building_unit["mana_regen"])
                mana_cost = numeric(effective_mana)
                wc3_cooldown = numeric(effective_wc3_cooldown)
                if mana_regen is not None and mana_regen > 0 and mana_cost is not None and mana_cost > 0:
                    cadence_seconds = mana_cost / mana_regen
                    cadence_text = value_as_text(cadence_seconds)
                    cadence_source = "ability-mana-cost/building-mana-regen"
                elif wc3_cooldown is not None and wc3_cooldown > 0:
                    # Direct event-listener building spells can be ordinary
                    # cooldown-driven WC3 casts rather than the Castle Fight
                    # full-mana timer convention (for example Tidal Guardian).
                    cadence_text = value_as_text(wc3_cooldown)
                    cadence_source = "effective-wc3-ability-cooldown"
                else:
                    cadence_text = ""
                    cadence_source = "event-driven/no-static-cadence"
                building_spell_by_pair[(building_rawcode, ability_rawcode)] = {
                    "race_index": race["race_index"],
                    "race_function": race["race_function"],
                    "builder_rawcode": race["builder_rawcode"],
                    "builder_names": race["builder_names"],
                    "campaign_only": race["campaign_only"],
                    "building_names": registration["building_names"],
                    "ability_name": definition["name"],
                    "ability_tip": definition["tip"],
                    "ability_ubertip": definition["ubertip"],
                    "cadence_seconds": cadence_text,
                    "cadence_source": cadence_source,
                }
                building_spell_rows.append([
                    race["race_index"], race["race_function"], race["builder_rawcode"], race["builder_names"], race["campaign_only"],
                    building_rawcode, registration["building_names"],
                    ability_rawcode, definition_source, definition["base_rawcode"], definition["name"], definition["tip"], definition["ubertip"],
                    static_mana, effective_mana,
                    "protected-runtime" if runtime_mana is not None else "static-resolved",
                    building_unit["mana_regen"], cadence_text, cadence_source,
                    static_cooldown, effective_wc3_cooldown,
                    "protected-runtime" if runtime_cooldown is not None else "static-resolved",
                    definition["range"], definition["area"], definition["targets"], definition["buffs"],
                    definition["data_fields_json"], definition["data_fields_labeled_json"],
                    registration["handler_function"], registration["registration_function"], registration.get("evidence_kind", ""), registration["byte_offset"],
                ])
    write_tsv(
        output / "building-spells.tsv",
        [
            "race_index", "race_function", "builder_rawcode", "builder_names", "campaign_only",
            "building_rawcode", "building_names", "ability_rawcode", "definition_source", "base_rawcode",
            "name", "tip", "ubertip", "static_mana_cost", "effective_mana_cost", "mana_cost_source",
            "building_mana_regen", "cadence_seconds", "cadence_source",
            "static_wc3_cooldown", "effective_wc3_cooldown", "wc3_cooldown_source", "range", "area", "targets", "buffs",
            "data_fields_json", "data_fields_labeled_json", "handler_function", "registration_function", "registration_evidence_kind", "byte_offset",
        ],
        building_spell_rows,
    )

    building_spell_evidence_rows: list[list[Any]] = []
    building_spell_evidence_path = map_root / "script" / "building-spell-evidence.tsv"
    if building_spell_evidence_path.exists():
        with building_spell_evidence_path.open(encoding="utf-8", newline="") as handle:
            for evidence in csv.DictReader(handle, delimiter="\t"):
                pair = (evidence["building_rawcode"], evidence["ability_rawcode"])
                spell = building_spell_by_pair.get(pair)
                if spell is None:
                    raise ValueError(f"building-spell evidence has no scripted registration: {pair}")
                reachable = json.loads(evidence["reachable_map_rawcode_paths_json"])
                enriched_objects: list[dict[str, Any]] = []
                for effect in reachable:
                    rawcode = str(effect["rawcode"])
                    enriched: dict[str, Any] = dict(effect)
                    definitions = ability_levels.get(rawcode, [])
                    definition = next((row for row in definitions if row["level"] == "1"), None)
                    if definition is None:
                        definition = inherited_ability_level_one(rawcode)
                    if definition is not None:
                        enriched["ability_level1"] = {
                            "name": definition["name"],
                            "range": definition["range"],
                            "area": definition["area"],
                            "targets": definition["targets"],
                            "buffs": definition["buffs"],
                            "duration_normal": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "adur", 1, 0)),
                            "duration_hero": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "ahdu", 1, 0)),
                            "data_fields_labeled_json": definition["data_fields_labeled_json"],
                        }
                    unit_effect = static_units.get(rawcode)
                    if unit_effect is not None:
                        enriched["unit_object"] = {
                            "name": unit_effect["name"],
                            "abilities": unit_effect["abilities"],
                            "move_speed": unit_effect["move_speed"],
                            "attack1_type": unit_effect["attack1_type"],
                            "attack1_weapon_type": unit_effect["attack1_weapon_type"],
                            "attack1_targets": unit_effect["attack1_targets"],
                        }
                    enriched_objects.append(enriched)
                building_spell_evidence_rows.append([
                    spell["race_index"], spell["builder_rawcode"], spell["builder_names"], spell["campaign_only"],
                    evidence["building_rawcode"], spell["building_names"], evidence["ability_rawcode"],
                    spell["ability_name"], spell["ability_tip"], spell["ability_ubertip"],
                    spell["cadence_seconds"], spell["cadence_source"],
                    evidence["mechanic_kind"], evidence["direct_calls"], evidence["helper_functions"],
                    evidence["delayed_callback_functions"], evidence["dynamic_callback_functions"],
                    evidence["scheduled_delays_json"], evidence["periodic_intervals_json"], evidence["random_real_ranges_json"],
                    evidence["direct_map_rawcodes"],
                    json.dumps(enriched_objects, separators=(",", ":"), sort_keys=True, ensure_ascii=False),
                    evidence["semantic_effect_sites_json"], evidence["source_numeric_literals_json"],
                    evidence["handler_function"], evidence["evidence_kind"], evidence["byte_offset"],
                ])
    write_tsv(
        output / "building-spell-evidence.tsv",
        [
            "race_index", "builder_rawcode", "builder_names", "campaign_only",
            "building_rawcode", "building_names", "ability_rawcode", "ability_name", "ability_tip", "ability_ubertip",
            "cadence_seconds", "cadence_source", "mechanic_kind", "direct_calls", "helper_functions",
            "delayed_callback_functions", "dynamic_callback_functions", "scheduled_delays_json", "periodic_intervals_json",
            "random_real_ranges_json", "direct_map_rawcodes", "reachable_map_objects_json", "semantic_effect_sites_json",
            "source_numeric_literals_json", "handler_function", "evidence_kind", "byte_offset",
        ],
        building_spell_evidence_rows,
    )

    corpse_building_rows: list[list[Any]] = []
    corpse_building_path = map_root / "script" / "corpse-building-mechanics.tsv"
    if corpse_building_path.exists():
        with corpse_building_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                pair = (mechanic["building_rawcode"], mechanic["ability_rawcode"])
                spell = building_spell_by_pair.get(pair)
                if spell is None:
                    raise ValueError(f"corpse building mechanic has no scripted building-spell registration: {pair}")
                auxiliary_rawcode = mechanic["auxiliary_ability_rawcode"]
                auxiliary_name = ""
                if auxiliary_rawcode:
                    auxiliary_definition = next(
                        (row for row in ability_levels.get(auxiliary_rawcode, []) if row["level"] == "1"),
                        None,
                    )
                    if auxiliary_definition is None:
                        auxiliary_definition = inherited_ability_level_one(auxiliary_rawcode)
                    if auxiliary_definition is None:
                        raise ValueError(f"corpse mechanic auxiliary ability has no definition: {auxiliary_rawcode}")
                    auxiliary_name = auxiliary_definition["name"]
                invulnerable_rawcode = mechanic["invulnerable_ability_rawcode"]
                invulnerable_name = ""
                if invulnerable_rawcode:
                    invulnerable_definition = inherited_ability_level_one(invulnerable_rawcode)
                    if invulnerable_definition is None:
                        invulnerable_definition = next(
                            (row for row in ability_levels.get(invulnerable_rawcode, []) if row["level"] == "1"),
                            None,
                        )
                    if invulnerable_definition is None:
                        raise ValueError(f"corpse mechanic exclusion ability has no definition: {invulnerable_rawcode}")
                    invulnerable_name = invulnerable_definition["name"]
                corpse_building_rows.append([
                    spell["race_index"], spell["race_function"], spell["builder_rawcode"], spell["builder_names"], spell["campaign_only"],
                    mechanic["building_rawcode"], spell["building_names"], mechanic["ability_rawcode"], spell["ability_name"], spell["ability_tip"],
                    spell["cadence_seconds"], spell["cadence_source"],
                    mechanic["mechanic_kind"], mechanic["corpse_phase"], mechanic["selection_predicate"],
                    mechanic["selection_rect_symbol"], mechanic["selection_function"], mechanic["requires_wc3_can_raise"],
                    mechanic["consumption_mode"], mechanic["consume_radius"], mechanic["effect_radius"], mechanic["damage"],
                    mechanic["attack_type"], mechanic["damage_type"], auxiliary_rawcode, auxiliary_name,
                    invulnerable_rawcode, invulnerable_name, mechanic["summon_outcomes_json"],
                    mechanic["handler_function"], mechanic["predicate_function"], mechanic["effect_function"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "corpse-building-mechanics.tsv",
        [
            "race_index", "race_function", "builder_rawcode", "builder_names", "campaign_only",
            "building_rawcode", "building_names", "ability_rawcode", "ability_name", "ability_tip",
            "cadence_seconds", "cadence_source", "mechanic_kind", "corpse_phase", "selection_predicate",
            "selection_rect_symbol", "selection_function", "requires_wc3_can_raise", "consumption_mode",
            "consume_radius", "effect_radius", "damage", "attack_type", "damage_type",
            "auxiliary_ability_rawcode", "auxiliary_ability_name", "invulnerable_ability_rawcode", "invulnerable_ability_name",
            "summon_outcomes_json", "handler_function", "predicate_function", "effect_function", "byte_offset",
        ],
        corpse_building_rows,
    )

    building_spell_mechanic_rows: list[list[Any]] = []
    building_spell_mechanics_path = map_root / "script" / "building-spell-mechanics.tsv"
    protected_unit_details: dict[str, dict[str, str]] = {}
    protected_unit_details_path = output / "protected-unit-stats.tsv"
    if protected_unit_details_path.exists():
        with protected_unit_details_path.open(encoding="utf-8", newline="") as handle:
            protected_unit_details = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}

    if building_spell_mechanics_path.exists():
        with building_spell_mechanics_path.open(encoding="utf-8", newline="") as handle:
            for mechanic in csv.DictReader(handle, delimiter="\t"):
                pair = (mechanic["building_rawcode"], mechanic["ability_rawcode"])
                spell = building_spell_by_pair.get(pair)
                if spell is None:
                    raise ValueError(f"building-spell mechanic has no scripted registration: {pair}")
                effects = json.loads(mechanic["effect_rawcodes_json"])
                enriched_effects: list[dict[str, Any]] = []
                for effect in effects:
                    rawcode = str(effect["rawcode"])
                    enriched: dict[str, Any] = dict(effect)
                    definitions = ability_levels.get(rawcode, [])
                    definition = next((row for row in definitions if row["level"] == "1"), None)
                    if definition is None:
                        definition = inherited_ability_level_one(rawcode)
                    if definition is not None:
                        enriched["ability_level1"] = {
                            "name": definition["name"],
                            "range": definition["range"],
                            "area": definition["area"],
                            "targets": definition["targets"],
                            "buffs": definition["buffs"],
                            "duration_normal": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "adur", 1, 0)),
                            "duration_hero": value_as_text(field_lookup(rows_by_object, "abilities", rawcode, "ahdu", 1, 0)),
                            "data_fields_labeled_json": definition["data_fields_labeled_json"],
                        }
                    unit = static_units.get(rawcode)
                    if unit is not None:
                        enriched["unit_object"] = {
                            "name": unit["name"],
                            "abilities": unit["abilities"],
                            "attack1_type": unit["attack1_type"],
                            "attack1_weapon_type": unit["attack1_weapon_type"],
                            "attack1_targets": unit["attack1_targets"],
                            "attack1_full_aoe": unit["attack1_full_aoe"],
                        }
                    protected_unit = protected_unit_details.get(rawcode)
                    if protected_unit is not None:
                        enriched["protected_unitstat"] = {
                            "attack1_min": protected_unit["unitstat_attack1_min"],
                            "attack1_max": protected_unit["unitstat_attack1_max"],
                            "attack1_cooldown": protected_unit["unitstat_attack1_cooldown"],
                            "attack1_range": protected_unit["unitstat_attack1_range"],
                            "override_fields": protected_unit["override_fields"],
                        }
                    enriched_effects.append(enriched)

                evidence_disagreements: list[str] = []
                clean_ubertip = re.sub(r"\|c[0-9A-Fa-f]{8}|\|r", "", spell["ability_ubertip"]).replace("|n", " ")
                tooltip_damage_match = re.search(r"\bdealing\s+(\d+(?:\.\d+)?)\s+spell damage\b", clean_ubertip, re.IGNORECASE)
                if tooltip_damage_match is not None:
                    tooltip_damage = float(tooltip_damage_match.group(1))
                    for enriched in enriched_effects:
                        ability = enriched.get("ability_level1")
                        if not isinstance(ability, dict):
                            continue
                        labeled = json.loads(str(ability["data_fields_labeled_json"]))
                        linked_damage = labeled.get("Damage")
                        if isinstance(linked_damage, (int, float)) and not math.isclose(
                            tooltip_damage, float(linked_damage), rel_tol=0.0, abs_tol=1e-9
                        ):
                            evidence_disagreements.append(
                                f"tooltip-spell-damage={value_as_text(tooltip_damage)};"
                                f"linked-{enriched['rawcode']}-Damage={value_as_text(linked_damage)}"
                            )

                building_spell_mechanic_rows.append([
                    spell["race_index"], spell["builder_rawcode"], spell["builder_names"], spell["campaign_only"],
                    mechanic["building_rawcode"], spell["building_names"], mechanic["ability_rawcode"],
                    spell["ability_name"], spell["ability_tip"], spell["ability_ubertip"],
                    spell["cadence_seconds"], spell["cadence_source"], mechanic["mechanic_kind"],
                    mechanic["target_selector"], mechanic["target_predicate"], mechanic["evidence_kind"],
                    ";".join(evidence_disagreements),
                    json.dumps(enriched_effects, separators=(",", ":"), sort_keys=True, ensure_ascii=False),
                    mechanic["parameters_json"], mechanic["source_functions"], mechanic["byte_offset"],
                ])
    write_tsv(
        output / "building-spell-mechanics.tsv",
        [
            "race_index", "builder_rawcode", "builder_names", "campaign_only",
            "building_rawcode", "building_names", "ability_rawcode", "ability_name", "ability_tip", "ability_ubertip",
            "cadence_seconds", "cadence_source", "mechanic_kind", "target_selector", "target_predicate", "evidence_kind",
            "evidence_disagreements", "effect_objects_json", "parameters_json", "source_functions", "byte_offset",
        ],
        building_spell_mechanic_rows,
    )

    def catalog_income(building_rawcode: str, stack: tuple[str, ...] = ()) -> float:
        if building_rawcode in stack:
            raise ValueError(f"cycle in CFBuilding precursor income chain: {' -> '.join(stack + (building_rawcode,))}")
        meta = next((row for row in metadata_rows if row["building_rawcode"] == building_rawcode), None)
        semantics = semantics_by_building.get(building_rawcode)
        if meta is None or semantics is None:
            raise ValueError(f"missing metadata/semantics while calculating CFBuilding income: {building_rawcode}")
        gold = numeric(meta["gold_cost"])
        factor = numeric(semantics["income_factor"])
        if gold is None or factor is None:
            raise ValueError(f"non-numeric CFBuilding income inputs for {building_rawcode}")
        own = gold * factor / 100.0
        precursor = semantics["precursor_rawcode"]
        return own + (catalog_income(precursor, stack + (building_rawcode,)) if precursor else 0.0)

    production_building_rows: list[list[Any]] = []
    production_building_count = 0
    campaign_production_count = 0
    normal_production_count = 0
    normal_xo_production_count = 0
    two_second_production_build_count = 0
    for meta in metadata_rows:
        building_rawcode = meta["building_rawcode"]
        building = resolved_buildings.get(building_rawcode)
        race = race_by_building.get(building_rawcode)
        semantics = semantics_by_building.get(building_rawcode)
        if building is None or race is None or semantics is None:
            raise ValueError(f"UnitObjectMeta building is missing resolved building/race/semantic data: {building_rawcode}")
        if meta["unit_rawcode"] != race["unit_rawcode"]:
            raise ValueError(
                f"race and UnitObjectMeta spawn rawcodes disagree for {building_rawcode}: "
                f"{race['unit_rawcode']} != {meta['unit_rawcode']}"
            )

        is_production = bool(meta["unit_rawcode"])
        campaign_only = race["campaign_only"] == "1"
        in_xo = building_rawcode in xo_buildings
        if is_production:
            production_building_count += 1
            if building["build_time"] == "2":
                two_second_production_build_count += 1
            if campaign_only:
                campaign_production_count += 1
            else:
                normal_production_count += 1
                if in_xo:
                    normal_xo_production_count += 1
        if in_xo and (campaign_only or not is_production):
            raise ValueError(f"xO contains non-normal-production building: {building_rawcode}")

        semantic_precursor = semantics["precursor_rawcode"]
        graph_precursors = sorted(upgrades_from.get(building_rawcode, []))
        if semantic_precursor:
            if graph_precursors != [semantic_precursor]:
                raise ValueError(
                    f"CFBuilding precursor/upgrades disagree for {building_rawcode}: "
                    f"semantic={semantic_precursor} graph={graph_precursors}"
                )
        elif graph_precursors:
            raise ValueError(f"upgrade graph has predecessor without CFBuilding precursor for {building_rawcode}: {graph_precursors}")

        food_used = int(meta["food_used"])
        lumber_cost = int(meta["lumber_cost"])
        own_income = float(meta["gold_cost"]) * float(semantics["income_factor"]) / 100.0
        total_income = catalog_income(building_rawcode)
        production_building_rows.append([
            race["race_index"], race["race_function"], race["builder_rawcode"], race["builder_names"],
            int(campaign_only), race["building_order"],
            building_rawcode, meta["building_names"], "production" if is_production else "non-production",
            meta["unit_rawcode"], meta["unit_names"],
            meta["gold_cost"], meta["lumber_cost"], meta["food_used"], int(food_used > 0), int(lumber_cost == 0),
            semantics["income_factor_symbol"], semantics["income_factor"], value_as_text(own_income), value_as_text(total_income),
            meta["spawn_build_time"], meta["attack_index"], meta["defense_index"],
            meta["is_air"], meta["is_melee"], meta["is_mechanical"], meta["is_caster"], int(in_xo),
            ",".join(graph_precursors),
            ",".join(sorted(upgrades_to.get(building_rawcode, []))),
            semantics["has_tier_assignment"], semantics["is_legendary_line"], semantics["is_anti_air"],
            semantics["is_siege"], semantics["is_artillery"], semantics["is_na_only"], semantics["is_ultimate_only"],
            semantics["no_pp"], semantics["ai_should_ignore"], semantics["provides_active_targeted_spell_shield"],
            semantics["area_spell"], semantics["multi_target_mult"], semantics["cage_pressure"], semantics["placement_strat"],
            semantics["spell_dps"], semantics["ai_tower_strength"], semantics["combat_power_factor"],
            semantics["tags"], semantics["extra_tags"], semantics["override_tags"],
            building["gold_cost"], building["lumber_cost"], building["build_time"],
            building["pathing_texture"], building["footprint_width_cells"], building["footprint_height_cells"],
            building["footprint_width_world_units"], building["footprint_height_world_units"], building["footprint_hex_rows"],
        ])

    write_tsv(
        output / "production-buildings.tsv",
        [
            "race_index", "race_function", "builder_rawcode", "builder_names", "campaign_only", "building_order",
            "building_rawcode", "building_names", "building_kind", "unit_rawcode", "unit_names",
            "gold_cost", "lumber_cost", "food_used", "is_legendary", "gives_lumber",
            "income_factor_symbol", "income_factor", "own_income_contribution", "catalog_income",
            "spawn_time",
            "attack_index", "defense_index", "is_air_unit", "is_melee", "is_mechanical", "is_caster", "in_xo_runtime_catalog",
            "upgrade_from", "upgrade_to", "has_tier_assignment", "is_legendary_line", "is_anti_air", "is_siege",
            "is_artillery", "is_na_only", "is_ultimate_only", "no_pp", "ai_should_ignore",
            "provides_active_targeted_spell_shield", "area_spell", "multi_target_mult", "cage_pressure", "placement_strat",
            "spell_dps", "ai_tower_strength", "combat_power_factor", "tags", "extra_tags", "override_tags",
            "static_object_gold_cost", "static_object_lumber_cost", "static_object_build_time",
            "pathing_texture", "footprint_width_cells", "footprint_height_cells", "footprint_width_world_units",
            "footprint_height_world_units", "footprint_hex_rows",
        ],
        production_building_rows,
    )

    # Resolve global death/decay constants from Blizzard MiscData plus the
    # map's war3mapMisc overrides. Keep base and override values side-by-side
    # so native content can distinguish engine defaults from map-authored
    # compatibility changes.
    death_constant_specs = [
        ("flesh_decay", "DecayTime"),
        ("bone_decay", "BoneDecayTime"),
        ("structure_decay", "StructureDecayTime"),
    ]
    death_constants: dict[str, str] = {}
    death_constant_rows: list[list[Any]] = []
    for name, misc_key in death_constant_specs:
        base_value, map_value, effective_value = effective_misc_value(misc_key)
        death_constants[name] = effective_value
        death_constant_rows.append([
            name, misc_key, base_value, map_value, effective_value,
            "map-override" if map_value else "base-default",
        ])
    write_tsv(
        output / "death-decay-constants.tsv",
        ["constant", "misc_key", "base_value", "map_override", "effective_value", "effective_source"],
        death_constant_rows,
    )

    death_type_labels = {
        0: "cant-raise-no-decay",
        1: "can-raise-no-decay",
        2: "cant-raise-decays",
        3: "can-raise-decays",
    }
    flesh_decay = numeric(death_constants["flesh_decay"])
    bone_decay = numeric(death_constants["bone_decay"])
    if flesh_decay is None or bone_decay is None:
        raise ValueError("death/decay constants must be numeric")

    production_corpse_rows: list[list[Any]] = []
    death_type_counts: Counter[int] = Counter()
    normal_death_type_counts: Counter[int] = Counter()
    for meta in metadata_rows:
        unit_rawcode = meta["unit_rawcode"]
        if not unit_rawcode:
            continue
        race = race_by_building[meta["building_rawcode"]]
        death_type_value = field_lookup(rows_by_object, "units", unit_rawcode, "udea", 0, 0)
        death_time_value = field_lookup(rows_by_object, "units", unit_rawcode, "udtm", 0, 0)
        death_type_number = numeric(death_type_value)
        death_time = numeric(death_time_value)
        if death_type_number is None or death_type_number != int(death_type_number) or int(death_type_number) not in death_type_labels:
            raise ValueError(f"invalid death type for production unit {unit_rawcode}: {death_type_value!r}")
        if death_time is None or death_time < 0:
            raise ValueError(f"invalid death time for production unit {unit_rawcode}: {death_time_value!r}")
        death_type = int(death_type_number)
        can_raise = bool(death_type & 0x1)
        does_decay = bool(death_type & 0x2)
        death_type_counts[death_type] += 1
        if race["campaign_only"] != "1":
            normal_death_type_counts[death_type] += 1
        decay_sequence_sum = death_time + flesh_decay + bone_decay if does_decay else None
        production_corpse_rows.append([
            meta["building_rawcode"], meta["building_names"], unit_rawcode, meta["unit_names"],
            race["campaign_only"], meta["is_mechanical"], death_type, death_type_labels[death_type],
            int(can_raise), int(does_decay), value_as_text(death_time),
            value_as_text(flesh_decay), value_as_text(bone_decay), value_as_text(decay_sequence_sum),
            "udea", "udtm",
        ])
    write_tsv(
        output / "production-unit-corpses.tsv",
        [
            "building_rawcode", "building_names", "unit_rawcode", "unit_names", "campaign_only", "is_mechanical",
            "death_type", "death_type_label", "can_raise", "does_decay", "death_time",
            "flesh_decay_time", "bone_decay_time", "death_plus_flesh_plus_bones",
            "death_type_field", "death_time_field",
        ],
        production_corpse_rows,
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
            "w3i_index": data_selection.data_set_index,
            "game_data_version": data_selection.game_data_version,
            "label": data_selection.label,
            "profile_variant": data_selection.profile_variant,
            "casc_overlay": data_selection.casc_overlay,
        },
        "map_objects": dict(Counter(record["category"] for record in object_records)),
        "resolved_field_rows": len(full_rows),
        "base_source_field_rows": len(source_rows),
        "protection_conflicts": len(conflict_rows),
        "objects_with_protection_conflicts": len({(row[0], row[2]) for row in conflict_rows}),
        "protection_conflict_selection_counts": dict(sorted(Counter(row[11] for row in conflict_rows).items())),
        "protection_conflicts_runtime_unitstat_confirmed": sum(
            row[11] == "w3p-runtime-unitstat-confirmed" for row in conflict_rows
        ),
        "protection_conflicts_stable_final_write": sum(
            row[11] == "map-stable-final-write" for row in conflict_rows
        ),
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
        "wurst_generated_object_marker_rows": len(wurst_marker_rows),
        "wurst_generated_object_marker_value": WURST_GENERATED_MARKER,
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
        "production_unit_attack_rows": len(production_attack_rows),
        "production_unit_available_attack_profiles": production_attack_profiles,
        "production_unit_conditional_attack_profiles": conditional_attack_profiles,
        "production_unit_two_profile_sum_patterns": two_profile_sum_patterns,
        "production_unit_special_mechanic_rows": len(production_special_rows),
        "production_unit_special_mechanic_kinds": dict(sorted(Counter(
            row[4] for row in production_special_rows
        ).items())),
        "production_unit_runtime_coverage_rows": len(runtime_coverage_rows),
        "production_unit_runtime_coverage_status_counts": dict(sorted(Counter(
            row[8] for row in runtime_coverage_rows
        ).items())),
        "building_improvement_spawn_mechanic_rows": len(building_improvement_spawn_by_source),
        "perk_mechanic_rows": len(perk_mechanic_rows),
        "perk_mechanic_kinds": dict(sorted(Counter(row[3] for row in perk_mechanic_rows).items())),
        "runtime_ai_mechanic_rows": len(runtime_ai_rows),
        "runtime_ai_mechanic_kinds": dict(sorted(Counter(row[1] for row in runtime_ai_rows).items())),
        "runtime_system_mechanic_rows": len(runtime_system_rows),
        "runtime_system_mechanic_kinds": dict(sorted(Counter(
            row[1] for row in runtime_system_rows
        ).items())),
        "production_unit_ability_links": len(production_ability_rows),
        "production_unit_unique_abilities": len(production_ability_unique),
        "production_unit_inherited_ability_links": production_ability_inherited_links,
        "production_unit_ability_links_with_protected_runtime_fields": production_ability_runtime_field_links,
        "scripted_unit_spell_rows": len(unit_spell_rows),
        "scripted_unit_spell_production_rows": unit_spell_production_rows,
        "scripted_unit_spell_resolved_order_ids": sum(bool(row[27]) for row in unit_spell_rows),
        "scripted_unit_spell_order_id_sources": dict(sorted(Counter(row[28] for row in unit_spell_rows).items())),
        "scripted_unit_spell_target_modes": dict(sorted(Counter(row[24] for row in unit_spell_rows).items())),
        "scripted_unit_spell_mechanic_rows": len(unit_spell_mechanic_rows),
        "scripted_unit_spell_mechanic_kinds": dict(sorted(Counter(row[13] for row in unit_spell_mechanic_rows).items())),
        "scripted_unit_spell_mechanics_with_delayed_callbacks": sum(bool(row[16]) for row in unit_spell_mechanic_rows),
        "scripted_unit_spell_mechanics_with_dynamic_callbacks": sum(bool(row[17]) for row in unit_spell_mechanic_rows),
        "scripted_unit_spell_semantic_rows": len(unit_spell_semantic_rows),
        "scripted_unit_spell_semantic_status_counts": dict(sorted(unit_spell_semantic_status_counts.items())),
        "scripted_unit_spell_semantic_kind_counts": dict(sorted(unit_spell_semantic_kind_counts.items())),
        "protected_filter_binding_rows": len(protected_filter_rows),
        "protected_filter_binding_resolved_rows": len(protected_filters_by_symbol),
        "protected_filter_binding_status_counts": dict(sorted(Counter(
            row["resolution_status"] for row in protected_filter_rows
        ).items())),
        "resolved_item_rows": len(item_rows),
        "castle_shop_item_rows": len(castle_shop_rows),
        "scripted_item_pickup_mechanic_rows": len(item_mechanic_rows),
        "scripted_item_spell_mechanic_rows": len(item_spell_rows),
        "resolved_item_mechanic_rows": len(resolved_item_mechanics),
        "element_building_bucket_rows": len(element_bucket_rows),
        "scripted_building_spell_rows": len(building_spell_rows),
        "scripted_building_spell_registration_evidence_kinds": dict(sorted(Counter(row[30] for row in building_spell_rows).items())),
        "scripted_building_spell_mana_timed_rows": sum(
            row[18] == "ability-mana-cost/building-mana-regen" for row in building_spell_rows
        ),
        "scripted_building_spell_wc3_cooldown_timed_rows": sum(
            row[18] == "effective-wc3-ability-cooldown" for row in building_spell_rows
        ),
        "scripted_building_spell_evidence_rows": len(building_spell_evidence_rows),
        "scripted_building_spell_mechanic_rows": len(building_spell_mechanic_rows),
        "scripted_building_spell_mechanic_rows_with_evidence_disagreement": sum(
            bool(row[16]) for row in building_spell_mechanic_rows
        ),
        "corpse_building_mechanic_rows": len(corpse_building_rows),
        "corpse_building_raise_rows": sum(row[12] == "scripted-raise-random" for row in corpse_building_rows),
        "building_catalog_rows": len(production_building_rows),
        "building_catalog_production_rows": production_building_count,
        "building_catalog_campaign_production_rows": campaign_production_count,
        "building_catalog_normal_production_rows": normal_production_count,
        "building_catalog_normal_production_rows_in_xo": normal_xo_production_count,
        "building_catalog_upgrade_edges": sum(len(values) for values in upgrades_to.values()),
        "building_catalog_semantic_rows": len(semantics_by_building),
        "building_catalog_two_second_production_build_rows": two_second_production_build_count,
        "production_unit_corpse_rows": len(production_corpse_rows),
        "production_unit_death_type_counts": {str(key): death_type_counts[key] for key in sorted(death_type_counts)},
        "normal_production_unit_death_type_counts": {
            str(key): normal_death_type_counts[key] for key in sorted(normal_death_type_counts)
        },
        "death_decay_constants": dict(sorted(death_constants.items())),
        "notes": [
            "object-fields.tsv preserves base, every map candidate, last-write and recovered values; the non-WC3 field wurs is classified as Wurst compiler provenance (GENERATED_BY_WURST=42), not gameplay data",
            "all 13 conflicting numeric unit fields are independently confirmed against the decoded protected UnitStat runtime table; the sole conflicting string field (Snowveil Fountain ability list) repeats the same final value twice and is classified as a stable final write",
            "pathing texture pixels are 32 world units; bits 1/2/4 mean unwalkable/unflyable/unbuildable",
            f"base-source-fields.tsv exposes the W3I-selected {data_selection.overlay_dir}/base SLK values before map overrides, including computed columns",
            "protected-ability-fields.tsv compares the protected Lua runtime table against static resolved cooldown/mana values without overwriting either source",
            "protected-unit-stats.tsv applies the exactly decoded jP UnitStat overrides on top of static resolved unit fields while preserving static, override, source and encoded-row provenance; further scripted modifiers may still change live values",
            "effective-unit-stats.tsv compares the generated xO building-to-unit effective stat catalog against static unit object data; DPS comparison allows 0.011 for hundredths quantization",
            "production-unit-attacks.tsv keeps both weapon profiles for every production unit and structurally labels Agra/War Club conditional attack switching instead of flattening it into xO's one-number summary",
            "production-unit-special-mechanics.tsv normalizes runtime-only production-unit behavior that bypasses the scripted unit-spell registry; current exact rows cover Mountain Giant War Club, Echofoot Echo Step/remnant, Gnoll anti-air retaliation, Defender Defend maintenance, Greater Fire Elemental splitting, Avatar/Avenging Spirit death/kill effects, Vampire Eternal Servitude, Troll-family Berserk, Winged Riptide Serpent anti-air damage amplification, Forest Troll Trapper persistent low-HP attack tiers, Ironpaw Guardian Whirlwind, Nature dispels/Bear hibernation, Razormane Razor Spray, Emerald corrosion, Greater Water Mirror Image, Greater Wind Kaboom charge, Earth health-scaled Aftershock, Lightning melee-retaliation Thunderbolt, Paladin summon mana reset, Mine Layer random trained mana, Goblin Rocketeer exploded/death-explosion setup, Lich King Mastery over Death, and Vampire Lord Blood Corrosion",
            "production-unit-runtime-coverage.tsv is a closure audit over core combat/train/summon/death handlers plus explicitly audited marker/listener hooks such as Earth, Lightning, Riptide, Troll Blood and Whirlwind; extraction fails if a referenced production unit is not covered by special mechanics, scripted unit spells, the verified Fire-split endpoint, or the strictly asserted Shadow Drake visual-only branch",
            "perk-mechanics.tsv currently normalizes all eight proven-live damage-listener draft perks, including target-type damage tradeoffs, cage-conditioned damage/base-damage changes, Combat Stance HP bands and toggle abilities, Mana Shielding, Spell's Edge and Containment Focus; remaining live perks stay separate until their non-damage runtime paths are normalized",
            "perk-mechanics.tsv now normalizes all 19/19 protected-registry draft perks. Script control flow remains authoritative where it disagrees with display text: Towerless retains its 45-DPS item text beside the protected Tiny Watch Tower's 53-DPS weapon, Production Enchantment applies separately rounded 0.95 then 1.15 scaling with explicit life-adjustment semantics, and Longline Formation preserves the generated weapon-index-1 range-write quirk rather than silently implementing the tooltip's intended weapon-0 +90 range",
            "runtime-ai-mechanics.tsv separates AI decision/observation semantics from authoritative combat rewrites. It preserves the 2-second/0.8 decayed engagement centroid and structure-pressure signals plus the damage-triggered Rescue Strike controller, including its HP/count threshold curve, 700/900 target geometry, siege-vs-tower -4 score, 28/75-second repeat throttles, 3-second coordination lock and protected A005 runtime fields (0 mana, 60-second cooldown)",
            "runtime-system-mechanics.tsv normalizes gameplay systems that cut across ordinary unit/spell rows, including Power Plant spawn augmentation/freeze cleanup, Heroic Shrine companion spawning, Golden Shrine revival, Blood Fiend procedural bodies/traits, first-15-second castle protection, Eye of Corruption's B00Q-gated 12% positive non-attack damage amplification, and Obelisk of Light's persistent Phoenix Fire cleanse carrier. Runtime probabilities and script/object discrepancies are preserved instead of silently flattened, and Blood Fiend body stats use protected UnitStat values rather than poisoned static object fields",
            "production-unit-abilities.tsv keeps every initial production-unit ability link, applies protected runtime cooldown/mana where available, preserves labeled editor Data fields, and retains inherited Blizzard utility abilities instead of dropping unmodified rawcodes",
            "unit-spells.tsv cross-links the generated scripted unit-spell registry to resolved unit/ability definitions, target-mode semantics, production source buildings and effective protected cooldown/mana; all 37 numeric order IDs are resolved independently from the abilities' canonical Warcraft base-order strings while the original protected registry expression is retained as provenance",
            "unit-spell-mechanics.tsv gives every scripted unit spell a complete static implementation-evidence profile: direct primitives/helper calls, exact generated doAfter/ForGroupCallback/CallbackPeriodic dispatch, calls made by lexically contained anonymous timer callbacks, semantic effect-call arguments, source numeric literals and bounded reachable map-object paths enriched with resolved ability/unit data; callback edges are followed only when statically exact and the map Lua is never executed",
            "protected-filter-bindings.tsv resolves all 21 fixed generated W3P Filter wrappers from exact use-site/compiler structure; generic registerPlayerUnitEvent local SCr is correctly classified as a dynamic caller-supplied wrapper, leaving no unresolved fixed filter globals",
            "items.tsv normalizes every authored map item, including helper/result items such as Gold and Multi Blast Staff; repeated attached abilities are preserved because Multi Blast Staff implements four simultaneous Blast effects with four A02D entries",
            "castle-shop-items.tsv recovers the exact 10-slot Castle shop mapping with stock/use flags and fully resolved attached abilities; item-mechanics.tsv separately normalizes script-only Gold scaling, Cheese legendary-slot/refund behavior, the four-Blast-Staff -> Multi Blast Staff inventory recipe, 29-second Double/Quad aura carriers, Orb of Lightning round-scaled dummy casts, and Scroll of Stone/Speed hidden dummy effects",
            "unit-spell-semantics.tsv is the stricter native-import normalization layer over that evidence: all 37 rows are implementation-ready; Master of Elements is fully normalized because its protected Frost target-filter symbol SX is statically resolved to the generated enemy-combat-sapper predicate; Mana Generator is normalized only to its reachable A0EL mana-regeneration improvement, while building-improvement-spawn-mechanics.tsv retains and explicitly marks the A0EL setupUnit/A0EZ+A0C5 branch unreachable because legal Mana Generator targets exclude production buildings and setupUnit is only called from production companion-spawn paths",
            "element-building-buckets.tsv resolves the exact Fire/Earth/Lightning/Water/Wind building-count groups consumed by Master of Elements formulas from generated vtb bucket assignments",
            "building-spells.tsv now covers both generated registration representations: 15 protected registry calls and 28 direct EVENT_PLAYER_UNIT_SPELL_EFFECT listeners. Forty-two use Castle Fight's mana-cost/building-regen cadence; Tidal Guardian is the explicit cooldown-driven exception at its protected 15-second WC3 cooldown",
            "building-spell-evidence.tsv gives all 43 scripted building spells the same bounded static handler/helper/callback/effect evidence used for unit spells, including direct and reachable rawcodes enriched with resolved WC3 object data",
            "building-spell-mechanics.tsv strictly normalizes all 43 scripted building spells across both registration families into target/delivery/mechanic parameters plus separately sourced linked WC3 object effects; all referenced fixed protected filter predicates are resolved, and explicit tooltip-vs-object disagreements are retained rather than resolved silently",
            "corpse-building-mechanics.tsv normalizes the two scripted Undead raise handlers and Vessel of Purity from exact Lua predicates/control flow; these mechanics do not consult Warcraft's Death Type can-raise bit, which remains a separate corpse capability",
            "production-buildings.tsv joins UnitObjectMeta, race wrapper semantics, the complete generated race partition, authored upgrade edges, exact footprints and xO coverage; spawn_time is the recurring CF production interval, while static_object_build_time is the Warcraft building-construction field",
            "all 167 authored production buildings have static_object_build_time=2; Castle Fight uses this as the short construction/cancellation window, distinct from recurring spawn_time",
            "production-unit-corpses.tsv retains Warcraft Death Type capability bits and per-unit Death Time beside the effective flesh/bone decay constants; no-decay removal behavior is not guessed",
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
