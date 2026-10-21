"""Lich King's single-roll damage listener: devour or a native Death and Decay at the target."""
import re
from decimal import Decimal


def _scaled(value, scale=1):
    number = Decimal(str(value)) * scale
    if number != number.to_integral_value():
        raise ValueError(f"value {value!r} cannot be represented exactly at scale {scale}")
    return int(number)


def project(recipe, fields, protected, detail):
    parameters = detail["death_mastery"]
    handler = detail["functions"]["devourAttackProc"]
    dispatcher = detail["functions"]["handleSourceDamageEffects"]
    marker = parameters["source_damage_marker_ability_id"]
    source_type = int.from_bytes(recipe["unit_rawcode"].encode(), "big")
    shape = (r'(\w+)=GetRandomInt\(0,99\)if\(\(\(\1<(\d+)\)and\(unit_getAbilityLevel\((\w+),(\d+)\)<=0\)\)and'
        r'\(not unit_isType\(\3,UNIT_TYPE_MECHANICAL\)\)\)then if\(widget_getLife\(\3\)<\.405\)then return end '
        r'(\w+)=\(widget_getLife\((\w+)\)\+\(widget_getLife\(\3\)/2\.\)\)__wurst_safe_SetWidgetLife\(\6,\5\)'
        r'(\w+)=ATTACK_TYPE_CHAOS (\w+)=DAMAGE_TYPE_DEATH (\w+)=WEAPON_TYPE_WHOKNOWS '
        r'__wurst_safe_UnitDamageTarget\(\6,\3,(\d+)\.,true,false,\7,\8,\9\)')
    matched = re.search(shape, handler)
    decay = re.search(r'elseif\(\1<(\d+)\)then' if matched is None else r'elseif\(' + re.escape(matched[1]) + r'<(\d+)\)then', handler)
    cast = re.search(r'addProtectedAbility\((\w+),(\d+)\)unit_issuePointOrderById\(\w+,(\d+),(\w+)\)'
        r'__wurst_safe_UnitApplyTimedLife\(\w+,1112820806,([0-9.]+)\)', handler)
    if (matched is None or decay is None or cast is None
            or int(matched[2]) != parameters["devour_roll_threshold_exclusive"]
            or int(matched[4]) != parameters["devour_target_excluded_ability_id"]
            or int(matched[10]) != parameters["devour_damage"]
            or int(decay[1]) != parameters["death_and_decay_roll_threshold_exclusive"]
            or int(cast[2]) != parameters["death_and_decay_ability_id"]
            or int(cast[3]) != parameters["death_and_decay_order_id"]
            or Decimal(cast[5]) != Decimal(str(parameters["death_and_decay_dummy_lifetime_seconds"]))
            or f"unit_getAbilityLevel(Qpq,{marker})<=0)then return end" not in dispatcher
            or f"Spq=={source_type})then devourAttackProc(Qpq,Rpq)" not in dispatcher
            or parameters["devour_attack_type"] != "chaos" or parameters["devour_damage_type"] != "death"
            or not parameters["devour_is_attack"] or parameters["devour_is_ranged"]
            or not parameters["devour_target_requires_not_mechanical"]
            or not parameters["devour_target_requires_alive_after_damage"]
            or not parameters["death_and_decay_branch_runs_when_devour_condition_fails"]
            or parameters["death_and_decay_cast_position"] != "damaged-target-position"
            or parameters["random_roll_min"] != 0 or parameters["random_roll_max"] != 99):
        raise ValueError("Lich King source-damage controller control flow changed")
    ability = int.from_bytes(recipe["source_key"].encode(), "big")
    if ability != parameters["death_and_decay_ability_id"]:
        raise ValueError("Death Mastery must be keyed by its Death and Decay ability")
    if (set(fields["targs1"].split(",")) != {"air", "enemies", "ground", "ward"}
            or _scaled(protected.get("mana_cost", fields["cost1"])) != 0
            or _scaled(protected.get("cooldown", fields["cool1"])) != 0
            or _scaled(fields["dur1"], 30) != _scaled(parameters["death_and_decay_duration_seconds"], 30)
            or Decimal(fields["area1"]) != Decimal(str(parameters["death_and_decay_area"]))
            or Decimal(fields["dataa1"]) != Decimal(str(parameters["death_and_decay_object_data"]["Max Life Drained per Second (%)"]))):
        raise ValueError("Death and Decay requires its free unit-only retained inventory")
    return {**recipe, "profile": {
        "ability": ability,
        "devour_chance_per_10k": int(matched[2]) * 100,
        "decay_chance_per_10k": int(decay[1]) * 100,
        "devour_damage": int(matched[10]),
        "devour_heal_per_10k": _scaled(parameters["devour_heal_fraction_of_target_current_hp"], 10_000),
        "devour_excluded_ability": int(matched[4]),
        "decay": {"ability": ability, "radius": _scaled(fields["area1"], 1024),
            "life_per_10k": _scaled(fields["dataa1"], 10_000),
            "duration_ticks": _scaled(fields["dur1"], 30), "targets": 3}},
        "provenance": {"git_tree": detail["git_tree"], "source": "resolved/production-unit-special-mechanics.tsv",
            "handler": "devourAttackProc", "dispatcher": "handleSourceDamageEffects",
            "marker": f"A03Q:{marker}", "native_base": "AUdd"}}
