#!/usr/bin/env python3
"""Project version-pinned Arcane Tower and Obelisk evidence from retained Git data."""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path

from build_runtime_catalog import _load_release, _retained_file_bytes

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "crates/sim/data/castle-fight/9.27/native-carriers.json"
OBJECTS = {"A015", "A03N", "A000", "B01M", "h014", "h005", "A03D"}
FIELDS = {
    "Efk1", "Efk2", "Efk3", "aare", "atar", "amsp", "amat", "amac",
    "acat", "aeat", "atat", "abuf", "adur", "ahdu", "acdn", "pxf1",
    "pxf2", "uhpr", "Ilif", "DataA", "ftat", "feat", "fcat", "ftip",
}


def validate_hostile_target_mask(mask):
    """Reject evidence whose relation/class qualifiers the runtime would otherwise discard."""
    if not isinstance(mask, str):
        raise ValueError("hostile native target mask must be a string")
    seen = set()
    for token in map(str.strip, mask.split(",")):
        token = "enemy" if token == "enemies" else token
        if token not in {"ground", "air", "structure", "enemy"}:
            raise ValueError(f"unsupported hostile native target qualifier {token!r}")
        if token in seen:
            raise ValueError(f"duplicate hostile native target qualifier {token!r}")
        seen.add(token)
    if "enemy" not in seen or not seen.intersection({"ground", "air", "structure"}):
        raise ValueError("hostile native targeting requires enemy relation and a physical class")


def build(map_version="9.27", revision="r1", repo_root=ROOT, releases=None):
    releases = releases or repo_root / "docs/original_map/releases.json"
    release = _load_release(releases, map_version, revision)
    if map_version != "9.27":
        raise ValueError(f"native carrier recipes do not support {map_version}")
    tree = release["extraction"]["git_tree"]
    if not tree:
        raise ValueError("native carrier projection requires a retained extraction tree")
    paths = ("object-fields.tsv", "runtime-system-mechanics.tsv", "objects.tsv")
    sources = {
        path: _retained_file_bytes(repo_root, tree, f"resolved/{path}")
        for path in paths
    }
    fields = {}
    for row in csv.DictReader(io.StringIO(sources[paths[0]].decode()), delimiter="\t"):
        if row["rawcode"] in OBJECTS and row["field_id"] in FIELDS:
            fields.setdefault(row["rawcode"], {})[row["field_id"]] = json.loads(
                row["recovered_value_json"]
            )
    for object_id in ("A015", "A000"):
        validate_hostile_target_mask(fields[object_id]["atar"])
    runtime = next(
        row for row in csv.DictReader(io.StringIO(sources[paths[1]].decode()), delimiter="\t")
        if row["system_id"] == "obelisk-of-light-cleansing-light"
    )
    native_buffs = sorted({
        int.from_bytes(row["rawcode"].encode("ascii"), "big")
        for row in csv.DictReader(io.StringIO(sources[paths[2]].decode()), delimiter="\t")
        if row["category"] == "buffs"
    })
    return {
        "map_version": map_version,
        "source_revision": revision,
        "extraction_git_tree": tree,
        "sources": {path: hashlib.sha256(data).hexdigest() for path, data in sources.items()},
        "fields": fields,
        "cleanse": json.loads(runtime["parameters_json"]),
        "native_buff_ids": native_buffs,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", default="9.27")
    parser.add_argument("--revision", default="r1")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--check", type=Path)
    args = parser.parse_args()
    artifact = build(args.map_version, args.revision)
    if args.check:
        if json.loads(args.check.read_text()) != artifact:
            raise SystemExit(f"stale native carrier projection: {args.check}")
        print(f"verified {args.check}")
    else:
        args.output.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n")
        print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
