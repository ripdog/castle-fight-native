#!/usr/bin/env python3
"""Project promoted script-owned building decorations from release-pinned Lua."""
from __future__ import annotations

import argparse
import csv
import io
import json
import re

import build_runtime_catalog as catalog

# Source identities only. Model, geometry and scale belong to retained script.
RECIPES = [("h06I", "UK", "by:create984", "BuildingAttachmentHandler_registerBuildingAttach_RaceOrc_call_registerBuildingAttach_RaceOrc")]


def project_call(body: str) -> dict:
    match = re.fullmatch(r'function \w+\(\w+,\w+\)attachBuildingEffect\(\w+,([^,]+),([^,]+),([^,]+),("(?:[^"\\]|\\.)*"),([^,]+),([^,]+)\)end', body)
    if match is None:
        raise ValueError("building decoration needs an audited direct attachment call")
    yaw, x, y, model, scale, z = match.groups()
    number = lambda value: float(value.strip("()"))
    return {"model_path": json.loads(model), "yaw_degrees": number(yaw),
        "offset_world": [number(x), number(y), number(z)], "scale": number(scale)}


def build(release: dict) -> dict:
    tree = release["extraction"]["git_tree"]
    script = catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "script/war3map.lua")
    spans = list(csv.DictReader(io.StringIO(catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "script/function-spans.tsv").decode()), delimiter="\t"))
    functions = {row["name"]: script[int(row["start_byte_offset"]):int(row["end_byte_offset"])].decode() for row in spans}
    entries = []
    for rawcode, registration, factory, callback in RECIPES:
        match = re.search(r'(\w+)=' + re.escape(factory) + r'\(\)Dzb:HashMap_put\((\d+),\1\)', functions[registration])
        if match is None or int(match[2]) != int.from_bytes(rawcode.encode(), "big"):
            raise ValueError("building decoration factory does not match retained registration")
        entries.append({"building_rawcode": rawcode, "source_registration": registration,
            "source_factory": factory, "source_callback": callback,
            "visuals": [project_call(functions[callback])]})
    return {"schema_version": 1, "map_version": release["map_version"],
        "source": {"git_tree": tree, "path": "script/war3map.lua", "placement_helper": "createAttachedBuildingEffect"},
        "buildings": entries}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    print(json.dumps(build(catalog._load_release(catalog.DEFAULT_RELEASES, args.map_version, args.revision)), indent=2))


if __name__ == "__main__":
    main()
