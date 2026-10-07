#!/usr/bin/env python3
"""Project retained ability evidence into native tuning; recipes contain identities, not values."""
from __future__ import annotations

import argparse
import csv
import io
import json
from decimal import Decimal
from pathlib import Path
from typing import Any

import build_runtime_catalog as catalog

# These are native Warcraft Frost Armor semantics, not Castle Fight map overrides.
# Keep them separate from the map's Frost attack constants (a different mechanic).
FROST_ARMOR_MOVEMENT_DELTA = -50
FROST_ARMOR_ATTACK_SPEED_DELTA = -25


def rows(data: bytes) -> list[dict[str, str]]:
    return list(csv.DictReader(io.StringIO(data.decode("utf-8")), delimiter="\t"))


def scalar(value: str) -> str:
    return value.strip('"')


def scaled(value: str | int | float, scale: int = 1) -> int:
    number = Decimal(str(value)) * scale
    if not number.is_finite() or number != number.to_integral_value():
        raise ValueError(f"value {value!r} cannot be represented exactly at scale {scale}")
    return int(number)


def unit_targets(value: str, *, allow_structures: bool = False) -> str:
    tokens = set(value.split(","))
    if "structure" in tokens and not allow_structures:
        raise ValueError("native unit-only proc primitive cannot silently drop structure targets")
    # Legacy child-proc restrictions are separately audited; directed Bash/Crit/
    # Feedback projections below reject class flattening now heroes are modeled.
    supported = {"air", "ground", "enemy", "enemies", "neutral", "ward", "nonhero"}
    if allow_structures:
        supported.add("structure")
    if tokens - supported:
        raise ValueError(f"unsupported native proc target constraints: {sorted(tokens - supported)}")
    movement = tokens & {"air", "ground"}
    if movement == {"air", "ground"}:
        return "air-ground-units-and-buildings" if "structure" in tokens else "air-ground-units"
    if movement == {"air"}:
        return "air-units"
    if movement == {"ground"}:
        return "ground-units-and-buildings" if "structure" in tokens else "ground-units"
    raise ValueError(f"unsupported native proc targets: {value!r}")


