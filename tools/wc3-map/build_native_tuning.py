#!/usr/bin/env python3
"""Project retained ability evidence into native tuning; recipes contain identities, not values."""
from __future__ import annotations

import argparse
import csv
import io
import json
import re
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


def buff_identity(fields: dict[str, str], *, positive: bool) -> dict[str, Any]:
    buff = fields["buffid1"].split(",")[0]
    if len(buff) != 4:
        raise ValueError("native transferable effect requires a buff identity")
    return {"rawcode": int.from_bytes(buff.encode("ascii"), "big"),
        "positive": positive, "stealable": fields.get("hero", "0") == "0",
        "organic_only": not positive or "organic" in fields["targs1"].split(",")}


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
    if tokens == {"enemies", "structure"} and allow_structures:
        return "buildings"
    raise ValueError(f"unsupported native proc targets: {value!r}")


def hostile_spell_bonus(recipe: dict[str, Any], fields: dict[str, str], mechanics: dict[str, Any]) -> int:
    """Script-owned non-attack vulnerability tied to the aura's own buff, or zero without a trace."""
    trace = mechanics.get("system")
    if trace is None:
        return 0
    if (trace["aura_ability_rawcode"] != recipe["source_key"] or trace["aura_buff_rawcodes"] != [fields["buffid1"]]
            or not trace["positive_damage_only"] or not trace["requires_target_buff_presence"]
            or trace["excluded_damage_event_semantics"] != "attack-damage"
            or not trace["multiple_source_buildings_do_not_stack_script_multiplier"]):
        raise ValueError("hostile aura spell vulnerability trace changed")
    return int((Decimal(str(trace["damage_multiplier"])) - 1) * 10000)


def self_destruct_profile(source: str, fields: dict[str, str], base: str) -> dict[str, Any]:
    if base != "Asds" or set(fields["targs1"].split(",")) != {"air", "enemies", "ward", "structure", "debris"}:
        raise ValueError("self-destruct requires audited native air/structure/debris targeting")
    if any(scaled(fields[key]) != 0 for key in ("rng1", "cool1", "cost1")) or scaled(fields["dataf1"]) != 1:
        raise ValueError("self-destruct requires free contact autocast and corpse explosion flag")
    return {"ability": int.from_bytes(source.encode(), "big"),
        "full_radius": scaled(fields["dataa1"], 1024), "full_damage": scaled(fields["datab1"]),
        "partial_radius": scaled(fields["datac1"], 1024), "partial_damage": scaled(fields["datad1"]),
        "building_damage_per_10k": scaled(fields["datae1"], 10_000), "targets": 6}


def damage_charge_profile(recipe: dict, unit: dict, mechanics: dict) -> dict:
    trace, handler = mechanics["damage_charge"], mechanics["damage_charge_handler"]
    marker = int.from_bytes(recipe["source_key"].encode(), "big")
    shape = (r'function vampireCharge\(vpq,wpq\)local xpq=nil local ypq=nil local zpq=nil '
        r'xpq=\(unit_getState\(vpq,UNIT_STATE_MANA\)\+wpq\)if\(xpq>=([0-9.]+)\)then '
        r'unit_removeAbility\(vpq,([0-9]+)\)__wurst_safe_SetUnitMoveSpeed\(vpq,([0-9.]+)\)'
        r'addProtectedAbility\(vpq,([0-9]+)\)jY=true unit_issueImmediateOrderById\(vpq,852041\)'
        r'else ypq=UNIT_STATE_MANA zpq=xpq __wurst_safe_SetUnitState\(vpq,ypq,zpq\)end end')
    matched = re.fullmatch(shape, handler.strip())
    if matched is None or int(matched[2]) != marker or int(matched[4]) != trace["kaboom_ability_id"]:
        raise ValueError("damage charge requires exact native controller control flow")
    threshold = re.search(r'if\(xpq>=([0-9.]+)\)then', handler)
    speed = re.search(r'SetUnitMoveSpeed\(vpq,([0-9.]+)\)', handler)
    expected = ('unit_removeAbility(vpq,' + str(marker) + ')')
    if (trace["charge_resource"] != "mana" or trace["charge_delta"] != "event-damage-amount"
        or not trace["remove_damage_marker_on_threshold"] or trace["kaboom_order_id"] != 852041
        or threshold is None or speed is None or expected not in handler
        or 'xpq=(unit_getState(vpq,UNIT_STATE_MANA)+wpq)' not in handler
        or 'unit_issueImmediateOrderById(vpq,852041)' not in handler
        or 'else ypq=UNIT_STATE_MANA zpq=xpq __wurst_safe_SetUnitState(vpq,ypq,zpq)' not in handler):
        raise ValueError("damage charge requires exact marker removal, native autocast and no threshold mana write")
    if scaled(threshold[1]) != trace["detonation_threshold"] or scaled(speed[1]) != trace["detonation_move_speed"]:
        raise ValueError("damage charge script disagrees with retained controller projection")
    child = trace["kaboom_ability_id"].to_bytes(4, "big").decode()
    if f'addProtectedAbility(vpq,{trace["kaboom_ability_id"]})' not in handler or unit["move_type"] != "fly":
        raise ValueError("damage charge requires its native child grant and flying order controller")
    return {"marker": marker, "mana_maximum": scaled(unit["mana_max"]),
        "mana_starting": 0 if unit["mana_start"].strip() in {"-", "_"} else scaled(unit["mana_start"]),
        "mana_regen_per_second_per_10k": scaled(unit["mana_regen"], 10_000),
        "threshold": scaled(threshold[1]), "armed_speed_per_tick": int(Decimal(speed[1]) * 1024 / 30),
        "explosion": self_destruct_profile(child, mechanics["charge_child_fields"], mechanics["charge_child_base"])}


