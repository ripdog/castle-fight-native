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
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "datab1": "0"}, unit, {}, {})

    def test_recipes_cannot_become_a_second_tuning_database(self) -> None:
        root = catalog.REPO_ROOT
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        recipes = {"schema_version": 1, "effects": [{"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability", "bonus_damage": 123}]}
        with self.assertRaises(ValueError):
            native.build_tuning(release, root, recipes)


if __name__ == "__main__":
    unittest.main()
