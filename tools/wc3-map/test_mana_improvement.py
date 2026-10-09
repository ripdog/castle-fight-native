import unittest
from build_native_tuning import project_effect


class ManaImprovementProjectionTests(unittest.TestCase):
    def test_native_regeneration_keeps_both_levels_and_rejects_percentage_or_unit_targets(self):
        recipe = {"kind": "building-mana-regeneration", "source_key": "REGE"}
        fields = {"targs1": "structure,self", "datab1": "0"}
        detail = {"native_base": "ANre", "improvement_levels": {"dataa1": "0.17", "dataa2": "0.42"}}
        self.assertEqual(project_effect(recipe, fields, None, {}, detail)["levels"], [1700, 4200])
        for changed in ({**fields, "datab1": "1"}, {**fields, "targs1": "ground,self"}):
            with self.assertRaises(ValueError):
                project_effect(recipe, changed, None, {}, detail)
