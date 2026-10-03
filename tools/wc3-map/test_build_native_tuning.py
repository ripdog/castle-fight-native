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

    def test_recipes_cannot_become_a_second_tuning_database(self) -> None:
        root = catalog.REPO_ROOT
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        recipes = {"schema_version": 1, "effects": [{"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability", "bonus_damage": 123}]}
        with self.assertRaises(ValueError):
            native.build_tuning(release, root, recipes)


if __name__ == "__main__":
    unittest.main()
