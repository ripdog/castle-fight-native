#!/usr/bin/env python3

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("release_manifest.py")
SPEC = importlib.util.spec_from_file_location("release_manifest", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
release_manifest = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = release_manifest
SPEC.loader.exec_module(release_manifest)


class ReleaseManifestTests(unittest.TestCase):
    def test_repository_manifest_registers_927_and_verified_932_archive(self) -> None:
        payload = release_manifest.load_manifest()
        release_927 = release_manifest.resolve_release(payload, "9.27", "r1")
        release_932 = release_manifest.resolve_release(payload, "9.32", "r1")

        self.assertEqual(release_927["extraction"]["status"], "retained")
        self.assertEqual(
            release_927["runtime_content"]["status"], "supported-development-subset"
        )
        self.assertEqual(release_932["availability"], "archived")
        self.assertEqual(release_932["extraction"]["status"], "pending")
        self.assertEqual(release_932["runtime_content"]["status"], "unsupported")
        self.assertEqual(release_manifest.verify_release(release_927), [])
        self.assertEqual(release_manifest.verify_release(release_932), [])

    def test_multiple_revisions_require_an_explicit_revision(self) -> None:
        payload = {
            "schema_version": 1,
            "releases": [
                self._release("9.27", "r1"),
                self._release("9.27", "r2"),
            ],
        }
        with tempfile.TemporaryDirectory() as temporary_directory:
            manifest = Path(temporary_directory) / "releases.json"
            manifest.write_text(json.dumps(payload), encoding="utf-8")
            loaded = release_manifest.load_manifest(manifest)
            with self.assertRaisesRegex(
                release_manifest.ManifestError, "multiple revisions"
            ):
                release_manifest.resolve_release(loaded, "9.27", None)

    def test_duplicate_release_revision_is_rejected(self) -> None:
        release = self._release("9.27", "r1")
        payload = {"schema_version": 1, "releases": [release, release]}
        with tempfile.TemporaryDirectory() as temporary_directory:
            manifest = Path(temporary_directory) / "releases.json"
            manifest.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(release_manifest.ManifestError, "duplicate"):
                release_manifest.load_manifest(manifest)

    def test_pending_extraction_cannot_claim_retained_tree(self) -> None:
        release = self._release("9.32", "r1")
        release["extraction"]["git_tree"] = "1" * 40
        payload = {"schema_version": 1, "releases": [release]}
        with tempfile.TemporaryDirectory() as temporary_directory:
            manifest = Path(temporary_directory) / "releases.json"
            manifest.write_text(json.dumps(payload), encoding="utf-8")
            with self.assertRaisesRegex(release_manifest.ManifestError, "cannot claim"):
                release_manifest.load_manifest(manifest)

    @staticmethod
    def _release(version: str, revision: str) -> dict:
        return {
            "map_version": version,
            "revision": revision,
            "declared_release": f"Castle Fight DE Beta {version}",
            "author": "Frotty",
            "availability": "archived",
            "source_archive": {
                "path": f"fixtures/{version}/{revision}/source.w3x",
                "sha256": "0" * 64,
                "bytes": 1,
            },
            "warcraft_base_data": {
                "map_declared_version": "2.0.4.23745",
                "resolved_version": None,
                "manifest_path": None,
                "manifest_sha256": None,
            },
            "extraction": {
                "status": "pending",
                "schema_version": 1,
                "extractor_revision": None,
                "git_tree": None,
                "working_alias": f"fixtures/{version}/{revision}/extracted",
                "summary_path": None,
            },
            "runtime_content": {
                "status": "unsupported",
                "content_revision": None,
                "source_revision": None,
                "path": None,
                "native_effect_tuning_sha256": None,
                "binding_registry_path": None,
                "binding_registry_sha256": None,
            },
        }


if __name__ == "__main__":
    unittest.main()
