#!/usr/bin/env python3
"""Audit a source-owned builder roster against a locally extracted SD presentation pack.

No installation assets are committed. This checks delivery/binding, not rendered fidelity.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]


def table(path):
    with path.open() as file:
        return list(csv.DictReader(file, delimiter="\t"))


def safe_file(root, relative, boundary=None):
    path = Path(relative.replace("\\", "/"))
    resolved = (root / path).resolve()
    if path.is_absolute() or not resolved.is_relative_to((boundary or root).resolve()):
        raise ValueError(f"unsafe asset path: {relative}")
    if not resolved.is_file():
        raise ValueError(f"missing asset: {resolved}")
    return resolved


def validate_gltf(path):
    """Check external delivery and animated skin/node bindings, including joint indices."""
    gltf = json.loads(path.read_text())
    nodes = gltf.get("nodes", [])
    accessors = gltf.get("accessors", [])
    for entry in gltf.get("buffers", []) + gltf.get("images", []):
        if entry.get("uri") and not entry["uri"].startswith("data:"):
            resource = safe_file(path.parent, entry["uri"], boundary=path.parent.parent)
            if "byteLength" in entry and resource.stat().st_size < entry["byteLength"]:
                raise ValueError(f"truncated buffer: {resource}")
    for node in nodes:
        for child in node.get("children", []):
            if not 0 <= child < len(nodes):
                raise ValueError(f"invalid child binding: {path}")
        if "skin" in node:
            skin = gltf["skins"][node["skin"]]
            joints = skin["joints"]
            if not joints or any(not 0 <= joint < len(nodes) for joint in joints):
                raise ValueError(f"invalid skeletal binding: {path}")
            if "inverseBindMatrices" in skin:
                matrices = accessors[skin["inverseBindMatrices"]]
                if matrices["type"] != "MAT4" or matrices["count"] != len(joints):
                    raise ValueError(f"invalid inverse bind matrices: {path}")
            for primitive in gltf["meshes"][node["mesh"]]["primitives"]:
                attributes = primitive["attributes"]
                if "JOINTS_0" not in attributes or "WEIGHTS_0" not in attributes:
                    raise ValueError(f"unbound skinned primitive: {path}")
                count = accessors[attributes["POSITION"]]["count"]
                if any(accessors[attributes[key]]["count"] != count
                       for key in ("JOINTS_0", "WEIGHTS_0")):
                    raise ValueError(f"mismatched skin vertex counts: {path}")
                joint_accessor = accessors[attributes["JOINTS_0"]]
                if any(index >= len(joints) for index in joint_accessor.get("max", [])):
                    raise ValueError(f"joint index out of bounds: {path}")
    for animation in gltf.get("animations", []):
        for channel in animation["channels"]:
            if not 0 <= channel["target"]["node"] < len(nodes):
                raise ValueError(f"invalid animation binding: {path}")
    return gltf


def renderable(path):
    return path.lower().endswith((".mdl", ".mdx")) and path.lower() not in (
        ".mdl", ".mdx", "none.mdl", "none.mdx")


def model_identity(path):
    return path.replace("\\", "/").lower().removesuffix(".mdl").removesuffix(".mdx")


def attachment_name(name):
    return re.sub(r"[^a-z0-9]", "", name.lower().replace("ref", ""))


def audit(evidence, assets, builder):
    roster = [row for row in table(evidence / "script/race-buildings.tsv")
              if row["builder_rawcode"] == builder]
    if not roster:
        raise ValueError(f"builder {builder} is absent from retained race evidence")
    fields_path = evidence / "resolved/object-fields.tsv"
    fields = table(fields_path)
    objects = {}
    for row in fields:
        objects.setdefault((row["category"], row["rawcode"]), {})[row["field_id"]] = json.loads(
            row["recovered_value_json"])
    roots = objects[("units", builder)]["ubui"].split(",")
    buildings = {row["building_rawcode"] for row in roster} | set(roots)
    units = {row["unit_rawcode"] for row in roster if row["unit_rawcode"]} | {builder}
    owners = buildings | units
    abilities = {ability for owner in owners
                 for ability in str(objects.get(("units", owner), {}).get("uabi", "")).split(",")
                 if ability}
    proxy_links = []
    for row in table(evidence / "resolved/unit-spell-semantics.tsv"):
        if row["unit_rawcode"] in units:
            children = [code for code in row["effect_rawcodes"].split(",") if code]
            abilities.update(children)
            proxy_links.append({"unit": row["unit_rawcode"], "parent": row["ability_rawcode"],
                                "children": children})
    model_variants = set()
    building_mechanics = evidence / "resolved/building-spell-mechanics.tsv"
    runtime_mechanics = evidence / "resolved/runtime-system-mechanics.tsv"
    for row in table(building_mechanics):
        if row["building_rawcode"] not in buildings:
            continue
        for effect in json.loads(row["effect_objects_json"]):
            if effect.get("categories") == "abilities":
                abilities.add(effect["rawcode"])
                data = json.loads(effect.get("ability_level1", {}).get("data_fields_labeled_json", "{}"))
                for field, value in data.items():
                    if field.startswith("Morph Units"):
                        model_variants.update(value.split(","))
    for row in table(runtime_mechanics):
        parameters = json.loads(row["parameters_json"])
        source = parameters.get("building_rawcode", parameters.get("source_rawcode"))
        if source in buildings and parameters.get("effect_ability_rawcode"):
            abilities.add(parameters["effect_ability_rawcode"])
    units.update(model_variants)
    buffs = {buff for ability in abilities
             for buff in str(objects.get(("abilities", ability), {}).get("abuf", "")).split(",")
             if buff}
    manifests = {pack: json.loads((assets / pack / "manifest.json").read_text())
                 for pack in ("units", "buildings", "effects", "ui")}
    findings = []
    checked_models = set()
    entities = []

    def check_model(pack, gltf):
        key = (pack, gltf)
        if key in checked_models:
            return
        checked_models.add(key)
        try:
            validate_gltf(safe_file(assets / pack, gltf))
            model = next(model for model in manifests[pack]["models"] if model["gltf"] == gltf)
            for texture in model.get("textures", []):
                if texture.get("png"):
                    safe_file(assets / pack, texture["png"])
            for role in ("attachments", "model_particle_emitters", "event_objects"):
                for child in model.get(role, []):
                    if child.get("gltf"):
                        check_model(pack, child["gltf"])
                    elif child.get("path"):
                        findings.append(f"unresolved child model {pack}/{gltf}: {child['path']}")
        except (ValueError, KeyError, IndexError, StopIteration) as error:
            findings.append(f"{pack}/{gltf}: {error}")

    for pack, codes in (("units", units), ("buildings", buildings)):
        bindings = {entry["rawcode"]: entry for entry in manifests[pack][pack]}
        for code in sorted(codes):
            entry = bindings.get(code)
            if not entry or not entry.get("gltf") or entry.get("fallback_to_base_art"):
                findings.append(f"unresolved/substituted {pack} object {code}")
                continue
            check_model(pack, entry["gltf"])
            model = next(model for model in manifests[pack]["models"] if model["gltf"] == entry["gltf"])
            points = {attachment_name(point["name"]) for point in model.get("attachments", [])}
            for attached in entry.get("attached_visuals", []):
                requested = attachment_name(attached["attachment_point"])
                if not any(point.startswith(requested) for point in points):
                    findings.append(f"missing skeletal attachment {code}: {attached['attachment_point']}")
            entities.append({"rawcode": code, "name": entry["name"], "pack": pack,
                             "gltf": entry["gltf"], "attachments": sorted(points),
                             "attached_visuals": entry.get("attached_visuals", []),
                             "model_warnings": model.get("warnings", [])})

    relevant = [entry for entry in manifests["effects"]["assets"]
                if entry["owner_rawcode"] in owners | abilities | buffs
                or entry.get("source_unit_rawcode") in owners]
    for entry in relevant:
        if entry.get("gltf"):
            check_model("effects", entry["gltf"])
        else:
            findings.append(f"unresolved effect {entry['owner_rawcode']}/{entry['role']}: {entry['source_model']}")
    # All explicit resolved visual fields must be delivered. Proxy ownership is additional,
    # never a replacement for the native effect identity emitted by authoritative events.
    roles = {"ua1m": "attack1_projectile", "ua2m": "attack2_projectile", "amat": "missile",
             "acat": "caster", "aeat": "effect", "atat": "target", "asat": "special",
             "feat": "effect", "ftat": "target", "fsat": "special"}
    for category, codes in (("units", owners), ("abilities", abilities), ("buffs", buffs)):
        for code in codes:
            for field, role in roles.items():
                for path in str(objects.get((category, code), {}).get(field, "")).split(","):
                    if renderable(path) and not any(entry["owner_rawcode"] == code
                            and entry["role"] == role and model_identity(entry["source_model"]) == model_identity(path)
                            and entry.get("gltf") for entry in relevant):
                        findings.append(f"missing resolved visual {code}/{field}: {path}")
    ui_bindings = [entry for entry in manifests["ui"]["assets"]
                   if entry["owner_rawcode"] in owners | units | abilities | buffs]
    for entry in ui_bindings:
        try:
            safe_file(assets / "ui", entry["png"])
        except (ValueError, TypeError) as error:
            findings.append(f"UI {entry['owner_rawcode']}/{entry['role']}: {error}")
    lightnings = manifests["effects"].get("native_lightnings", {})
    for effect in lightnings.get("effects", []):
        safe_file(assets / "effects", effect["png"])
    return {"builder": builder, "map_version": manifests["units"]["castle_fight_catalog_version"],
            "source_sha256": {str(path.relative_to(evidence)): hashlib.sha256(path.read_bytes()).hexdigest()
                              for path in (fields_path, evidence / "script/race-buildings.tsv",
                                           evidence / "resolved/unit-spell-semantics.tsv",
                                           building_mechanics, runtime_mechanics)},
            "direct_roots": roots, "model_variants": sorted(model_variants),
            "entities": entities, "abilities": sorted(abilities),
            "buffs": sorted(buffs), "proxy_links": proxy_links,
            "visual_bindings": relevant, "ui_bindings": ui_bindings,
            "checked_models": len(checked_models),
            "native_lightnings": [binding for binding in lightnings.get("abilities", [])
                                  if binding["rawcode"] in abilities],
            "findings": findings,
            "limitations": ["Delivery and binding audit only; no renderer or native WC3 comparison.",
                            "Model warnings are retained per entity; delivery does not close semantic fidelity."]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--builder", required=True)
    parser.add_argument("--evidence", type=Path, default=ROOT / "docs/original_map/extracted")
    parser.add_argument("--assets", type=Path, default=ROOT / "assets/wc3")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = audit(args.evidence, args.assets, args.builder)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"{len(report['entities'])} source-owned entities; {report['checked_models']} model/dependency bindings; "
          f"{len(report['findings'])} findings; report: {args.output}")
    raise SystemExit(bool(report["findings"]))


if __name__ == "__main__":
    main()
