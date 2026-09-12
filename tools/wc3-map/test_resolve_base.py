#!/usr/bin/env python3
from __future__ import annotations

import csv
import importlib.util
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("resolve-base-data.py")
SPEC = importlib.util.spec_from_file_location("resolve_base_data", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
resolve = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = resolve
SPEC.loader.exec_module(resolve)


class SylkTests(unittest.TestCase):
    def test_split_record_keeps_quoted_semicolon(self) -> None:
        self.assertEqual(
            resolve.split_semicolon_record('C;X2;Y3;K"alpha;beta"'),
            ["C", "X2", "Y3", 'K"alpha;beta"'],
        )

    def test_parse_slk_tracks_sparse_coordinates(self) -> None:
        text = "\r\n".join([
            "ID;PWXL;N;E",
            "B;X3;Y3;D0",
            'C;X1;Y1;K"id"',
            'C;X2;K"hp"',
            'C;X3;K"name"',
            'C;X1;Y2;K"hfoo"',
            'C;X2;K250',
            'C;X3;K"Foot; Man"',
            'C;X1;Y3;K"hrif"',
            'C;X2;K270',
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tiny.slk"
            path.write_text(text, encoding="utf-8")
            rows = resolve.parse_slk(path)
        self.assertEqual(rows["hfoo"]["hp"], 250)
        self.assertEqual(rows["hfoo"]["name"], "Foot; Man")
        self.assertEqual(rows["hrif"]["hp"], 270)
        self.assertNotIn("name", rows["hrif"])


class ProfileTests(unittest.TestCase):
    def test_custom_v0_variant_wins(self) -> None:
        profile = {
            "hbar": {
                "Ubertip": "generic",
                "Ubertip:custom,V0": "custom zero",
                "Buttonpos": "1,2",
            }
        }
        self.assertEqual(resolve.profile_value(profile, "hbar", "Ubertip"), "custom zero")
        self.assertEqual(resolve.selected_index(resolve.profile_value(profile, "hbar", "Buttonpos"), 1), "2")

    def test_single_quoted_csv_value_is_unquoted(self) -> None:
        self.assertEqual(resolve.selected_index('"Shop Sharing, Allied Bldg."', 0), "Shop Sharing, Allied Bldg.")


class ProtectionRecoveryTests(unittest.TestCase):
    def candidate(self, index: int, value):
        return resolve.MapCandidate(index=index, value=value, value_type="int")

    def test_numeric_zero_one_sentinels_recover_first_value(self) -> None:
        value, reason = resolve.choose_recovered([
            self.candidate(0, 280),
            self.candidate(1, 1),
            self.candidate(2, 1),
        ])
        self.assertEqual(value, 280)
        self.assertEqual(reason, "w3p-recovered-first-numeric-sentinel")

    def test_conflicting_strings_keep_last_write(self) -> None:
        value, reason = resolve.choose_recovered([
            resolve.MapCandidate(0, "A0HO", "string"),
            resolve.MapCandidate(1, "A0HO,AM0{", "string"),
        ])
        self.assertEqual(value, "A0HO,AM0{")
        self.assertEqual(reason, "ambiguous-string-last-write")


class PathingTextureTests(unittest.TestCase):
    def test_tga_channels_map_to_pathing_bits(self) -> None:
        # 2x2, bottom-origin 24-bit TGA. File pixels are BGR.
        header = bytearray(18)
        header[2] = 2
        struct.pack_into("<HH", header, 12, 2, 2)
        header[16] = 24
        # Source bottom row then top row: blue, green / red, blue+red.
        pixels = bytes([
            255, 0, 0,
            0, 255, 0,
            0, 0, 255,
            255, 0, 255,
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "mask.tga"
            path.write_bytes(bytes(header) + pixels)
            decoded = resolve.parse_tga_pathing(path)
        self.assertEqual(decoded["hex_rows"], ["15", "42"])
        self.assertEqual(decoded["width_world_units"], 64)
        self.assertEqual(decoded["counts"], {"unwalkable": 2, "unflyable": 1, "unbuildable": 2})


class ResolvedEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[2]
        cls.resolved = cls.root / "docs" / "original_map" / "extracted" / "resolved"

    def test_known_combat_values_use_recovered_protection_fields(self) -> None:
        with (self.resolved / "units.tsv").open(encoding="utf-8") as handle:
            units = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(units["hfoo"]["hp"], "250")
        self.assertEqual(units["hfoo"]["attack1_range"], "90")
        self.assertEqual(units["hmtm"]["hp"], "280")
        self.assertEqual(units["hmtm"]["attack1_range"], "1000")
        self.assertEqual(units["hrif"]["hp"], "270")
        self.assertEqual(units["hrif"]["attack1_range"], "500")

    def test_building_footprint_is_actual_tga_mask(self) -> None:
        with (self.resolved / "buildings.tsv").open(encoding="utf-8") as handle:
            buildings = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        barracks = buildings["h000"]
        self.assertEqual(barracks["footprint_width_cells"], "4")
        self.assertEqual(barracks["footprint_height_cells"], "4")
        self.assertEqual(barracks["footprint_width_world_units"], "128")
        self.assertEqual(barracks["footprint_hex_rows"], "5555/5555/5555/5555")

    def test_resolution_has_no_inheritance_or_pathing_gaps(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["unresolved_base_objects"], [])
        self.assertEqual(summary["missing_pathing_textures"], [])
        self.assertEqual(summary["unresolved_placed_object_types"], [])
        self.assertEqual(summary["missing_placed_pathing_textures"], [])


if __name__ == "__main__":
    unittest.main()
