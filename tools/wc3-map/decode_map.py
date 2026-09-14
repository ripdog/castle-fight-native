#!/usr/bin/env python3
"""Decode extracted Warcraft III map members into diffable plaintext.

The heavy binary translators are source-built separately. This script fills the
remaining format gaps, resolves trigger strings, emits flat catalogs, and
indexes protected/obfuscated Lua without executing it.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import shutil
import struct
import sys
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
if str(TOOLS_DIR) not in sys.path:
    sys.path.insert(0, str(TOOLS_DIR))

from lua_index import analyze_lua


TRIGSTR_RE = re.compile(r"^TRIGSTR_(\d+)$")


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.offset = 0

    @property
    def remaining(self) -> int:
        return len(self.data) - self.offset

    def _take(self, size: int) -> bytes:
        end = self.offset + size
        if end > len(self.data):
            raise ValueError(f"unexpected EOF at {self.offset}, need {size} bytes")
        value = self.data[self.offset:end]
        self.offset = end
        return value

    def u8(self) -> int:
        return self._take(1)[0]

    def u32(self) -> int:
        return struct.unpack("<I", self._take(4))[0]

    def i32(self) -> int:
        return struct.unpack("<i", self._take(4))[0]

    def f32(self) -> float:
        return struct.unpack("<f", self._take(4))[0]

    def rawcode(self) -> str | None:
        raw = self._take(4)
        if raw == b"\0\0\0\0":
            return None
        return raw.decode("latin1")

    def cstr(self) -> str:
        end = self.data.find(b"\0", self.offset)
        if end < 0:
            raise ValueError(f"unterminated string at {self.offset}")
        raw = self.data[self.offset:end]
        self.offset = end + 1
        return raw.decode("utf-8", errors="replace")


MAP_FLAGS = {
    0x000001: "hide_minimap_on_preview",
    0x000002: "modify_ally_priorities",
    0x000004: "melee_map",
    0x000008: "custom_terrain_tileset",
    0x000010: "masked_areas_partially_visible",
    0x000020: "fixed_player_parameters",
    0x000040: "custom_forces",
    0x000080: "custom_tech_tree",
    0x000100: "custom_abilities",
    0x000200: "custom_upgrades",
    0x000400: "map_properties_seen",
    0x000800: "water_waves_cliff_shores",
    0x001000: "water_waves_rolling_shores",
    0x002000: "terrain_fog",
    0x004000: "requires_expansion",
    0x008000: "item_classification",
    0x010000: "custom_water_tint",
    0x020000: "accurate_probability",
    0x040000: "custom_ability_skins",
}

FORCE_FLAGS = {
    0x0001: "allied",
    0x0002: "allied_victory",
    0x0004: "unknown_0x4",
    0x0008: "shared_vision",
    0x0010: "shared_control",
    0x0020: "shared_advanced_control",
}

PATHING_FLAGS = {
    0x01: "unused_0x01",
    0x02: "no_walk",
    0x04: "no_fly",
    0x08: "no_build",
    0x10: "unused_0x10",
    0x20: "blight",
    0x40: "no_water",
    0x80: "unknown_0x80",
}

PLAYER_TYPES = {1: "human", 2: "computer", 3: "neutral", 4: "reserved"}
PLAYER_RACES = {0: "random", 1: "human", 2: "orc", 3: "undead", 4: "night_elf"}
SCRIPT_LANGUAGES = {0: "jass", 1: "lua"}
GRAPHICS_MODES = {1: "sd", 2: "hd", 3: "sd+hd"}
GAME_DATA_VERSIONS = {0: "roc", 1: "tft"}


def flags_to_names(value: int, table: dict[int, str]) -> list[str]:
    return [name for bit, name in table.items() if value & bit]


def read_rgba(r: Reader) -> dict[str, int]:
    return {"r": r.u8(), "g": r.u8(), "b": r.u8(), "a": r.u8()}


def parse_w3i(path: Path) -> dict[str, Any]:
    r = Reader(path.read_bytes())
    version = r.u32()
    result: dict[str, Any] = {"format_version": version}

    if version >= 16:
        result["number_of_saves"] = r.u32()
        result["world_editor_version"] = r.u32()
    if version >= 27:
        game_version = [r.u32(), r.u32(), r.u32(), r.u32()]
        result["game_version"] = {
            "major": game_version[0],
            "minor": game_version[1],
            "patch": game_version[2],
            "build": game_version[3],
            "display": ".".join(str(v) for v in game_version),
        }

    result["name"] = r.cstr()
    result["author"] = r.cstr()
    result["description"] = r.cstr()
    if version >= 8:
        result["recommended_players"] = r.cstr()

    if version <= 3:
        result["legacy_unknown_before_camera"] = r._take(8).hex()
    elif version <= 8:
        result["legacy_unknown_before_camera"] = {
            "float1": r.f32(),
            "int1": r.i32(),
            "float2": r.f32(),
            "float3": r.f32(),
            "float4": r.f32(),
            "int2": r.i32(),
        }

    camera = [r.f32() for _ in range(8)]
    result["camera_bounds"] = {
        "left_bottom": camera[0:2],
        "right_top": camera[2:4],
        "left_top": camera[4:6],
        "right_bottom": camera[6:8],
        "raw": camera,
    }

    if version >= 14:
        complements = [r.i32() for _ in range(4)]
        result["unplayable_border_tiles"] = {
            "left": complements[0],
            "right": complements[1],
            "bottom": complements[2],
            "top": complements[3],
            "raw": complements,
        }
    else:
        complements = [0, 0, 0, 0]

    if version >= 1:
        width = r.i32()
        height = r.i32()
        result["playable_size_tiles"] = {"width": width, "height": height}
        result["total_size_tiles"] = {
            "width": complements[0] + width + complements[1],
            "height": complements[2] + height + complements[3],
        }

    if version >= 2:
        if version <= 8:
            result["legacy_unknown_before_flags"] = r.i32()
        flags = r.u32()
        result["flags"] = {"raw": flags, "hex": f"0x{flags:08x}", "set": flags_to_names(flags, MAP_FLAGS)}

    if version >= 8:
        result["main_tileset"] = chr(r.u8())

    if version >= 17:
        result["loading_screen_preset"] = r.i32()
    if version >= 10 and version not in (18, 19):
        result["loading_screen_path"] = r.cstr()
    if version >= 10:
        result["loading_screen_text"] = r.cstr()
        if version >= 11:
            result["loading_screen_title"] = r.cstr()
            result["loading_screen_subtitle"] = r.cstr()
    if version >= 17:
        result["game_data_set_version"] = r.i32()
    if version >= 13 and version not in (18, 19):
        result["prologue_path"] = r.cstr()
    if version >= 13:
        result["prologue_text"] = r.cstr()
        result["prologue_title"] = r.cstr()
        result["prologue_subtitle"] = r.cstr()
    if version >= 19:
        result["fog"] = {
            "type": r.i32(),
            "start_z": r.f32(),
            "end_z": r.f32(),
            "density": r.f32(),
            "color": read_rgba(r),
        }
    if version >= 21:
        result["global_weather_rawcode"] = r.rawcode()
    if version >= 22:
        result["sound_environment"] = r.cstr()
    if version >= 23:
        result["light_environment"] = chr(r.u8())
    if version >= 25:
        result["water_tint"] = read_rgba(r)
    if version >= 28:
        lang = r.i32()
        result["scripting_language"] = {"raw": lang, "name": SCRIPT_LANGUAGES.get(lang, "unknown")}
    if version >= 29:
        graphics = r.i32()
        result["supported_graphics"] = {"raw": graphics, "name": GRAPHICS_MODES.get(graphics, "unknown")}
    if version >= 30:
        data_version = r.i32()
        result["game_data_version"] = {"raw": data_version, "name": GAME_DATA_VERSIONS.get(data_version, "unknown")}
    if version >= 32:
        result["forced_default_camera_zoom"] = r.i32()
        result["forced_max_camera_zoom"] = r.i32()
    if version >= 33:
        result["forced_min_camera_zoom"] = r.i32()

    if r.remaining >= 4:
        players = []
        for _ in range(r.i32()):
            slot = r.i32()
            player_type = r.i32()
            race = r.i32()
            fixed = r.u32()
            p: dict[str, Any] = {
                "slot": slot,
                "type": {"raw": player_type, "name": PLAYER_TYPES.get(player_type, "unknown")},
                "race": {"raw": race, "name": PLAYER_RACES.get(race, "unknown")},
                "fixed_start_position": bool(fixed & 1),
                "flags_raw": fixed,
                "name": r.cstr(),
                "start": {"x": r.f32(), "y": r.f32()},
            }
            if version >= 5:
                p["ally_priority_low"] = r.u32()
                p["ally_priority_high"] = r.u32()
            if version >= 31:
                p["enemy_priority_low"] = r.u32()
                p["enemy_priority_high"] = r.u32()
            players.append(p)
        result["players"] = players

    if r.remaining >= 4:
        forces = []
        for _ in range(r.i32()):
            force_flags = r.u32()
            players_mask = r.u32()
            forces.append({
                "flags": {
                    "raw": force_flags,
                    "hex": f"0x{force_flags:08x}",
                    "set": flags_to_names(force_flags, FORCE_FLAGS),
                },
                "players_mask": players_mask,
                "players": [i for i in range(32) if players_mask & (1 << i)],
                "name": r.cstr(),
            })
        result["forces"] = forces

    if r.remaining >= 4 and version >= 6:
        upgrades = []
        for _ in range(r.i32()):
            upgrades.append({
                "players_mask": r.u32(),
                "rawcode": r.rawcode(),
                "level": r.i32(),
                "availability": r.i32(),
            })
        result["custom_upgrades"] = upgrades

    if r.remaining >= 4 and version >= 7:
        tech = []
        for _ in range(r.i32()):
            tech.append({"players_mask": r.u32(), "rawcode": r.rawcode()})
        result["custom_tech"] = tech

    if r.remaining >= 4 and version >= 12:
        tables = []
        for _ in range(r.i32()):
            table_number = r.i32()
            name = r.cstr()
            columns = r.i32()
            column_types = [r.i32() for _ in range(columns)]
            rows = []
            for _ in range(r.i32()):
                rows.append({
                    "chance": r.i32(),
                    "rawcodes": [r.rawcode() for _ in range(columns)],
                })
            tables.append({
                "number": table_number,
                "name": name,
                "column_types": column_types,
                "rows": rows,
            })
        result["random_unit_tables"] = tables

    if r.remaining >= 4 and version >= 24:
        tables = []
        for _ in range(r.i32()):
            table_number = r.i32()
            name = r.cstr()
            sets = []
            for _ in range(r.i32()):
                items = []
                for _ in range(r.i32()):
                    items.append({"chance": r.i32(), "rawcode": r.rawcode()})
                sets.append(items)
            tables.append({"number": table_number, "name": name, "sets": sets})
        result["random_item_tables"] = tables

    if version in (26, 27) and r.remaining >= 4:
        lang = r.i32()
        result["trailing_scripting_language"] = {"raw": lang, "name": SCRIPT_LANGUAGES.get(lang, "unknown")}

    result["parse"] = {"bytes_total": len(r.data), "bytes_consumed": r.offset, "bytes_remaining": r.remaining}
    if r.remaining:
        result["parse"]["trailing_hex"] = r._take(r.remaining).hex()
    return result


def parse_wpm(path: Path, output: Path) -> dict[str, Any]:
    r = Reader(path.read_bytes())
    signature = r._take(4).decode("ascii", errors="replace")
    version = r.u32()
    width = r.u32()
    height = r.u32()
    cells = r._take(width * height)
    if r.remaining:
        raise ValueError(f"war3map.wpm has {r.remaining} unexpected trailing bytes")

    histogram = Counter(cells)
    bit_counts = {name: sum(1 for value in cells if value & bit) for bit, name in PATHING_FLAGS.items()}
    summary = {
        "signature": signature,
        "version": version,
        "width_cells": width,
        "height_cells": height,
        "cells_per_terrain_tile": 4,
        "bytes": len(cells),
        "flag_semantics": {f"0x{bit:02x}": name for bit, name in PATHING_FLAGS.items()},
        "flag_counts": bit_counts,
        "value_histogram": {f"0x{value:02x}": count for value, count in sorted(histogram.items())},
    }

    hex_lines = []
    walk_lines = []
    build_lines = []
    for y in range(height):
        row = cells[y * width:(y + 1) * width]
        hex_lines.append(" ".join(f"{value:02x}" for value in row))
        walk_lines.append("".join("#" if value & 0x02 else "." for value in row))
        build_lines.append("".join("#" if value & 0x08 else "." for value in row))
    (output / "pathing-grid.hex.txt").write_text("\n".join(hex_lines) + "\n", encoding="utf-8")
    (output / "walkability.txt").write_text("\n".join(walk_lines) + "\n", encoding="utf-8")
    (output / "buildability.txt").write_text("\n".join(build_lines) + "\n", encoding="utf-8")
    return summary


def parse_shadow(path: Path, width: int, height: int, output: Path) -> dict[str, Any]:
    data = path.read_bytes()
    expected = width * height
    summary: dict[str, Any] = {
        "bytes": len(data),
        "expected_from_pathing_dimensions": expected,
        "matches_pathing_dimensions": len(data) == expected,
        "value_histogram": {f"0x{k:02x}": v for k, v in sorted(Counter(data).items())},
    }
    if len(data) == expected:
        lines = []
        for y in range(height):
            row = data[y * width:(y + 1) * width]
            lines.append(" ".join(f"{value:02x}" for value in row))
        (output / "shadow-grid.hex.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
    return summary


def parse_mmp(path: Path) -> dict[str, Any]:
    r = Reader(path.read_bytes())
    version = r.u32()
    count = r.u32()
    icons = []
    for _ in range(count):
        icons.append({
            "type": r.i32(),
            "x": r.i32(),
            "y": r.i32(),
            "color": read_rgba(r),
        })
    return {
        "version": version,
        "count": count,
        "icons": icons,
        "parse": {"bytes_total": len(r.data), "bytes_consumed": r.offset, "bytes_remaining": r.remaining},
    }


def map_archive_header(path: Path) -> dict[str, Any]:
    data = path.read_bytes()
    mpq_offset = data.find(b"MPQ\x1a")
    name_end = data.find(b"\0", 8, max(mpq_offset, 8))
    name = None
    if name_end >= 0:
        name = data[8:name_end].decode("utf-8", errors="replace")
    result = {
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
        "header_signature": data[:4].decode("ascii", errors="replace"),
        "header_value_at_0x04": struct.unpack_from("<I", data, 4)[0] if len(data) >= 8 else None,
        "header_map_name": name,
        "mpq_offset": mpq_offset,
    }
    return result


def parse_wts(path: Path) -> dict[str, dict[str, str]]:
    """Parse Warcraft trigger strings without recoding their UTF-8 payload."""
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    entries: dict[str, dict[str, str]] = {}
    index = 0
    header = re.compile(r"^\s*STRING\s+(\d+)(?:\s*//\s*(.*))?\s*$")

    while index < len(lines):
        match = header.match(lines[index])
        if not match:
            index += 1
            continue

        string_id = str(int(match.group(1)))
        comment = match.group(2)
        index += 1
        while index < len(lines) and not lines[index].strip():
            index += 1
        if index >= len(lines) or lines[index].strip() != "{":
            raise ValueError(f"STRING {string_id} is missing an opening brace")
        index += 1

        value_lines: list[str] = []
        while index < len(lines) and lines[index].strip() != "}":
            value_lines.append(lines[index])
            index += 1
        if index >= len(lines):
            raise ValueError(f"STRING {string_id} is missing a closing brace")
        index += 1

        if string_id in entries:
            raise ValueError(f"duplicate trigger string id {string_id}")
        entry = {"value": "\n".join(value_lines)}
        if comment:
            entry["comment"] = comment
        entries[string_id] = entry

    return entries


def repair_translator_text(value: Any) -> Any:
    """Repair UTF-8 bytes that WC3MapTranslator exposed as Latin-1 text.

    Only strings containing common UTF-8-as-Latin-1 markers are considered,
    and the conversion is accepted only when the resulting byte sequence is
    valid UTF-8. Legacy single-byte strings therefore remain untouched.
    """
    if isinstance(value, list):
        return [repair_translator_text(item) for item in value]
    if isinstance(value, dict):
        return {key: repair_translator_text(item) for key, item in value.items()}
    if not isinstance(value, str) or not any(marker in value for marker in ("Ã", "Â", "â", "Ð", "Ñ")):
        return value
    try:
        repaired = value.encode("latin1").decode("utf-8")
    except (UnicodeEncodeError, UnicodeDecodeError):
        return value
    old_markers = sum(value.count(marker) for marker in ("Ã", "Â", "â", "Ð", "Ñ"))
    new_markers = sum(repaired.count(marker) for marker in ("Ã", "Â", "â", "Ð", "Ñ"))
    return repaired if new_markers < old_markers else value


def resolve_trigger_string(value: Any, strings: dict[str, str]) -> Any:
    value = repair_translator_text(value)
    if not isinstance(value, str):
        return value
    match = TRIGSTR_RE.fullmatch(value)
    if not match:
        return value
    return strings.get(str(int(match.group(1))), value)


def split_object_key(key: str, table: str) -> tuple[str, str | None]:
    if table == "custom" and ":" in key:
        new_id, base_id = key.split(":", 1)
        return new_id, base_id
    return key, None


def write_object_catalog(translated: Path, output: Path, strings: dict[str, str]) -> dict[str, Any]:
    object_files = {
        "units": "obj-units.json",
        "items": "obj-items.json",
        "abilities": "obj-abilities.json",
        "buffs": "obj-buffs.json",
        "destructables": "obj-destructables.json",
        "doodads": "obj-doodads.json",
    }
    catalog_dir = output / "catalog"
    catalog_dir.mkdir(parents=True, exist_ok=True)

    summary: dict[str, Any] = {}
    field_rows: list[list[Any]] = []
    index_rows: list[list[Any]] = []

    # Useful rawcode fields for a concise index. The full field table remains authoritative.
    name_fields = {
        "units": "unam",
        "items": "unam",
        "abilities": "anam",
        "buffs": "ftip",
        "destructables": "bnam",
        "doodads": "dnam",
    }

    selected_unit_fields = [
        "unam", "utip", "utub", "ubld", "uhpm", "umpm", "umvs", "uspe", "ucol", "uacq",
        "udef", "udty", "uabi", "utyp", "upat", "uubs", "ugol", "ulum", "ufoo",
        "ua1b", "ua1d", "ua1s", "ua1c", "ua1r", "ua1t", "ua1w", "ua1g", "ua1m", "ua1z",
        "ua2b", "ua2d", "ua2s", "ua2c", "ua2r", "ua2t", "ua2w", "ua2g", "ua2m", "ua2z",
    ]
    unit_rows: list[list[Any]] = []
    building_rows: list[list[Any]] = []
    ability_rows: list[list[Any]] = []

    selected_building_fields = [
        "unam", "utip", "utub", "uhpm", "umpm", "ucol", "usca", "umdl", "upat", "uabi",
        "ugol", "ulum", "ufoo", "ubld", "udef", "udty", "uacq",
    ]

    def choose_map_value(values: list[Any]) -> Any:
        if not values:
            return ""
        first = values[0]
        if all(value == first for value in values[1:]):
            return first
        if isinstance(first, (int, float)) and not isinstance(first, bool) and abs(float(first)) > 1:
            if all(isinstance(value, (int, float)) and not isinstance(value, bool) and float(value) in (0.0, 1.0) for value in values[1:]):
                return first
        # Conflicting strings/non-sentinel values retain ordinary last-write
        # semantics. The resolved/base-data catalog records this ambiguity.
        return values[-1]

    def field_variants(modifications: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
        grouped: dict[tuple[str, int, int], list[dict[str, Any]]] = {}
        for mod in modifications:
            key = (mod["id"].rstrip("\0"), mod.get("level", 0), mod.get("column", 0))
            grouped.setdefault(key, []).append(mod)
        variants: dict[str, list[dict[str, Any]]] = {}
        for (field_id, level, column), group in grouped.items():
            values = [resolve_trigger_string(mod.get("value"), strings) for mod in group]
            variants.setdefault(field_id, []).append({
                "level": level,
                "column": column,
                "type": group[0].get("type", ""),
                "value": choose_map_value(values),
            })
        return variants

    def compact_variant(variants: dict[str, list[dict[str, Any]]], field: str) -> Any:
        values = variants.get(field, [])
        if not values:
            return ""
        if len(values) == 1:
            return values[0]["value"]
        return json.dumps(values, ensure_ascii=False, separators=(",", ":"))

    def footprint_hint(pathing_texture: Any) -> str:
        if not isinstance(pathing_texture, str):
            return ""
        match = re.search(r"(?i)(\d+)x(\d+)", pathing_texture)
        return f"{match.group(1)}x{match.group(2)}" if match else ""

    for category, filename in object_files.items():
        source = translated / filename
        if not source.exists():
            continue
        data = repair_translator_text(json.loads(source.read_text(encoding="utf-8")))
        (output / filename).write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        counts = {table: len(objects) for table, objects in data.items()}
        summary[category] = counts

        for table, objects in data.items():
            for key, modifications in objects.items():
                rawcode, base_rawcode = split_object_key(key, table)
                fields: dict[str, Any] = {}
                grouped_values: dict[tuple[str, int, int], list[Any]] = defaultdict(list)
                occurrence_counts: Counter[tuple[str, int, int]] = Counter()
                for mod in modifications:
                    key = (mod["id"].rstrip("\0"), mod.get("level", 0), mod.get("column", 0))
                    grouped_values[key].append(resolve_trigger_string(mod.get("value"), strings))
                    occurrence_counts[key] += 1
                seen_counts: Counter[tuple[str, int, int]] = Counter()
                for modification_index, mod in enumerate(modifications):
                    raw_value = mod.get("value")
                    resolved = resolve_trigger_string(raw_value, strings)
                    field_id = mod["id"].rstrip("\0")
                    key = (field_id, mod.get("level", 0), mod.get("column", 0))
                    seen_counts[key] += 1
                    values = grouped_values[key]
                    conflict = len(values) > 1 and any(value != values[0] for value in values[1:])
                    if key[1] == 0 and key[2] == 0:
                        fields[field_id] = choose_map_value(values)
                    field_rows.append([
                        category,
                        table,
                        rawcode,
                        base_rawcode or "",
                        field_id,
                        mod.get("type", ""),
                        mod.get("level", 0),
                        mod.get("column", 0),
                        modification_index,
                        seen_counts[key],
                        occurrence_counts[key],
                        int(conflict),
                        json.dumps(raw_value, ensure_ascii=False),
                        json.dumps(resolved, ensure_ascii=False),
                    ])
                name = fields.get(name_fields[category], "")
                index_rows.append([category, table, rawcode, base_rawcode or "", name, len(modifications)])
                variants = field_variants(modifications)
                if category == "units":
                    unit_rows.append([
                        table,
                        rawcode,
                        base_rawcode or "",
                        *[fields.get(field, "") for field in selected_unit_fields],
                    ])
                    pathing_texture = fields.get("upat", "")
                    if fields.get("ubld") == 1 or pathing_texture:
                        building_rows.append([
                            table,
                            rawcode,
                            base_rawcode or "",
                            footprint_hint(pathing_texture),
                            *[fields.get(field, "") for field in selected_building_fields],
                        ])
                elif category == "abilities":
                    ability_rows.append([
                        table,
                        rawcode,
                        base_rawcode or "",
                        compact_variant(variants, "anam"),
                        compact_variant(variants, "alev"),
                        compact_variant(variants, "amcs"),
                        compact_variant(variants, "acdn"),
                        compact_variant(variants, "aran"),
                        compact_variant(variants, "aare"),
                        compact_variant(variants, "atar"),
                        compact_variant(variants, "abuf"),
                        compact_variant(variants, "aeff"),
                        compact_variant(variants, "atp1"),
                        compact_variant(variants, "aub1"),
                        json.dumps(variants, ensure_ascii=False, separators=(",", ":")),
                    ])

    with (catalog_dir / "object-fields.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "category", "table", "rawcode", "base_rawcode", "field_id", "type", "level", "column",
            "modification_index", "duplicate_ordinal", "duplicate_count", "conflicting_duplicate", "raw_value", "resolved_value",
        ])
        writer.writerows(field_rows)

    with (catalog_dir / "objects.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["category", "table", "rawcode", "base_rawcode", "name", "modification_count"])
        writer.writerows(index_rows)

    with (catalog_dir / "units.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["table", "rawcode", "base_rawcode", *selected_unit_fields])
        writer.writerows(unit_rows)

    with (catalog_dir / "buildings.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["table", "rawcode", "base_rawcode", "pathing_size_hint", *selected_building_fields])
        writer.writerows(building_rows)

    with (catalog_dir / "abilities.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "table", "rawcode", "base_rawcode", "anam", "alev", "amcs", "acdn", "aran", "aare",
            "atar", "abuf", "aeff", "atp1", "aub1", "all_fields_json",
        ])
        writer.writerows(ability_rows)

    return summary


def write_script_index(lua_path: Path, output: Path) -> dict[str, Any]:
    script_dir = output / "script"
    script_dir.mkdir(parents=True, exist_ok=True)
    destination = script_dir / "war3map.lua"
    shutil.copyfile(lua_path, destination)

    object_metadata: dict[int, list[dict[str, str]]] = defaultdict(list)
    with (output / "catalog" / "objects.tsv").open(encoding="utf-8", newline="") as f:
        for row in csv.DictReader(f, delimiter="\t"):
            raw = row["rawcode"].encode("latin1")
            if len(raw) != 4:
                continue
            object_metadata[int.from_bytes(raw, "big")].append(row)

    data = lua_path.read_bytes()
    analysis = analyze_lua(data, set(object_metadata))
    functions = analysis["functions"]
    calls = analysis["calls"]
    call_edges = analysis["call_edges"]
    rawcode_sites = analysis["rawcode_sites"]
    function_rawcodes = analysis["function_rawcodes"]
    runtime_mutators = analysis["runtime_mutators"]
    rawcode_mutator_traces = analysis["rawcode_mutator_traces"]
    protected_ability_fields = analysis["protected_ability_fields"]
    jass_add_protected_fields = analysis["jass_add_protected_fields"]
    unit_object_metadata = analysis["unit_object_metadata"]
    unit_object_metadata_fingerprint = int(analysis["unit_object_metadata_fingerprint"])
    unit_object_upgrades = analysis["unit_object_upgrades"]
    race_buildings = analysis["race_buildings"]
    income_factor_constants = analysis["income_factor_constants"]
    race_building_semantics = analysis["race_building_semantics"]
    element_building_buckets = analysis["element_building_buckets"]
    effective_unit_stats = analysis["effective_unit_stats"]
    protected_unit_stats = analysis["protected_unit_stats"]
    function_aliases = analysis["function_aliases"]
    function_value_arguments = analysis["function_value_arguments"]
    protected_filter_bindings = analysis["protected_filter_bindings"]
    protected_perk_registry_audit = analysis["protected_perk_registry_audit"]
    perk_mechanics = analysis["perk_mechanics"]
    runtime_ai_mechanics = analysis["runtime_ai_mechanics"]
    runtime_session_mechanics = analysis["runtime_session_mechanics"]
    runtime_mode_mechanics = analysis["runtime_mode_mechanics"]
    damage_listener_coverage = analysis["damage_listener_coverage"]
    event_listener_coverage = analysis["event_listener_coverage"]
    production_unit_special_mechanics = analysis["production_unit_special_mechanics"]
    building_improvement_spawn_mechanics = analysis["building_improvement_spawn_mechanics"]
    runtime_system_mechanics = analysis["runtime_system_mechanics"]
    castle_item_mechanics = analysis["castle_item_mechanics"]
    building_spell_registrations = analysis["building_spell_registrations"]
    building_spell_evidence = analysis["building_spell_evidence"]
    unit_spell_registrations = analysis["unit_spell_registrations"]
    unit_spell_mechanics = analysis["unit_spell_mechanics"]
    corpse_building_mechanics = analysis["corpse_building_mechanics"]
    building_spell_mechanics = analysis["building_spell_mechanics"]
    resolved_call_edges = int(analysis["resolved_call_edges"])

    function_names = [str(function["name"]) for function in functions]
    definitions = Counter(function_names)
    first = {str(function["name"]): int(function["start"]) for function in functions}

    with (script_dir / "functions.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["name", "definition_count", "first_byte_offset"])
        for name in sorted(definitions):
            writer.writerow([name, definitions[name], first[name]])

    with (script_dir / "function-spans.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["name", "start_byte_offset", "end_byte_offset", "byte_length"])
        for function in sorted(functions, key=lambda function: int(function["start"])):
            start = int(function["start"])
            end = int(function["end"])
            writer.writerow([function["name"], start, end, end - start])

    with (script_dir / "calls.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["callee", "count"])
        for name, count in sorted(calls.items(), key=lambda item: (-item[1], item[0])):
            writer.writerow([name, count])

    with (script_dir / "call-graph.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["caller", "callee", "count"])
        for (caller, callee), count in sorted(call_edges.items()):
            writer.writerow([caller, callee, count])

    slot_prefix_aliases = 0
    with (script_dir / "function-aliases.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "alias", "slot", "target_function", "structural_relation",
            "alias_byte_offset", "target_byte_offset",
        ])
        for alias in function_aliases:
            alias_name = str(alias["alias"])
            slot = alias_name.split(".", 1)[-1]
            target = str(alias["target_function"])
            relation = "slot-name-prefix" if target.startswith(slot) else "assigned-other-target"
            slot_prefix_aliases += int(relation == "slot-name-prefix")
            writer.writerow([
                alias_name,
                slot,
                target,
                relation,
                alias["alias_byte_offset"],
                alias["target_byte_offset"],
            ])

    with (script_dir / "function-value-arguments.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["target_function", "byte_offset", "function", "containing_call"])
        for reference in function_value_arguments:
            writer.writerow([
                reference["target_function"],
                reference["byte_offset"],
                reference["function"],
                reference["containing_call"],
            ])

    def rawcode_metadata(integer_id: int) -> tuple[str, str, str, str, int]:
        rows = object_metadata[integer_id]
        rawcode = rows[0]["rawcode"]
        categories = ",".join(sorted({row["category"] for row in rows}))
        tables = ",".join(sorted({row["table"] for row in rows}))
        names = " | ".join(sorted({row["name"] for row in rows if row["name"]}))
        return rawcode, categories, tables, names, len(rows)

    def rawcode_text(integer_id: int) -> str:
        if integer_id < 0 or integer_id > 0xFFFFFFFF:
            raise ValueError(f"rawcode integer is outside u32 range: {integer_id}")
        return integer_id.to_bytes(4, "big").decode("latin1")

    def script_json(value: object) -> str:
        return json.dumps(value, separators=(",", ":"), sort_keys=True, ensure_ascii=False, default=str)

    with (script_dir / "building-spell-registrations.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "ability_rawcode", "ability_rawcode_integer", "ability_names",
            "handler_function", "closure_class", "closure_variable", "registration_function", "evidence_kind", "byte_offset",
        ])
        for row in building_spell_registrations:
            building_id = int(row["building_id"])
            ability_id = int(row["ability_id"])
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            ability_rawcode, ability_categories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            if building_categories != "units" or ability_categories != "abilities":
                raise ValueError(
                    f"building-spell registration does not resolve to building/ability objects: "
                    f"{building_rawcode}/{ability_rawcode}"
                )
            writer.writerow([
                building_rawcode, building_id, building_names,
                ability_rawcode, ability_id, ability_names,
                row["handler_function"], row["closure_class"], row["closure_variable"],
                row["registration_function"], row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "unit-spell-registrations.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "unit_rawcode", "unit_rawcode_integer", "unit_names", "unit_categories",
            "ability_rawcode", "ability_rawcode_integer", "ability_names", "ability_categories",
            "target_mode", "target_mode_label", "order_id", "order_expression_kind",
            "expected_immediate_unit_rawcode", "expected_immediate_unit_rawcode_integer", "expected_immediate_unit_names",
            "handler_function", "closure_class", "closure_variable", "registration_function", "evidence_kind", "byte_offset",
        ])
        for row in unit_spell_registrations:
            unit_id = int(row["unit_id"])
            ability_id = int(row["ability_id"])
            unit_rawcode = rawcode_text(unit_id)
            unit_names = ""
            unit_categories = ""
            if unit_id in object_metadata:
                unit_rawcode, unit_categories, _utables, unit_names, _udefs = rawcode_metadata(unit_id)
            ability_rawcode = rawcode_text(ability_id)
            ability_names = ""
            ability_categories = ""
            if ability_id in object_metadata:
                ability_rawcode, ability_categories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            expected_id = int(row["expected_immediate_unit_id"])
            expected_rawcode = rawcode_text(expected_id) if expected_id else ""
            expected_names = ""
            if expected_id and expected_id in object_metadata:
                expected_rawcode, _ecategories, _etables, expected_names, _edefs = rawcode_metadata(expected_id)
            writer.writerow([
                unit_rawcode, unit_id, unit_names, unit_categories,
                ability_rawcode, ability_id, ability_names, ability_categories,
                row["target_mode"], row["target_mode_label"],
                row["order_id"] if row["order_id"] is not None else "", row["order_expression_kind"],
                expected_rawcode, expected_id if expected_id else "", expected_names,
                row["handler_function"], row["closure_class"], row["closure_variable"],
                row["registration_function"], row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "unit-spell-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "unit_rawcode", "unit_rawcode_integer", "unit_names",
            "ability_rawcode", "ability_rawcode_integer", "ability_names",
            "mechanic_kind", "direct_calls", "helper_functions", "delayed_callback_functions", "dynamic_callback_functions",
            "scheduled_delays_json", "periodic_intervals_json", "random_real_ranges_json",
            "direct_map_rawcodes", "reachable_map_rawcode_paths_json", "semantic_effect_sites_json", "source_numeric_literals_json",
            "handler_function", "evidence_kind", "byte_offset",
        ])
        for row in unit_spell_mechanics:
            unit_id = int(row["unit_id"])
            ability_id = int(row["ability_id"])
            unit_rawcode = rawcode_text(unit_id)
            unit_names = ""
            if unit_id in object_metadata:
                unit_rawcode, _ucategories, _utables, unit_names, _udefs = rawcode_metadata(unit_id)
            ability_rawcode = rawcode_text(ability_id)
            ability_names = ""
            if ability_id in object_metadata:
                ability_rawcode, _acategories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            direct_map_rawcodes = [rawcode_text(int(value)) for value in row["direct_map_rawcodes"]]
            reachable_paths = []
            for path in row["reachable_map_rawcode_paths"]:
                integer_id = int(path["rawcode_integer"])
                rawcode = rawcode_text(integer_id)
                names = ""
                categories = ""
                if integer_id in object_metadata:
                    rawcode, categories, _tables, names, _defs = rawcode_metadata(integer_id)
                reachable_paths.append({
                    "rawcode": rawcode,
                    "rawcode_integer": integer_id,
                    "categories": categories,
                    "names": names,
                    "hops": int(path["hops"]),
                    "path": path["path"],
                })
            writer.writerow([
                unit_rawcode, unit_id, unit_names,
                ability_rawcode, ability_id, ability_names,
                row["mechanic_kind"], ",".join(row["direct_calls"]), ",".join(row["helper_functions"]),
                ",".join(row["delayed_callback_functions"]), ",".join(row["dynamic_callback_functions"]),
                script_json(list(row["scheduled_delays"])), script_json(list(row["periodic_intervals"])),
                script_json([list(values) for values in row["random_real_ranges"]]),
                ",".join(direct_map_rawcodes), script_json(reachable_paths), script_json(list(row["semantic_effect_sites"])),
                script_json(list(row["source_numeric_literals"])), row["handler_function"], row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "building-spell-evidence.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "ability_rawcode", "ability_rawcode_integer", "ability_names",
            "mechanic_kind", "direct_calls", "helper_functions", "delayed_callback_functions", "dynamic_callback_functions",
            "scheduled_delays_json", "periodic_intervals_json", "random_real_ranges_json",
            "direct_map_rawcodes", "reachable_map_rawcode_paths_json", "semantic_effect_sites_json", "source_numeric_literals_json",
            "handler_function", "evidence_kind", "byte_offset",
        ])
        for row in building_spell_evidence:
            building_id = int(row["building_id"])
            ability_id = int(row["ability_id"])
            building_rawcode = rawcode_text(building_id)
            building_names = ""
            if building_id in object_metadata:
                building_rawcode, _bcategories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            ability_rawcode = rawcode_text(ability_id)
            ability_names = ""
            if ability_id in object_metadata:
                ability_rawcode, _acategories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            direct_map_rawcodes = [rawcode_text(int(value)) for value in row["direct_map_rawcodes"]]
            reachable_paths = []
            for path in row["reachable_map_rawcode_paths"]:
                integer_id = int(path["rawcode_integer"])
                rawcode = rawcode_text(integer_id)
                names = ""
                categories = ""
                if integer_id in object_metadata:
                    rawcode, categories, _tables, names, _defs = rawcode_metadata(integer_id)
                reachable_paths.append({
                    "rawcode": rawcode,
                    "rawcode_integer": integer_id,
                    "categories": categories,
                    "names": names,
                    "hops": int(path["hops"]),
                    "path": path["path"],
                })
            writer.writerow([
                building_rawcode, building_id, building_names,
                ability_rawcode, ability_id, ability_names,
                row["mechanic_kind"], ",".join(row["direct_calls"]), ",".join(row["helper_functions"]),
                ",".join(row["delayed_callback_functions"]), ",".join(row["dynamic_callback_functions"]),
                script_json(list(row["scheduled_delays"])), script_json(list(row["periodic_intervals"])),
                script_json([list(values) for values in row["random_real_ranges"]]),
                ",".join(direct_map_rawcodes), script_json(reachable_paths), script_json(list(row["semantic_effect_sites"])),
                script_json(list(row["source_numeric_literals"])), row["handler_function"], row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "protected-filter-bindings.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "symbol", "initializer_function", "resolved_function", "predicate",
            "resolution_status", "evidence_kind", "byte_offset",
        ])
        for row in protected_filter_bindings:
            writer.writerow([
                row["symbol"], row["initializer_function"], row["resolved_function"], row["predicate"],
                row["resolution_status"], row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "protected-perk-registry-audit.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "perk_id", "perk_name", "factory_function", "damage_listener_function",
            "protected_initializer_function", "protected_vm_index", "protected_registry_slot_count",
            "protected_registry_slot", "factory_global_index", "signature_global_index", "signature_global_name",
            "factory_value_evidence", "registration_pc", "normal_draft_initializer_callers",
            "runtime_registry_path_status", "individual_factory_registration_status",
            "individual_factory_registration_proven", "evidence_kind", "byte_offset",
        ])
        for row in protected_perk_registry_audit:
            writer.writerow([
                row["perk_id"], row["perk_name"], row["factory_function"], row["damage_listener_function"],
                row["protected_initializer_function"], row["protected_vm_index"], row["protected_registry_slot_count"],
                row["protected_registry_slot"], row["factory_global_index"], row["signature_global_index"],
                row["signature_global_name"], row["factory_value_evidence"], row["registration_pc"],
                ",".join(str(value) for value in row["normal_draft_initializer_callers"]),
                row["runtime_registry_path_status"], row["individual_factory_registration_status"],
                int(bool(row["individual_factory_registration_proven"])), row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "perk-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "perk_id", "perk_name", "protected_registry_slot", "mechanic_kind", "trigger",
            "related_objects_json", "parameters_json", "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in perk_mechanics:
            related_objects: list[dict[str, object]] = []
            for related_id_value in row["related_rawcode_ids"]:
                related_id = int(related_id_value)
                related = {
                    "rawcode": rawcode_text(related_id),
                    "rawcode_integer": related_id,
                    "categories": "",
                    "names": "",
                }
                if related_id in object_metadata:
                    rawcode, related_categories, _rtables, related_names, _rdefs = rawcode_metadata(related_id)
                    related.update({
                        "rawcode": rawcode,
                        "categories": related_categories,
                        "names": related_names,
                    })
                related_objects.append(related)
            writer.writerow([
                row["perk_id"], row["perk_name"], row["protected_registry_slot"], row["mechanic_kind"],
                row["trigger"], script_json(related_objects), script_json(row["parameters"]),
                ",".join(str(value) for value in row["source_functions"]), row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "runtime-ai-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in runtime_ai_mechanics:
            related_objects: list[dict[str, object]] = []
            for related_id_value in row["related_rawcode_ids"]:
                related_id = int(related_id_value)
                related = {
                    "rawcode": rawcode_text(related_id),
                    "rawcode_integer": related_id,
                    "categories": "",
                    "names": "",
                }
                if related_id in object_metadata:
                    rawcode, related_categories, _rtables, related_names, _rdefs = rawcode_metadata(related_id)
                    related.update({
                        "rawcode": rawcode,
                        "categories": related_categories,
                        "names": related_names,
                    })
                related_objects.append(related)
            writer.writerow([
                row["system_id"], row["mechanic_kind"], row["trigger"], script_json(related_objects),
                script_json(row["parameters"]), ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "runtime-session-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in runtime_session_mechanics:
            writer.writerow([
                row["system_id"], row["mechanic_kind"], row["trigger"], "[]",
                script_json(row["parameters"]), ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "runtime-mode-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in runtime_mode_mechanics:
            writer.writerow([
                row["system_id"], row["mechanic_kind"], row["trigger"], "[]",
                script_json(row["parameters"]), ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "event-listener-coverage.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "listener_function", "coverage_status", "normalized_sources", "dispatch_path",
            "evidence_note", "byte_offset",
        ])
        for row in event_listener_coverage:
            writer.writerow([
                row["listener_function"], row["coverage_status"],
                ",".join(str(value) for value in row["normalized_sources"]),
                " -> ".join(str(value) for value in row["dispatch_path"]),
                row["evidence_note"], row["byte_offset"],
            ])

    with (script_dir / "damage-listener-coverage.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "listener_function", "coverage_status", "candidate_perk_factory",
            "normalized_sources", "evidence_note", "byte_offset",
        ])
        for row in damage_listener_coverage:
            writer.writerow([
                row["listener_function"], row["coverage_status"], row["candidate_perk_factory"],
                ",".join(str(value) for value in row["normalized_sources"]),
                row["evidence_note"], row["byte_offset"],
            ])

    with (script_dir / "production-unit-special-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "unit_rawcode", "unit_rawcode_integer", "unit_names", "mechanic_kind", "trigger",
            "related_objects_json", "parameters_json", "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in production_unit_special_mechanics:
            unit_id = int(row["unit_id"])
            unit_rawcode, categories, _tables, unit_names, _defs = rawcode_metadata(unit_id)
            if categories != "units":
                raise ValueError(f"special production-unit mechanic resolves to non-unit rawcode: {unit_rawcode}")
            related_objects: list[dict[str, object]] = []
            for related_id_value in row["related_rawcode_ids"]:
                related_id = int(related_id_value)
                related = {
                    "rawcode": rawcode_text(related_id),
                    "rawcode_integer": related_id,
                    "categories": "",
                    "names": "",
                }
                if related_id in object_metadata:
                    rawcode, related_categories, _rtables, related_names, _rdefs = rawcode_metadata(related_id)
                    related.update({
                        "rawcode": rawcode,
                        "categories": related_categories,
                        "names": related_names,
                    })
                related_objects.append(related)
            writer.writerow([
                unit_rawcode, unit_id, unit_names, row["mechanic_kind"], row["trigger"],
                script_json(related_objects), script_json(row["parameters"]),
                ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "building-improvement-spawn-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "source_unit_rawcode", "source_unit_rawcode_integer", "source_unit_names",
            "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in building_improvement_spawn_mechanics:
            source_unit_id = int(row["source_unit_id"])
            source_rawcode, categories, _tables, source_names, _defs = rawcode_metadata(source_unit_id)
            if categories != "units":
                raise ValueError(f"building-improvement source resolves to non-unit rawcode: {source_rawcode}")
            related_objects: list[dict[str, object]] = []
            for related_id_value in row["related_rawcode_ids"]:
                related_id = int(related_id_value)
                related = {
                    "rawcode": rawcode_text(related_id),
                    "rawcode_integer": related_id,
                    "categories": "",
                    "names": "",
                }
                if related_id in object_metadata:
                    rawcode, related_categories, _rtables, related_names, _rdefs = rawcode_metadata(related_id)
                    related.update({
                        "rawcode": rawcode,
                        "categories": related_categories,
                        "names": related_names,
                    })
                related_objects.append(related)
            writer.writerow([
                source_rawcode, source_unit_id, source_names, row["mechanic_kind"], row["trigger"],
                script_json(related_objects), script_json(row["parameters"]),
                ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "runtime-system-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "system_id", "mechanic_kind", "trigger", "related_objects_json", "parameters_json",
            "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in runtime_system_mechanics:
            related_objects: list[dict[str, object]] = []
            for related_id_value in row["related_rawcode_ids"]:
                related_id = int(related_id_value)
                related = {
                    "rawcode": rawcode_text(related_id),
                    "rawcode_integer": related_id,
                    "categories": "",
                    "names": "",
                }
                if related_id in object_metadata:
                    rawcode, related_categories, _rtables, related_names, _rdefs = rawcode_metadata(related_id)
                    related.update({
                        "rawcode": rawcode,
                        "categories": related_categories,
                        "names": related_names,
                    })
                related_objects.append(related)
            writer.writerow([
                row["system_id"], row["mechanic_kind"], row["trigger"],
                script_json(related_objects), script_json(row["parameters"]),
                ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    with (script_dir / "castle-shop-items.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "slot", "item_rawcode", "item_rawcode_integer", "item_name", "script_item_value",
            "source_function", "byte_offset",
        ])
        for row in castle_item_mechanics["shop_slots"]:
            item_id = int(row["item_id"])
            item_rawcode, categories, _tables, item_name, _defs = rawcode_metadata(item_id)
            if categories != "items":
                raise ValueError(f"castle shop slot resolves to non-item rawcode: {item_rawcode}")
            writer.writerow([
                row["slot"], item_rawcode, item_id, item_name,
                castle_item_mechanics["item_values"].get(item_id, ""),
                row["function"], row["byte_offset"],
            ])

    pickup = castle_item_mechanics["pickup"]
    pickup_rows = [
        {
            "item_id": pickup["gold_item_rawcode_integer"],
            "mechanic_kind": "gold-pickup-scaling",
            "linked_ability_ids": [],
            "parameters": {
                "gold_amount_formula": pickup["gold_amount_formula"],
                "gold_item_consumed": pickup["gold_item_consumed"],
            },
            "source_functions": ["ZH"],
        },
        {
            "item_id": pickup["cheese_item_rawcode_integer"],
            "mechanic_kind": "legendary-slot-or-refund-on-pickup",
            "linked_ability_ids": [],
            "parameters": {
                "active_when_no_cheese_mode": False,
                "no_cheese_mode_flag": pickup["cheese_no_cheese_mode_flag"],
                "legendary_mode_enabled_flag": pickup["cheese_legendary_mode_enabled_flag"],
                "food_cap_delta_when_active": pickup["cheese_food_cap_delta"],
                "refund_gold_when_inactive": pickup["cheese_refund_amount"],
            },
            "source_functions": ["ZH", "YH"],
        },
        {
            "item_id": pickup["blast_staff_rawcode_integer"],
            "mechanic_kind": "four-copy-inventory-upgrade",
            "linked_ability_ids": [],
            "parameters": {
                "required_count": pickup["blast_staff_recipe_required_count"],
                "remove_item_rawcode": rawcode_text(int(pickup["blast_staff_recipe_removes_rawcode"])),
                "add_item_rawcode": rawcode_text(int(pickup["blast_staff_recipe_adds_rawcode"])),
                "inventory_slot_max_index": pickup["item_inventory_slot_max_index"],
            },
            "source_functions": ["ZH", "VH", "WH"],
        },
        {
            "item_id": 1227894840,
            "mechanic_kind": "temporary-double-damage-aura-carrier",
            "linked_ability_ids": [pickup["double_damage_aura_ability_rawcode_integer"]],
            "parameters": {
                "carrier_unit_rawcode": rawcode_text(int(pickup["damage_aura_carrier_rawcode_integer"])),
                "aura_ability_rawcode": rawcode_text(int(pickup["double_damage_aura_ability_rawcode_integer"])),
                "lifetime_seconds": pickup["damage_aura_lifetime_seconds"],
            },
            "source_functions": ["ZH"],
        },
        {
            "item_id": 1227894839,
            "mechanic_kind": "temporary-quad-damage-aura-carrier",
            "linked_ability_ids": [pickup["quad_damage_aura_ability_rawcode_integer"]],
            "parameters": {
                "carrier_unit_rawcode": rawcode_text(int(pickup["damage_aura_carrier_rawcode_integer"])),
                "aura_ability_rawcode": rawcode_text(int(pickup["quad_damage_aura_ability_rawcode_integer"])),
                "lifetime_seconds": pickup["damage_aura_lifetime_seconds"],
            },
            "source_functions": ["ZH"],
        },
    ]
    with (script_dir / "item-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "item_rawcode", "item_rawcode_integer", "item_name", "mechanic_kind",
            "linked_ability_rawcodes", "parameters_json", "source_functions", "evidence_kind",
        ])
        for row in pickup_rows:
            item_id = int(row["item_id"])
            item_rawcode, categories, _tables, item_name, _defs = rawcode_metadata(item_id)
            if categories != "items":
                raise ValueError(f"item mechanic resolves to non-item rawcode: {item_rawcode}")
            writer.writerow([
                item_rawcode, item_id, item_name, row["mechanic_kind"],
                ",".join(rawcode_text(int(value)) for value in row["linked_ability_ids"]),
                script_json(row["parameters"]), ",".join(row["source_functions"]), "script-direct",
            ])

    with (script_dir / "item-spell-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "trigger_ability_rawcode", "trigger_ability_rawcode_integer", "trigger_ability_name",
            "mechanic_kind", "effect_ability_rawcodes", "parameters_json", "source_functions",
            "evidence_kind", "byte_offset",
        ])
        for row in castle_item_mechanics["item_spell_mechanics"]:
            trigger_id = int(row["trigger_ability_id"])
            trigger_rawcode, categories, _tables, trigger_name, _defs = rawcode_metadata(trigger_id)
            if categories != "abilities":
                raise ValueError(f"item spell trigger resolves to non-ability rawcode: {trigger_rawcode}")
            effect_rawcodes: list[str] = []
            for effect_id in row["effect_ability_ids"]:
                effect_rawcode, effect_categories, _etables, _ename, _edefs = rawcode_metadata(int(effect_id))
                if effect_categories != "abilities":
                    raise ValueError(f"item spell effect resolves to non-ability rawcode: {effect_rawcode}")
                effect_rawcodes.append(effect_rawcode)
            parameters = dict(row["parameters"])
            carrier_id = parameters.pop("carrier_unit_rawcode_integer", None)
            if carrier_id is not None:
                parameters["carrier_unit_rawcode"] = rawcode_text(int(carrier_id))
            writer.writerow([
                trigger_rawcode, trigger_id, trigger_name, row["mechanic_kind"], ",".join(effect_rawcodes),
                script_json(parameters), ",".join(row["source_functions"]), "script-direct", row["byte_offset"],
            ])

    with (script_dir / "corpse-building-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "ability_rawcode", "ability_rawcode_integer", "ability_names",
            "mechanic_kind", "corpse_phase", "selection_predicate", "selection_rect_symbol",
            "selection_function", "requires_wc3_can_raise", "consumption_mode", "consume_radius",
            "effect_radius", "damage", "attack_type", "damage_type",
            "auxiliary_ability_rawcode", "auxiliary_ability_rawcode_integer", "auxiliary_ability_names",
            "invulnerable_ability_rawcode", "invulnerable_ability_rawcode_integer",
            "summon_outcomes_json", "handler_function", "predicate_function", "effect_function", "byte_offset",
        ])
        for row in corpse_building_mechanics:
            building_id = int(row["building_id"])
            ability_id = int(row["ability_id"])
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            ability_rawcode, ability_categories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            if building_categories != "units" or ability_categories != "abilities":
                raise ValueError(f"corpse building mechanic has invalid building/ability: {building_rawcode}/{ability_rawcode}")

            auxiliary_id = row["auxiliary_ability_id"]
            auxiliary_rawcode = ""
            auxiliary_names = ""
            if auxiliary_id is not None:
                auxiliary_id = int(auxiliary_id)
                auxiliary_rawcode = rawcode_text(auxiliary_id)
                if auxiliary_id in object_metadata:
                    _arc, auxiliary_categories, _atables, auxiliary_names, _adefs = rawcode_metadata(auxiliary_id)
                    if auxiliary_categories != "abilities":
                        raise ValueError(f"corpse mechanic auxiliary rawcode is not an ability: {auxiliary_rawcode}")

            invulnerable_id = row["invulnerable_ability_id"]
            invulnerable_rawcode = ""
            if invulnerable_id is not None:
                invulnerable_id = int(invulnerable_id)
                invulnerable_rawcode = rawcode_text(invulnerable_id)

            outcomes: list[dict[str, object]] = []
            for outcome_id, probability in row["summon_outcomes"]:
                outcome_id = int(outcome_id)
                outcome_rawcode, outcome_categories, _otables, outcome_names, _odefs = rawcode_metadata(outcome_id)
                if outcome_categories != "units":
                    raise ValueError(f"corpse mechanic summon outcome is not a unit: {outcome_rawcode}")
                outcomes.append({
                    "rawcode": outcome_rawcode,
                    "rawcode_integer": outcome_id,
                    "names": outcome_names,
                    "probability_percent": int(probability),
                })

            writer.writerow([
                building_rawcode, building_id, building_names,
                ability_rawcode, ability_id, ability_names,
                row["mechanic_kind"], row["corpse_phase"], row["selection_predicate"], row["selection_rect_symbol"],
                row["selection_function"], int(bool(row["requires_wc3_can_raise"])), row["consumption_mode"],
                row["consume_radius"], row["effect_radius"], row["damage"], row["attack_type"], row["damage_type"],
                auxiliary_rawcode, auxiliary_id if auxiliary_id is not None else "", auxiliary_names,
                invulnerable_rawcode, invulnerable_id if invulnerable_id is not None else "",
                json.dumps(outcomes, separators=(",", ":"), ensure_ascii=False),
                row["handler_function"], row["predicate_function"], row["effect_function"], row["byte_offset"],
            ])

    with (script_dir / "building-spell-mechanics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "ability_rawcode", "ability_rawcode_integer", "ability_names",
            "mechanic_kind", "target_selector", "target_predicate", "effect_rawcodes_json",
            "parameters_json", "source_functions", "evidence_kind", "byte_offset",
        ])
        for row in building_spell_mechanics:
            building_id = int(row["building_id"])
            ability_id = int(row["ability_id"])
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            ability_rawcode, ability_categories, _atables, ability_names, _adefs = rawcode_metadata(ability_id)
            if building_categories != "units" or ability_categories != "abilities":
                raise ValueError(
                    f"building-spell mechanic does not resolve to building/ability objects: "
                    f"{building_rawcode}/{ability_rawcode}"
                )
            effects: list[dict[str, object]] = []
            for rawcode_id in row["effect_rawcode_ids"]:
                integer_id = int(rawcode_id)
                effect = {
                    "rawcode": rawcode_text(integer_id),
                    "rawcode_integer": integer_id,
                    "categories": "",
                    "names": "",
                }
                if integer_id in object_metadata:
                    effect_rawcode, categories, _tables, names, _defs = rawcode_metadata(integer_id)
                    effect["rawcode"] = effect_rawcode
                    effect["categories"] = categories
                    effect["names"] = names
                effects.append(effect)
            writer.writerow([
                building_rawcode, building_id, building_names,
                ability_rawcode, ability_id, ability_names,
                row["mechanic_kind"], row["target_selector"], row["target_predicate"],
                script_json(effects), script_json(row["parameters"]),
                ",".join(str(value) for value in row["source_functions"]),
                row["evidence_kind"], row["byte_offset"],
            ])

    sites_by_rawcode: dict[int, list[dict[str, object]]] = defaultdict(list)
    for site in rawcode_sites:
        sites_by_rawcode[int(site["rawcode_integer"])].append(site)

    with (script_dir / "rawcode-reference-sites.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "names", "byte_offset", "function", "call",
        ])
        for site in sorted(rawcode_sites, key=lambda site: int(site["byte_offset"])):
            integer_id = int(site["rawcode_integer"])
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                names,
                site["byte_offset"],
                site["function"],
                site["call"],
            ])

    with (script_dir / "rawcode-summary.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "tables", "names", "definition_count",
            "reference_count", "function_count", "functions", "first_byte_offset",
        ])
        for integer_id in sorted(object_metadata, key=lambda value: object_metadata[value][0]["rawcode"]):
            rawcode, categories, tables, names, definition_count = rawcode_metadata(integer_id)
            sites = sites_by_rawcode.get(integer_id, [])
            functions_for_rawcode = sorted({str(site["function"]) for site in sites})
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                tables,
                names,
                definition_count,
                len(sites),
                len(functions_for_rawcode),
                ",".join(functions_for_rawcode),
                min((int(site["byte_offset"]) for site in sites), default=""),
            ])

    with (script_dir / "function-rawcodes.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow(["function", "rawcode", "rawcode_integer", "categories", "names", "reference_count"])
        for (function, integer_id), count in sorted(function_rawcodes.items()):
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            writer.writerow([function, rawcode, integer_id, categories, names, count])

    with (script_dir / "runtime-mutators.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "callee", "normalized_callee", "byte_offset", "function", "direct_map_rawcodes", "direct_map_object_names",
        ])
        for site in runtime_mutators:
            integer_ids = [int(value) for value in site["direct_map_rawcodes"]]
            rawcodes = [rawcode_metadata(value)[0] for value in integer_ids]
            names = sorted({
                name
                for value in integer_ids
                for name in (rawcode_metadata(value)[3],)
                if name
            })
            writer.writerow([
                site["callee"],
                site["normalized_callee"],
                site["byte_offset"],
                site["function"],
                ",".join(rawcodes),
                " | ".join(names),
            ])

    with (script_dir / "protected-ability-fields.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "names", "level_index", "level",
            "field", "runtime_value", "source_function", "byte_offset", "jass_add_restore",
        ])
        for row in protected_ability_fields:
            integer_id = int(row["ability_id"])
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            level_index = int(row["level_index"])
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                names,
                level_index,
                level_index + 1,
                row["field"],
                row["value_text"],
                row["source_function"],
                row["byte_offset"],
                int(bool(row["jass_add_restore"])),
            ])

    with (script_dir / "protected-ability-jass-add-restores.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "names", "level_index", "level",
            "field", "runtime_value", "canonical_relation", "source_function", "byte_offset",
        ])
        for row in jass_add_protected_fields:
            integer_id = int(row["ability_id"])
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            level_index = int(row["level_index"])
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                names,
                level_index,
                level_index + 1,
                row["field"],
                row["value_text"],
                row["canonical_relation"],
                row["source_function"],
                row["byte_offset"],
            ])

    with (script_dir / "unit-object-metadata.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "unit_rawcode", "unit_rawcode_integer", "unit_names",
            "gold_cost", "lumber_cost", "food_used", "spawn_build_time",
            "attack_index", "defense_index", "is_air", "is_melee", "is_mechanical", "is_caster",
            "source_function", "byte_offset",
        ])
        for row in unit_object_metadata:
            building_id = int(row["building_id"])
            unit_id = int(row["unit_id"])
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            if building_categories != "units":
                raise ValueError(f"UnitObjectMeta building {building_rawcode} does not resolve uniquely to a unit/building object")
            if unit_id:
                unit_rawcode, unit_categories, _utables, unit_names, _udefs = rawcode_metadata(unit_id)
                if unit_categories != "units":
                    raise ValueError(f"UnitObjectMeta spawn {unit_rawcode} does not resolve uniquely to a unit object")
            else:
                unit_rawcode = ""
                unit_names = ""
            writer.writerow([
                building_rawcode, building_id, building_names,
                unit_rawcode, unit_id, unit_names,
                row["gold_cost"], row["lumber_cost"], row["food_used"], row["spawn_build_time"],
                row["attack_index"], row["defense_index"],
                int(bool(row["is_air"])), int(bool(row["is_melee"])), int(bool(row["is_mechanical"])), int(bool(row["is_caster"])),
                row["source_function"], row["byte_offset"],
            ])

    with (script_dir / "building-upgrades.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "source_building_rawcode", "source_building_rawcode_integer", "source_building_names",
            "target_building_rawcode", "target_building_rawcode_integer", "target_building_names",
            "source_function", "byte_offset",
        ])
        for row in unit_object_upgrades:
            source_id = int(row["source_building_id"])
            target_id = int(row["target_building_id"])
            source_rawcode, source_categories, _stables, source_names, _sdefs = rawcode_metadata(source_id)
            target_rawcode, target_categories, _ttables, target_names, _tdefs = rawcode_metadata(target_id)
            if source_categories != "units" or target_categories != "units":
                raise ValueError(f"authored building upgrade does not resolve to unit/building objects: {source_rawcode}->{target_rawcode}")
            writer.writerow([
                source_rawcode, source_id, source_names,
                target_rawcode, target_id, target_names,
                row["source_function"], row["byte_offset"],
            ])

    with (script_dir / "race-buildings.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "race_index", "race_function", "builder_rawcode", "builder_rawcode_integer", "builder_names", "campaign_only",
            "building_order", "building_rawcode", "building_rawcode_integer", "building_names",
            "unit_rawcode", "unit_rawcode_integer", "unit_names", "byte_offset",
        ])
        for row in race_buildings:
            builder_id = int(row["builder_id"])
            building_id = int(row["building_id"])
            unit_id = int(row["unit_id"])
            builder_rawcode, builder_categories, _brtables, builder_names, _brdefs = rawcode_metadata(builder_id)
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            if builder_categories != "units" or building_categories != "units":
                raise ValueError(f"race catalog rawcodes do not resolve to unit/building objects: {builder_rawcode}/{building_rawcode}")
            if unit_id:
                unit_rawcode, unit_categories, _utables, unit_names, _udefs = rawcode_metadata(unit_id)
                if unit_categories != "units":
                    raise ValueError(f"race catalog spawn {unit_rawcode} does not resolve to a unit object")
            else:
                unit_rawcode = ""
                unit_names = ""
            writer.writerow([
                row["race_index"], row["race_function"], builder_rawcode, builder_id, builder_names, int(bool(row["campaign_only"])),
                row["building_order"], building_rawcode, building_id, building_names,
                unit_rawcode, unit_id, unit_names, row["byte_offset"],
            ])

    upgrade_pairs = {
        (int(row["source_building_id"]), int(row["target_building_id"]))
        for row in unit_object_upgrades
    }
    semantic_precursor_pairs = {
        (int(row["precursor_building_id"]), int(row["building_id"]))
        for row in race_building_semantics
        if row["precursor_building_id"] is not None
    }
    if semantic_precursor_pairs != upgrade_pairs:
        missing_from_semantics = sorted(upgrade_pairs - semantic_precursor_pairs)
        missing_from_upgrades = sorted(semantic_precursor_pairs - upgrade_pairs)
        raise ValueError(
            "race precursor wrappers disagree with authored upgrade metadata: "
            f"missing_from_semantics={missing_from_semantics[:10]} "
            f"missing_from_upgrades={missing_from_upgrades[:10]}"
        )

    with (script_dir / "race-building-semantics.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "income_factor_symbol", "income_factor", "precursor_rawcode", "precursor_rawcode_integer", "precursor_names",
            "has_tier_assignment", "is_legendary_line", "is_anti_air", "is_siege", "is_artillery",
            "is_na_only", "is_ultimate_only", "no_pp", "ai_should_ignore",
            "provides_active_targeted_spell_shield", "area_spell",
            "multi_target_mult", "cage_pressure", "placement_strat", "spell_dps", "ai_tower_strength",
            "combat_power_factor", "tags", "extra_tags", "override_tags",
            "source_function", "first_wrapper_byte_offset",
        ])
        for row in race_building_semantics:
            building_id = int(row["building_id"])
            building_rawcode, building_categories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            if building_categories != "units":
                raise ValueError(f"race building semantic {building_rawcode} does not resolve to a unit/building object")
            precursor_id = int(row["precursor_building_id"] or 0)
            if precursor_id:
                precursor_rawcode, precursor_categories, _ptables, precursor_names, _pdefs = rawcode_metadata(precursor_id)
                if precursor_categories != "units":
                    raise ValueError(f"race building precursor {precursor_rawcode} does not resolve to a unit/building object")
            else:
                precursor_rawcode = ""
                precursor_names = ""
            writer.writerow([
                building_rawcode, building_id, building_names,
                row["income_factor_symbol"], row["income_factor"],
                precursor_rawcode, precursor_id, precursor_names,
                int(bool(row["has_tier_assignment"])), int(bool(row["is_legendary_line"])),
                int(bool(row["is_anti_air"])), int(bool(row["is_siege"])), int(bool(row["is_artillery"])),
                int(bool(row["is_na_only"])), int(bool(row["is_ultimate_only"])), int(bool(row["no_pp"])),
                int(bool(row["ai_should_ignore"])), int(bool(row["provides_active_targeted_spell_shield"])),
                int(bool(row["area_spell"])), row["multi_target_mult"] or "", row["cage_pressure"] or "",
                row["placement_strat"] or "", row["spell_dps"] or "", row["ai_tower_strength"] or "",
                row["combat_power_factor"] or "", ",".join(str(value) for value in row["tags"]),
                ",".join(str(value) for value in row["extra_tags"]),
                ",".join(str(value) for value in row["override_tags"]),
                row["source_function"], row["first_wrapper_byte_offset"],
            ])

    with (script_dir / "element-building-buckets.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "bucket", "building_rawcode", "building_rawcode_integer", "building_names", "source_function", "byte_offset",
        ])
        for row in element_building_buckets:
            building_id = int(row["building_id"])
            building_rawcode, building_categories, _tables, building_names, _defs = rawcode_metadata(building_id)
            if building_categories != "units":
                raise ValueError(f"Elemental bucket building {building_rawcode} does not resolve uniquely to a unit/building object")
            writer.writerow([
                row["bucket"], building_rawcode, building_id, building_names, row["source_function"], row["byte_offset"],
            ])

    with (script_dir / "effective-unit-stats.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "building_rawcode", "building_rawcode_integer", "building_names",
            "unit_rawcode", "unit_rawcode_integer", "unit_names",
            "hp", "armor", "dps", "attack_range", "move_speed", "spawns_per_cycle",
            "can_hit_air", "source_function", "byte_offset",
        ])
        for row in effective_unit_stats:
            building_id = int(row["building_id"])
            unit_id = int(row["unit_id"])
            building_rawcode, _bcategories, _btables, building_names, _bdefs = rawcode_metadata(building_id)
            unit_rawcode, _ucategories, _utables, unit_names, _udefs = rawcode_metadata(unit_id)
            writer.writerow([
                building_rawcode,
                building_id,
                building_names,
                unit_rawcode,
                unit_id,
                unit_names,
                row["hp"],
                row["armor"],
                row["dps"],
                row["attack_range"],
                row["move_speed"],
                row["spawns_per_cycle"],
                int(bool(row["can_hit_air"])),
                row["source_function"],
                row["byte_offset"],
            ])

    with (script_dir / "protected-unit-stats.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "names", "source_fingerprint", "override_field_count",
            "hp", "armor", "defense_type", "move_speed",
            "attack1_base_damage", "attack1_dice_number", "attack1_dice_sides",
            "attack1_cooldown_microseconds", "attack1_cooldown", "attack1_range",
            "attack2_base_damage", "attack2_dice_number", "attack2_dice_sides",
            "attack2_cooldown_microseconds", "attack2_cooldown", "attack2_range",
            "encoded_values_json", "source_function", "byte_offset",
        ])
        decoded_fields = [
            "hp", "armor", "defense_type", "move_speed",
            "attack1_base_damage", "attack1_dice_number", "attack1_dice_sides",
            "attack1_cooldown_microseconds", "attack1_range",
            "attack2_base_damage", "attack2_dice_number", "attack2_dice_sides",
            "attack2_cooldown_microseconds", "attack2_range",
        ]
        for row in protected_unit_stats:
            integer_id = int(row["unit_id"])
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            if categories != "units":
                raise ValueError(f"protected UnitStat row {rawcode} does not resolve uniquely to a unit object")
            writer.writerow([
                rawcode,
                integer_id,
                names,
                row["source_fingerprint"],
                sum(row[field] is not None for field in decoded_fields),
                row["hp"],
                row["armor"],
                row["defense_type"],
                row["move_speed"],
                row["attack1_base_damage"],
                row["attack1_dice_number"],
                row["attack1_dice_sides"],
                row["attack1_cooldown_microseconds"],
                row["attack1_cooldown"],
                row["attack1_range"],
                row["attack2_base_damage"],
                row["attack2_dice_number"],
                row["attack2_dice_sides"],
                row["attack2_cooldown_microseconds"],
                row["attack2_cooldown"],
                row["attack2_range"],
                json.dumps(row["encoded_values"], separators=(",", ":")),
                row["source_function"],
                row["byte_offset"],
            ])

    traces_by_rawcode: dict[int, list[dict[str, object]]] = defaultdict(list)
    with (script_dir / "rawcode-mutator-traces.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "names", "evidence_kind",
            "source_function", "source_reference_count", "mutation_function", "mutator_callee",
            "normalized_mutator", "mutator_byte_offset", "hop_count", "call_path",
        ])
        for trace in rawcode_mutator_traces:
            integer_id = int(trace["rawcode_integer"])
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            traces_by_rawcode[integer_id].append(trace)
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                names,
                trace["evidence_kind"],
                trace["source_function"],
                trace["source_reference_count"],
                trace["mutation_function"],
                trace["mutator_callee"],
                trace["normalized_mutator"],
                trace["mutator_byte_offset"],
                trace["hop_count"],
                " -> ".join(str(part) for part in trace["call_path"]),
            ])

    with (script_dir / "rawcode-mutator-summary.tsv").open("w", encoding="utf-8", newline="") as f:
        writer = csv.writer(f, delimiter="\t", lineterminator="\n")
        writer.writerow([
            "rawcode", "rawcode_integer", "categories", "names", "trace_count",
            "direct_trace_count", "source_function_count", "reachable_mutator_site_count",
            "mutation_function_count", "normalized_mutators", "min_hops", "max_hops",
        ])
        for integer_id in sorted(traces_by_rawcode, key=lambda value: rawcode_metadata(value)[0]):
            rawcode, categories, _tables, names, _definitions = rawcode_metadata(integer_id)
            traces = traces_by_rawcode[integer_id]
            writer.writerow([
                rawcode,
                integer_id,
                categories,
                names,
                len(traces),
                sum(trace["evidence_kind"] == "direct-same-function" for trace in traces),
                len({str(trace["source_function"]) for trace in traces}),
                len({int(trace["mutator_byte_offset"]) for trace in traces}),
                len({str(trace["mutation_function"]) for trace in traces}),
                ",".join(sorted({str(trace["normalized_mutator"]) for trace in traces})),
                min(int(trace["hop_count"]) for trace in traces),
                max(int(trace["hop_count"]) for trace in traces),
            ])

    readable = sorted({
        name for name in function_names
        if len(name) >= 5 and ("_" in name or any(ch.isupper() for ch in name[1:]))
    })
    (script_dir / "readable-function-names.txt").write_text("\n".join(readable) + "\n", encoding="utf-8")

    return {
        "bytes": len(data),
        "line_count": data.count(b"\n") + 1,
        "w3p_marker": data.startswith(b"--W3P"),
        "function_definitions": len(functions),
        "unique_function_names": len(definitions),
        "unique_call_tokens": len(calls),
        "call_graph_edges": len(call_edges),
        "resolved_call_graph_edges": resolved_call_edges,
        "function_alias_assignments": len(function_aliases),
        "slot_prefix_function_aliases": slot_prefix_aliases,
        "aliased_function_targets": len({str(alias["target_function"]) for alias in function_aliases}),
        "function_value_arguments": len(function_value_arguments),
        "function_value_argument_targets": len({
            str(reference["target_function"]) for reference in function_value_arguments
        }),
        "building_spell_registrations": len(building_spell_registrations),
        "building_spell_handlers": len({str(row["handler_function"]) for row in building_spell_registrations}),
        "unit_spell_registrations": len(unit_spell_registrations),
        "unit_spell_handlers": len({str(row["handler_function"]) for row in unit_spell_registrations}),
        "unit_spell_mechanics": len(unit_spell_mechanics),
        "unit_spell_mechanic_kinds": dict(sorted(Counter(str(row["mechanic_kind"]) for row in unit_spell_mechanics).items())),
        "unit_spell_mechanics_with_delayed_callbacks": sum(bool(row["delayed_callback_functions"]) for row in unit_spell_mechanics),
        "unit_spell_mechanics_with_dynamic_callbacks": sum(bool(row["dynamic_callback_functions"]) for row in unit_spell_mechanics),
        "unit_spell_inlined_registrations": sum(
            str(row["evidence_kind"]) == "inlined-registration" for row in unit_spell_registrations
        ),
        "protected_filter_bindings": len(protected_filter_bindings),
        "protected_filter_bindings_resolved": sum(
            row["resolution_status"] == "resolved" for row in protected_filter_bindings
        ),
        "protected_perk_registry_audit_rows": len(protected_perk_registry_audit),
        "protected_perk_registry_registration_status_counts": dict(sorted(Counter(
            str(row["individual_factory_registration_status"]) for row in protected_perk_registry_audit
        ).items())),
        "perk_mechanics": len(perk_mechanics),
        "perk_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in perk_mechanics
        ).items())),
        "runtime_ai_mechanics": len(runtime_ai_mechanics),
        "runtime_ai_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in runtime_ai_mechanics
        ).items())),
        "runtime_session_mechanics": len(runtime_session_mechanics),
        "runtime_session_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in runtime_session_mechanics
        ).items())),
        "runtime_mode_mechanics": len(runtime_mode_mechanics),
        "runtime_mode_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in runtime_mode_mechanics
        ).items())),
        "damage_listener_coverage_rows": len(damage_listener_coverage),
        "damage_listener_coverage_status_counts": dict(sorted(Counter(
            str(row["coverage_status"]) for row in damage_listener_coverage
        ).items())),
        "event_listener_coverage_rows": len(event_listener_coverage),
        "event_listener_coverage_status_counts": dict(sorted(Counter(
            str(row["coverage_status"]) for row in event_listener_coverage
        ).items())),
        "production_unit_special_mechanics": len(production_unit_special_mechanics),
        "production_unit_special_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in production_unit_special_mechanics
        ).items())),
        "building_improvement_spawn_mechanics": len(building_improvement_spawn_mechanics),
        "building_improvement_spawn_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in building_improvement_spawn_mechanics
        ).items())),
        "runtime_system_mechanics": len(runtime_system_mechanics),
        "runtime_system_mechanic_kinds": dict(sorted(Counter(
            str(row["mechanic_kind"]) for row in runtime_system_mechanics
        ).items())),
        "building_spell_mechanics": len(building_spell_mechanics),
        "building_spell_mechanics_with_unresolved_target_filter": sum(
            "unresolved" in str(row["evidence_kind"]) for row in building_spell_mechanics
        ),
        "corpse_building_mechanics": len(corpse_building_mechanics),
        "corpse_building_raise_mechanics": sum(
            str(row["mechanic_kind"]) == "scripted-raise-random" for row in corpse_building_mechanics
        ),
        "readable_function_names": len(readable),
        "direct_map_rawcode_references": len(rawcode_sites),
        "referenced_map_rawcodes": len(sites_by_rawcode),
        "known_map_rawcodes": len(object_metadata),
        "runtime_mutator_sites": len(runtime_mutators),
        "protected_ability_field_assignments": len(protected_ability_fields),
        "protected_ability_rows": len({
            (int(row["ability_id"]), int(row["level_index"])) for row in protected_ability_fields
        }),
        "protected_abilities": len({int(row["ability_id"]) for row in protected_ability_fields}),
        "protected_ability_jass_add_assignments": len(jass_add_protected_fields),
        "protected_ability_jass_add_only_assignments": sum(
            str(row["canonical_relation"]) == "jass-only" for row in jass_add_protected_fields
        ),
        "unit_object_metadata_rows": len(unit_object_metadata),
        "unit_object_metadata_production_rows": sum(int(row["unit_id"]) > 0 for row in unit_object_metadata),
        "unit_object_metadata_fingerprint": unit_object_metadata_fingerprint,
        "unit_object_upgrade_edges": len(unit_object_upgrades),
        "race_catalogs": len({int(row["race_index"]) for row in race_buildings}),
        "race_building_links": len(race_buildings),
        "campaign_only_race_catalogs": len({int(row["race_index"]) for row in race_buildings if bool(row["campaign_only"])}),
        "race_building_semantic_rows": len(race_building_semantics),
        "race_building_precursor_edges": len(semantic_precursor_pairs),
        "element_building_bucket_rows": len(element_building_buckets),
        "income_factor_constants": dict(sorted(income_factor_constants.items())),
        "effective_unit_stat_rows": len(effective_unit_stats),
        "effective_unit_stat_buildings": len({int(row["building_id"]) for row in effective_unit_stats}),
        "effective_unit_stat_units": len({int(row["unit_id"]) for row in effective_unit_stats}),
        "protected_unit_stat_rows": len(protected_unit_stats),
        "protected_unit_stat_units": len({int(row["unit_id"]) for row in protected_unit_stats}),
        "protected_unit_stat_override_assignments": sum(
            row[field] is not None
            for row in protected_unit_stats
            for field in (
                "hp", "armor", "defense_type", "move_speed",
                "attack1_base_damage", "attack1_dice_number", "attack1_dice_sides",
                "attack1_cooldown_microseconds", "attack1_range",
                "attack2_base_damage", "attack2_dice_number", "attack2_dice_sides",
                "attack2_cooldown_microseconds", "attack2_range",
            )
        ),
        "runtime_mutator_trace_rows": len(rawcode_mutator_traces),
        "rawcodes_with_runtime_mutator_paths": len(traces_by_rawcode),
        "runtime_mutator_sites_with_rawcode_paths": len({
            int(trace["mutator_byte_offset"]) for trace in rawcode_mutator_traces
        }),
        "max_runtime_mutator_path_hops": max(
            (int(trace["hop_count"]) for trace in rawcode_mutator_traces),
            default=0,
        ),
        "note": "The source is retained verbatim but is W3P-obfuscated; Lua-aware indexes are static, byte-accurate, skip strings/comments, and do not execute map code. The protected ability-field table is parsed structurally from its generated initializer and cross-checked against overlapping JASS-add restores. Function aliases/value arguments are indexed separately from direct calls. Rawcode-to-mutator traces prove only lexical direct-call reachability, not argument data flow, virtual dispatch, callback execution, or branch execution.",
    }


def copy_plaintext(raw: Path, output: Path) -> list[str]:
    copied = []
    for filename in ("war3mapMisc.txt", "war3mapSkin.txt", "war3map.wts"):
        source = raw / filename
        if source.exists():
            shutil.copyfile(source, output / filename)
            copied.append(filename)
    return copied


def write_archive_manifest(raw: Path, output: Path) -> list[dict[str, Any]]:
    rows = []
    for path in sorted(raw.iterdir()):
        if not path.is_file() or path.name.startswith("File") or path.name.startswith("obj-"):
            continue
        data = path.read_bytes()
        rows.append({
            "name": path.name,
            "bytes": len(data),
            "sha256": hashlib.sha256(data).hexdigest(),
        })
    (output / "archive-members.json").write_text(json.dumps(rows, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    return rows


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--map", type=Path, required=True)
    parser.add_argument("--raw", type=Path, required=True)
    parser.add_argument("--translated", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    args.output.mkdir(parents=True, exist_ok=True)

    wts_entries = parse_wts(args.raw / "war3map.wts")
    strings = {key: entry["value"] for key, entry in wts_entries.items()}
    (args.output / "strings.json").write_text(
        json.dumps(wts_entries, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )

    terrain_path = args.translated / "terrain.json"
    if terrain_path.exists():
        terrain = repair_translator_text(json.loads(terrain_path.read_text(encoding="utf-8")))
        (args.output / "terrain.json").write_text(json.dumps(terrain, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    doodads_path = args.translated / "doodads.json"
    if doodads_path.exists():
        doodads = repair_translator_text(json.loads(doodads_path.read_text(encoding="utf-8")))
        (args.output / "doodads.json").write_text(json.dumps(doodads, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    map_info = parse_w3i(args.raw / "war3map.w3i")
    (args.output / "map-info.json").write_text(json.dumps(map_info, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    pathing = parse_wpm(args.raw / "war3map.wpm", args.output)
    (args.output / "pathing.json").write_text(json.dumps(pathing, indent=2) + "\n", encoding="utf-8")

    shadow = parse_shadow(args.raw / "war3map.shd", pathing["width_cells"], pathing["height_cells"], args.output)
    (args.output / "shadow.json").write_text(json.dumps(shadow, indent=2) + "\n", encoding="utf-8")

    minimap = parse_mmp(args.raw / "war3map.mmp")
    (args.output / "minimap-icons.json").write_text(json.dumps(minimap, indent=2) + "\n", encoding="utf-8")

    object_summary = write_object_catalog(args.translated, args.output, strings)
    script_summary = write_script_index(args.raw / "war3map.lua", args.output)
    copied_text = copy_plaintext(args.raw, args.output)
    members = write_archive_manifest(args.raw, args.output)
    archive_header = map_archive_header(args.map)

    summary = {
        "source_map": str(args.map.resolve().relative_to(Path.cwd().resolve())) if args.map.resolve().is_relative_to(Path.cwd().resolve()) else str(args.map),
        "archive": archive_header,
        "canonical_members": len(members),
        "map_info": {
            "name": map_info.get("name"),
            "author": map_info.get("author"),
            "format_version": map_info.get("format_version"),
            "game_version": map_info.get("game_version"),
            "playable_size_tiles": map_info.get("playable_size_tiles"),
            "total_size_tiles": map_info.get("total_size_tiles"),
            "player_count": len(map_info.get("players", [])),
            "force_count": len(map_info.get("forces", [])),
        },
        "pathing": {
            "width_cells": pathing["width_cells"],
            "height_cells": pathing["height_cells"],
            "flag_counts": pathing["flag_counts"],
        },
        "objects": object_summary,
        "script": script_summary,
        "plaintext_members_copied": copied_text,
    }
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