def project_effect(recipe: dict[str, str], fields: dict[str, str],
                   unit: dict[str, str] | None, protected: dict[str, str],
                   mechanics: dict[str, Any]) -> dict[str, Any]:
    def number(field: str, scale: int = 1) -> int:
        return scaled(fields[field.lower()], scale)

    kind = recipe["kind"]
    if kind == "death-explosion":
        from project_death_explosion import project
        return project(recipe, fields, mechanics["native_base"])
    if kind == "mana-attack-orb":
        from project_mana_attack_orb import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "incinerate":
        from project_incinerate import project
        return project(recipe, fields, mechanics["native_base"])
    if kind == "hostile-armor-aura":
        if (mechanics["native_base"] != "AHad" or set(fields["targs1"].split(",")) - {"ward"} != {"air", "enemies", "ground", "vulnerable"}
                or number("DataB1") != 0 or number("DataA1", 100) >= 0
                or number("Dur1", 30) != number("HeroDur1", 30) or len(fields["buffid1"]) != 4):
            raise ValueError("hostile Devotion requires its native vulnerable unit mask, flat armor and shared family")
        return {**recipe, "profile": {"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "buff_rawcode": int.from_bytes(fields["buffid1"].encode(), "big"), "radius": number("Area1", 1024),
            "armor_bonus_per_100": number("DataA1", 100), "targets": 3,
            "linger_ticks": number("Dur1", 30),
            "spell_damage_taken_bonus_per_10k": hostile_spell_bonus(recipe, fields, mechanics)}}
    if kind == "permanent-immolation":
        if (mechanics['native_base'] not in {'ANpi', 'ACpi'}
            or set(fields['targs1'].split(',')) not in ({'air', 'ground', 'enemies', 'neutral', 'organic'}, {'air', 'ground', 'enemies', 'neutral'})
            or any(number(key) != 0 for key in ['Cost1', 'Cool1', 'DataB1', 'DataC1'])
            or number('Dur1', 30) <= 0 or number('DataA1') < 0):
            raise ValueError('permanent immolation requires the passive free native pulse')
        return {**recipe, 'profile': {
            'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
            'buff_rawcode': buff_identity(fields, positive=False)['rawcode'],
            'radius': number('Area1', 1024), 'damage': number('DataA1'),
            'interval_ticks': number('Dur1', 30), 'targets': 3, 'organic_only': 'organic' in fields['targs1'].split(',')},
            'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': mechanics['native_base']}}
    if kind == "death-spawn":
        if mechanics["native_base"] != "Aspy" or any(number(key) != 0 for key in ("Cost1", "Cool1", "Dur1", "HeroDur1")):
            raise ValueError("death spawn requires the passive permanent native Aspy inventory")
        child = fields["datab1"]
        if len(child) != 4 or not 0 < number("DataA1") <= 255:
            raise ValueError("death spawn requires a child identity and positive count")
        return {**recipe, "child_rawcode": int.from_bytes(child.encode("ascii"), "big"), "count": number("DataA1"),
            "provenance": {"source": "resolved/object-fields.tsv", "native_base": "Aspy"}}
    if kind == "native-heal":
        from project_native_heal import project
        return project(recipe, fields, unit, protected, mechanics)
    effect: dict[str, Any] = dict(recipe)
    if kind in {"building-weather", "native-weather-wave"}:
        from project_weather import project
        return project(recipe, fields, unit, protected, mechanics, scaled)
    if kind in {"building-enchantment", "native-enchantment-grant", "source-damage-dispatcher"}:
        from project_enchantment import project
        return project(recipe, fields, unit, protected, mechanics, scaled)
    if kind in {"building-improvement", "building-mana-regeneration"}:
        from project_mana_improvement import project
        return project(recipe, fields, mechanics, scaled)
    if kind == "health-scaled-aftershock":
        from project_aftershock import project
        return project(recipe, mechanics, scaled)
    if kind in {"damage-retaliation", "retaliation-projectile"}:
        from project_damage_retaliation import project
        return project(recipe, mechanics, scaled)
    if kind == "forked-lightning":
        if mechanics["native_base"] != "ACfl" or scaled(protected["mana_cost"]) != 0:
            raise ValueError("forked lightning requires a free protected native effect")
        tokens = set(fields["targs1"].split(","))
        if tokens not in ({"air","ground","enemies","ward"}, {"enemies","structure"}):
            raise ValueError("forked lightning effect target constraints changed")
        effect.update(profile={"ability":int.from_bytes(recipe["source_key"].encode(),"big"),
            "damage":number("DataA1"), "maximum_targets":number("DataB1"),
            "distance":number("DataC1",1024), "initial_radius":number("Area1",1024),
            "final_radius":number("DataD1",1024), "targets":3 if "ground" in tokens else 4})
        return effect
    if kind == "master-projectiles":
        from project_master_projectiles import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "wind-walk":
        from project_wind_walk import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind in {"siren-purge", "feral-spirit"}:
        from project_naga_unit import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "native-training-grant":
        if (mechanics['native_base'] != 'ANde' or fields['targs1'] != 'enemy,structure'
            or any(number(key) != 0 for key in ['DataC1', 'DataD1', 'Cost1', 'Cool1'])):
            raise ValueError('training Demolish requires its passive structure-only native modifier')
        return {**recipe, 'kind': 'native-enchantment-grant',
            'grant': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
                'effect': {'Demolish': {'chance_per_10k': number('DataA1', 100),
                    'multiplier_per_10k': number('DataB1', 10000), 'targets': 4}}}, 'dependencies': [],
            'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': mechanics['native_base']}}
    if kind == "native-target-debuff":
        from project_cripple import project
        return project(recipe, fields, protected, mechanics)
    if kind == "orb-charm":
        from project_orb_charm import project
        return project(recipe, fields, protected, mechanics)
    if kind == "native-debuff":
        from project_native_debuff import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "native-curse":
        from project_curse import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "parasite-area-buff":
        from project_parasite_area_buff import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind in {"impale-fan", "native-impale"}:
        from project_impale import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "travelling-mana-clap":
        from project_travelling_mana_clap import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "fan-of-knives":
        from project_fan_of_knives import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "life-steal":
        from project_life_steal import project
        return project(recipe, fields, unit, mechanics)
    if kind == "parasite-script":
        from project_parasite_script import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "scripted-roots":
        from project_scripted_roots import project
        return project(recipe, fields, unit, protected, mechanics)
    if kind == "lava-spawn":
        trace = mechanics["lava_spawn"]
        if mechanics["native_base"] != "ANlm" or fields["unitid1"] != trace["runtime_combat_unit_rawcode"]:
            raise ValueError("lava-spawn requires its native summoned combat body")
        if (number("DataA1") != 1 or number("Dur1") != 0 or number("DataE1") != 0
                or scaled(protected["mana_cost"]) != 0 or trace["nested_h030_child_replace_method"] != 2
                or trace["trained_proxy_is_gameplay_combat_body"] or not trace["remove_summoned_type_from_h030_child"]):
            raise ValueError("lava-spawn requires the audited permanent one-body summon and default-state child replacement")
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "body_rawcode": trace["runtime_combat_unit_id"], "child_rawcode": trace["nested_h030_child_replacement_unit_id"],
            "attacks_to_split": number("DataC1"), "split_delay_ticks": number("DataB1", 30),
            "maximum_splits": number("DataF1") - 1, "healing_per_10k": number("DataD1", 10_000),
            "order_refresh_ticks": scaled(trace["post_child_attack_order_delay_seconds"], 30),
            "targets": 3}, provenance={"git_tree": mechanics["git_tree"],
                "source": "resolved/production-unit-special-mechanics.tsv", "native_base": "ANlm"})
        if set(fields["targs1"].split(",")) != {"enemy", "neutral", "air", "ground", "ward"}:
            raise ValueError("lava-spawn attack-count target constraints changed")
        return effect
    if kind == "cone-wave":
        base = mechanics["native_base"]
        if base not in {"ACbc", "AUcs"} or (base == "ACbc" and number("DataE1") != 0) or scaled(protected["mana_cost"]) != 0:
            raise ValueError("cone-wave requires audited free native Breath of Fire or Carrion Swarm")
        if int((Decimal(protected["cooldown"]) * 30).to_integral_value(rounding="ROUND_CEILING")) != 1:
            raise ValueError("orb cone-wave child needs at most one tick of native cooldown")
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "damage": number("DataA1"), "maximum_damage": number("DataB1"), "distance": number("DataC1", 1024),
            "initial_radius": number("Area1", 1024), "final_radius": number("DataD1", 1024),
            "speed_per_tick": int(Decimal(fields["missilespeed"]) * 1024 / 30),
            "targets": 3 if base == "ACbc" else 5})
        expected = {"air", "enemies", "ground"} if base == "ACbc" else {"enemies", "ground", "structure"}
        if set(fields["targs1"].split(",")) != expected:
            raise ValueError("cone-wave target constraints changed")
        effect["provenance"] = {"native_base":base, "protected_resources":protected,
            "source":"resolved/object-fields.tsv", "native_child_cooldown_ticks":1}
        return effect
    if kind == "mirror-image":
        trace = mechanics["mirror_image"]
        handler = mechanics["mirror_image_handler"]
        shape = r'function randomN02YOrder\(upq\)if\(GetRandomInt\(0,99\)<(\d+)\)then jY=true unit_issueImmediateOrderById\(upq,(\d+)\)end end'
        match = re.fullmatch(shape, handler.strip())
        if match is None or int(match[1]) != trace["proc_chance_percent"] or int(match[2]) != trace["mirror_image_order_id"]:
            raise ValueError("Mirror Image requires exact damage-event immediate-order control flow")
        if mechanics["native_base"] != "AOmi" or scaled(protected.get("mana_cost", fields["cost1"])) != 0:
            raise ValueError("Mirror Image requires its free protected native ability")
        from decimal import ROUND_CEILING
        split = (Decimal(fields["datad1"]) + Decimal(fields["rng1"]) / Decimal(fields["missilespeed"])) * 30
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "chance_per_10k": scaled(trace["proc_chance_percent"], 100),
            "cooldown_ticks": scaled(protected.get("cooldown", fields["cool1"]), 30),
            "split_ticks": int(split.to_integral_value(rounding=ROUND_CEILING)),
            "lifetime_ticks": scaled(fields["dur1"], 30), "displacement": scaled(fields["rng1"], 1024),
            "images": number("DataA1"), "damage_dealt_per_10k": number("DataB1", 10_000),
            "damage_taken_per_10k": number("DataC1", 10_000)},
            provenance={"git_tree": mechanics["git_tree"], "source": "resolved/production-unit-special-mechanics.tsv",
                "handler": "handleSourceDamageEffects,randomN02YOrder", "native_base": "AOmi"})
        return effect
    if kind == "self-destruct":
        effect.update(profile=self_destruct_profile(recipe["source_key"], fields, mechanics["native_base"]))
        return effect
    if kind == "damage-charge":
        profile = damage_charge_profile(recipe, unit, mechanics)
        effect.update(profile=profile, effect_ability_rawcode=profile["explosion"]["ability"].to_bytes(4, "big").decode(),
            provenance={"git_tree": mechanics["git_tree"], "source": "resolved/production-unit-special-mechanics.tsv",
                "handler": "vampireCharge", "damage_handler": "handleTargetDamageEffects", "phase": "pre-health-commit-reduced-damage-event"})
        return effect
    if kind == "poison":
        mask = set(fields["targs1"].split(","))
        if mask not in ({"air", "ground", "organic"}, {"air", "ground"}) or number("DataD1") != 1:
            raise ValueError("poison requires the audited unit mask and damage-only stacking flags")
        ticks = lambda field: int((Decimal(fields[field]) * 30).to_integral_value(rounding="ROUND_CEILING"))
        if mechanics["native_base"] not in ("Aven", "ACvs", "Aspo"):
            raise ValueError("unsupported native poison base")
        # Resolved metadata labels Poi2/Spo2 as movement and Poi3/Spo3 as attack speed for every base.
        movement_field, attack_field = "DataB1", "DataC1"
        buffs = [code for code in fields["buffid1"].split(",") if code not in ("", "_", "-", "0")]
        buff = buffs[min(1, len(buffs) - 1)]
        # Health resolution is whole points: a sub-point poison rate is a slow-only marker and its
        # fraction is recorded rather than silently rounded.
        rate = abs(Decimal(fields["dataa1"]))
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "buff_rawcode": int.from_bytes(buff.encode(), "big"),
            "damage_per_second": int(rate), "movement_percent_delta": -number(movement_field, 100),
            "attack_speed_percent_delta": -number(attack_field, 100), "duration_ticks": ticks("dur1"),
            "hero_duration_ticks": ticks("herodur1"), "targets": 3, "stack_damage": True, "can_kill": False,
            "organic_only": "organic" in mask},
            provenance={"source": "resolved/object-fields.tsv", "native_base": mechanics["native_base"], "stacking_flags": number("DataD1"),
                "unrepresented_damage_per_second": str(rate - int(rate))})
        return effect
    if kind == "damage-threshold":
        trace = mechanics["damage_threshold"]
        if trace["bonus_ability_rawcode"] != recipe["source_key"] or not trace["levels_only_increase"] or not trace["healing_does_not_downgrade_reached_level"]:
            raise ValueError("damage threshold requires the audited permanent ability controller")
        handler = mechanics["damage_threshold_handler"]
        if f'unit_getTypeId(o8n)=={int.from_bytes(recipe["unit_rawcode"].encode(), "big")}' not in handler:
            raise ValueError("damage threshold handler must identify its source unit")
        branches = re.findall(r'p8n<([0-9.]+)\)then(.*?)(?=elseif|end end end end$)', handler)
        thresholds = []
        for threshold, (ratio, branch) in zip(trace["thresholds"], reversed(branches), strict=True):
            level = threshold["ability_level"]
            bonus = scaled(mechanics["ability_levels"][f"dataa{level}"])
            color = re.search(r'SetUnitVertexColor\([^,]+,(\d+),(\d+),(\d+),(\d+)\)', branch)
            if scaled(ratio, 10_000) != scaled(threshold["hp_ratio_below"], 10_000) or bonus != scaled(threshold["attack_bonus"]) or color is None or list(map(int, color.groups())) != threshold["vertex_rgba"]:
                raise ValueError("damage threshold projection disagrees with script/native evidence")
            adds = "addProtectedAbility(" in branch
            if not adds and f"SetUnitAbilityLevel(" not in branch:
                raise ValueError("damage threshold branch needs an explicit native grant/level operation")
            thresholds.append({"hp_ratio_below_per_10k": scaled(ratio, 10_000), "attack_bonus": bonus,
                "vertex_rgba": list(map(int, color.groups())), "adds_if_absent": adds})
        effect.update(thresholds=thresholds, provenance={"git_tree": mechanics["git_tree"],
            "source": "resolved/production-unit-special-mechanics.tsv", "fields": "resolved/object-fields.tsv",
            "handler": "DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood", "phase": "pre-health-commit-reduced-damage-event"})
        return effect
    if kind == "ensnare":
        return project_ensnare(recipe, fields, unit, protected, mechanics)
    if kind == "kill-berserk":
        trace = mechanics["kill_mechanics"]
        if not (trace["victim_requires_combat_sapper"] and trace["killer_must_not_be_structure"] and trace["resume_attack_immediately"]):
            raise ValueError("native kill Berserk requires its audited death-handler predicates")
        if scaled(protected.get("cooldown", fields["cool1"])) != 0 or scaled(protected.get("mana_cost", fields["cost1"])) != 0:
            raise ValueError("kill Berserk requires explicit native resource support for non-free orders")
        if trace["berserk_ability_id"] != int.from_bytes(recipe["source_key"].encode(), "big") or trace["berserk_base_order"] != "berserk":
            raise ValueError("death handler must issue this native Berserk ability")
        effect.update(duration_ticks=number("Dur1", 30), movement_percent_delta=number("DataA1", 100),
            attack_speed_percent_delta=number("DataB1", 100), damage_taken_bonus_per_10k=number("DataC1", 10_000),
            provenance={"source": "resolved/object-fields.tsv", "protected": "resolved/protected-ability-fields.tsv",
                "death_handler": "resolved/production-unit-special-mechanics.tsv"})
        return effect
    if kind in {"inner-fire", "spell-steal"}:
        if kind == "spell-steal":
            if unit is None or set(fields["targs1"].split(",")) != {"air", "ground", "enemies", "friend", "self", "sapper", "ward"}:
                raise ValueError("Spell Steal requires the audited native unit mask")
            identity = int.from_bytes(recipe["source_key"].encode("ascii"), "big")
            rate = scaled(unit["mana_regen"], 10_000)
            return {"kind": "scripted-automatic", "source_kind": recipe["source_kind"],
                "source_key": recipe["source_key"], "unit_rawcode": recipe["unit_rawcode"],
                "spellcasting": {"mana": {"maximum": scaled(unit["mana_max"]), "starting": scaled(unit["mana_start"]),
                    "regen_per_tick_per_10k": (1 << 31) | rate}, "ability": {"id": identity,
                    "mana_cost": scaled(protected.get("mana_cost", fields["cost1"])),
                    "cooldown_ticks": scaled(protected.get("cooldown", fields["cool1"]), 30),
                    "range": number("Rng1", 1024), "target_policy": "NativeBuffDonor",
                    "effect": {"SpellSteal": {"recipient_radius": number("Area1", 1024)}}}},
                "mana_regen_per_second_per_10k": rate, "effect_ability_rawcode": None,
                "provenance": {"source": "resolved/object-fields.tsv", "unit": "resolved/units.tsv",
                    "protected": "resolved/protected-ability-fields.tsv"}}
        if unit is None or set(fields["targs1"].split(",")) != {"air", "ground", "friend", "neutral", "self"}:
            raise ValueError("Inner Fire requires the audited native friendly unit mask")
        identity = int.from_bytes(recipe["source_key"].encode("ascii"), "big")
        buff = fields["buffid1"]
        if len(buff) != 4:
            raise ValueError("Inner Fire requires a single buff family")
        tick = lambda value: scaled(value, 30)
        rate = scaled(unit["mana_regen"], 10_000)
        return {"kind": "scripted-automatic", "source_kind": recipe["source_kind"],
            "source_key": recipe["source_key"], "unit_rawcode": recipe["unit_rawcode"],
            "spellcasting": {"mana": {"maximum": scaled(unit["mana_max"]), "starting": scaled(unit["mana_start"]),
                "regen_per_tick_per_10k": (1 << 31) | rate}, "ability": {"id": identity,
                "mana_cost": scaled(protected.get("mana_cost", fields["cost1"])),
                "cooldown_ticks": tick(protected.get("cooldown", fields["cool1"])),
                "range": number("Rng1", 1024), "target_policy": "FriendlyUnitInCombat",
                "effect": {"StatBuff": {"modifier": identity,
                    "buff": buff_identity(fields, positive=True),
                    "armor_bonus_per_100": number("DataB1", 100), "damage_bonus_per_10k": number("DataA1", 10_000),
                    "regeneration_per_second_per_10k": number("DataD1", 10_000),
                    "duration_ticks": tick(fields["dur1"]), "hero_duration_ticks": tick(fields["herodur1"]),
                    "autocast_range": number("DataC1", 1024)}}}},
            "mana_regen_per_second_per_10k": rate, "effect_ability_rawcode": None,
            "provenance": {"source": "resolved/object-fields.tsv", "unit": "resolved/units.tsv",
                "protected": "resolved/protected-ability-fields.tsv"}}
    if kind in {"war-stomp", "howl-of-terror", "frost-nova"}:
        return project_scripted_area(recipe, fields, unit, protected, mechanics)
    if kind in {"healing-wave", "solar-strike", "phoenix-fire"}:
        return project_elven_automatic(recipe, fields, unit, protected, mechanics)
    if kind in {"bash", "critical-strike", "feedback", "faerie-fire"}:
        supported = {"air", "ground", "enemy", "enemies", "neutral"}
        if kind == "critical-strike": supported.update({"structure", "ward"})
        if set(fields["targs1"].split(",")) - supported:
            raise ValueError("native directed passive/debuff cannot flatten class restrictions")
    if kind == "evasion":
        effect["chance_per_10k"] = number("DataA1", 10_000)
    elif kind == "spell-resistance":
        effect["damage_taken_per_10k"] = 10_000 - number("DataB1", 10_000)
    elif kind == "hardened-skin":
        if mechanics.get("native_base") != "Assk" or set(fields["targs1"].split(",")) != {"enemy", "ally"} or number("DataD1") != 1 or number("DataE1") != 1:
            raise ValueError("Hardened Skin requires its native ranged/melee flags and incoming mask")
        effect["profile"] = {"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "effect": {"HardenedSkin": {"chance_per_10k": number("DataA1", 100),
                "minimum_damage": number("DataB1"), "ignored_damage": number("DataC1")}}}
    elif kind == "spiked-shell":
        if mechanics.get("native_base") not in {"ANth", "AUts"} or fields["targs1"] != "_":
            raise ValueError("Spiked Shell requires the native intrinsic passive")
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "returned_damage_per_10k": max(0, number("DataA1", 10_000)),
            "received_damage_per_10k": max(0, number("DataB1", 10_000)),
            "armor_bonus_per_100": number("DataC1", 100)})
    elif kind == "cleave":
        if mechanics.get("native_base") not in {"ACce", "ANca"}:
            raise ValueError("cleave requires the native Cleaving Attack base")
        tokens = set(fields["targs1"].split(","))
        if tokens - {"ground", "enemy", "enemies", "neutral", "structure"} or "ground" not in tokens:
            raise ValueError("cleave requires an explicitly supported ground target mask")
        effect.update(radius_world=number("Area1"), damage_per_10k=number("DataA1", 10_000),
                      targets=1 | (4 if "structure" in tokens else 0))
    elif kind == "critical-strike":
        effect.update(chance_per_10k=number("DataA1", 100),
                      damage_multiplier_per_10k=number("DataB1", 10_000),
                      targets=unit_targets(fields["targs1"], allow_structures=True))
    elif kind == "corruption":
        if (mechanics["native_base"] != "AIcb" or number("DataA1") < 0
            or number("DataE1") != 1 or set(fields["targs1"].split(",")) != {"ground", "air", "ward"}):
            raise ValueError("Corruption requires audited native orb damage bonus and weapon index")
        ticks = lambda key: int((Decimal(fields[key]) * 30).to_integral_value(rounding="ROUND_CEILING"))
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"), "damage_bonus": number("DataA1"),
            "buff": {"rawcode": int.from_bytes(fields["buffid1"].encode(), "big"), "positive": False,
                "stealable": False, "organic_only": False},
            "armor_reduction_per_100": number("DataB1", 100), "duration_ticks": ticks("dur1"),
            "hero_duration_ticks": ticks("herodur1"), "targets": 3})
    elif kind == "sphere":
        if mechanics["native_base"] not in {"Asph", "Asp1", "Asp2", "Asp3"} or not 1 <= number("Targetattachcount") <= 3:
            raise ValueError("Sphere must retain an audited native attachment count")
        # Native Sphere launches independent visual-only missiles to the attack/cast point.
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "attachment_count": number("Targetattachcount"),
            "cooldown_ticks": int((Decimal(protected.get("cooldown", fields["cool1"])) * 30).to_integral_value(rounding="ROUND_CEILING")),
            "speed_per_tick": int(Decimal(fields["missilespeed"]) * 1024 / 30)})
    elif kind == "speed-aura":
        tokens = set(fields["targs1"].split(","))
        hostile = tokens == {"air", "enemies", "ward"}
        friendly = tokens == {"ground", "friend", "air", "self", "invulnerable", "vulnerable", "ward"}
        if mechanics["native_base"] != "AOae" or not (hostile or friendly):
            raise ValueError("speed aura needs audited native hostile-air or friendly-unit semantics")
        if number("dur1") != 0 or number("herodur1") != 0:
            raise ValueError("speed aura currently supports zero linger only")
        buffs = fields["buffid1"].split(",")
        if len(buffs) != 1 or len(buffs[0]) != 4:
            raise ValueError("speed aura must retain exactly one native buff family")
        effect.update(profile={"ability": int.from_bytes(recipe["source_key"].encode(), "big"),
            "buff_rawcode": int.from_bytes(buffs[0].encode(), "big"), "radius": number("Area1", 1024),
            "targets": 2 if hostile else 3, "hostile": hostile, "organic_only": False, "affects_invulnerable": friendly,
            "movement_percent_delta": number("DataA1", 100), "attack_speed_percent_delta": number("DataB1", 100)})
    elif kind == "native-aura":
        base = mechanics["native_base"]
        self_only = base == 'ACvp' and fields['targs1'] == 'self'
        if not self_only and set(fields["targs1"].split(",")) != {"air", "ground", "friend", "self", "vuln", "invu"}:
            raise ValueError("native aura requires explicit target semantics")
        damage, armor, mana, returned, melee, ranged = 0, 0, 0, 0, True, True
        if base == "AIar":
            if number("DataD1") != 0 or number("DataB1") not in {0, 1} or number("DataC1") not in {0, 1}:
                raise ValueError("native damage aura requires percent damage and explicit weapon classes")
            damage, melee, ranged = number("DataA1", 10_000), bool(number("DataB1")), bool(number("DataC1"))
        elif base == "AHad":
            if number("DataB1") != 0:
                raise ValueError("native armor aura requires flat armor")
            armor = number("DataA1", 100)
        elif base in {"ACba", "AHab"}:
            if number("DataB1") != 0:
                raise ValueError("native mana aura requires flat regeneration")
            mana = number("DataA1", 10_000)
        elif base == "ACvp":
            if not self_only or number('DataA1', 10000) < 0:
                raise ValueError('self Vampiric Aura requires its exact native self mask and positive factor')
            melee, ranged = True, False
        elif base == "AEah":
            if number("DataB1") != 1 or number("DataA1", 10_000) < 0:
                raise ValueError("native thorns aura requires nonnegative percent-return damage")
            returned = number("DataA1", 10_000)
        else:
            raise ValueError(f"unsupported native aura base {base}")
        effect.update(radius_world=number("Area1"), damage_bonus_per_10k=damage, armor_bonus_per_100=armor,
            self_only=self_only, melee_life_steal_per_10k=number("DataA1", 10000) if base == "ACvp" else 0,
            melee_damage_return_per_10k=returned,
            mana_regeneration_per_second_per_10k=mana, affects_melee=melee, affects_ranged=ranged,
            linger_ticks=int((Decimal(fields["dur1"]) * 30).to_integral_value(rounding="ROUND_CEILING")))
    elif kind == "pulverize":
        if set(fields["targs1"].split(",")) - {"ground", "enemy", "enemies", "neutral", "ward"}:
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
        # Native order: DataB units, DataC heroes, DataD summoned units.
        chances = [number(field, 100) for field in ("DataB1", "DataC1", "DataD1")]
        if not all(0 <= chance <= 10000 for chance in chances):
            raise ValueError("native orb chances must be bounded percentages")
        if len(set(chances)) != 1:
            effect.update(hero_chance_per_10k=chances[1], summoned_chance_per_10k=chances[2])
        if number("DataA1") != 0:
            raise ValueError("orb weapon damage bonus requires an explicit native modifier")
        enabled = number("DataE1")
        if enabled not in {0, 1, 2}:
            raise ValueError("unsupported orb enabled attack index")
        if enabled:
            if unit is None:
                raise ValueError("orb attack enablement requires its source weapon inventory")
            slot = str(enabled)
            if (unit["attack" + slot + "_enabled"] != "True"
                    and unit["attack" + slot + "_weapon_type"] not in {"_", "-", ""}):
                raise ValueError("orb enables a usable disabled weapon requiring native attack projection")
        effect.update(chance_per_10k=chances[0], effect_ability_rawcode=fields["unitid1"],
                      targets=unit_targets(fields["targs1"], allow_structures=True))
    elif kind == "chain-lightning":
        effect.update(initial_damage=number("DataA1"), maximum_targets=number("DataB1"),
                      jump_radius_world=number("Area1"),
                      damage_reduction_per_10k=number("DataC1", 10_000),
                      targets=unit_targets(fields["targs1"]))
    elif kind == "entangling-roots":
        effect.update(damage_per_second=number("DataA1"), duration_millis=number("Dur1", 1000),
                      hero_duration_millis=number("HeroDur1", 1000),
                      nonhero_only="nonhero" in fields["targs1"].split(","),
                      targets=unit_targets(fields["targs1"], allow_structures=True))
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
    expected = "dummy-target-ability-from-caster" if recipe["kind"] == "frost-nova" else "dummy-immediate-ability-from-caster"
    if row["mechanic_kind"] != expected or json.loads(row["scheduled_delays_json"]):
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
    if recipe["kind"] == "frost-nova":
        if set(child_fields["targs1"].split(",")) != {"air", "ground", "enemies", "neutral"}:
            raise ValueError("unsupported Frost Nova effect mask")
        effect = {"FrostNova": {"ability": fourcc(child), "radius": scaled(child_fields["area1"], 1024),
            "primary_damage": scaled(child_fields["datab1"]), "area_damage": scaled(child_fields["dataa1"]),
            "duration_ticks": ticks(child_fields["dur1"]), "hero_duration_ticks": ticks(child_fields["herodur1"]),
            "movement_percent_delta": -scaled(mechanics["misc"]["FrostMoveSpeedDecrease"], 100),
            "attack_speed_percent_delta": -scaled(mechanics["misc"]["FrostAttackSpeedDecrease"], 100), "targets": 3}}
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