def project_effect(recipe: dict[str, str], fields: dict[str, str],
                   unit: dict[str, str] | None, protected: dict[str, str],
                   mechanics: dict[str, Any]) -> dict[str, Any]:
    def number(field: str, scale: int = 1) -> int:
        return scaled(fields[field.lower()], scale)

    kind = recipe["kind"]
    effect: dict[str, Any] = dict(recipe)
    if kind in {"war-stomp", "howl-of-terror"}:
        return project_scripted_area(recipe, fields, unit, protected, mechanics)
    if kind in {"healing-wave", "solar-strike", "phoenix-fire"}:
        return project_elven_automatic(recipe, fields, unit, protected, mechanics)
    if kind in {"bash", "critical-strike", "feedback", "faerie-fire"}:
        supported = {"air", "ground", "enemy", "enemies", "neutral"}
        if kind == "critical-strike": supported.add("structure")
        if set(fields["targs1"].split(",")) - supported:
            raise ValueError("native directed passive/debuff cannot flatten class restrictions")
    if kind == "evasion":
        effect["chance_per_10k"] = number("DataA1", 10_000)
    elif kind == "spell-resistance":
        effect["damage_taken_per_10k"] = 10_000 - number("DataB1", 10_000)
    elif kind == "cleave":
        tokens = set(fields["targs1"].split(","))
        if tokens - {"ground", "enemy", "enemies", "neutral", "structure"} or "ground" not in tokens:
            raise ValueError("cleave requires an explicitly supported ground target mask")
        effect.update(radius_world=number("Area1"), damage_per_10k=number("DataA1", 10_000),
                      targets=1 | (4 if "structure" in tokens else 0))
    elif kind == "critical-strike":
        effect.update(chance_per_10k=number("DataA1", 100),
                      damage_multiplier_per_10k=number("DataB1", 10_000),
                      targets=unit_targets(fields["targs1"], allow_structures=True))
    elif kind == "pulverize":
        if set(fields["targs1"].split(",")) - {"ground", "enemy", "enemies", "neutral"}:
            raise ValueError("unsupported native Pulverize mask")
        effect.update(chance_per_10k=number("DataA1", 100), damage=number("DataB1"),
            full_radius_world=number("DataC1"), half_radius_world=number("DataD1"), targets=unit_targets(fields["targs1"]))
    elif kind == "frost-attack":
        if set(fields["targs1"].split(",")) - {"air", "ground", "enemy", "enemies", "neutral"}:
            raise ValueError("Frost Attack cannot discard target class restrictions")
        effect.update(duration_millis=number("Dur1", 1000), hero_duration_millis=number("HeroDur1", 1000),
            movement_percent_delta=-scaled(mechanics["misc"]["FrostMoveSpeedDecrease"], 100),
            attack_speed_percent_delta=-scaled(mechanics["misc"]["FrostAttackSpeedDecrease"], 100),
            targets=unit_targets(fields["targs1"]))
    elif kind == "feedback":
        if number("DataA1") != number("DataC1") or number("DataB1", 10_000) != number("DataD1", 10_000):
            raise ValueError("class-specific Feedback requires a richer native primitive")
        effect.update(maximum_mana_drained=number("DataC1"), damage_per_mana_per_10k=number("DataD1", 10_000),
                      summoned_damage=number("DataE1"), targets=unit_targets(fields["targs1"]))
    elif kind == "faerie-fire":
        if unit is None or number("DataB1") not in {0, 1}:
            raise ValueError("Faerie Fire requires a mana source and a boolean Always Autocast")
        if unit_targets(fields["targs1"]) != "air-ground-units":
            raise ValueError("Faerie Fire needs native coverage for this target mask")
        effect.update(mana_maximum=scaled(unit["mana_max"]), mana_starting=scaled(unit["mana_start"]),
                      mana_regen_per_second_per_10k=scaled(unit["mana_regen"], 10_000),
                      mana_cost=scaled(protected.get("mana_cost", fields["cost1"])),
                      cooldown_millis=scaled(protected.get("cooldown", fields["cool1"]), 1000),
                      range_world=number("Rng1"), armor_reduction_per_100=number("DataA1", 100),
                      duration_millis=number("Dur1", 1000), hero_duration_millis=number("HeroDur1", 1000),
                      always_autocast=bool(number("DataB1")))
    elif kind == "bash":
        if any(number(field) != 0 for field in ("DataB1", "DataD1", "DataE1")):
            raise ValueError("Bash multiplier/miss/never-miss fields require explicit native coverage")
        effect.update(chance_per_10k=number("DataA1", 100),
                      bonus_damage=number("DataC1"),
                      stun_duration_millis=number("Dur1", 1000),
                      hero_stun_duration_millis=number("HeroDur1", 1000),
                      targets=unit_targets(fields["targs1"]))
    elif kind == "defend":
        effect.update(ranged_damage_taken_per_10k=number("DataA1", 10_000),
                      spell_damage_taken_per_10k=number("DataE1", 10_000),
                      deflect_chance_per_10k=number("DataF1", 100),
                      deflected_pierce_damage_taken_per_10k=number("DataG1", 10_000),
                      activation_delay_millis=scaled(mechanics["initial_defend_delay_seconds"], 1000))
    elif kind == "orb-spell-proc":
        chances = [number(field, 100) for field in ("DataB1", "DataC1", "DataD1")]
        if len(set(chances)) != 1:
            raise ValueError("class-specific orb chances require a richer native primitive")
        effect.update(chance_per_10k=chances[0], effect_ability_rawcode=fields["unitid1"],
                      targets=unit_targets(fields["targs1"]))
    elif kind == "chain-lightning":
        effect.update(initial_damage=number("DataA1"), maximum_targets=number("DataB1"),
                      jump_radius_world=number("Area1"),
                      damage_reduction_per_10k=number("DataC1", 10_000),
                      targets=unit_targets(fields["targs1"]))
    elif kind == "entangling-roots":
        effect.update(damage_per_second=number("DataA1"), duration_millis=number("Dur1", 1000),
                      hero_duration_millis=number("HeroDur1", 1000),
                      nonhero_only="nonhero" in fields["targs1"].split(","),
                      targets=unit_targets(fields["targs1"]))
    elif kind == "burning-oil":
        tokens = set(fields["targs1"].split(","))
        if number("DataE1") != 1 or "air" in tokens:
            raise ValueError("Burning Oil needs native coverage for this target/reduction profile")
        effect.update(radius_world=number("Area1"), full_damage=number("DataA1"),
                      full_interval_millis=number("DataB1", 1000), half_damage=number("DataC1"),
                      half_interval_millis=number("DataD1", 1000),
                      full_duration_millis=number("HeroDur1", 1000),
                      total_duration_millis=number("Dur1", 1000),
                      target_ground_units="ground" in tokens, target_buildings="structure" in tokens)
    elif kind == "frost-armor":
        if unit is None:
            raise ValueError("Frost Armor must identify its mana profile source")
        effect.update(mana_maximum=scaled(unit["mana_max"]), mana_starting=scaled(unit["mana_start"]),
                      mana_regen_per_second_per_10k=scaled(unit["mana_regen"], 10_000),
                      mana_cost=scaled(protected.get("mana_cost", fields["cost1"])),
                      cooldown_millis=scaled(protected.get("cooldown", fields["cool1"]), 1000),
                      range_world=number("Rng1"), armor_bonus_per_100=number("DataB1", 100),
                      armor_duration_millis=number("DataA1", 1000),
                      slow_duration_millis=number("Dur1", 1000),
                      movement_percent_delta=FROST_ARMOR_MOVEMENT_DELTA,
                      attack_speed_percent_delta=FROST_ARMOR_ATTACK_SPEED_DELTA)
    else:
        raise ValueError(f"no native tuning projection for {kind!r}")
    effect["provenance"] = {"ability": recipe["source_key"], "source": "resolved/object-fields.tsv"}
    if kind == "defend":
        effect["provenance"]["script"] = "resolved/production-unit-special-mechanics.tsv"
    if kind in {"frost-armor", "faerie-fire"}:
        effect["provenance"]["unit"] = "resolved/units.tsv"
        effect["provenance"]["protected"] = "resolved/protected-ability-fields.tsv"
    return effect


