"""Native Raise Dead whose summoned carrier the map script replaces with a random skeleton."""
import re
from decimal import Decimal


def _scaled(value, scale=1):
    number = Decimal(str(value)) * scale
    if number != number.to_integral_value():
        raise ValueError(f"value {value!r} cannot be represented exactly at scale {scale}")
    return int(number)


def project(recipe, fields, unit, protected, detail):
    raw = int.from_bytes(recipe["source_key"].encode(), "big")
    body = fields["datac1"]
    if (detail["native_base"] != "Arai" or unit is None or detail["default_active"] != recipe["source_key"]
            or set(fields["targs1"].split(",")) != {"dead"} or len(body) != 4
            or fields["datad1"] not in ("-", "") or _scaled(fields["datab1"]) != 0 or not 0 < _scaled(fields["dataa1"]) <= 255
            or _scaled(fields["area1"]) <= 0 or _scaled(fields["dur1"]) != 0 or _scaled(fields["herodur1"]) != 0):
        raise ValueError("Raise Dead requires the default-active native corpse order with one carrier body type")
    system = detail["raise_dead_system"]
    functions = detail["functions"]
    table = system["skeleton_table_unit_ids"]
    carrier = int.from_bytes(body.encode(), "big")
    listener, replace = functions["onChaosPortalUnitSummoned"], functions["replaceChaosPortalSummon"]
    registration = functions["cG"]
    assigned = [int(value) for value in re.findall(r"ajb\[\d\]=(\d+)", registration)]
    if (assigned != table or len(table) != 8 or system["replacement_method"] != 2
            or not system["replacement_orders_attack"]
            or "ReplaceUnitBJ(yeq,zeq,2)orderCodeAttack(Aeq)" not in replace.replace("__wurst_safe_", "")):
        raise ValueError("Raise Dead skeleton table or replacement call changed")
    first, last, special = None, None, None
    if carrier == system["necromancer_carrier_unit_id"]:
        indexes = system["necromancer_table_indexes"]
        shape = r"Ceq==%d\)then replaceChaosPortalSummon\(Beq,__wurst_ensureInt\(ajb\[GetRandomInt\(0,%d\)\]\)\)" % (carrier, indexes[-1])
        bonuses = []
    elif carrier == system["mighty_necromancer_carrier_unit_id"]:
        indexes = system["mighty_necromancer_table_indexes"]
        shape = r"Ceq==%d\)then replaceChaosPortalSummon\(Beq,__wurst_ensureInt\(ajb\[GetRandomInt\(0,%d\)\]\)\)" % (carrier, indexes[-1])
        bonuses = []
    elif carrier == system["lich_king_carrier_unit_id"]:
        indexes = system["lich_king_other_table_indexes"]
        general = system["lich_king_general_table_index"]
        rolls = 100 / Decimal(str(system["lich_king_general_probability_percent"]))
        if rolls != rolls.to_integral_value() or Decimal(str(system["lich_king_each_other_probability_percent"])) * len(indexes) \
                != 100 - Decimal(str(system["lich_king_general_probability_percent"])):
            raise ValueError("Lich King replacement distribution is not a one-in-N draw")
        shape = (r"Ceq==%d\)then Deq=GetRandomInt\(0,%d\)if\(Deq==0\)then Deq=%d else Deq=GetRandomInt\(%d,%d\)end"
            % (carrier, int(rolls) - 1, general, indexes[0], indexes[-1]))
        special = {"rolls": int(rolls), "index": general}
        data = {"attack": system["lich_king_bonus_damage_object_data"]["Attack Bonus"],
            "armor": system["lich_king_bonus_armor_object_data"]["Defense Bonus"]}
        bonuses = [
            {"ability": system["lich_king_bonus_damage_ability_id"], "effect": {"AttackDamage": _scaled(data["attack"])}},
            {"ability": system["lich_king_bonus_armor_ability_id"], "effect": {"Armor": _scaled(data["armor"], 100)}}]
        if (f"addProtectedAbility(Feq,{system['lich_king_bonus_damage_ability_id']})" not in listener
                or f"addProtectedAbility(Geq,{system['lich_king_bonus_armor_ability_id']})" not in listener):
            raise ValueError("Lich King replacement bonus abilities changed")
    else:
        raise ValueError("Raise Dead carrier has no retained replacement distribution")
    if not re.search(shape, listener) or indexes != list(range(indexes[0], indexes[-1] + 1)):
        raise ValueError("Raise Dead replacement draw changed")
    first, last = indexes[0], indexes[-1]
    mana = lambda key, scale=1: 0 if unit[key] in {"", "-", "_"} else _scaled(unit[key], scale)
    rate = mana("mana_regen", 10_000)
    return {"kind": "scripted-automatic", "source_kind": recipe["source_kind"], "source_key": recipe["source_key"],
        "unit_rawcode": recipe["unit_rawcode"], "spellcasting": {
            "mana": {"maximum": mana("mana_max"), "starting": mana("mana_start"), "regen_per_tick_per_10k": (1 << 31) | rate},
            "ability": {"id": raw, "mana_cost": _scaled(protected.get("mana_cost", fields["cost1"])),
                "cooldown_ticks": _scaled(protected.get("cooldown", fields["cool1"]), 30),
                "range": _scaled(fields["area1"], 1024), "target_policy": "RaisableCorpse",
                "effect": {"RaiseDead": {"map_version": detail["map_version"], "ability": raw,
                    "body_rawcode": carrier, "count": _scaled(fields["dataa1"]),
                    "radius": _scaled(fields["area1"], 1024), "table": table,
                    "first_index": first, "last_index": last, "special": special,
                    "bonus_grants": (bonuses + [None, None])[:2]}}}},
        "mana_regen_per_second_per_10k": rate, "effect_ability_rawcode": None,
        "provenance": {"git_tree": detail["git_tree"], "source": "resolved/object-fields.tsv",
            "system": "resolved/runtime-system-mechanics.tsv", "handler": "onChaosPortalUnitSummoned",
            "protected": "resolved/protected-ability-fields.tsv", "native_base": "Arai"}}
