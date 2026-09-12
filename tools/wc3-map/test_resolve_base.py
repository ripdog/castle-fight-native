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
    def test_selected_profile_variant_wins(self) -> None:
        profile = {
            "hbar": {
                "Ubertip": "generic",
                "Ubertip:custom,V0": "custom zero",
                "Ubertip:custom,V1": "custom one",
                "Buttonpos": "1,2",
            }
        }
        self.assertEqual(resolve.profile_value(profile, "hbar", "Ubertip", "custom,V1"), "custom one")
        self.assertEqual(resolve.profile_value(profile, "hbar", "Ubertip", "custom,V0"), "custom zero")
        self.assertEqual(resolve.selected_index(resolve.profile_value(profile, "hbar", "Buttonpos", "custom,V1"), 1), "2")

    def test_w3i_default_tft_selects_custom_v1(self) -> None:
        selection = resolve.select_game_data({
            "game_data_set_version": 0,
            "game_data_version": {"raw": 1},
        })
        self.assertEqual(selection.overlay_dir, "custom_v1")
        self.assertEqual(selection.profile_variant, "custom,V1")
        self.assertEqual(selection.label, "Default (TFT)")

    def test_w3i_explicit_data_sets_map_to_balance_overlays(self) -> None:
        custom = resolve.select_game_data({"game_data_set_version": 1, "game_data_version": {"raw": 1}})
        melee = resolve.select_game_data({"game_data_set_version": 2, "game_data_version": {"raw": 1}})
        self.assertEqual((custom.overlay_dir, custom.profile_variant), ("custom_v0", "custom,V0"))
        self.assertEqual((melee.overlay_dir, melee.profile_variant), ("melee_v0", "melee,V0"))

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

    def test_protected_unit_stat_overlay_recovers_attack_primitives(self) -> None:
        with (self.resolved / "protected-unit-stats.tsv").open(encoding="utf-8") as handle:
            units = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(units), 549)
        self.assertEqual(units["e000"]["static_hp"], "1")
        self.assertEqual(units["e000"]["override_hp"], "460")
        self.assertEqual(units["e000"]["unitstat_hp"], "460")
        self.assertEqual(units["e000"]["unitstat_attack1_base_damage"], "49")
        self.assertEqual(units["e000"]["unitstat_attack1_dice_number"], "1")
        self.assertEqual(units["e000"]["unitstat_attack1_dice_sides"], "1")
        self.assertEqual(units["e000"]["unitstat_attack1_cooldown"], "1.75")
        self.assertEqual(units["e000"]["unitstat_attack1_range"], "350")
        self.assertEqual(units["h00W"]["unitstat_attack1_cooldown"], "1.8")
        self.assertEqual(units["h00W"]["unitstat_attack1_range"], "650")
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["protected_unit_stat_rows"], 549)
        self.assertEqual(summary["protected_unit_stat_override_assignments"], 1887)
        self.assertEqual(summary["selected_game_data_set"]["label"], "Default (TFT)")
        self.assertEqual(summary["selected_game_data_set"]["profile_variant"], "custom,V1")
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["armor"], {"unitstat-match": 162})
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["hp"], {"unitstat-match": 162})
        self.assertEqual(
            summary["effective_unit_stat_vs_unitstat_comparisons"]["dps"],
            {"unitstat-match": 160, "unitstat-missing": 2},
        )
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["move_speed"], {"unitstat-match": 162})

    def test_resolution_has_no_inheritance_or_pathing_gaps(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["unresolved_base_objects"], [])
        self.assertEqual(summary["missing_pathing_textures"], [])
        self.assertEqual(summary["unresolved_placed_object_types"], [])
        self.assertEqual(summary["missing_placed_pathing_textures"], [])

    def test_protected_ability_runtime_table_overrides_static_sentinels(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["protected_ability_runtime_fields"], 452)
        self.assertEqual(summary["protected_ability_runtime_field_comparisons"], {"static-differs": 452})

        with (self.resolved / "protected-ability-fields.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        indexed = {(row["rawcode"], row["level"], row["field"]): row for row in rows}

        rescue = indexed[("A005", "1", "cooldown")]
        self.assertEqual(rescue["runtime_value"], "60.0")
        self.assertEqual(rescue["static_resolved_value"], "99")
        self.assertEqual(rescue["comparison"], "static-differs")

        snowfall = indexed[("A0HO", "1", "mana_cost")]
        self.assertEqual(snowfall["runtime_value"], "15")
        self.assertEqual(snowfall["static_resolved_value"], "9999")

    def test_jass_add_restore_cross_check_keeps_static_match(self) -> None:
        with (self.resolved / "protected-ability-jass-add-restores.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        jass_only = [row for row in rows if row["canonical_relation"] == "jass-only"]
        self.assertEqual(len(jass_only), 1)
        self.assertEqual(jass_only[0]["rawcode"], "A010")
        self.assertEqual(jass_only[0]["runtime_value"], "3.")
        self.assertEqual(jass_only[0]["static_resolved_value"], "3")
        self.assertEqual(jass_only[0]["comparison"], "static-match")

    def test_effective_unit_catalog_exposes_poisoned_static_stats(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["effective_unit_stat_rows"], 162)
        self.assertEqual(summary["effective_unit_stat_comparisons"]["hp"], {
            "static-differs": 152,
            "static-match": 10,
        })
        self.assertEqual(summary["effective_unit_stat_comparisons"]["move_speed"], {
            "static-match": 162,
        })

        with (self.resolved / "effective-unit-stats.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        faerie = rows["e000"]
        self.assertEqual(faerie["building_rawcode"], "h00B")
        self.assertEqual(faerie["effective_hp"], "460")
        self.assertEqual(faerie["static_hp"], "1")
        self.assertEqual(faerie["effective_attack_range"], "350")
        self.assertEqual(faerie["static_attack1_range"], "1")
        self.assertEqual(faerie["hp_comparison"], "static-differs")

        footman = rows["hfoo"]
        self.assertEqual(footman["effective_hp"], "250")
        self.assertEqual(footman["static_hp"], "250")
        self.assertEqual(footman["dps_comparison"], "static-match")

        mortar = rows["hmtm"]
        self.assertEqual(mortar["effective_move_speed"], "270")
        self.assertEqual(mortar["static_move_speed"], "270")
        self.assertEqual(mortar["move_speed_comparison"], "static-match")


if __name__ == "__main__":
    unittest.main()
