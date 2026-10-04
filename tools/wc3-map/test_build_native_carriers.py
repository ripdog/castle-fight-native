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


if __name__ == '__main__':
    unittest.main()
