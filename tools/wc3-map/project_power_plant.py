"""Power Plant's aura trio, disable-cleanse sweep and production augmentation."""
import json
import re
from decimal import Decimal, ROUND_CEILING


def project(row, object_rows, traces):
    params = json.loads(row['parameters_json'])
    plant = params['power_plant_unit_id']
    levels = {}
    for field in object_rows:
        if field['category'] == 'abilities' and field['source_field']:
            levels.setdefault((field['rawcode'], field['source_field'].lower(), field['level']), json.loads(field['recovered_value_json']))
    value = lambda code, name, level='1': Decimal(str(levels[(code, name, level)]))
    word = lambda code, name, level='1': str(levels[(code, name, level)])
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    ticks = lambda v: int((Decimal(str(v)) * 30).to_integral_value(rounding=ROUND_CEILING))
    world = lambda v: int((Decimal(str(v)) * 1024).to_integral_value(rounding=ROUND_CEILING))
    armor_aura, mana_aura, drum_aura = (params[key] for key in ('building_armor_aura_ability_id', 'mana_regen_aura_ability_id', 'tower_damage_aura_ability_id'))
    code = lambda ident: ident.to_bytes(4, 'big').decode()
    register = traces['registerPowerPlant']['source']
    init = traces['dK']['source']
    sweep = traces['ForGroupCallback_forUnitsInRange_PowerPlantRuntime_callback_forUnitsInRange_PowerPlantRuntime']['source']
    armor = traces['applyPowerArmor']['source']
    exclusion = traces['hasNoPowerArmorExclusion']['source']
    setup = traces['setupUnit']['source']
    cleaned = params['cleaned_buff_ids']
    ordered = re.findall(r'addProtectedAbility\(\w+,(\d+)\)', register)
    if (ordered != [str(armor_aura), str(mana_aura), str(drum_aura)] or f'unit_getTypeId(tir)==O1' not in register
            or f'O1={plant}' not in init or f'N1={params["disable_cleanse_radius"]}.' not in init
            or f'TimerStart(qir,{params["disable_cleanse_sweep_interval_seconds"]}.,true' not in init
            or not params['disable_cleanse_requires_structure'] or params['disable_cleanse_requires_allied_structure']
            or not all(f'unit_removeAbility(Ajn,{buff})' in sweep for buff in cleaned)
            or 'UNIT_TYPE_STRUCTURE' not in sweep or 'unit_getTypeId(Ajn)==O1' not in sweep):
        raise ValueError('Power Plant registration, cleanse timer or cleanse recipient predicate changed')
    stack, armor_bonus, damage, resist = (params[key] for key in ('spawn_hp_stack_ability_id', 'spawn_armor_bonus_ability_id', 'spawn_damage_bonus_ability_id', 'spawn_spell_resist_ability_id'))
    if (re.findall(r'addProtectedAbility\(\w+,(\d+)\)', armor) != [str(stack), str(armor_bonus), str(damage), str(resist)]
            or f'SetUnitAbilityLevel(Jfs,{stack},{params["spawn_hp_stack_temporary_level"]})' not in armor
            or f'unit_removeAbility(Hfs,{stack})' not in armor or 'if hasNoPowerArmorExclusion(Hfs)then' not in armor
            or set(map(int, re.findall(r'unit_getAbilityLevel\(hfs,(\d+)\)==0', exclusion))) != set(params['spell_resist_exclusion_ability_ids'])
            or f'Rfs=(unit_getAbilityLevel(Ofs,{params["spawn_requires_building_power_armor_buff_id"]})>0)' not in setup
            or 'if Rfs then applyPowerArmor(Sfs)end' not in setup):
        raise ValueError('Power Armor spawn augmentation control flow changed')
    stack_code = code(stack)
    hp = -int(value(stack_code, 'dataa4', '4'))
    if hp != params['spawn_permanent_max_hp_bonus'] or value(stack_code, 'dataa1') != 0:
        raise ValueError('Power Armor max-life trick disagrees with its retained levels')
    masks = {name: set(word(code(ident), 'targs1').split(',')) for name, ident in (('armor', armor_aura), ('mana', mana_aura), ('drums', drum_aura))}
    if (masks['armor'] != {'friend', 'structure', 'self', 'invulnerable', 'vulnerable'}
            or masks['mana'] != {'friend', 'structure', 'invulnerable', 'vulnerable'}
            or masks['drums'] != {'friend', 'structure', 'invulnerable', 'vulnerable'}
            or value(code(drum_aura), 'datab1') != 0 or value(code(drum_aura), 'datac1') != 1 or value(code(drum_aura), 'datad1') != 0
            or value(code(armor_aura), 'datab1') != 0 or value(code(mana_aura), 'datab1') != 0
            or value(code(damage), 'datab1') != 1 or value(code(damage), 'datac1') != 1 or word(code(damage), 'targs1') != 'self'):
        raise ValueError('Power Plant aura masks or melee/ranged flags changed')
    aura = lambda ident, name, bonus: {'ability': ident, 'buff': raw(word(code(ident), 'buffid1')), 'radius': world(value(code(ident), 'area1')), name: bonus}
    return {'source_rawcode': plant,
        'armor': aura(armor_aura, 'bonus_per_100', int(value(code(armor_aura), 'dataa1') * 100)),
        'mana': aura(mana_aura, 'regeneration_per_second_per_10k', int(value(code(mana_aura), 'dataa1') * 10000)),
        'drums': aura(drum_aura, 'damage_per_10k', int(value(code(drum_aura), 'dataa1') * 10000)),
        'cleanse': {'radius': world(params['disable_cleanse_radius']), 'interval_ticks': ticks(params['disable_cleanse_sweep_interval_seconds']),
            'buffs': cleaned},
        'spawn': {'requires_buff': params['spawn_requires_building_power_armor_buff_id'], 'max_life_bonus': hp,
            'armor_ability': armor_bonus, 'armor_bonus_per_100': int(value(code(armor_bonus), 'dataa1') * 100),
            'damage_ability': damage, 'damage_per_10k': int(value(code(damage), 'dataa1') * 10000),
            'life_ability': stack,
            'spell_resist_ability': resist, 'spell_damage_taken_per_10k': 10000 - int(value(code(resist), 'datab1') * 10000),
            'spell_resist_exclusions': sorted(params['spell_resist_exclusion_ability_ids'])}}
