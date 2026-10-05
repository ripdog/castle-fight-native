import importlib.util
import json
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location('native_carriers', Path(__file__).with_name('build_native_carriers.py'))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class NativeCarrierProjectionTests(unittest.TestCase):
    def test_projection_is_reproducible_including_source_identity_and_all_removal_ids(self):
        self.assertEqual(json.loads(MODULE.OUTPUT.read_text()), MODULE.build())

    def test_hostile_mask_support_is_explicit_and_unknown_qualifiers_are_not_flattened(self):
        for mask in ("ground,enemy", "air,enemies,ground", "structure, ground, enemy"):
            with self.subTest(mask=mask):
                MODULE.validate_hostile_target_mask(mask)
        for mask in ("ground", "enemy", "", "ground,friend", "ground,self,enemy",
                     "ground,organic,enemy", "ground,vulnerable,enemy", "ground,hero,enemy",
                     "ground,ground,enemy", "ground,enemy,enemies", "ground,enemy,", None):
            with self.subTest(mask=mask), self.assertRaises(ValueError):
                MODULE.validate_hostile_target_mask(mask)


if __name__ == '__main__':
    unittest.main()
