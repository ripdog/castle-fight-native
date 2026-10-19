"""Desert building scripts released by an inert native Parasite trigger."""
import json
import re
from decimal import Decimal, ROUND_CEILING

PARENT_MASK = {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}


def _helpers():
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    return number, raw


def _replenish(params, detail, number, raw):
    functions = detail['functions']
    body = functions['spellReplenish']
    severe = re.search(r'\(\(\w+-(\d+)\)>\w+\)', functions['tK'])
    percent = re.search(r'\(\w+>0\.0\)\)and\(\w+<([\d.]+)\)', functions['uK'])
    order = re.findall(r'=([A-Z]0) __wurst_safe_GroupEnumUnitsInRect', body)
    gains = re.findall(r'SetUnitState\(\w+,\w+,\(unit_getState\(\w+,\w+\)\+(\d+)\.\)\)', body)
    if (params['selection_order'] != ['Y0', 'X0', 'W0'] or order != params['selection_order']
        or severe is None or percent is None or 'group_getRandom(' not in body
        or gains != [str(params['life_gain']), str(params['mana_gain'])]
        or 'unit_isAllyOf' not in functions['tK'] or 'isCombatSapper' not in functions['tK']
        or 'BlzGetUnitMaxHP' not in functions['sK']):
        raise ValueError('Beacon replenish control flow changed: ordered random tiers, ally combat bodies and fixed gains')
    return {'Replenish': {'life': params['life_gain'], 'mana': params['mana_gain'],
        'severe_missing_life': int(severe[1]), 'mana_below_per_10k': number(percent[1], 100)}}


def _hallucination(params, detail, number, raw):
    functions = detail['functions']
    body = functions['spellPeyote']
    child = params['effect_ability_id'].to_bytes(4, 'big').decode()
    fields = detail['all_fields'][child]
    excluded = re.search(r'GetUnitAbilityLevel\(\w+,(\d+)\)<=0', functions['isNotTentacle'])
    if (detail['all_bases'][child] != 'AIil' or 'checkForShield(' not in body or 'group_getRandom(' not in body
        or 'dummyCastTargetWithVision(' not in body or excluded is None
        or 'unit_isEnemyOf' not in functions['vK'] or 'isAliveCombatSapper' not in functions['vK']
        or set(fields['targs1'].split(',')) != {'ground', 'air'}
        or number(fields['dur1']) != number(fields['herodur1'])):
        raise ValueError('Peyote control flow changed: shield-checked random vulnerable enemy illusion cast')
    return {'Hallucination': {'illusion_ability': raw(child), 'illusion_ticks': number(fields['dur1'], 30),
        'damage_dealt_per_10k': number(fields['dataa1'], 10000), 'damage_taken_per_10k': number(fields['datab1'], 10000),
        'excluded_ability': int(excluded[1])}}


def _sandstorm(params, detail, number, raw):
    functions = detail['functions']
    handler = functions['BuildingSpellClosure_registerBuildingSpell_RaceDesertAbilities_cast_registerBuildingSpell_RaceDesertAbilities2']
    debuff_code, pulse_code = (code.to_bytes(4, 'big').decode() for code in params['carrier_ability_ids'])
    carrier = re.search(r'dummyCarrierWithAbilities\(unit_getOwner\(\w+\),%d,%d,\{0\.;3500\.\},([\d.]+)\)unit_issuePointOrderById\(\w+,%d,\{16\.;3500\.\}\)'
        % (params['carrier_ability_ids'][0], params['carrier_ability_ids'][1], params['order_id']), handler)
    fields = detail['all_fields']
    debuff, pulse = fields[debuff_code], fields[pulse_code]
    tokens = set(debuff['targs1'].split(','))
    if (carrier is None or Decimal(carrier[1]) != Decimal(params['duration_seconds'])
        or detail['all_bases'][debuff_code] != 'ACsi' or detail['all_bases'][pulse_code] != 'ANpi'
        or tokens - {'air', 'ground', 'enemies', 'vulnerable'} or tokens != set(pulse['targs1'].split(','))
        or number(debuff['dataa1']) != 0 or number(pulse['datab1']) != 0 or number(pulse['datac1']) != 0
        or Decimal(debuff['area1']) < 20000 or Decimal(pulse['area1']) < 20000
        or len(debuff['buffid1']) != 4 or number(debuff['cast1']) != 0):
        raise ValueError('Sandstorm control flow changed: owner dummy with a map-wide silence and immolation pair')
    movement = tokens & {'air', 'ground'}
    mask = sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement)
    return {'Sandstorm': {'debuff': {'ability': raw(debuff_code), 'buff': raw(debuff['buffid1']), 'targets': mask,
        'organic_only': False, 'duration_ticks': number(debuff['dur1'], 30), 'hero_duration_ticks': number(debuff['herodur1'], 30),
        'movement_percent_delta': number(debuff['datac1'], 100), 'attack_speed_percent_delta': number(debuff['datad1'], 100),
        'damage_bonus_per_10k': 0, 'ethereal': None, 'miss_chance_per_10k': number(debuff['datab1'], 10000)},
        'pulse_ability': raw(pulse_code), 'pulse_damage': number(pulse['dataa1']), 'pulse_interval_ticks': number(pulse['dur1'], 30),
        'duration_ticks': number(carrier[1], 30), 'targets': mask}}


def project(recipe, fields, unit, protected, detail):
    number, raw = _helpers()
    row = detail['building_spell']
    params = json.loads(row['parameters_json'])
    tokens = set(fields['targs1'].split(','))
    if detail['native_base'] != 'ACpa' or tokens != PARENT_MASK or any(number(fields[key]) != 0 for key in ('dataa1', 'datab1', 'datac1', 'datad1')):
        raise ValueError('building script parent must be the inert native Parasite with the audited hostile mask')
    kind = row['mechanic_kind']
    if kind == 'priority-random-replenish':
        child = _replenish(params, detail, number, raw)
    elif kind == 'dummy-target-illusion':
        child = _hallucination(params, detail, number, raw)
    elif kind == 'global-weather-ability-carrier':
        child = _sandstorm(params, detail, number, raw)
    else:
        raise ValueError(f'unsupported building script {kind!r}')
    rate = number(unit['mana_regen'], 10000)
    profile = {'ability': raw(recipe['source_key']), 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': False, 'child': child}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': number(unit['mana_max']), 'starting': 0, 'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected['mana_cost']),
                'cooldown_ticks': number(protected['cooldown'], 30), 'range': number(fields['rng1'], 1024),
                'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'BuildingScript': profile}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/building-spell-mechanics.tsv',
            'fields': 'resolved/object-fields.tsv', 'protected': 'resolved/protected-ability-fields.tsv',
            'functions': row['source_functions']}}
