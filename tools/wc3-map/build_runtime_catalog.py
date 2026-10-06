#!/usr/bin/env python3
"""Generate compact, version-pinned runtime catalog supplements from retained extraction evidence."""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import re
import subprocess
from decimal import Decimal, ROUND_CEILING
from pathlib import Path
from typing import Any, Callable

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


def _line_world_integer(value: str) -> int:
    # The current world-unit importer is integral; reject lossy projections.
    number = Decimal(value)
    if not number.is_finite() or number != number.to_integral_value() or number < 0:
        raise SystemExit(f"line distance must be a nonnegative integral world value: {value!r}")
    return int(number)


def _line_damage_retention(value: str) -> int:
    retention = (1 - Decimal(value)) * 10_000
    if (
        not retention.is_finite()
        or retention != retention.to_integral_value()
        or not 0 <= retention <= 10_000
    ):
        raise SystemExit("line damage retention must be exact in native fixed point")
    return int(retention)


def _hotkey_recovered_value(value: str, *, rawcode: str) -> str | None:
    normalized = value.strip('"')
    if normalized in {"", "null"}:
        return None
    if len(normalized) != 1:
        raise SystemExit(f"{rawcode} uhot has invalid recovered value {value!r}")
    return normalized


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


def _canonical_text_bytes(data: bytes) -> bytes:
    return data.replace(b"\r\n", b"\n")


