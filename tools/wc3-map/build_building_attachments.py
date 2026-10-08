#!/usr/bin/env python3
"""Project script and native building decorations from release-pinned evidence."""
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
    fields = list(csv.DictReader(io.StringIO(catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "resolved/object-fields.tsv").decode()), delimiter="\t"))
    abilities, inventories = {}, {}
    for row in fields:
        if row["category"] == "abilities":
            ability = abilities.setdefault(row["rawcode"], {"base": row["base_rawcode"]})
            if row["level"] in {"0", "1"}:
                ability[row["field_id"]] = json.loads(row["recovered_value_json"])
        elif row["category"] == "units" and row["field_id"] == "uabi":
            inventories[row["rawcode"]] = json.loads(row["recovered_value_json"])
    buildings = list(csv.DictReader(io.StringIO(catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "resolved/units.tsv").decode()), delimiter="\t"))
    for building in buildings:
        if building["is_building"] != "1":
            continue
        attachments = []
        for rawcode in inventories.get(building["rawcode"], "").split(","):
            ability = abilities.get(rawcode, {})
            base = ability.get("base")
            marker_aura = (base == "Aasl" and ability.get("Slo1") == 0
                and ability.get("Slo2") == 0 and ability.get("aare") == 0)
            if not (base == "Asph" or marker_aura) or not ability.get("atat"):
                continue
            count = int(ability["atac"])
            points = [ability.get(f"ata{index}", "") for index in range(count)]
            if not all(points):
                raise ValueError("native building attachment must retain every authored point")
            attachments.append({"ability_rawcode": rawcode, "attachment_points": points})
        if attachments:
            entry = next((row for row in entries if row["building_rawcode"] == building["rawcode"]), None)
            if entry is None:
                entry = {"building_rawcode": building["rawcode"], "visuals": []}
                entries.append(entry)
            entry["native_abilities"] = attachments
    entries.sort(key=lambda row: row["building_rawcode"])
    return {"schema_version": 1, "map_version": release["map_version"],
        "source": {"git_tree": tree, "path": "script/war3map.lua", "native_path": "resolved/object-fields.tsv", "placement_helper": "createAttachedBuildingEffect"},
        "buildings": entries}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", required=True)
    parser.add_argument("--revision", required=True)
    args = parser.parse_args()
    print(json.dumps(build(catalog._load_release(catalog.DEFAULT_RELEASES, args.map_version, args.revision)), indent=2))


if __name__ == "__main__":
    main()