def project_elven_automatic(recipe: dict[str, str], fields: dict[str, str], unit: dict[str, str] | None,
                            protected: dict[str, str], mechanics: dict[str, Any]) -> dict[str, Any]:
    if unit is None:
        raise ValueError("native automatic spell needs a retained mana source")
    source = recipe["source_key"]
    effect_key = mechanics.get("effect_key", source)
    effect_fields = mechanics.get("effect_fields", fields)
    def field(name: str, scale: int = 1) -> int:
        return scaled(effect_fields[name.lower()], scale)
    def fourcc(value: str) -> int:
        return int.from_bytes(value.encode("ascii"), "big")
    def ticks(value: str | int | float) -> int:
        # Native deadlines use ceiling to the first observable simulation tick.
        value = Decimal(str(value)) * 30
        return int(value.to_integral_value(rounding="ROUND_CEILING"))
    rate = scaled(unit["mana_regen"] if unit["mana_regen"].strip() not in {"-", "", "_"} else 0, 10_000)
    def mana(name: str) -> int:
        value = unit[name].strip()
        return 0 if value in {"-", "", "_"} else scaled(value)
    # Wire-compatible ManaProfile::per_second encoding (bit 31 distinguishes the
    # rate unit). Keep the source rate exact; dividing by Hz would lose 1/s and 3.5/s.
    if not 0 <= rate <= 10_000_000:
        raise ValueError("native mana regeneration is outside the validated per-second range")
    mana_profile = {"maximum": mana("mana_max"), "starting": mana("mana_start"), "regen_per_tick_per_10k": (1 << 31) | rate}
    kind = recipe["kind"]
    if effect_key != source or recipe["source_kind"] == "ability-effect":
        child_protected = mechanics.get("effect_protected", protected)
        if scaled(child_protected.get("mana_cost", effect_fields.get("cost1", 0))) != 0 or Decimal(child_protected.get("cooldown", effect_fields.get("cool1", 0))) != 0:
            raise ValueError("automatic proxy requires independent resource state for a non-free child")
    if kind == "healing-wave":
        delay = json.loads(mechanics["mechanics_row"]["scheduled_delays_json"])
        if len(delay) != 1:
            raise ValueError("Healing Wave recovery needs one resolved script deadline")
        effect = {"HealingWave": {"ability": fourcc(effect_key), "healing": field("DataA1"),
            "trigger_healing": 0 if recipe["source_kind"] == "ability-effect" else scaled(fields["dataa1"]),
            "maximum_targets": field("DataB1"), "jump_radius": field("Area1", 1024),
            "retention_per_10k": 10_000 - field("DataC1", 10_000), "recovery_ticks": ticks(delay[0])}}
        policy = "WoundedFriendlyUnit"
    else:
        tokens = set(effect_fields["targs1"].split(","))
        mask = (1 if "ground" in tokens else 0) | (2 if "air" in tokens else 0) | (4 if "structure" in tokens else 0)
        bolt = {"ability": fourcc(effect_key), "damage": field("DataA1"),
            "stun_ticks": ticks(effect_fields["dur1"]) if kind == "solar-strike" else 0,
            "hero_stun_ticks": ticks(effect_fields["herodur1"]) if kind == "solar-strike" else 0,
            "damage_per_second": field("DataB1") if kind == "phoenix-fire" else 0,
            "duration_ticks": ticks(effect_fields["dur1"]),
            # World motion uses integer subunits/tick, like ordinary imported missiles.
            "speed_per_tick": int(Decimal(effect_fields["missilespeed"]) * 1024 / 30),
            "cleanse": False, "targets": mask}
        if kind == "solar-strike":
            parameters = json.loads(mechanics["semantics_row"]["parameters_json"])
            effect = {"SolarStrike": {"profile": bolt, "radius": scaled(parameters["search_radius"], 1024),
                "maximum_targets": parameters["max_targets"]}}
            policy = "FlyingEnemyUnit"
        else:
            effect = {"PhoenixFire": bolt}
            policy = "RandomEnemyUnitOrBuilding"
    child = recipe["source_kind"] == "ability-effect"
    profile = {"mana": mana_profile, "ability": {"id": fourcc(source),
        "mana_cost": 0 if child else scaled(protected.get("mana_cost", fields.get("cost1", 0))),
        "cooldown_ticks": 0 if child else ticks(protected.get("cooldown", fields.get("cool1", 0))),
        "range": 0 if child else scaled(fields["area1"] if kind == "phoenix-fire" else fields["rng1"], 1024),
        "target_policy": policy, "effect": effect}}
    return {"kind": "elven-automatic", "source_kind": recipe["source_kind"], "source_key": source,
        "unit_rawcode": recipe["unit_rawcode"], "spellcasting": profile,
        "mana_regen_per_second_per_10k": rate,
        "effect_ability_rawcode": effect_key if effect_key != source else None,
        "provenance": {"source": "resolved/object-fields.tsv", "unit": "resolved/units.tsv",
            "protected": "resolved/protected-ability-fields.tsv", "script": "resolved/unit-spell-mechanics.tsv"}}


