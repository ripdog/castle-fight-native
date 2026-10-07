from __future__ import annotations

import json
import unittest
from pathlib import Path

import build_native_tuning as native
import build_runtime_catalog as catalog


class NativeTuningProjectionTest(unittest.TestCase):
    def test_committed_tuning_is_a_projection_of_retained_evidence(self) -> None:
        root = Path(__file__).resolve().parents[2]
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        directory = root / release["runtime_content"]["path"]
        recipes = json.loads((directory / "native-effect-recipes.json").read_text())
        generated = native.build_tuning(release, root, recipes)
        self.assertEqual(json.loads((directory / "native-effect-tuning.json").read_text()), generated)

    def test_critical_strike_retains_structure_permission_without_flattening_classes(self) -> None:
        recipe = {"kind": "critical-strike", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "17", "datab1": "1.7", "targs1": "air,ground,enemies,structure"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual(effect["targets"], "air-ground-units-and-buildings")
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,enemies,structure,nonhero"}, None, {}, {})

    def test_cleave_projects_geometry_and_rejects_unrepresented_classes(self) -> None:
        recipe = {"kind": "cleave", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"area1": "123", "dataa1": "0.37", "targs1": "ground,enemy,structure"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["radius_world"], effect["damage_per_10k"], effect["targets"]), (123, 3700, 5))
        for mask in ("ground,air,enemy", "ground,enemy,organic", "ground,enemy,nonhero"):
            with self.subTest(mask=mask), self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, "targs1": mask}, None, {}, {})

    def test_frost_attack_retains_hero_duration_and_uses_supplied_map_misc(self) -> None:
        recipe = {"kind": "frost-attack", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dur1": "1.25", "herodur1": "0.1", "targs1": "air,ground"}
        misc = {"FrostMoveSpeedDecrease": "0.17", "FrostAttackSpeedDecrease": "0.23"}
        effect = native.project_effect(recipe, fields, None, {}, {"misc": misc})
        self.assertEqual((effect["duration_millis"], effect["hero_duration_millis"], effect["movement_percent_delta"], effect["attack_speed_percent_delta"]), (1250, 100, -17, -23))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,nonhero"}, None, {}, {"misc": misc})

    def test_pulverize_projects_distinct_full_and_half_radii_without_class_flattening(self) -> None:
        recipe = {"kind": "pulverize", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "17", "datab1": "31", "datac1": "41", "datad1": "73", "targs1": "ground,enemy"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["chance_per_10k"], effect["damage"], effect["full_radius_world"], effect["half_radius_world"]), (1700, 31, 41, 73))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,enemy,nonhero"}, None, {}, {})

    def test_fixed_point_conversion_is_exact_not_float_rounded(self) -> None:
        self.assertEqual(native.scaled("0.1234", 10_000), 1234)
        self.assertEqual(native.scaled("0.123", 1000), 123)
        for value in ("0.12345", "NaN", "Infinity"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                native.scaled(value, 10_000)

    def test_native_proc_projection_does_not_silently_drop_unsupported_targets(self) -> None:
        with self.assertRaises(ValueError):
            native.unit_targets("air,ground,enemy,structure")
        with self.assertRaises(ValueError):
            native.unit_targets("enemy,ward")

    def test_bash_keeps_hero_duration_and_rejects_unimplemented_native_fields(self) -> None:
        recipe = {"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "33", "datab1": "0", "datac1": "7", "datad1": "0", "datae1": "0", "dur1": "0.5", "herodur1": "0.25", "targs1": "air,ground"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["stun_duration_millis"], effect["hero_stun_duration_millis"]), (500, 250))
        for field in ("datab1", "datad1", "datae1"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, field: "1"}, None, {}, {})
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "air,ground,nonhero"}, None, {}, {})

    def test_class_specific_orb_chances_require_explicit_native_coverage(self) -> None:
        recipe = {"kind": "orb-spell-proc", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"datab1": "10", "datac1": "20", "datad1": "10", "unitid1": "CHLD", "targs1": "ground"}
        with self.assertRaises(ValueError):
            native.project_effect(recipe, fields, None, {}, {})

    def test_feedback_projects_native_fields_and_rejects_unequal_class_tuning(self) -> None:
        recipe = {"kind": "feedback", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "7", "datab1": "0.5", "datac1": "7", "datad1": "0.5", "datae1": "3", "targs1": "air,ground,enemy"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["maximum_mana_drained"], effect["damage_per_mana_per_10k"], effect["summoned_damage"]), (7, 5000, 3))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "dataa1": "8"}, None, {}, {})

    def test_faerie_fire_uses_protected_costs_and_keeps_distinct_hero_duration(self) -> None:
        recipe = {"kind": "faerie-fire", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "2", "datab1": "1", "cost1": "999", "cool1": "99", "rng1": "123", "dur1": "13", "herodur1": "2.5", "targs1": "air,ground,enemy"}
        unit = {"mana_max": "50", "mana_start": "30", "mana_regen": "1.2"}
        effect = native.project_effect(recipe, fields, unit, {"mana_cost": "4", "cooldown": "3"}, {})
        self.assertEqual((effect["mana_cost"], effect["cooldown_millis"]), (4, 3000))
        self.assertEqual((effect["duration_millis"], effect["hero_duration_millis"]), (13000, 2500))
        self.assertEqual(effect["mana_regen_per_second_per_10k"], 12000)
        self.assertTrue(effect["always_autocast"])
        combat_only = native.project_effect(recipe, {**fields, "datab1": "0"}, unit, {}, {})
        self.assertFalse(combat_only["always_autocast"])
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "datab1": "2"}, unit, {}, {})

    def test_automatic_projection_keeps_exact_rate_and_native_delivery_fields(self) -> None:
        recipe = {"kind": "phoenix-fire", "source_key": "FIRE", "source_kind": "unit-ability", "unit_rawcode": "UNIT"}
        fields = {"dataa1": "7", "datab1": "3", "dur1": "4", "herodur1": "2",
                  "missilespeed": "700", "targs1": "ground,air,structure,enemies", "area1": "123", "cool1": "3", "cost1": "0"}
        for rate in ("1", "3.5", "0.0001"):
            unit = {"mana_max": "100", "mana_start": "17", "mana_regen": rate}
            projected = native.project_effect(recipe, fields, unit, {}, {})
            profile = projected["spellcasting"]
            self.assertEqual(profile["mana"]["regen_per_tick_per_10k"], (1 << 31) | native.scaled(rate, 10_000))
            self.assertEqual(profile["ability"]["effect"]["PhoenixFire"]["targets"], 7)
            self.assertEqual(profile["ability"]["effect"]["PhoenixFire"]["damage_per_second"], 3)
            self.assertEqual(profile["ability"]["range"], 123 * 1024)
        for rate in ("-1", "1000.0001", "214748.3648"):
            with self.subTest(rate=rate), self.assertRaises(ValueError):
                native.project_effect(recipe, fields, {"mana_max": "100", "mana_start": "17", "mana_regen": rate}, {}, {})

    def test_non_free_proxy_child_cannot_silently_bypass_native_resources(self) -> None:
        recipe = {"kind": "healing-wave", "source_key": "HEAL", "source_kind": "ability-effect", "unit_rawcode": "UNIT"}
        fields = {"dataa1": "7", "datab1": "2", "datac1": "0.25", "area1": "123", "cost1": "9999", "cool1": "99"}
        unit = {"mana_max": "100", "mana_start": "17", "mana_regen": "1"}
        mechanics = {"mechanics_row": {"scheduled_delays_json": '["0.8"]'}}
        for protected in ({}, {"mana_cost": "1", "cooldown": "0"}, {"mana_cost": "0", "cooldown": "1"}):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, fields, unit, protected, mechanics)
        projected = native.project_effect(recipe, fields, unit, {"mana_cost": "0", "cooldown": "0"}, mechanics)
        self.assertEqual(projected["spellcasting"]["ability"]["mana_cost"], 0)
        self.assertEqual(projected["spellcasting"]["ability"]["effect"]["HealingWave"]["recovery_ticks"], 24)

    def test_recipes_cannot_become_a_second_tuning_database(self) -> None:
        root = catalog.REPO_ROOT
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        recipes = {"schema_version": 1, "effects": [{"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability", "bonus_damage": 123}]}
        with self.assertRaises(ValueError):
            native.build_tuning(release, root, recipes)


if __name__ == "__main__":
    unittest.main()