def project_ensnare(recipe, fields, unit, protected, mechanics):
    row = mechanics["mechanics_row"]
    semantic = mechanics["semantics_row"]
    child = mechanics["effect_key"]
    effect_fields = mechanics["effect_fields"]
    if unit is None or row["target_mode_label"] != "enemy-flying-combat-sapper" or json.loads(row["scheduled_delays_json"]):
        raise ValueError("net parent requires immediate flying combat-target dispatch")
    if set(effect_fields["targs1"].split(",")) != {"air", "enemies", "mechanical", "neutral", "organic"} or effect_fields["missilehoming"] != "1":
        raise ValueError("unsupported native net delivery/mask")
    tick = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding="ROUND_CEILING"))
    raw = lambda value: int.from_bytes(value.encode("ascii"), "big")
    mana = lambda field, scale=1: 0 if unit[field].strip() in {"-", "", "_"} else scaled(unit[field], scale)
    trace = json.loads(semantic["parameters_json"])
    if row["mechanic_kind"] == "dummy-target-ability-from-caster":
        if trace["delivery_sites"][0]["callee"] != "dummyCastTargetFrom":
            raise ValueError("single net must originate at its caster")
        radius, maximum = 0, 1
    elif row["mechanic_kind"] == "delegated-helper":
        if trace["delivery_sites"][0]["callee"] != "dummyCastTargetWithVision" or trace["target_predicate"] != "alive-combat-sapper;enemy;flying":
            raise ValueError("unsupported bounded net helper")
        radius, maximum = scaled(trace["search_radius"], 1024), trace["max_targets"]
    else:
        raise ValueError("unsupported net script path")
    child_only = recipe["source_kind"] == "ability-effect"
    profile = {"ability": raw(child), "duration_ticks": tick(effect_fields["dur1"]), "hero_duration_ticks": tick(effect_fields["herodur1"]),
        "range": scaled(effect_fields["rng1"], 1024), "speed_per_tick": int(Decimal(effect_fields["missilespeed"]) * 1024 / 30),
        "lowering_ticks": tick(effect_fields["dataa1"]), "air_height": scaled(effect_fields["datab1"], 1024),
        "melee_range": scaled(effect_fields["datac1"], 1024)}
    rate = mana("mana_regen", 10_000)
    return {"kind": "scripted-automatic", "source_kind": recipe["source_kind"], "source_key": recipe["source_key"], "unit_rawcode": recipe["unit_rawcode"],
        "spellcasting": {"mana": {"maximum": mana("mana_max"), "starting": mana("mana_start"), "regen_per_tick_per_10k": (1 << 31) | rate},
            "ability": {"id": raw(recipe["source_key"]), "mana_cost": 0 if child_only else scaled(protected.get("mana_cost", fields["cost1"])),
                "cooldown_ticks": 0 if child_only else tick(protected.get("cooldown", fields["cool1"])),
                "range": 0 if child_only else scaled(fields["rng1"], 1024), "target_policy": "FlyingEnemyUnit",
                "effect": {"Ensnare": {"profile": profile, "search_radius": radius, "maximum_targets": maximum}}}},
        "mana_regen_per_second_per_10k": rate, "effect_ability_rawcode": None if child_only else child,
        "provenance": {"source": "resolved/object-fields.tsv", "unit": "resolved/units.tsv", "protected": "resolved/protected-ability-fields.tsv",
            "script": "resolved/unit-spell-mechanics.tsv", "semantics": "resolved/unit-spell-semantics.tsv"}}


