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
RECIPES = [
    ("h060", "wK", "Ku:create854", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental15"),
    ("h041", "wK", "zu:create843", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental4"),
    ("h040", "wK", "yu:create842", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental3"),
    ("h044", "wK", "Bu:create845", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental6"),
    ("h042", "wK", "Au:create844", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental5"),
    ("h04E", "wK", "Hu:create851", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental12"),
    ("h046", "wK", "Du:create847", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental8"),
    ("h045", "wK", "Cu:create846", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental7"),
    ("h03Z", "wK", "xu:create841", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental2"),
    ("h03Y", "wK", "wu:create840", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental1"),
    ("h03X", "wK", "vu:create839", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental"),
    ("h04D", "wK", "Gu:create850", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental11"),
    ("h06I", "UK", "by:create984", "BuildingAttachmentHandler_registerBuildingAttach_RaceOrc_call_registerBuildingAttach_RaceOrc"),
    ("h04C", "wK", "Fu:create849", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental10"),
    ("h04A", "wK", "Eu:create848", "BuildingAttachmentHandler_registerBuildingAttach_RaceElemental_call_registerBuildingAttach_RaceElemental9"),
]


def project_call(body: str) -> dict:
    return project_calls(body)[0]


def project_calls(body: str) -> list[dict]:
    match = re.fullmatch(r'function (\w+)\((\w+),(\w+)\)(.*)end', body)
    if match is None:
        raise ValueError("building decoration needs an audited direct callback")
    _, _, building, calls = match.groups()
    scalar = r'-?\d+(?:\.\d*)?'
    number = rf'(?:{scalar}|\({scalar}\))'
    model = r'"(?:[^"\\]|\\.)*"'
    geometry = ','.join([f'({number})'] * 3 + [f'({model})'] + [f'({number})'] * 2)
    pattern = (r'attachBuildingEffect(?P<color>Color)?\(' + re.escape(building) + ',' + geometry
        + r'(?P<rgb>,\d+,\d+,\d+)?\)')
    parts = list(re.finditer(pattern, calls))
    if not parts or ''.join(part[0] for part in parts) != calls:
        raise ValueError("building decoration needs only audited direct attachment calls")
    visuals = []
    for part in parts:
        if bool(part['color']) != bool(part['rgb']):
            raise ValueError("colored decoration must retain all RGB channels")
        yaw, x, y, model_path, scale, z = part.groups()[1:7]
        value = lambda text: float(text.strip('()'))
        visual = {"model_path": json.loads(model_path), "yaw_degrees": value(yaw),
            "offset_world": [value(x), value(y), value(z)], "scale": value(scale)}
        if part['rgb']:
            rgb = list(map(int, part['rgb'][1:].split(',')))
            if any(channel > 255 for channel in rgb):
                raise ValueError("decoration RGB channels must fit native bytes")
            visual['color_rgb'] = rgb
        visuals.append(visual)
    return visuals


def build(release: dict) -> dict:
    tree = release["extraction"]["git_tree"]
    script = catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "script/war3map.lua")
    script_text = script.decode()
    spans = list(csv.DictReader(io.StringIO(catalog._retained_file_bytes(catalog.REPO_ROOT, tree, "script/function-spans.tsv").decode()), delimiter="\t"))
    functions = {row["name"]: script[int(row["start_byte_offset"]):int(row["end_byte_offset"])].decode() for row in spans}
    entries = []
    for rawcode, registration, factory, callback in RECIPES:
        match = re.search(r'(\w+)=' + re.escape(factory) + r'\(\)Dzb:HashMap_put\((\d+),\1\)', functions[registration])
        if match is None or int(match[2]) != int.from_bytes(rawcode.encode(), "big"):
            raise ValueError("building decoration factory does not match retained registration")
        if factory.split(":")[0] + ".call=" + callback + " " not in script_text:
            raise ValueError("building decoration callback does not match retained factory dispatch")
        entries.append({"building_rawcode": rawcode, "source_registration": registration,
            "source_factory": factory, "source_callback": callback,
            "visuals": project_calls(functions[callback])})
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
