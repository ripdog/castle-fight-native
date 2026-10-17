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
        'trigger_spell_immune': False, 'trigger_mechanical_only': False, 'selector_exclusions': exclusions,
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


def project_oracle(row, fields, units, abilities, protected, traces, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda value: integer(value, 30)
    params = json.loads(row['parameters_json'])
    child = params['effect_ability_id'].to_bytes(4, 'big').decode()
    parent = fields[row['ability_rawcode']]
    native = fields[child]
    source = lambda name: traces[name]['source']
    callback = source('oracleSpell')
    if (row['target_selector'] != 'random-alive-enemy' or row['mechanic_kind'] != 'dummy-target-ability'
        or 'Btr=randomAliveEnemy(Atr)' not in callback
        or 'if((Btr==nil)or checkForShield(Btr))then return end' not in callback
        or f'dummyCastTargetWithVision(Atr,{raw(child)},{params["order_id"]},Btr,unit_getPos(ztr),{params["dummy_lifetime_seconds"]}.)' not in callback):
        raise ValueError('Oracle requires independent selection, single shield attempt and target vision dummy')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(zOn,(\d+)\)<=0', selector)))
    excluded_type = re.search(r'not\(unit_getTypeId\(zOn\)==(\d+)\)', selector)
    if len(exclusions) != 2 or not excluded_type or not all(t in selector for t in ('isAliveCombatSapper(zOn)', 'unit_isEnemyOf(zOn,yOn.p)')):
        raise ValueError('Oracle selector guards changed')
    native_base = next(a['base_rawcode'] for a in abilities if a['rawcode'] == child)
    if native_base != 'ACch' or set(native['atar:1'].split(',')) != {'air', 'ground', 'nonhero', 'enemy', 'neutral'} or integer(native['acas:1']) != 0:
        raise ValueError('Oracle requires immediate native Charm with independent nonhero mask')
    if next(a['base_rawcode'] for a in abilities if a['rawcode'] == row['ability_rawcode']) != 'ACpa' or parent['abuf:1'] != '' or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}:
        raise ValueError('Oracle native parent changed')
    cost = lambda ability, field: next(p['runtime_value'] for p in protected if p['rawcode'] == ability and p['field'] == field)
    if integer(cost(child, 'mana_cost')) != 0:
        raise ValueError('Charm dummy must be free')
    ownership = source('mJ')
    rejection = re.search(r'unit_getAbilityLevel\(U3q,(\d+)\)>0\)or hasShield\(U3q\)', ownership)
    defender = re.search(r'unit_getTypeId\(U3q\)==(\d+)', ownership)
    delay = re.search(r'doAfter\(([\d.]+),Z3q\)', ownership)
    order = source('orderCodeAttackIfAllowed')
    order_block = re.search(r'unit_getAbilityLevel\(V9o,(\d+)\)>=1', order)
    if not all((rejection, defender, delay, order_block)) or '__wurst_safe_SetUnitOwner(U3q,W3q,true)' not in ownership or 'orderCodeAttackIfAllowed(U3q)' not in ownership:
        raise ValueError('Charm ownership rollback and attack ordering changed')
    profile = {'map_version': version, 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': False, 'selector_exclusions': exclusions,
        'selector_excluded_rawcode': int(excluded_type[1]), 'child': raw(child),
        'child_range': integer(native['aran:1'], 1024), 'targets': 3,
        'maximum_creep_level': integer(native['Nch1:1']), 'ownership_rejection': int(rejection[1]),
        'order_block': int(order_block[1]), 'defender_rawcode': int(defender[1]), 'defender_delay_ticks': tick(delay[1]),
        'vision_radius': integer(re.search(r'(?:getY\(\w+\)|\w+\[2\]),([\d.]+),true,false', source('dummyCastTargetWithVision1'))[1], 1024),
        'vision_ticks': tick(re.search(r'doAfter\(([\d.]+),', source('dummyCastTargetWithVision1'))[1])}
    profile['parent'] = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    unit = units[row['building_rawcode']]
    amount = lambda k: 0 if unit[k].strip() in {'', '-', '_'} else integer(unit[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | integer(unit['mana_regen'], 10000)},
        'ability': {'id': raw(row['ability_rawcode']), 'mana_cost': integer(cost(row['ability_rawcode'], 'mana_cost')),
        'cooldown_ticks': tick(cost(row['ability_rawcode'], 'cooldown')), 'range': integer(parent['aran:1'], 1024),
        'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'NagaCharm': profile}}}


def project_coral(fields, abilities, traces, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    source = traces['iD']['source']
    match = re.search(r'elseif\(mho==(\d+)\)then safeUnitAddAbility\(jho,(\d+)\)safeUnitAddAbility\(jho,(\d+)\)', source)
    if not match:
        raise ValueError('Coral native construction aura additions changed')
    building, armor_code, regen_code = [int(code).to_bytes(4, 'big').decode() for code in match.groups()]
    if fields[building]['uaen:0'] != 0 or set(fields[building]['uabi:0'].split(',')) != {'A07V', 'A09A'}:
        raise ValueError('Coral requires its unarmed native UI-only weapon modifier inventory')
    armor, regen = fields[armor_code], fields[regen_code]
    base = lambda code: next(a['base_rawcode'] for a in abilities if a['rawcode'] == code)
    if base(armor_code) != 'ACav' or base(regen_code) != 'ACnr' or base('A07V') != 'Amgl':
        raise ValueError('Coral native aura bases changed')
    if (set(armor['atar:1'].split(',')) != {'friend', 'invulnerable', 'self', 'structure', 'vulnerable'}
        or set(regen['atar:1'].split(',')) != {'air', 'friend', 'ground', 'invulnerable', 'vulnerable'}
        or any(integer(a[k]) != 0 for a in [armor, regen] for k in ['adur:1', 'ahdu:1'])
        or integer(armor['Had2:1']) != 0 or integer(regen['Oar2:1']) != 0):
        raise ValueError('Coral requires separate zero-linger flat native masks')
    return {'map_version': version, 'source_rawcode': raw(building), 'inventory': raw('A07V'),
        'armor_ability': raw(armor_code), 'armor_buff': raw(armor['abuf:1']),
        'armor_radius': integer(armor['aare:1'], 1024), 'armor_bonus_per_100': integer(armor['Had1:1'], 100),
        'armor_targets': 4, 'armor_invulnerable': True,
        'regeneration_ability': raw(regen_code), 'regeneration_radius': integer(regen['aare:1'], 1024),
        'regeneration_per_second_per_10k': integer(regen['Oar1:1'], 10000),
        'regeneration_targets': 3, 'regeneration_invulnerable': True}


def project_tidal(row, fields, units, abilities, protected, traces, version):
    raw = lambda c: int.from_bytes(c.encode(), 'big')
    integer = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: integer(v, 30)
    params = json.loads(row['parameters_json'])
    child = params['effect_ability_id'].to_bytes(4, 'big').decode()
    parent, native, building = fields[row['ability_rawcode']], fields[child], fields[row['building_rawcode']]
    callback = traces['tidalSpell']['source']
    if (row['target_selector'] != 'random-alive-enemy-ground' or row['mechanic_kind'] != 'dummy-target-ability'
        or 'ytr=randomAliveEnemyGround(xtr)' not in callback or 'if(ytr==nil)then return end' not in callback
        or f'dummyCastTargetWithVision(xtr,{raw(child)},{params["order_id"]},ytr,unit_getPos(wtr),{params["dummy_lifetime_seconds"]}.)' not in callback):
        raise ValueError('Tidal requires independent ground selection and building-origin target vision dummy')
    selector = traces['ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1']['source']
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(COn,(\d+)\)<=0', selector)))
    if len(exclusions) != 2 or not all(t in selector for t in ['isAliveCombatSapper(COn)', 'unit_isEnemyOf(COn,BOn.p)', 'UNIT_TYPE_GROUND']):
        raise ValueError('Tidal ground selector changed')
    base = lambda code: next(a['base_rawcode'] for a in abilities if a['rawcode'] == code)
    if (base(row['ability_rawcode']) != 'ACpa' or base(child) != 'ACcv'
        or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'neutral', 'nonhero', 'sapper', 'vulnerable'}
        or set(native['atar:1'].split(',')) != {'enemies', 'ground', 'neutral'}
        or any(integer(parent[f'Poi{i}:1']) != 0 for i in range(1, 5)) or integer(native['acas:1']) != 0
        or set(building['ua1g:0'].split(',')) != {'enemies', 'ground', 'sapper', 'vulnerable'}):
        raise ValueError('Tidal parent, child or ordinary weapon native eligibility changed')
    cost = lambda c, f: next(p['runtime_value'] for p in protected if p['rawcode'] == c and p['field'] == f)
    if any(integer(cost(child, f)) != 0 for f in ['mana_cost', 'cooldown']):
        raise ValueError('Tidal child must be free and immediate')
    vision = traces['dummyCastTargetWithVision1']['source']
    profile = {'map_version': version, 'source_rawcode': raw(row['building_rawcode']),
        'selector_exclusions': exclusions, 'trigger_targets': 1, 'trigger_nonhero': True, 'trigger_sapper': True,
        'trigger_invulnerable': False, 'trigger_spell_immune': False, 'child_range': integer(native['aran:1'], 1024),
        'weapon_sapper': True, 'weapon_vulnerable': True,
        'vision_radius': integer(re.search(r'(?:getY\(\w+\)|\w+\[2\]),([\d.]+),true,false', vision)[1], 1024),
        'vision_ticks': tick(re.search(r'doAfter\(([\d.]+),', vision)[1]),
        'wave': {'ability': raw(child), 'damage': integer(native['Ucs1:1']), 'maximum_damage': integer(native['Ucs2:1']),
            'distance': integer(native['Ucs3:1'], 1024), 'initial_radius': integer(native['aare:1'], 1024),
            'final_radius': integer(native['Ucs4:1'], 1024),
            'speed_per_tick': integer(Decimal(str(native['amsp:0'])) / 30, 1024), 'targets': 1}}
    profile['parent'] = project_parasite({'buffid1': parent['abuf:1'], 'missilehoming': parent['amho:0'],
        'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'], 'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'],
        'unitid1': parent['ipmu:1'], 'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    unit = units[row['building_rawcode']]
    amount = lambda k: 0 if unit[k].strip() in {'', '-', '_'} else integer(unit[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | amount('mana_regen')},
        'ability': {'id': raw(row['ability_rawcode']), 'mana_cost': integer(cost(row['ability_rawcode'], 'mana_cost')),
            'cooldown_ticks': tick(cost(row['ability_rawcode'], 'cooldown')), 'range': integer(parent['aran:1'], 1024),
            'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'NagaTidalWave': profile}}}
