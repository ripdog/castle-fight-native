"""Native building parent and independent map callback, from retained source only."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project_pyramid(row, fields, units, abilities, protected, traces, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda value: integer(value, 30)
    params = json.loads(row['parameters_json'])
    source = lambda name: traces[name]['source']
    parent = fields[row['ability_rawcode']]
    child = params['effect_ability_id'].to_bytes(4, 'big').decode()
    native = fields[child]
    a = next(a for a in abilities if a['rawcode'] == child and a['level'] == '1')
    parent_a = next(a for a in abilities if a['rawcode'] == row['ability_rawcode'] and a['level'] == '1')
    if (row['mechanic_kind'] != 'dummy-immediate-ability-at-target-point'
        or row['target_selector'] != 'random-alive-enemy-ground'
        or 'Etr=randomAliveEnemyGround(Dtr)' not in source('pyramidSpell')
        or 'if(Etr==nil)then return end' not in source('pyramidSpell')
        or f'dummyCastImmediateFrom1(Dtr,{raw(child)},{params["order_id"]},unit_getPos(Etr),{params["dummy_lifetime_seconds"]}.)' not in source('pyramidSpell')):
        raise ValueError('earthquake requires independent ground selection and selected-point dummy')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1')
    if not all(token in selector for token in ('isAliveCombatSapper(COn)', 'unit_isEnemyOf(COn,BOn.p)', 'UNIT_TYPE_GROUND')):
        raise ValueError('ground selector must retain living combat, enemy and ground guards')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(COn,(\d+)\)<=0', selector)))
    if len(exclusions) != 2 or 'unit_getTypeId' in selector:
        raise ValueError('ground selector requires exactly its two native marker exclusions')
    if (parent_a['base_rawcode'] != 'ACpa'
        or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}
        or parent['abuf:1'] != ''
        or any(Decimal(str(parent[f'Poi{i}:1'])) != 0 for i in range(1, 5))):
        raise ValueError('building trigger requires its empty, zero-effect native Parasite parent')
    if (a['base_rawcode'] != 'AHtc' or set(native['atar:1'].split(',')) != {'enemies', 'ground'}
        or integer(native['Htc2:1']) != 0 or integer(native['acas:1']) != 0):
        raise ValueError('earthquake requires immediate ground native Thunder Clap without specific-target damage')
    cost = lambda ability, field: next(p['runtime_value'] for p in protected if p['rawcode'] == ability and p['field'] == field)
    if any(Decimal(str(cost(child, field))) != 0 for field in ('mana_cost', 'cooldown')):
        raise ValueError('earthquake dummy requires free protected native resources')
    unit = units[row['building_rawcode']]
    amount = lambda key: 0 if unit[key].strip() in {'', '-', '_'} else integer(unit[key])
    rate = integer(unit['mana_regen'], 10000)
    profile = {'map_version': version, 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': True, 'selector_exclusions': exclusions,
        'child': raw(child), 'buff': raw(native['abuf:1']), 'radius': integer(native['aare:1'], 1024),
        'damage': integer(native['Htc1:1']), 'movement_percent_delta': -integer(native['Htc3:1'], 100),
        'attack_speed_percent_delta': -integer(native['Htc4:1'], 100),
        'duration_ticks': tick(native['adur:1']), 'hero_duration_ticks': tick(native['ahdu:1']), 'targets': 1}
    # An empty native buff list falls back to the base ability; it is not Channel.
    profile['parent'] = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {'id': raw(row['ability_rawcode']),
        'mana_cost': integer(cost(row['ability_rawcode'], 'mana_cost')),
        'cooldown_ticks': tick(cost(row['ability_rawcode'], 'cooldown')),
        'range': integer(parent['aran:1'], 1024), 'target_policy': 'NativeBuildingSpellTrigger',
        'effect': {'NagaEarthquake': profile}}}