def _project_tsv_rows(
    data: bytes,
    fields: tuple[str, ...],
    *,
    predicate: tuple[str, str] | None = None,
    sort_field: str,
) -> bytes:
    with io.StringIO(_canonical_text_bytes(data).decode("utf-8"), newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        fieldnames = reader.fieldnames or []
        missing = [field for field in (*fields, sort_field) if field not in fieldnames]
        if predicate is not None and predicate[0] not in fieldnames:
            missing.append(predicate[0])
        if missing:
            raise SystemExit(
                f"runtime catalog evidence is missing TSV columns: {sorted(set(missing))}"
            )
        rows = []
        for row in reader:
            if predicate is not None and row[predicate[0]] != predicate[1]:
                continue
            rows.append(tuple(row[field] for field in fields))
        sort_index = fields.index(sort_field)
        rows.sort(key=lambda row: row[sort_index])
    lines = ["\t".join(fields), *("\t".join(row) for row in rows)]
    return ("\n".join(lines) + "\n").encode("utf-8")


def _runtime_evidence_bytes(relative_path: str, data: bytes) -> bytes:
    if relative_path == "script/race-building-semantics.tsv":
        return _project_tsv_rows(
            data,
            ("building_rawcode", "income_factor", "precursor_rawcode", "is_siege"),
            sort_field="building_rawcode",
        )
    if relative_path == "resolved/production-buildings.tsv":
        return _project_tsv_rows(
            data,
            ("building_rawcode", "unit_rawcode", "spawn_time"),
            predicate=("building_kind", "production"),
            sort_field="building_rawcode",
        )
    return _canonical_text_bytes(data)


def _fnv64_write_raw(value: int, data: bytes) -> int:
    for byte in data:
        value ^= byte
        value = (value * FNV64_PRIME) & 0xFFFF_FFFF_FFFF_FFFF
    return value


def _fnv64_write_bytes(value: int, data: bytes) -> int:
    value = _fnv64_write_raw(value, len(data).to_bytes(8, "little"))
    return _fnv64_write_raw(value, data)


def _action_duration_ticks(point: str, backswing: str, hz: int) -> int:
    values = [Decimal(0) if value in {"", "-", "_"} else Decimal(value) for value in (point.strip(), backswing.strip())]
    if any(not value.is_finite() or value < 0 for value in values):
        raise SystemExit("action animation timing must be finite and nonnegative")
    ticks = int((sum(values) * hz).to_integral_value(rounding=ROUND_CEILING))
    if ticks > 65535:
        raise SystemExit("action animation timing exceeds native tick capacity")
    return ticks


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
    frequency = re.search(
        r"^pub const CASTLE_FIGHT_SIMULATION_HZ: i32 = ([0-9]+);$",
        (repo_root / "crates/sim/src/content.rs").read_text(encoding="utf-8"), re.MULTILINE,
    )
    if frequency is None or int(frequency.group(1)) <= 0:
        raise SystemExit("cannot resolve the engine's simulation frequency")
    simulation_hz = int(frequency.group(1))
    script_bytes = _retained_file_bytes(repo_root, git_tree, "script/war3map.lua")
    if b"FogMaskEnable(false)FogEnable(true)" not in script_bytes:
        raise SystemExit("map fog initialization changed")
    def fog_rectangle(symbol: bytes) -> list[int]:
        match = re.search(rb"\b" + symbol + rb"=_b\[.{1,180}?\]\((.*?)\)(?=[A-Za-z_])", script_bytes)
        if match is None:
            raise SystemExit("map permanent vision rectangle changed")
        values = match[1].replace(b"(", b"").replace(b")", b"").split(b",")
        if len(values) != 4:
            raise SystemExit("map vision rectangle is not a rectangle")
        return [int(Decimal(value.decode())) for value in values]
    base_misc_path = repo_root / "docs/original_map/base/2.0.4.23745/miscdata.txt"
    base_misc = base_misc_path.read_bytes()
    base_manifest = json.loads(_retained_file_bytes(repo_root, git_tree, "resolved/base-data-manifest.json"))
    expected_misc = next(row["sha256"] for row in base_manifest if row["path"] == "base/units/miscdata.txt")
    if hashlib.sha256(base_misc).hexdigest() != expected_misc:
        raise SystemExit("retained Warcraft vision defaults drifted")
    misc_values = dict(re.findall(r"^([A-Za-z]+)=([^\r\n]+)", base_misc.decode(), re.MULTILINE))
    misc_values.update(dict(re.findall(r"^([A-Za-z]+)=([^\r\n]+)", _retained_file_bytes(repo_root, git_tree, "war3mapMisc.txt").decode(), re.MULTILINE)))
    cycle_ticks = int(misc_values["DayLength"]) * simulation_hz
    def clock_phase(hour: int) -> int:
        value = Decimal(hour) * cycle_ticks / Decimal(misc_values["DayHours"])
        if value != value.to_integral_value():
            raise SystemExit("day/night boundaries must fall on simulation ticks")
        return int(value)
    helper = re.search(rb"function dummyCastTargetWithVision1\(.*?end function", script_bytes)
    if helper is None:
        raise SystemExit("script dummy target vision helper changed")
    helper_radius = re.search(rb",([0-9.]+),true,false\)", helper[0])
    helper_duration = re.search(rb"doAfter\(([0-9.]+),", helper[0])
    if helper_radius is None or helper_duration is None or b"player_hasVisibility" not in helper[0]:
        raise SystemExit("script dummy target vision geometry/timing changed")
    graph: dict[str, set[str]] = {}
    for row in csv.DictReader(io.StringIO(_retained_file_bytes(repo_root, git_tree, "script/call-graph.tsv").decode()), delimiter="\t"):
        graph.setdefault(row["caller"], set()).add(row["callee"])
    def reaches_helper(roots: list[str]) -> bool:
        pending, seen = list(roots), set()
        while pending:
            name = pending.pop()
            if name in seen:
                continue
            seen.add(name)
            if name == "dummyCastTargetWithVision1":
                return True
            pending.extend(graph.get(name, set()) - seen)
        return False
    spell_reveals: dict[str, dict[str, Any]] = {}
    for path, column in (("resolved/unit-spell-mechanics.tsv", "handler_function"),
                         ("resolved/building-spell-mechanics.tsv", "source_functions")):
        for row in csv.DictReader(io.StringIO(_retained_file_bytes(repo_root, git_tree, path).decode()), delimiter="\t"):
            if reaches_helper(row[column].split(",")):
                spell_reveals[row["ability_rawcode"]] = {
                    "rawcode": row["ability_rawcode"], "radius_world": _line_world_integer(helper_radius[1].decode()),
                    "duration_ticks": int(Decimal(helper_duration[1].decode()) * simulation_hz), "only_if_hidden": True,
                    "detects_invisible": False,
                }
            if "starfallSpell" in row[column].split(","):
                parameters = json.loads(row["parameters_json"])
                spell_reveals[row["ability_rawcode"]] = {
                    "rawcode": row["ability_rawcode"], "radius_world": parameters["temporary_vision_radius"],
                    "duration_ticks": int(Decimal(parameters["visual_and_fog_cleanup_delay_seconds"]) * simulation_hz),
                    "only_if_hidden": False,
                    "detects_invisible": False,
                }

    action_fields: dict[str, dict[str, str]] = {}
    sight_fields: dict[str, dict[str, int]] = {}
    bounce_fields: dict[str, dict[str, str]] = {}
    line_fields: dict[str, dict[str, str]] = {}
    objects: dict[str, dict[str, Any]] = {}
    far_sight: dict[str, dict[str, int]] = {}
    with io.StringIO(object_fields_bytes.decode("utf-8"), newline="") as handle:
        reader = csv.DictReader(handle, delimiter="\t")
        for row in reader:
            if row["category"] == "abilities" and row["base_rawcode"] == "AOfs" and row["level"] == "1" and row["field_id"] in {"aare", "adur", "Ofs1"}:
                far_sight.setdefault(row["rawcode"], {})[row["field_id"]] = _integer_recovered_value(
                    row["recovered_value_json"], rawcode=row["rawcode"], field_id=row["field_id"])
            if row["category"] == "units" and row["field_id"] in {"usid", "usin"}:
                value = _integer_recovered_value(
                    row["recovered_value_json"], rawcode=row["rawcode"], field_id=row["field_id"]
                )
                if value < 0:
                    raise SystemExit("sight radius must be nonnegative")
                sight_fields.setdefault(row["rawcode"], {})[row["field_id"]] = value
            if row["category"] == "units" and row["field_id"] in {"udp1", "ubs1", "udp2", "ubs2", "ucpt", "ucbs"}:
                action_fields.setdefault(row["rawcode"], {})[row["field_id"]] = row["recovered_value_json"].strip('"')
            if row["category"] == "units" and row["field_id"] in {
                "usd1", "usr1", "udl1", "uamn", "ua1p"
            }:
                line_fields.setdefault(row["rawcode"], {})[row["field_id"]] = (
                    row["recovered_value_json"].strip('"')
                )
            if row["category"] == "units" and row["field_id"] in {"utc1", "udl1", "ua1f"}:
                bounce_fields.setdefault(row["rawcode"], {})[row["field_id"]] = row["recovered_value_json"].strip('"')
            if row["category"] != "units" or row["field_id"] not in {"urtm", "ubpx", "ubpy", "uhot", "ubui"}:
                continue
            rawcode = row["rawcode"]
            output = objects.setdefault(
                rawcode,
                {
                    "rawcode": rawcode,
                    "repair_time_seconds": None,
                    "button_x": None,
                    "button_y": None,
                    "hotkey": None,
                    "build_catalog": None,
                },
            )
            if row["field_id"] == "ubui":
                text = row["recovered_value_json"].strip('"')
                value = [] if text in {"", "_", "-"} else text.split(",")
                if any(len(code) != 4 for code in value):
                    raise SystemExit(f"{rawcode} ubui contains an invalid rawcode")
            elif row["field_id"] == "uhot":
                value = _hotkey_recovered_value(row["recovered_value_json"], rawcode=rawcode)
            else:
                value = _integer_recovered_value(
                    row["recovered_value_json"], rawcode=rawcode, field_id=row["field_id"]
                )
            key = {
                "urtm": "repair_time_seconds",
                "ubpx": "button_x",
                "ubpy": "button_y",
                "uhot": "hotkey",
                "ubui": "build_catalog",
            }[row["field_id"]]
            previous = output[key]
            if previous is not None and previous != value:
                raise SystemExit(
                    f"{rawcode} {row['field_id']} has conflicting recovered values {previous} and {value}"
                )
            output[key] = value

    for row in csv.DictReader(io.StringIO(_retained_file_bytes(repo_root, git_tree, "resolved/building-spell-mechanics.tsv").decode()), delimiter="\t"):
        parameters = json.loads(row["parameters_json"])
        auxiliary = parameters.get("auxiliary_ability_id")
        if auxiliary is not None:
            rawcode = int(auxiliary).to_bytes(4, "big").decode()
            if rawcode in far_sight:
                fields = far_sight[rawcode]
                spell_reveals[row["ability_rawcode"]] = {
                    "rawcode": row["ability_rawcode"], "source_ability_rawcode": rawcode,
                    "radius_world": fields["aare"], "duration_ticks": fields["adur"] * simulation_hz,
                    "only_if_hidden": False, "detects_invisible": bool(fields["Ofs1"] & 1),
                }

    bounce_weapons = []
    line_weapons = []
    unit_rows = csv.DictReader(io.StringIO(_retained_file_bytes(repo_root, git_tree, "resolved/units.tsv").decode()), delimiter="\t")
    for unit in unit_rows:
        if unit["attack1_weapon_type"] in {"mline", "aline"}:
            fields = line_fields[unit["rawcode"]]
            line_weapons.append({
                "rawcode": unit["rawcode"],
                "spill_distance_world": _line_world_integer(fields["usd1"]),
                "spill_radius_world": _line_world_integer(fields["usr1"]),
                "minimum_range_world": (
                    _line_world_integer(fields["uamn"])
                    if fields["uamn"] not in {"-", "_", ""} else 0
                ),
                "damage_retention_per_10k": _line_damage_retention(fields["udl1"]),
                "splash_targets": fields["ua1p"],
            })
        if unit["attack1_weapon_type"] != "mbounce":
            continue
        fields = bounce_fields[unit["rawcode"]]
        retained_percent = (1 - Decimal(fields["udl1"])) * 100
        if not retained_percent.is_finite() or retained_percent != retained_percent.to_integral_value() or not 1 <= retained_percent <= 100:
            raise SystemExit("bounce damage retention must be an exact supported percentage")
        targets = int(fields["utc1"])
        if not 1 <= targets <= 9:
            raise SystemExit("bounce target count exceeds native capacity")
        bounce_weapons.append({"rawcode": unit["rawcode"], "maximum_targets": targets,
                               "damage_percent_per_bounce": int(retained_percent), "range_world": int(fields["ua1f"])})
    return {
        "schema_version": 5,
        "fog_rules": {
            "initially_explored": True,
            "attack_reveal_radius_world": _line_world_integer(misc_values["FoggedAttackRevealRadius"]),
            "attack_reveal_duration_ticks": int(Decimal(misc_values["FogFlashTime"]) * simulation_hz),
            "clock": {
                "cycle_ticks": cycle_ticks,
                "dawn_phase_ticks": clock_phase(int(misc_values["Dawn"])),
                "dusk_phase_ticks": clock_phase(int(misc_values["Dusk"])),
                # Native custom-map convention: noon. CF never sets/scales/suspends time;
                # do not apply Blizzard.j's unrelated melee-only starting time.
                "initial_phase_ticks": clock_phase(12),
            },
            "source_base_misc_sha256": expected_misc,
            "permanent_rectangles_world": [[fog_rectangle(b"NFb"), fog_rectangle(b"JFb")],
                                           [fog_rectangle(b"MFb"), fog_rectangle(b"JFb")]],
            "source_script_sha256": hashlib.sha256(script_bytes).hexdigest(),
        },
        "sight_profiles": [
            {"rawcode": rawcode, "day_world": fields["usid"], "night_world": fields["usin"]}
            for rawcode, fields in sorted(sight_fields.items())
        ],
        "simulation_hz": simulation_hz,
        "spell_reveals": sorted(spell_reveals.values(), key=lambda reveal: reveal["rawcode"]),
        "action_timings": [
            {
                "rawcode": rawcode,
                "primary_attack_ticks": _action_duration_ticks(fields.get("udp1", "-"), fields.get("ubs1", "-"), simulation_hz),
                "primary_attack_point_ticks": _action_duration_ticks(fields.get("udp1", "-"), "0", simulation_hz),
                "secondary_attack_ticks": _action_duration_ticks(fields.get("udp2", "-"), fields.get("ubs2", "-"), simulation_hz),
                "secondary_attack_point_ticks": _action_duration_ticks(fields.get("udp2", "-"), "0", simulation_hz),
                "cast_ticks": _action_duration_ticks(fields.get("ucpt", "-"), fields.get("ucbs", "-"), simulation_hz),
            }
            for rawcode, fields in sorted(action_fields.items())
        ],
        "bounce_weapons": sorted(bounce_weapons, key=lambda weapon: weapon["rawcode"]),
        "line_weapons": sorted(line_weapons, key=lambda weapon: weapon["rawcode"]),
        "map_version": release["map_version"],
        "release_revision": release["revision"],
        "extraction_git_tree": git_tree,
        "source_object_fields_sha256": hashlib.sha256(object_fields_bytes).hexdigest(),
        "objects": [objects[rawcode] for rawcode in sorted(objects)],
    }


def _source_evidence_fnv64(
    release: dict[str, Any],
    repo_root: Path,
    extraction_file_bytes: Callable[[str], bytes],
) -> int:
    working_alias = release["extraction"].get("working_alias")
    if not working_alias:
        raise SystemExit("retained extraction has no working alias")

    value = FNV64_OFFSET
    for relative_path in RUNTIME_EXTRACTION_FILES:
        label = f"{working_alias}/{relative_path}".encode("utf-8")
        contents = _runtime_evidence_bytes(
            relative_path, extraction_file_bytes(relative_path)
        )
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
    return _fnv64_write_bytes(
        value, _canonical_text_bytes(supplement_path.read_bytes())
    )


def working_alias_source_evidence_fnv64(
    release: dict[str, Any], repo_root: Path
) -> int:
    extraction = release["extraction"]
    if extraction.get("status") != "retained":
        raise SystemExit(
            f"release {release['map_version']}/{release['revision']} has no retained extraction"
        )
    working_alias = extraction.get("working_alias")
    if not working_alias:
        raise SystemExit("retained extraction has no working alias")
    extraction_root = repo_root / working_alias

    def read_working_file(relative_path: str) -> bytes:
        path = extraction_root / relative_path
        if not path.is_file():
            raise SystemExit(f"working extraction file is missing: {path}")
        return path.read_bytes()

    return _source_evidence_fnv64(release, repo_root, read_working_file)


def build_source_manifest(
    release: dict[str, Any], repo_root: Path, content_revision: str
) -> dict[str, Any]:
    extraction = release["extraction"]
    if extraction.get("status") != "retained":
        raise SystemExit(
            f"release {release['map_version']}/{release['revision']} has no retained extraction"
        )
    git_tree = extraction["git_tree"]
    value = _source_evidence_fnv64(
        release,
        repo_root,
        lambda relative_path: _retained_file_bytes(repo_root, git_tree, relative_path),
    )

    return {
        "schema_version": 2,
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
        "--check", type=Path,
        help="verify an existing generated artifact instead of printing it",
    )
    parser.add_argument(
        "--content-revision",
        help="required with --kind source-manifest (for example cf-native-dev-slice-r4)",
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
    if args.check is not None:
        committed = json.loads(args.check.read_text(encoding="utf-8"))
        if committed != output:
            raise SystemExit(f"generated runtime catalog is stale: {args.check}")
    else:
        print(json.dumps(output, indent=2, separators=(",", ": "), sort_keys=False))


if __name__ == "__main__":
    main()
