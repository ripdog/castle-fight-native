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


def unit_targets(value: str) -> str:
    tokens = set(value.split(","))
    if "structure" in tokens:
        raise ValueError("native unit-only proc primitive cannot silently drop structure targets")
    # Current promoted combat units are non-hero, non-ward entities. Do not accept new
    # class/team restrictions until the native target primitive can express them.
    supported = {"air", "ground", "enemy", "enemies", "neutral", "ward", "nonhero"}
    if tokens - supported:
        raise ValueError(f"unsupported native proc target constraints: {sorted(tokens - supported)}")
    movement = tokens & {"air", "ground"}
    if movement == {"air", "ground"}:
        return "air-ground-units"
    if movement == {"air"}:
        return "air-units"
    if movement == {"ground"}:
        return "ground-units"
    raise ValueError(f"unsupported native proc targets: {value!r}")


def project_effect(recipe: dict[str, str], fields: dict[str, str],
                   unit: dict[str, str] | None, protected: dict[str, str],
                   mechanics: dict[str, Any]) -> dict[str, Any]:
    def number(field: str, scale: int = 1) -> int:
        return scaled(fields[field.lower()], scale)

    kind = recipe["kind"]
    effect: dict[str, Any] = dict(recipe)
    if kind == "evasion":
        effect["chance_per_10k"] = number("DataA1", 10_000)
    elif kind == "spell-resistance":
        effect["damage_taken_per_10k"] = 10_000 - number("DataB1", 10_000)
    elif kind == "critical-strike":
        effect.update(chance_per_10k=number("DataA1", 100),
                      damage_multiplier_per_10k=number("DataB1", 10_000),
                      targets=unit_targets(fields["targs1"]))
    elif kind == "feedback":
        if number("DataA1") != number("DataC1") or number("DataB1", 10_000) != number("DataD1", 10_000):
            raise ValueError("class-specific Feedback requires a richer native primitive")
        effect.update(maximum_mana_drained=number("DataC1"), damage_per_mana_per_10k=number("DataD1", 10_000),
                      summoned_damage=number("DataE1"), targets=unit_targets(fields["targs1"]))
    elif kind == "faerie-fire":
        if unit is None or number("DataB1") != 1:
            raise ValueError("Faerie Fire requires a mana source and Always Autocast")
        if unit_targets(fields["targs1"]) != "air-ground-units":
            raise ValueError("Faerie Fire needs native coverage for this target mask")
        effect.update(mana_maximum=scaled(unit["mana_max"]), mana_starting=scaled(unit["mana_start"]),
                      mana_regen_per_second_per_10k=scaled(unit["mana_regen"], 10_000),
                      mana_cost=scaled(protected.get("mana_cost", fields["cost1"])),
                      cooldown_millis=scaled(protected.get("cooldown", fields["cool1"]), 1000),
                      range_world=number("Rng1"), armor_reduction_per_100=number("DataA1", 100),
                      duration_millis=number("Dur1", 1000), hero_duration_millis=number("HeroDur1", 1000))
    elif kind == "bash":
        effect.update(chance_per_10k=number("DataA1", 100),
                      bonus_damage=number("DataC1"),
                      stun_duration_millis=number("Dur1", 1000),
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


def build_tuning(release: dict[str, Any], repo_root: Path, recipes: dict[str, Any]) -> dict[str, Any]:
    if recipes["schema_version"] != 1:
        raise ValueError("unsupported native recipe schema")
    tree = release["extraction"]["git_tree"]
    def retained(path: str) -> bytes:
        return catalog._retained_file_bytes(repo_root, tree, path)

    abilities: dict[str, dict[str, str]] = {}
    for row in rows(retained("resolved/object-fields.tsv")):
        if row["category"] == "abilities" and row["level"] == "1":
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
        effects.append(project_effect(recipe, abilities[recipe["source_key"]], unit,
                                      protected.get(recipe["source_key"], {}),
                                      mechanics.get(recipe.get("unit_rawcode", ""), {})))
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