def project_scripted_area(recipe: dict[str, str], fields: dict[str, str], unit: dict[str, str] | None,
                          protected: dict[str, str], mechanics: dict[str, Any]) -> dict[str, Any]:
    if unit is None:
        raise ValueError("scripted area spell needs a retained mana source")
    child = mechanics["effect_key"]
    child_fields = mechanics["effect_fields"]
    child_protected = mechanics["effect_protected"]
    if scaled(child_protected.get("mana_cost", child_fields["cost1"])) != 0 or scaled(child_protected.get("cooldown", child_fields["cool1"])) != 0:
        raise ValueError("scripted area child requires separate non-free resource state")
    if recipe["kind"] == "war-stomp" and set(child_fields["targs1"].split(",")) != {"ground"}:
        raise ValueError("War Stomp needs explicit coverage for this effect mask")
    row = mechanics["mechanics_row"]
    if row["mechanic_kind"] != "dummy-immediate-ability-from-caster" or json.loads(row["scheduled_delays_json"]):
        raise ValueError("area spell requires immediate caster-centered source control flow")
    def ticks(value: str) -> int:
        return int((Decimal(value) * 30).to_integral_value(rounding="ROUND_CEILING"))
    def fourcc(value: str) -> int:
        return int.from_bytes(value.encode("ascii"), "big")
    def mana(name: str, scale: int = 1) -> int:
        return 0 if unit[name].strip() in {"-", "", "_"} else scaled(unit[name], scale)
    rate = mana("mana_regen", 10_000)
    child_only = recipe["source_kind"] == "ability-effect"
    if recipe["kind"] == "war-stomp":
        effect = {"AreaStun": {"ability": fourcc(child), "damage": scaled(child_fields["dataa1"]),
            "radius": scaled(child_fields["area1"], 1024), "stun_ticks": ticks(child_fields["dur1"]),
            "hero_stun_ticks": ticks(child_fields["herodur1"]), "targets": 1}}
    if recipe["kind"] == "howl-of-terror":
        if set(child_fields["targs1"].split(",")) != {"air", "ground", "enemy", "neutral"} or scaled(child_fields["datac1"]) != 0:
            raise ValueError("unsupported Howl effect constraints")
        effect = {"AreaDebuff": {"ability": fourcc(child), "radius": scaled(child_fields["area1"], 1024),
            "armor_delta_per_100": -scaled(child_fields["datab1"], 100), "damage_delta_per_10k": -scaled(child_fields["dataa1"], 10_000),
            "duration_ticks": ticks(child_fields["dur1"]), "hero_duration_ticks": ticks(child_fields["herodur1"]), "targets": 3}}
    profile = {"mana": {"maximum": mana("mana_max"), "starting": mana("mana_start"),
        "regen_per_tick_per_10k": (1 << 31) | rate}, "ability": {"id": fourcc(recipe["source_key"]),
        "mana_cost": 0 if child_only else scaled(protected.get("mana_cost", fields["cost1"])),
        "cooldown_ticks": 0 if child_only else ticks(protected.get("cooldown", fields["cool1"])),
        "range": 0 if child_only else scaled(fields["rng1"], 1024),
        "target_policy": "RandomGroundEnemyUnit", "effect": effect}}
    return {"kind": "scripted-automatic", "source_kind": recipe["source_kind"], "source_key": recipe["source_key"],
        "unit_rawcode": recipe["unit_rawcode"], "spellcasting": profile, "mana_regen_per_second_per_10k": rate,
        "effect_ability_rawcode": None if child_only else child,
        "provenance": {"source": "resolved/object-fields.tsv", "unit": "resolved/units.tsv",
            "protected": "resolved/protected-ability-fields.tsv", "script": "resolved/unit-spell-mechanics.tsv"}}


