from __future__ import annotations

import importlib.util
import json
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
MODULE_PATH = Path(__file__).with_name("build_runtime_catalog.py")
SPEC = importlib.util.spec_from_file_location("build_runtime_catalog", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class RuntimeCatalogTest(unittest.TestCase):
    def test_action_timing_rounds_total_duration_once_and_preserves_zero(self) -> None:
        self.assertEqual(MODULE._action_duration_ticks("0.014", "0.014", 30), 1)
        self.assertEqual(MODULE._action_duration_ticks("0.016", "0.018", 30), 2)
        self.assertEqual(MODULE._action_duration_ticks("-", "_", 30), 0)
        for value in ("-0.1", "NaN", "Infinity", "4000"):
            with self.assertRaises(SystemExit):
                MODULE._action_duration_ticks(value, "0", 30)

    def test_attack_damage_point_is_projected_independently_without_rounding_backswing_twice(self) -> None:
        self.assertEqual(MODULE._action_duration_ticks("0.014", "0", 30), 1)
        self.assertEqual(MODULE._action_duration_ticks("0.034", "0", 30), 2)
        self.assertEqual(MODULE._action_duration_ticks("0", "0", 30), 0)
        self.assertEqual(MODULE._action_duration_ticks("-", "0", 30), 0)

    def test_line_projection_rejects_lossy_world_distances(self) -> None:
        for value in ("0", "37", "100.0"):
            self.assertEqual(MODULE._line_world_integer(value), int(float(value)))
        for value in ("1.25", "-1", "NaN", "Infinity"):
            with self.assertRaises(SystemExit):
                MODULE._line_world_integer(value)

    def test_line_projection_retains_damage_loss_without_binary_float_rounding(self) -> None:
        for loss, retained in (("0", 10000), ("0.125", 8750), ("0.9999", 1), ("1", 0)):
            self.assertEqual(MODULE._line_damage_retention(loss), retained)
        for loss in ("0.00001", "-0.1", "1.1", "NaN", "Infinity"):
            with self.assertRaises(SystemExit):
                MODULE._line_damage_retention(loss)

    def test_catalog_text_hashing_normalizes_crlf(self) -> None:
        self.assertEqual(
            MODULE._canonical_text_bytes(b"a\r\nb\r\n"),
            b"a\nb\n",
        )
        self.assertEqual(MODULE._canonical_text_bytes(b"a\nb\n"), b"a\nb\n")

    def test_runtime_evidence_projection_ignores_unconsumed_report_columns(self) -> None:
        retained_income = (
            b"building_rawcode\tincome_factor\tprecursor_rawcode\tis_siege\n"
            b"h000\t0.2\t\t0\n"
        )
        expanded_income = (
            b"building_rawcode\tincome_factor\tprecursor_rawcode\ttier_symbol\tis_siege\n"
            b"h000\t0.2\t\tQ\t0\n"
        )
        self.assertEqual(
            MODULE._runtime_evidence_bytes(
                "script/race-building-semantics.tsv", retained_income
            ),
            MODULE._runtime_evidence_bytes(
                "script/race-building-semantics.tsv", expanded_income
            ),
        )

        retained_production = (
            b"building_rawcode\tbuilding_kind\tunit_rawcode\tspawn_time\n"
            b"h000\tproduction\thfoo\t20\n"
            b"h006\tnon-production\t\t0\n"
        )
        expanded_production = (
            b"building_rawcode\tbuilding_kind\tunit_rawcode\tspawn_time\ttier_symbol\n"
            b"h006\tnon-production\t\t0\tY\n"
            b"h000\tproduction\thfoo\t20\tQ\n"
        )
        self.assertEqual(
            MODULE._runtime_evidence_bytes(
                "resolved/production-buildings.tsv", retained_production
            ),
            MODULE._runtime_evidence_bytes(
                "resolved/production-buildings.tsv", expanded_production
            ),
        )

    def test_committed_927_supplement_is_generated_from_retained_object_fields(self) -> None:
        release = MODULE._load_release(
            REPO_ROOT / "docs/original_map/releases.json", "9.27", "r1"
        )
        generated = MODULE.build_supplement(release, REPO_ROOT)
        committed = json.loads(
            (
                REPO_ROOT
                / "crates/sim/data/castle-fight/9.27/catalog-supplement-r1.json"
            ).read_text(encoding="utf-8")
        )

        self.assertEqual(committed, generated)

    def test_committed_927_source_manifest_is_generated_from_retained_tree(self) -> None:
        release = MODULE._load_release(
            REPO_ROOT / "docs/original_map/releases.json", "9.27", "r1"
        )
        committed = json.loads(
            (
                REPO_ROOT / "crates/sim/data/castle-fight/9.27/catalog-source-r1.json"
            ).read_text(encoding="utf-8")
        )
        generated = MODULE.build_source_manifest(
            release, REPO_ROOT, committed["content_revision"]
        )
        self.assertEqual(committed, generated)

    def test_working_927_alias_matches_retained_runtime_evidence(self) -> None:
        release = MODULE._load_release(
            REPO_ROOT / "docs/original_map/releases.json", "9.27", "r1"
        )
        committed = json.loads(
            (
                REPO_ROOT / "crates/sim/data/castle-fight/9.27/catalog-source-r1.json"
            ).read_text(encoding="utf-8")
        )
        self.assertEqual(
            committed["source_evidence_fnv64"],
            MODULE.working_alias_source_evidence_fnv64(release, REPO_ROOT),
        )


if __name__ == "__main__":
    unittest.main()
