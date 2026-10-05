#!/usr/bin/env python3
"""Project referenced buff art/attachments using release-pinned objects and verified native skin."""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
from pathlib import Path

from build_runtime_catalog import _load_release, _retained_file_bytes

ROOT = Path(__file__).resolve().parents[2]
SKIN_PATH = "base/units/abilityskin.txt"
OUTPUT = ROOT / "crates/wc3-assets/data/castle-fight/9.27/native-buff-visuals-r1.json"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def verify_skin(data, manifest):
    matches = [row for row in manifest if row["path"] == SKIN_PATH]
    if len(matches) != 1:
        raise ValueError("native buff skin must have one retained manifest entry")
    source = matches[0]
    if len(data) != source["bytes"] or digest(data) != source["sha256"]:
        raise ValueError("native buff skin does not match retained base-data manifest")
    return source


def sections(text):
    result = {}
    current = None
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            current = result.setdefault(line[1:-1], {})
        elif current is not None and "=" in line and not line.startswith("//"):
            key, value = line.split("=", 1)
            key = key.lower()
            if key != "targetart" and not key.startswith("targetattach"):
                continue  # Unrelated native icon fields can intentionally repeat.
            if key in current and current[key] != value:
                raise ValueError(f"conflicting native skin field {key}")
            current[key] = value
    return result


def string_list(value):
    if not isinstance(value, str):
        raise ValueError("buff art/reference must be a string")
    return [item.strip() for item in value.split(",") if item.strip() not in {"", "_", "-", "0"}]


def project(skin, rows):
    native = sections(skin)
    references = set()
    buffs = {}
    for row in rows:
        if row["category"] == "abilities" and row["field_id"] == "abuf":
            references.update(string_list(json.loads(row["recovered_value_json"])))
        elif row["category"] == "buffs":
            buff = buffs.setdefault(row["rawcode"], {"base": row["base_rawcode"], "fields": {}})
            if buff["base"] != row["base_rawcode"]:
                raise ValueError("conflicting buff base identity")
            if row["field_id"].startswith("fta"):
                if row["field_id"] not in {"ftat", "ftac", *(f"fta{i}" for i in range(6))}:
                    raise ValueError("unsupported buff attachment field")
                value = json.loads(row["recovered_value_json"])
                previous = buff["fields"].get(row["field_id"])
                if previous is not None and previous != value:
                    raise ValueError("conflicting resolved buff presentation field")
                buff["fields"][row["field_id"]] = value
    result = []
    for rawcode in sorted(references):
        if len(rawcode) != 4 or not rawcode.isascii():
            raise ValueError(f"invalid buff reference {rawcode!r}")
        buff = buffs.get(rawcode, {"base": rawcode, "fields": {}})
        base, fields = buff["base"], buff["fields"]
        inherited = native.get(base, {})
        art = fields.get("ftat", inherited.get("targetart", ""))
        attachments = []
        for index in range(6):
            field = f"fta{index}"
            native_field = "targetattach" + (str(index) if index else "")
            point = fields.get(field, inherited.get(native_field))
            if point is not None:
                if not isinstance(point, str):
                    raise ValueError("buff attachment point must be a string")
                attachments.append({"index": index, "point": point,
                                    "source": "resolved" if field in fields else "native"})
        count = fields.get("ftac", inherited.get("targetattachcount"))
        if count is not None:
            if isinstance(count, bool) or str(count) not in {str(n) for n in range(7)}:
                raise ValueError("unsupported buff attachment count")
            count = int(count)
        result.append({"rawcode": rawcode, "base_rawcode": base,
                       "native_section": base if base in native else None,
                       "target_art": string_list(art),
                       "target_art_source": "resolved" if "ftat" in fields else
                                            "native" if "targetart" in inherited else "absent",
                       "target_attachment_count": count,
                       "target_attachments": attachments})
    return result


def build(skin_path, map_version="9.27", revision="r1", repo_root=ROOT, releases=None):
    release = _load_release(releases or repo_root / "docs/original_map/releases.json", map_version, revision)
    tree = release["extraction"]["git_tree"]
    if not tree:
        raise ValueError("buff projection requires a retained extraction tree")
    sources = {path: _retained_file_bytes(repo_root, tree, f"resolved/{path}")
               for path in ("object-fields.tsv", "base-data-manifest.json", "summary.json")}
    skin = Path(skin_path).read_bytes()
    identity = verify_skin(skin, json.loads(sources["base-data-manifest.json"]))
    return {"schema_version": 1, "map_version": map_version, "source_revision": revision,
            "extraction_git_tree": tree,
            "sources": {path: digest(data) for path, data in sources.items()},
            "native_skin": identity,
            "warcraft_install_build": json.loads(sources["summary.json"])["warcraft_install_build"],
            "buffs": project(skin.decode("utf-8-sig"),
                             csv.DictReader(io.StringIO(sources["object-fields.tsv"].decode()), delimiter="\t"))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--skin", type=Path, default=ROOT / ".wc3-base/source" / SKIN_PATH)
    parser.add_argument("--map-version", default="9.27")
    parser.add_argument("--revision", default="r1")
    parser.add_argument("--output", type=Path, default=OUTPUT)
    parser.add_argument("--check", type=Path)
    args = parser.parse_args()
    artifact = build(args.skin, args.map_version, args.revision)
    if args.check:
        if json.loads(args.check.read_text()) != artifact:
            raise SystemExit(f"stale native buff projection: {args.check}")
        print(f"verified {args.check}")
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(artifact, indent=2, sort_keys=True) + "\n")
        print(f"wrote {args.output}")


if __name__ == "__main__":
    main()
