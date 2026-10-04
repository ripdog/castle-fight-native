import importlib.util
import json
import unittest
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "building_mechanics", Path(__file__).with_name("build_building_mechanics.py")
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BuildingMechanicsProjectionTests(unittest.TestCase):
    def test_complete_artifact_is_reproducible_from_the_registered_tree(self):
        expected = json.loads(MODULE.OUT.read_text())
        with mock.patch.object(
            MODULE, "_retained_file_bytes", wraps=MODULE._retained_file_bytes
        ) as read_source:
            self.assertEqual(MODULE.build(), expected)
        self.assertTrue(read_source.call_args_list)
        self.assertTrue(all(
            call.args[1] == expected["extraction_git_tree"]
            for call in read_source.call_args_list
        ))

    def test_unsupported_version_does_not_reuse_an_older_recipe(self):
        with self.assertRaisesRegex(ValueError, "do not support"):
            MODULE.build("9.32")

    def test_missing_retained_tree_is_rejected_before_any_projection(self):
        with mock.patch.object(
            MODULE, "_load_release", return_value={"extraction": {"git_tree": None}}
        ), mock.patch.object(MODULE, "_retained_file_bytes") as read_source:
            with self.assertRaisesRegex(ValueError, "retained extraction"):
                MODULE.build()
            read_source.assert_not_called()


if __name__ == "__main__":
    unittest.main()