def build_tuning(release: dict[str, Any], repo_root: Path, recipes: dict[str, Any]) -> dict[str, Any]:
    if recipes["schema_version"] != 1:
        raise ValueError("unsupported native recipe schema")
    tree = release["extraction"]["git_tree"]
    def retained(path: str) -> bytes:
        return catalog._retained_file_bytes(repo_root, tree, path)

    abilities: dict[str, dict[str, str]] = {}
    for row in rows(retained("resolved/object-fields.tsv")):
        if row["category"] == "abilities" and row["level"] in {"0", "1"}:
            field = row["source_field"].lower()
            if field:
                abilities.setdefault(row["rawcode"], {})[field] = scalar(row["recovered_value_json"])
    units = {row["rawcode"]: row for row in rows(retained("resolved/units.tsv"))}
    protected: dict[str, dict[str, str]] = {}
    for row in rows(retained("resolved/protected-ability-fields.tsv")):
        if row["level"] == "1":
            protected.setdefault(row["rawcode"], {})[row["field"]] = row["runtime_value"]
    mechanics = {
        row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "automatic-defend-state-maintenance"
    }
    spell_mechanics = {row["ability_rawcode"]: row for row in rows(retained("resolved/unit-spell-mechanics.tsv"))}
    spell_semantics = {row["ability_rawcode"]: row for row in rows(retained("resolved/unit-spell-semantics.tsv"))}
    misc = dict(line.split("=", 1) for line in retained("war3mapMisc.txt").decode().splitlines() if "=" in line and not line.startswith("//"))
    effects = []
    seen = set()
    for recipe in recipes["effects"]:
        allowed = {"kind", "source_kind", "source_key", "unit_rawcode"}
        if set(recipe) - allowed:
            raise ValueError("native recipes may contain identities only, not tuning values")
        if recipe["source_kind"] not in {"unit-ability", "ability-effect"}:
            raise ValueError("unsupported native recipe source kind")
        key = (recipe["source_kind"], recipe["source_key"])
        if key in seen:
            raise ValueError(f"duplicate recipe source: {key}")
        seen.add(key)
        unit = units.get(recipe.get("unit_rawcode", ""))
        if recipe["source_kind"] == "unit-ability":
            if unit is None or recipe["source_key"] not in unit["abilities"].split(","):
                raise ValueError(f"recipe {key} is not in its source unit's extracted ability inventory")
        source = recipe["source_key"]
        spell = spell_mechanics.get(source)
        if spell is None and recipe["kind"] in {"healing-wave", "solar-strike", "war-stomp", "howl-of-terror"}:
            spell = next((row for row in spell_mechanics.values() if source in row["direct_map_rawcodes"].split(",")
                or source in row["reachable_map_objects_json"]), None)
        detail = dict(mechanics.get(recipe.get("unit_rawcode", ""), {}))
        detail["misc"] = misc
        if spell is not None:
            semantics = spell_semantics[spell["ability_rawcode"]]
            effect_key = semantics["effect_rawcodes"].split(",")[0]
            detail.update(effect_key=effect_key, effect_fields=abilities[effect_key],
                effect_protected=protected.get(effect_key, {}), mechanics_row=spell, semantics_row=semantics)
        effects.append(project_effect(recipe, abilities[source], unit, protected.get(source, {}), detail))
    return {"schema_version": 2, "map_version": release["map_version"], "effects": effects}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--map-version", required=True)
    parser.add_argument("--revision", required=True)
    parser.add_argument("--check", type=Path)
    args = parser.parse_args()
    release = catalog._load_release(catalog.DEFAULT_RELEASES, args.map_version, args.revision)
    root = catalog.REPO_ROOT
    recipe_path = root / release["runtime_content"]["path"] / "native-effect-recipes.json"
    result = build_tuning(release, root, json.loads(recipe_path.read_text(encoding="utf-8")))
    if args.check:
        if json.loads(args.check.read_text(encoding="utf-8")) != result:
            raise SystemExit(f"generated native tuning is stale: {args.check}")
    else:
        print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
