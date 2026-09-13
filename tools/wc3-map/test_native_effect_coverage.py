#!/usr/bin/env python3

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("native_effect_coverage.py")
SPEC = importlib.util.spec_from_file_location("native_effect_coverage", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
coverage = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = coverage
SPEC.loader.exec_module(coverage)


class NativeEffectCoverageTests(unittest.TestCase):
    def test_versioned_binding_only_covers_its_declared_range(self) -> None:
        binding = coverage.Binding(
            source_kind="unit-ability",
            source_key="A05K",
            implementation="warcraft-bash-v1",
            valid_from=coverage.MapVersion.parse("9.27"),
            valid_through=coverage.MapVersion.parse("9.31"),
        )
        inventory = [coverage.InventoryItem("unit-ability", "A05K", "Bash", 1)]

        inside = coverage.build_coverage(
            inventory, [binding], coverage.MapVersion.parse("9.29")
        )
        outside = coverage.build_coverage(
            inventory, [binding], coverage.MapVersion.parse("9.32")
        )

        self.assertEqual(inside[0].status, "implemented")
        self.assertEqual(outside[0].status, "unimplemented")

    def test_overlapping_behavior_ranges_are_rejected(self) -> None:
        payload = {
            "schema_version": 1,
            "bindings": [
                {
                    "source_kind": "unit-ability",
                    "source_key": "A05K",
                    "implementation": "warcraft-bash-v1",
                    "valid_from": "9.27",
                    "valid_through": "9.31",
                },
                {
                    "source_kind": "unit-ability",
                    "source_key": "A05K",
                    "implementation": "warcraft-bash-v2",
                    "valid_from": "9.31",
                    "valid_through": "9.32",
                },
            ],
        }
        with tempfile.TemporaryDirectory() as temporary_directory:
            path = Path(temporary_directory) / "bindings.json"
            path.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "overlapping native-effect ranges"):
                coverage.load_bindings(path)

    def test_repository_inventory_marks_gryphon_bash_implemented_for_927(self) -> None:
        repo_root = Path(__file__).resolve().parents[2]
        inventory = coverage.load_inventory(
            repo_root / "docs/original_map/extracted/resolved"
        )
        bindings = coverage.load_bindings(
            repo_root / "crates/sim/data/castle-fight/native-effect-bindings.json"
        )
        rows = coverage.build_coverage(
            inventory, bindings, coverage.MapVersion.parse("9.27")
        )
        bash = next(
            row
            for row in rows
            if row.item.source_kind == "unit-ability" and row.item.source_key == "A05K"
        )
        self.assertEqual(bash.status, "implemented")
        self.assertEqual(bash.implementation, "warcraft-bash-v1")
        self.assertGreater(len(rows), 300)


if __name__ == "__main__":
    unittest.main()