def build_tuning(release: dict[str, Any], repo_root: Path, recipes: dict[str, Any]) -> dict[str, Any]:
    if recipes["schema_version"] != 1:
        raise ValueError("unsupported native recipe schema")
    tree = release["extraction"]["git_tree"]
    def retained(path: str) -> bytes:
        return catalog._retained_file_bytes(repo_root, tree, path)

    abilities: dict[str, dict[str, str]] = {}
    ability_levels: dict[str, dict[str, str]] = {}
    ability_bases: dict[str, str] = {}
    for row in rows(retained("resolved/object-fields.tsv")):
        if row["category"] == "abilities" and row["source_field"]:
            ability_levels.setdefault(row["rawcode"], {})[row["source_field"].lower()] = scalar(row["recovered_value_json"])
        if row["category"] == "abilities" and row["level"] in {"0", "1"}:
            ability_bases[row["rawcode"]] = row["base_rawcode"]
            field = row["source_field"].lower()
            if field:
                abilities.setdefault(row["rawcode"], {})[field] = scalar(row["recovered_value_json"])
    from project_native_reference import project as project_reference
    reference = project_reference(repo_root, retained)
    for code, fields in reference['abilities'].items():
        abilities.setdefault(code, {key: str(value) for key, value in fields.items()})
        ability_levels.setdefault(code, {key: str(value) for key, value in fields.items()})
        ability_bases.setdefault(code, fields['code'])
    unit_fields = {}
    for row in rows(retained("resolved/object-fields.tsv")):
        if row["category"] == "units" and row["source_field"]:
            unit_fields.setdefault(row["rawcode"], {})[row["source_field"].lower()] = scalar(row["recovered_value_json"])
    units = {row["rawcode"]: row for row in rows(retained("resolved/units.tsv"))}
    unit_stats = {row['rawcode']: row for row in rows(retained('resolved/protected-unit-stats.tsv'))}
    ability_ui = {}
    for row in rows(retained("resolved/object-fields.tsv")):
        if row["category"] == "abilities" and row["field_id"] in {"abpx", "abpy", "ahky"}:
            ability_ui.setdefault(row["rawcode"], {})[row["field_id"]] = json.loads(row["recovered_value_json"])
    default_autocast = {row["rawcode"]: scalar(row["recovered_value_json"])
        for row in rows(retained("resolved/object-fields.tsv")) if row["category"] == "units" and row["field_id"] == "udaa"}
    protected: dict[str, dict[str, str]] = {}
    for row in rows(retained("resolved/protected-ability-fields.tsv")):
        if row["level"] == "1":
            protected.setdefault(row["rawcode"], {})[row["field"]] = row["runtime_value"]
    mechanics = {
        row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "automatic-defend-state-maintenance"
    }
    system_mechanics = {row["system_id"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/runtime-system-mechanics.tsv"))}
    kill_mechanics = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "kill-triggered-native-berserk"}
    damage_thresholds = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "damage-triggered-persistent-low-hp-attack-bonus"}
    mirror_images = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "attack-proc-native-mirror-image"}
    damage_charges = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "damage-to-mana-kaboom-charge"}
    aftershocks = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "source-damage-health-scaled-aftershock"}
    retaliations = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "target-damage-melee-thunderbolt-retaliation"}
    all_protected = {}
    for row in rows(retained("resolved/protected-ability-fields.tsv")):
        all_protected.setdefault(row["rawcode"],{}).setdefault(row["level"],{})[row["field"]] = row["runtime_value"]
    fan_of_knives = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "damage-triggered-auto-fan-of-knives"}
    lava_spawns = {row["unit_rawcode"]: json.loads(row["parameters_json"])
        for row in rows(retained("resolved/production-unit-special-mechanics.tsv"))
        if row["mechanic_kind"] == "native-lava-spawn-split-with-child-conversion"}
    lua = retained("script/war3map.lua")
    functions = {row["name"]: lua[int(row["start_byte_offset"]):int(row["end_byte_offset"])].decode()
        for row in rows(retained("script/function-spans.tsv"))}
    from project_training_variants import project as project_training
    training = project_training(functions, units, ability_bases)
    training_grants = {code.to_bytes(4, 'big').decode() for group in training['grant_groups'] for code in [choice['ability'] for choice in group['choices']]}
    training_roots = {code.to_bytes(4, 'big').decode() for code in training['roots']}
    building_spells = {row["ability_rawcode"]: row for row in rows(retained("resolved/building-spell-mechanics.tsv"))}
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
        if recipe["kind"] in {"damage-charge", "damage-retaliation", "health-scaled-aftershock"}:
            key += (recipe["unit_rawcode"],)
        if key in seen:
            raise ValueError(f"duplicate recipe source: {key}")
        seen.add(key)
        unit = units.get(recipe.get("unit_rawcode", ""))
        if recipe["source_kind"] == "unit-ability":
            if unit is None or (recipe["source_key"] not in unit["abilities"].split(",")
                and not (recipe["source_key"] in training_grants and recipe["unit_rawcode"] in training_roots)):
                raise ValueError(f"recipe {key} is not in its source unit's extracted ability inventory")
        source = recipe["source_key"]
        if recipe["kind"] in {"inner-fire", "spell-steal", "native-curse"} and default_autocast.get(recipe.get("unit_rawcode")) != source:
            raise ValueError("native buff source must retain default-active autocast")
        spell = spell_mechanics.get("A02U" if recipe["kind"] == "siren-purge" else source)
        if recipe['kind'] in {'travelling-mana-clap', 'parasite-area-buff', 'parasite-script', 'impale-fan', 'native-impale'}:
            spell = next(row for row in spell_mechanics.values() if row['unit_rawcode'] == recipe['unit_rawcode'])
        if spell is None and recipe["kind"] in {"healing-wave", "solar-strike", "war-stomp", "howl-of-terror", "frost-nova", "ensnare"}:
            matches = [row for row in spell_mechanics.values() if row["unit_rawcode"] == recipe.get("unit_rawcode")
                and source in spell_semantics[row["ability_rawcode"]]["effect_rawcodes"].split(",")]
            if len(matches) != 1:
                raise ValueError(f"proxy child {source} needs one semantic parent for its source unit")
            spell = matches[0]
        detail = dict(mechanics.get(recipe.get("unit_rawcode", ""), {}))
        if recipe['kind'] in {'travelling-mana-clap', 'parasite-area-buff', 'parasite-script', 'impale-fan', 'native-impale'}:
            detail.update(all_fields=abilities, all_bases=ability_bases, all_units=units,
                all_unit_fields=unit_fields, all_unit_stats=unit_stats,
                all_protected=protected, functions=functions, git_tree=tree,
                map_version=dict(zip(['major', 'minor'], map(int, release['map_version'].split('.')))))
        if recipe["kind"] in {"siren-purge", "feral-spirit"}:
            parent = "A02U"
            spell = spell_mechanics[parent]
            names = [spell["handler_function"], *spell["delayed_callback_functions"].split(",")]
            detail.update(map_version=dict(zip(["major", "minor"], map(int, release["map_version"].split(".")))),
                git_tree=tree, naga_functions={name: functions[name] for name in names},
                purge_fields=abilities["A02T"], purge_base=ability_bases["A02T"],
                purge_protected=protected["A02T"], parent_fields=abilities[parent])

        if recipe["kind"] == "health-scaled-aftershock":
            detail.update(aftershock=aftershocks[recipe["unit_rawcode"]], aftershock_handler=functions["handleSourceDamageEffects"],
                damage_runtime=functions["code__addAction_DamageRuntime"], sapper_registration=functions["onUnitEnteredMap"],
                damage_registration=functions["registerSapperDamage"], git_tree=tree)
        if recipe["kind"] in {"damage-retaliation", "retaliation-projectile"}:
            detail.update(retaliation=retaliations[recipe["unit_rawcode"]], retaliation_handler=functions["castLightningShield"],
                retaliation_dispatch=functions["handleTargetDamageEffects"], retaliation_fields=ability_levels["A0D5"],
                retaliation_protected=all_protected["A0D5"], retaliation_base=ability_bases["A0D5"], git_tree=tree)
        if recipe["kind"] == "master-projectiles":
            names = ["onMasterSpell", "onLightningBolt", "processMasterLightningBolts", "findMasterLightningHit", "spawnSnowMissileFanStep", "processSnowMissiles", "dummyCastTargetWithVision1"]
            detail.update(map_version=dict(zip(["major","minor"],map(int,release["map_version"].split(".")))), master_trace=json.loads(spell_semantics[source]["parameters_json"]),
                master_functions={name:functions[name] for name in names}, master_nova=abilities["A043"],
                master_nova_protected=protected["A043"], master_dummy=unit_fields["e008"],
                master_units={key:unit_fields[key] for key in ["h03H","h068"]}, git_tree=tree)
        if recipe["kind"] == "lava-spawn":
            trace = lava_spawns[recipe["unit_rawcode"]]
            summon = functions["EventListener_add_FireElemental_onEvent_add_FireElemental"]
            if (f'unit_getTypeId(kxm)=={trace["runtime_combat_unit_id"]}' not in summon
                or f'ReplaceUnitBJ(jxm,{trace["nested_h030_child_replacement_unit_id"]},2)' not in summon
                or '__wurst_safe_UnitRemoveType(jxm,UNIT_TYPE_SUMMONED)' not in summon
                or f'doAfter({trace["post_child_attack_order_delay_seconds"]},dzq)' not in functions["setupFireSummon"]):
                raise ValueError("lava-spawn child conversion/order control flow changed")
            detail.update(lava_spawn=trace, git_tree=tree)
        detail["kill_mechanics"] = kill_mechanics.get(recipe.get("unit_rawcode", ""))
        detail["system"] = next((trace for trace in system_mechanics.values()
            if trace.get("aura_ability_rawcode") == source), None)
        if recipe["kind"] == "damage-threshold":
            init = functions["IN"]
            if f'jS={int.from_bytes(source.encode(), "big")}' not in init or "DamageListener_unreduced=false" not in init:
                raise ValueError("damage threshold requires its reduced-event listener registration")
            detail.update(damage_threshold=damage_thresholds[recipe["unit_rawcode"]],
                damage_threshold_handler=functions["DamageListener_addListener_TrollBlood_onEvent_addListener_TrollBlood"],
                ability_levels=ability_levels[source], git_tree=tree)
        if recipe["kind"] == "damage-charge":
            trace = damage_charges[recipe["unit_rawcode"]]
            child = trace["kaboom_ability_id"].to_bytes(4, "big").decode()
            handler = functions["handleTargetDamageEffects"]
            if f'elseif(hqq=={int.from_bytes(recipe["unit_rawcode"].encode(), "big")})then vampireCharge(gqq,GetEventDamage())' not in handler or "widget_getLife(gqq)<=.405" not in handler or f'unit_getAbilityLevel(gqq,{trace["runtime_damage_marker_ability_id"]})<=0' not in handler:
                raise ValueError("damage charge requires its source unit and native damage event guards")
            detail.update(damage_charge=trace, damage_charge_handler=functions["vampireCharge"],
                charge_child_fields=abilities[child], charge_child_base=ability_bases[child], git_tree=tree)
        if recipe["kind"] == "mirror-image":
            trace = mirror_images[recipe["unit_rawcode"]]
            handler = functions["handleSourceDamageEffects"]
            if f'elseif(Spq=={int.from_bytes(recipe["unit_rawcode"].encode(), "big")})then randomN02YOrder(Qpq)' not in handler or f'unit_getAbilityLevel(Qpq,{trace["source_damage_marker_ability_id"]})<=0' not in handler:
                raise ValueError("Mirror Image requires the native damage-event marker/type dispatch")
            detail.update(mirror_image=trace, mirror_image_handler=functions["randomN02YOrder"], git_tree=tree)
        if recipe["kind"] in {"building-improvement", "building-mana-regeneration"}:
            child = spell_semantics[source]["effect_rawcodes"] if recipe["kind"] == "building-improvement" else source
            detail["improvement_levels"] = ability_levels[child]
        if recipe["kind"] == "building-improvement":
            detail.update(improvement_handler=functions["improveSpecialBuilding"], improvement_cleanup=functions["yK"], improvement_cleanup_registration=re.search(r'_I\[_T\(\d+,"[0-9a-f]+"\)\]=yK',lua.decode()).group(), improvement_death=functions["fJ"], improvement_ui=ability_ui[source], git_tree=tree)
        if recipe["kind"] in {"building-weather", "native-weather-wave", "building-enchantment", "native-enchantment-grant", "source-damage-dispatcher"}:
            detail.update(all_fields=abilities, all_bases=ability_bases, all_protected=protected, functions=functions, building_spell=building_spells.get(source), git_tree=tree, map_version=dict(zip(["major","minor"],map(int,release["map_version"].split(".")))))
        if recipe["kind"] == "fan-of-knives":
            detail.update(fan_of_knives=fan_of_knives[recipe["unit_rawcode"]], functions=functions, git_tree=tree)
        if recipe["kind"] in {"scripted-roots", "wind-walk", "native-heal", "orb-charm"}:
            detail.update(all_bases=ability_bases, functions=functions, git_tree=tree,
                map_version=dict(zip(["major", "minor"], map(int, release["map_version"].split(".")))))
        detail["misc"] = misc
        if recipe['kind'] == 'native-debuff':
            default = default_autocast.get(recipe['unit_rawcode'])
            if default not in (source, ability_bases[source]):
                raise ValueError('native Slow default-active alias differs from its source class')
            detail.update(reference_source_sha256=reference['source_sha256'], default_active_alias=default)
        detail["native_base"] = ability_bases[source]
        detail["source_unit_stats"] = unit_stats.get(recipe.get("unit_rawcode", ""), {})
        if recipe["kind"] in {"impale-fan", "native-impale", "mana-attack-orb"}:
            detail["default_active_alias"] = default_autocast[recipe["unit_rawcode"]]
        if spell is not None and recipe["kind"] != "master-projectiles":
            semantics = spell_semantics[spell["ability_rawcode"]]
            effect_key = semantics["effect_rawcodes"].split(",")[0]
            detail.update(effect_key=effect_key, effect_fields=abilities.get(effect_key, {}),
                effect_protected=protected.get(effect_key, {}), mechanics_row=spell, semantics_row=semantics)
        effects.append(project_effect(recipe, abilities[source], unit, protected.get(source, {}), detail))
    from project_unit_death_rules import project as project_death_rules
    death_rules = project_death_rules(rows(retained("resolved/production-unit-special-mechanics.tsv")), functions)
    families = [{"source_key": key, "buff": buff_identity(abilities[key], positive=base != "ACff")}
        for key, base in sorted(ability_bases.items()) if base in {"Ainf", "ACf2", "ACff", "AIrr", "AEer", "Aenr"}]
    from project_flying_retarget import project as project_flying_retarget
    flying_retargets = project_flying_retarget(rows(retained("resolved/production-unit-special-mechanics.tsv")), functions)
    return {"schema_version": 3, "map_version": release["map_version"], "effects": effects,
        "unit_flying_retarget_rules": flying_retargets,
        "unit_death_rules": death_rules, "unit_death_rules_source": {"git_tree": tree, "path": "resolved/production-unit-special-mechanics.tsv", "function": "fJ"},
        "native_reference_source_sha256": reference["source_sha256"], "buff_families": families, "buff_family_source": {"git_tree": tree, "path": "resolved/object-fields.tsv"}}


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
