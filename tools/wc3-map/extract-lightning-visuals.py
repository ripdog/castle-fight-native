#!/usr/bin/env python3
"""Reproduce the 9.27/r1 native lightning presentation projection from retained objects + SD CASC."""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
RESOLVED = ROOT / "docs/original_map/extracted/resolved"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def sections(text):
    result = {}
    section = None
    for line in text.splitlines():
        line = line.strip()
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1]
            result.setdefault(section, {})
        elif section and "=" in line and not line.startswith("//"):
            key, value = line.split("=", 1)
            result[section][key.lower()] = value
    return result


def sylk(text):
    rows = {}
    y = None
    for line in text.splitlines():
        if not line.startswith("C;"):
            continue
        x = value = None
        for field in line.split(";")[1:]:
            if field.startswith("X"):
                x = int(field[1:])
            elif field.startswith("Y"):
                y = int(field[1:])
            elif field.startswith("K"):
                value = field[1:].strip('"')
        if x is not None and y is not None and value is not None:
            rows.setdefault(y, {})[x] = value
    headers = rows.pop(1)
    return [{headers[x]: v for x, v in row.items()} for row in rows.values()]


def project(skin, lightning, fields):
    skins = sections(skin)
    native = {row["Name"]: row for row in sylk(lightning)}
    # Native SD uses AChV for the visual section but AChv for the gameplay object.
    # Prefer exact sections with actual visual fields, then an unambiguous case variant.
    def visual_section(rawcode):
        exact = skins.get(rawcode, {})
        if "lightningeffect" in exact:
            return rawcode, exact
        candidates = [(key, value) for key, value in skins.items()
                      if key.lower() == rawcode.lower() and "lightningeffect" in value]
        return candidates[0] if len(candidates) == 1 else (rawcode, exact)

    abilities = {}
    for row in fields:
        if row["category"] != "abilities":
            continue
        rawcode, base = row["rawcode"], row["base_rawcode"]
        section, inherited = visual_section(base)
        binding = abilities.setdefault(rawcode, {"rawcode": rawcode, "base_rawcode": base,
                                                "native_section": section,
                                                "effects": inherited.get("lightningeffect", "").split(","),
                                                "target_art": inherited.get("targetart", "").split(",")})
        if row["field_id"] == "alig":
            binding["effects"] = json.loads(row["recovered_value_json"]).split(",")
        if row["field_id"] == "atat":
            binding["target_art"] = json.loads(row["recovered_value_json"]).split(",")
    abilities = [binding for binding in abilities.values() if binding["effects"] != [""]]
    used = {effect for binding in abilities for effect in binding["effects"]}
    effects = []
    for effect in sorted(used):
        row = native[effect]
        effects.append({"id": effect, "texture": row["Dir"] + "\\" + row["file"],
                        "width": float(row["Width"]), "segment_length": float(row["AvgSegLen"]),
                        "noise_scale": float(row["NoiseScale"]),
                        "texcoord_scale": float(row["TexCoordScale"]),
                        "color": [int(row[channel]) for channel in ("R", "G", "B", "A")]})
    return sorted(abilities, key=lambda binding: binding["rawcode"]), effects


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--wc3", required=True, type=Path)
    parser.add_argument("--casc-extract", required=True, type=Path)
    parser.add_argument("--output", type=Path, default=RESOLVED / "native-lightning-visuals.json")
    args = parser.parse_args()
    source_paths = ("war3.w3mod:units/abilityskin.txt", "war3.w3mod:splats/lightningdata.slk")
    with tempfile.TemporaryDirectory() as directory:
        data = []
        for index, source in enumerate(source_paths):
            output = Path(directory) / str(index)
            subprocess.run([str(args.casc_extract), str(args.wc3), "extract", source, str(output)], check=True)
            data.append(output.read_bytes())
    fields_path = RESOLVED / "object-fields.tsv"
    with fields_path.open() as file:
        abilities, effects = project(*(source.decode("utf-8-sig") for source in data),
                                     csv.DictReader(file, delimiter="\t"))
    projection = {"map_version": "9.27", "release_revision": "r1",
                  "sources": [{"path": path, "sha256": digest(source)}
                              for path, source in zip(source_paths, data)],
                  "objects_sha256": digest(fields_path.read_bytes()),
                  "abilities": abilities, "effects": effects}
    args.output.write_text(json.dumps(projection, indent=2) + "\n")


if __name__ == "__main__":
    main()
