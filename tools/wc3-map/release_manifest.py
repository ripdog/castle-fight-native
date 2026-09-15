#!/usr/bin/env python3
"""Resolve and verify retained Castle Fight release revisions.

The registry deliberately separates a map label (for example 9.27) from a
revision (for example r1).  Callers must resolve an exact revision before
using source, extraction, or runtime-content paths; there is no nearest-version
fallback.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any


REPO_ROOT = Path(__file__).resolve().parents[2]
DEFAULT_MANIFEST = REPO_ROOT / "docs/original_map/releases.json"
VERSION_RE = re.compile(r"^[0-9]+\.[0-9]+$")
REVISION_RE = re.compile(r"^r[1-9][0-9]*$")
AVAILABILITY = {"archived", "supported-development-subset", "supported-full"}
EXTRACTION_STATUS = {"pending", "retained"}
RUNTIME_STATUS = {"unsupported", "supported-development-subset", "supported-full"}


@dataclass(frozen=True)
class ReleaseKey:
    map_version: str
    revision: str


class ManifestError(ValueError):
    pass


def _relative_repo_path(value: Any, field: str, *, nullable: bool = False) -> str | None:
    if value is None and nullable:
        return None
    if not isinstance(value, str) or not value:
        raise ManifestError(f"{field} must be a non-empty repository-relative path")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise ManifestError(f"{field} must stay inside the repository: {value!r}")
    return value


def _sha256(value: Any, field: str, *, nullable: bool = False) -> str | None:
    if value is None and nullable:
        return None
    if not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{64}", value):
        raise ManifestError(f"{field} must be a lowercase SHA-256 digest")
    return value


def load_manifest(path: Path = DEFAULT_MANIFEST) -> dict[str, Any]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    if payload.get("schema_version") != 1:
        raise ManifestError("unsupported Castle Fight release-manifest schema")
    releases = payload.get("releases")
    if not isinstance(releases, list) or not releases:
        raise ManifestError("release manifest must contain at least one release")

    seen: set[ReleaseKey] = set()
    for index, release in enumerate(releases):
        if not isinstance(release, dict):
            raise ManifestError(f"releases[{index}] must be an object")
        version = release.get("map_version")
        revision = release.get("revision")
        if not isinstance(version, str) or VERSION_RE.fullmatch(version) is None:
            raise ManifestError(f"invalid map_version at releases[{index}]: {version!r}")
        if not isinstance(revision, str) or REVISION_RE.fullmatch(revision) is None:
            raise ManifestError(f"invalid revision at releases[{index}]: {revision!r}")
        key = ReleaseKey(version, revision)
        if key in seen:
            raise ManifestError(f"duplicate release revision {version}/{revision}")
        seen.add(key)

        if release.get("availability") not in AVAILABILITY:
            raise ManifestError(f"invalid availability for {version}/{revision}")

        source = release.get("source_archive")
        if not isinstance(source, dict):
            raise ManifestError(f"missing source_archive for {version}/{revision}")
        _relative_repo_path(source.get("path"), f"{version}/{revision}.source_archive.path")
        _sha256(source.get("sha256"), f"{version}/{revision}.source_archive.sha256")
        if not isinstance(source.get("bytes"), int) or source["bytes"] <= 0:
            raise ManifestError(f"invalid source_archive.bytes for {version}/{revision}")

        base_data = release.get("warcraft_base_data")
        if not isinstance(base_data, dict):
            raise ManifestError(f"missing warcraft_base_data for {version}/{revision}")
        base_manifest = _relative_repo_path(
            base_data.get("manifest_path"),
            f"{version}/{revision}.warcraft_base_data.manifest_path",
            nullable=True,
        )
        base_manifest_digest = _sha256(
            base_data.get("manifest_sha256"),
            f"{version}/{revision}.warcraft_base_data.manifest_sha256",
            nullable=True,
        )
        if (base_manifest is None) != (base_manifest_digest is None):
            raise ManifestError(
                f"{version}/{revision} base-data manifest path/digest must be both set or both null"
            )

        extraction = release.get("extraction")
        if not isinstance(extraction, dict) or extraction.get("status") not in EXTRACTION_STATUS:
            raise ManifestError(f"invalid extraction status for {version}/{revision}")
        _relative_repo_path(
            extraction.get("working_alias"),
            f"{version}/{revision}.extraction.working_alias",
        )
        tree = extraction.get("git_tree")
        if extraction["status"] == "retained":
            if not isinstance(tree, str) or re.fullmatch(r"[0-9a-f]{40}", tree) is None:
                raise ManifestError(f"retained extraction {version}/{revision} needs a git_tree")
            if not isinstance(extraction.get("extractor_revision"), str):
                raise ManifestError(
                    f"retained extraction {version}/{revision} needs extractor_revision"
                )
        elif tree is not None:
            raise ManifestError(f"pending extraction {version}/{revision} cannot claim a git_tree")

        runtime = release.get("runtime_content")
        if not isinstance(runtime, dict) or runtime.get("status") not in RUNTIME_STATUS:
            raise ManifestError(f"invalid runtime-content status for {version}/{revision}")
        runtime_path = _relative_repo_path(
            runtime.get("path"),
            f"{version}/{revision}.runtime_content.path",
            nullable=True,
        )
        _sha256(
            runtime.get("native_effect_tuning_sha256"),
            f"{version}/{revision}.runtime_content.native_effect_tuning_sha256",
            nullable=True,
        )
        binding_path = _relative_repo_path(
            runtime.get("binding_registry_path"),
            f"{version}/{revision}.runtime_content.binding_registry_path",
            nullable=True,
        )
        binding_digest = _sha256(
            runtime.get("binding_registry_sha256"),
            f"{version}/{revision}.runtime_content.binding_registry_sha256",
            nullable=True,
        )
        if (binding_path is None) != (binding_digest is None):
            raise ManifestError(
                f"{version}/{revision} binding-registry path/digest must be both set or both null"
            )
        if runtime["status"] == "unsupported" and runtime_path is not None:
            raise ManifestError(
                f"unsupported runtime content {version}/{revision} must not advertise a path"
            )
        if runtime["status"] != "unsupported" and runtime_path is None:
            raise ManifestError(f"supported runtime content {version}/{revision} needs a path")

    return payload


def resolve_release(
    payload: dict[str, Any], map_version: str, revision: str | None
) -> dict[str, Any]:
    matching = [
        release
        for release in payload["releases"]
        if release["map_version"] == map_version
        and (revision is None or release["revision"] == revision)
    ]
    if not matching:
        suffix = f"/{revision}" if revision else ""
        raise ManifestError(f"Castle Fight release {map_version}{suffix} is not registered")
    if revision is None and len(matching) != 1:
        revisions = ", ".join(release["revision"] for release in matching)
        raise ManifestError(
            f"Castle Fight {map_version} has multiple revisions ({revisions}); choose one explicitly"
        )
    return matching[0]


def _hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_source(release: dict[str, Any], repo_root: Path = REPO_ROOT) -> list[str]:
    source = release["source_archive"]
    path = repo_root / source["path"]
    errors: list[str] = []
    if not path.is_file():
        return [f"source archive is missing: {source['path']}"]
    actual_size = path.stat().st_size
    if actual_size != source["bytes"]:
        errors.append(
            f"source archive size mismatch: expected {source['bytes']}, got {actual_size}"
        )
    actual_digest = _hash_file(path)
    if actual_digest != source["sha256"]:
        errors.append(
            f"source archive digest mismatch: expected {source['sha256']}, got {actual_digest}"
        )
    return errors


def verify_retained_git_tree(release: dict[str, Any], repo_root: Path = REPO_ROOT) -> list[str]:
    extraction = release["extraction"]
    if extraction["status"] != "retained":
        return []
    tree = extraction["git_tree"]
    result = subprocess.run(
        ["git", "cat-file", "-e", f"{tree}^{{tree}}"],
        cwd=repo_root,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        return [f"retained extraction git tree is unavailable: {tree}"]

    base_data = release["warcraft_base_data"]
    manifest_path = base_data.get("manifest_path")
    manifest_digest = base_data.get("manifest_sha256")
    if manifest_path is None or manifest_digest is None:
        return []
    alias = extraction["working_alias"].rstrip("/") + "/"
    if not manifest_path.startswith(alias):
        return [
            "base-data manifest for retained extraction must live under its working alias"
        ]
    tree_relative_path = manifest_path[len(alias) :]
    actual = _git_file_sha256(repo_root, tree, tree_relative_path)
    if actual is None:
        return [f"base-data manifest is unavailable in retained tree: {tree_relative_path}"]
    if actual != manifest_digest:
        return [f"base-data manifest digest mismatch in retained tree: {tree_relative_path}"]
    return []


def _git_file_sha256(repo_root: Path, revision: str, path: str) -> str | None:
    result = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        cwd=repo_root,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        check=False,
    )
    if result.returncode != 0:
        return None
    return hashlib.sha256(result.stdout).hexdigest()


def verify_runtime_content(release: dict[str, Any], repo_root: Path = REPO_ROOT) -> list[str]:
    runtime = release["runtime_content"]
    if runtime["status"] == "unsupported":
        return []
    revision = runtime.get("source_revision")
    if not isinstance(revision, str):
        return ["supported runtime content is missing source_revision"]

    errors: list[str] = []
    tuning_digest = runtime.get("native_effect_tuning_sha256")
    if tuning_digest is not None:
        tuning_path = f"{runtime['path']}/native-effect-tuning.json"
        actual = _git_file_sha256(repo_root, revision, tuning_path)
        if actual is None:
            errors.append(f"runtime tuning is unavailable at {revision}:{tuning_path}")
        elif actual != tuning_digest:
            errors.append(f"runtime tuning digest mismatch at {revision}:{tuning_path}")
    binding_path = runtime.get("binding_registry_path")
    binding_digest = runtime.get("binding_registry_sha256")
    if binding_path is not None and binding_digest is not None:
        actual = _git_file_sha256(repo_root, revision, binding_path)
        if actual is None:
            errors.append(f"binding registry is unavailable at {revision}:{binding_path}")
        elif actual != binding_digest:
            errors.append(f"binding registry digest mismatch at {revision}:{binding_path}")
    return errors


def verify_release(release: dict[str, Any], repo_root: Path = REPO_ROOT) -> list[str]:
    return (
        verify_source(release, repo_root)
        + verify_retained_git_tree(release, repo_root)
        + verify_runtime_content(release, repo_root)
    )


def release_path(release: dict[str, Any], kind: str) -> str:
    if kind == "source":
        return release["source_archive"]["path"]
    if kind == "extraction":
        return release["extraction"]["working_alias"]
    if kind == "runtime":
        path = release["runtime_content"]["path"]
        if path is None:
            raise ManifestError(
                f"{release['map_version']}/{release['revision']} has no supported runtime content"
            )
        return path
    raise AssertionError(f"unhandled path kind {kind}")


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, default=DEFAULT_MANIFEST)
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="list registered release revisions")
    list_parser.add_argument("--json", action="store_true")

    show = subparsers.add_parser("show", help="show one exact release revision")
    show.add_argument("map_version")
    show.add_argument("revision", nargs="?")

    path = subparsers.add_parser("path", help="print a registered repository-relative path")
    path.add_argument("kind", choices=("source", "extraction", "runtime"))
    path.add_argument("map_version")
    path.add_argument("revision", nargs="?")
    path.add_argument(
        "--require-pending",
        action="store_true",
        help="for extraction paths, reject already-retained revisions",
    )

    verify = subparsers.add_parser("verify", help="verify retained archive/content identities")
    verify.add_argument("map_version", nargs="?")
    verify.add_argument("revision", nargs="?")

    return parser


def main(argv: list[str] | None = None) -> int:
    args = _parser().parse_args(argv)
    try:
        payload = load_manifest(args.manifest)
        if args.command == "list":
            releases = payload["releases"]
            if args.json:
                print(json.dumps(releases, indent=2))
            else:
                for release in releases:
                    print(
                        f"{release['map_version']}/{release['revision']}\t"
                        f"{release['availability']}\t"
                        f"extraction={release['extraction']['status']}\t"
                        f"runtime={release['runtime_content']['status']}"
                    )
            return 0

        if args.command in {"show", "path"}:
            release = resolve_release(payload, args.map_version, args.revision)
            if args.command == "show":
                print(json.dumps(release, indent=2))
            else:
                if args.require_pending:
                    if args.kind != "extraction":
                        raise ManifestError("--require-pending is valid only for extraction paths")
                    if release["extraction"]["status"] != "pending":
                        raise ManifestError(
                            f"{release['map_version']}/{release['revision']} is already a retained extraction; create a new revision instead of overwriting it"
                        )
                print(release_path(release, args.kind))
            return 0

        if args.command == "verify":
            releases = payload["releases"]
            if args.map_version is not None:
                releases = [resolve_release(payload, args.map_version, args.revision)]
            errors: list[str] = []
            for release in releases:
                release_errors = verify_release(release)
                key = f"{release['map_version']}/{release['revision']}"
                if release_errors:
                    errors.extend(f"{key}: {error}" for error in release_errors)
                else:
                    print(f"{key}: ok")
            if errors:
                for error in errors:
                    print(error, file=sys.stderr)
                return 1
            return 0

        raise AssertionError(f"unhandled command {args.command}")
    except (ManifestError, OSError, json.JSONDecodeError) as error:
        print(f"release manifest error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
