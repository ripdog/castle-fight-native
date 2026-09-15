#!/usr/bin/env python3
"""Generate compact, version-pinned runtime catalog supplements from retained extraction evidence."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import subprocess
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_RELEASES = REPO_ROOT / "docs/original_map/releases.json"
RUNTIME_EXTRACTION_FILES = (
    "resolved/buildings.tsv",
    "script/building-upgrades.tsv",
    "script/race-building-semantics.tsv",
    "script/race-buildings.tsv",
    "war3mapMisc.txt",
    "resolved/units.tsv",
    "resolved/protected-unit-stats.tsv",
    "resolved/production-unit-attacks.tsv",
    "resolved/production-buildings.tsv",
    "resolved/production-unit-corpses.tsv",
)
FNV64_OFFSET = 0xCBF29CE484222325
FNV64_PRIME = 0x100000001B3


def _load_release(manifest_path: Path, map_version: str, revision: str) -> dict[str, Any]:
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    for release in manifest.get("releases", []):
        if release.get("map_version") == map_version and release.get("revision") == revision:
            return release
    raise SystemExit(f"release {map_version}/{revision} is not registered in {manifest_path}")


def _integer_recovered_value(value: str, *, rawcode: str, field_id: str) -> int:
    # object-fields.tsv is tab-delimited but preserves CSV-style quote escaping around
    # some string-backed integer fields. Stripping the quote wrapper handles both the
    # scalar JSON integers used by urtm and values such as \"\"\"0\"\"\" used by ubpx/ubpy.
    normalized = value.strip('"')
    try:
        return int(normalized)
    except ValueError as error:
        raise SystemExit(
            f"{rawcode} {field_id} has non-integral recovered value {value!r}"
        ) from error


def _retained_file_bytes(repo_root: Path, git_tree: str, relative_path: str) -> bytes:
    result = subprocess.run(
        ["git", "show", f"{git_tree}:{relative_path}"],
        cwd=repo_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace").strip()
        raise SystemExit(
            f"retained extraction file {git_tree}:{relative_path} is unavailable: {detail}"
        )
    return result.stdout


def _fnv64_write_raw(value: int, data: bytes) -> int:
    for byte in data:
        value ^= byte
        value = (value * FNV64_PRIME) & 0xFFFF_FFFF_FFFF_FFFF
    return value


def _fnv64_write_bytes(value: int, data: bytes) -> int:
    value = _fnv64_write_raw(value, len(data).to_bytes(8, "little"))
    return _fnv64_write_raw(value, data)


def build_supplement(release: dict[str, Any], repo_root: Path) -> dict[str, Any]:
    extraction = release["extraction"]
    if extraction.get("status") != "retained":
        raise SystemExit(
            f"release {release['map_version']}/{release['revision']} has no retained extraction"
        )
    git_tree = extraction["git_tree"]
    object_fields_bytes = _retained_file_bytes(
        repo_root, git_tree, "resolved/object-fields.tsv"
    )

    objects: dict[str, dict[str, int | str | None]] = {}
    with io.StringIO(object_fields_bytes.decode("utf-8"), newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            if row["category"] != "units" or row["field_id"] not in {"urtm", "ubpx", "ubpy"}:
                continue
            rawcode = row["rawcode"]
            output = objects.setdefault(
                rawcode,
                {
                    "rawcode": rawcode,
                    "repair_time_seconds": None,
                    "button_x": None,
                    "button_y": None,
                },
            )
            value = _integer_recovered_value(
                row["recovered_value_json"], rawcode=rawcode, field_id=row["field_id"]
            )
            key = {
                "urtm": "repair_time_seconds",
                "ubpx": "button_x",
                "ubpy": "button_y",
            }[row["field_id"]]
            previous = output[key]
            if previous is not None and previous != value:
                raise SystemExit(
                    f"{rawcode} {row['field_id']} has conflicting recovered values {previous} and {value}"
                )
            output[key] = value

    return {
        "schema_version": 1,
        "map_version": release["map_version"],
        "release_revision": release["revision"],
        "extraction_git_tree": git_tree,
        "source_object_fields_sha256": hashlib.sha256(object_fields_bytes).hexdigest(),
        "objects": [objects[rawcode] for rawcode in sorted(objects)],
    }


def build_source_manifest(
    release: dict[str, Any], repo_root: Path, content_revision: str
) -> dict[str, Any]:
    extraction = release["extraction"]
    if extraction.get("status") != "retained":
        raise SystemExit(
            f"release {release['map_version']}/{release['revision']} has no retained extraction"
        )
    git_tree = extraction["git_tree"]
    working_alias = extraction.get("working_alias")
    if not working_alias:
        raise SystemExit("retained extraction has no working alias")

    value = FNV64_OFFSET
    for relative_path in RUNTIME_EXTRACTION_FILES:
        label = f"{working_alias}/{relative_path}".encode("utf-8")
        contents = _retained_file_bytes(repo_root, git_tree, relative_path)
        value = _fnv64_write_bytes(value, label)
        value = _fnv64_write_bytes(value, contents)

    runtime_path = release["runtime_content"]["path"]
    supplement_relative = (
        f"{runtime_path}/catalog-supplement-{release['revision']}.json"
    )
    supplement_path = repo_root / supplement_relative
    if not supplement_path.is_file():
        raise SystemExit(f"missing runtime catalog supplement: {supplement_path}")
    value = _fnv64_write_bytes(value, supplement_relative.encode("utf-8"))
    value = _fnv64_write_bytes(value, supplement_path.read_bytes())

    return {
        "schema_version": 1,
        "map_version": release["map_version"],
        "release_revision": release["revision"],
        "content_revision": content_revision,
        "extraction_git_tree": git_tree,
        "source_evidence_fnv64": value,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_RELEASES)
    parser.add_argument(
        "--kind",
        choices=("supplement", "source-manifest"),
        default="supplement",
        help="runtime catalog artifact to print",
    )
    parser.add_argument(
        "--content-revision",
        help="required with --kind source-manifest (for example cf-native-dev-slice-r2)",
    )
    args = parser.parse_args()

    manifest_path = args.manifest.resolve()
    repo_root = REPO_ROOT if manifest_path == DEFAULT_RELEASES.resolve() else manifest_path.parents[2]
    release = _load_release(manifest_path, args.map_version, args.revision)
    if args.kind == "supplement":
        output = build_supplement(release, repo_root)
    else:
        if not args.content_revision:
            parser.error("--content-revision is required with --kind source-manifest")
        output = build_source_manifest(release, repo_root, args.content_revision)
    print(json.dumps(output, indent=2, separators=(",", ": "), sort_keys=False))


if __name__ == "__main__":
    main()
