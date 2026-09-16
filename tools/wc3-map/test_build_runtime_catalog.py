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

        self.assertEqual(committed["schema_version"], generated["schema_version"])
        self.assertEqual(committed["map_version"], generated["map_version"])
        self.assertEqual(committed["release_revision"], generated["release_revision"])
        self.assertEqual(committed["extraction_git_tree"], generated["extraction_git_tree"])
        self.assertEqual(
            committed["source_object_fields_sha256"],
            generated["source_object_fields_sha256"],
        )

        generated_by_rawcode = {row["rawcode"]: row for row in generated["objects"]}
        self.assertGreater(len(generated_by_rawcode), 500)
        for row in committed["objects"]:
            self.assertEqual(row, generated_by_rawcode[row["rawcode"]])

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
