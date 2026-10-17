"""Project the independently selected, single-shot native building attack proxy."""
import json
import re
from decimal import Decimal, ROUND_CEILING

from project_naga_unit import project_parasite


def project(row, fields, units, abilities, protected, traces, version):
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    integer = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: integer(v, 30)
    params = json.loads(row['parameters_json'])
    callback = traces['shrineOfDestructionSpell']['source']
    selector = traces[row['source_functions'].split(',')[-1]]['source']
    dummy = params['dummy_unit_id'].to_bytes(4, 'big').decode()
    freeze = params['freeze_ability_id'].to_bytes(4, 'big').decode()
    poison = params['damage_over_time_ability_id'].to_bytes(4, 'big').decode()
    parent_code = row['ability_rawcode']
    parent, body = fields[parent_code], fields[dummy]
    base = lambda code: next(a['base_rawcode'] for a in abilities if a['rawcode'] == code)
    required = ['group_getRandom', 'if(rkr==nil)then return end',
        f'createUnit(okr,{raw(dummy)},unit_getPos(nkr),{{0.}})',
        f'__wurst_safe_UnitApplyTimedLife(Jkr,{raw("BTLF")},{params["timed_life_seconds"]}.)',
        f'unit_issueTargetOrderById(skr,{params["order_id"]},rkr)']
    if (row['mechanic_kind'] != 'spawn-attack-dummy-with-round-scaling'
            or not all(token in callback for token in required) or 'checkForShield' in callback):
        raise ValueError('attack proxy must retain caster origin, independent selector and timed attack order')
    exclusions = list(map(int, re.findall(r'not\(unit_getTypeId\(wkn\)==(\d+)\)', selector)))
    marker = re.search(r'unit_getAbilityLevel\(wkn,(\d+)\)<=0', selector)
    if (exclusions != params['excluded_unit_ids'] or not marker
            or int(marker[1]) != params['excluded_ability_id']
            or not all(s in selector for s in ['widget_getLife(wkn)>0.405', 'unit_isEnemyOf(wkn,vkn.p)',
                'UNIT_TYPE_STRUCTURE', 'player_getId(unit_getOwner(wkn))<bj_MAX_PLAYERS'])):
        raise ValueError('attack proxy must retain normal-player living structure exclusions')
    schedule = params['ability_level_schedule']
    actual = re.findall(r'(?:if|elseif)\(VGb>(\d+)\)then.*?SetUnitAbilityLevel\(\w+,(\d+),(\d+)\).*?SetUnitAbilityLevel\(\w+,(\d+),(\d+)\)', callback)
    expected = [(str(s['round_minutes_gt']), str(raw(freeze)), str(s['level']), str(raw(poison)), str(s['level'])) for s in reversed(schedule)]
    if actual != expected:
        raise ValueError('both native abilities must use the same descending whole-minute schedule')
    if ('UGb=(UGb+1)if(UGb==60)then UGb=0 VGb=(VGb+1)end' not in traces['yd']['source']
            or 'VGb=0 UGb=0' not in traces['completeRoundStart']['source']):
        raise ValueError('round scaling requires reset, whole elapsed minutes')
    if (base(parent_code) != 'ACpa' or parent['abuf:1'] != ''
            or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}):
        raise ValueError('native Parasite trigger mask changed')
    if (base(freeze) != 'Afrz' or base(poison) != 'ACvs'
            or set(body['uabi:0'].split(',')) != {'Aloc', 'Avul', freeze, poison}
            or body['ua1w:0'] != 'missile' or body['ua1t:0'] != 'chaos'
            or set(body['ua1g:0'].split(',')) != {'structure', 'enemy'}
            or body['ua2w:0'] != '_' or body['ua2g:0'] != '_'
            or body['umvt:0'] != 'fly' or body['umdl:0'].lower() != 'none.mdl'
            or integer(body['udp1:0']) != 0 or integer(body['ubs1:0']) != 0):
        raise ValueError('proxy requires an immediate, ordinary single-target Chaos missile and inert second weapon')
    objects = json.loads(row['effect_objects_json'])
    weapon = next(o['protected_unitstat'] for o in objects if o['rawcode'] == dummy)
    lifetime = tick(params['timed_life_seconds'])
    if (tick(weapon['attack1_cooldown']) <= lifetime
            or Decimal(weapon['attack1_min']) != Decimal(weapon['attack1_max'])):
        raise ValueError('single-shot proxy requires fixed damage and cooldown beyond timed life')
    native = fields[poison]
    stacking = integer(native['Poi4:1'])
    if stacking != 1:
        raise ValueError('proxy poison requires the audited nonlethal damage-stacking native mode')
    buffs = native['abuf:1'] or native['native_base_buff']
    buff = buffs.split(',')[1 if stacking else 0]
    levels = []
    for level in range(1, integer(fields[freeze]['alev:0']) + 1):
        f, p = fields[freeze], fields[poison]
        at = lambda obj, key: obj.get(f'{key}:{level}', obj[f'{key}:1'])
        if (set(at(f, 'atar').split(',')) != {'enemies', 'ground', 'structure'}
                or set(at(p, 'atar').split(',')) != {'enemies', 'structure'}
                or any(integer(at(p, key)) != 0 for key in ['Poi2', 'Poi3'])):
            raise ValueError('independent native freeze and structure poison masks changed')
        levels.append({'freeze_ticks': tick(at(f, 'adur')), 'hero_freeze_ticks': tick(at(f, 'ahdu')),
            'poison': {'ability': raw(poison), 'buff_rawcode': raw(buff),
                'damage_per_second': integer(at(p, 'Poi1')), 'movement_percent_delta': 0,
                'attack_speed_percent_delta': 0, 'duration_ticks': tick(at(p, 'adur')),
                'hero_duration_ticks': tick(at(p, 'ahdu')), 'targets': 4,
                'stack_damage': bool(stacking & 1), 'can_kill': False}})
    native_parent = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    profile = {'map_version': version, 'parent': native_parent, 'trigger_targets': 5, 'trigger_mechanical_only': False, 'trigger_spell_immune': False, 'dummy_rawcode': raw(dummy),
        'freeze_ability': raw(freeze), 'levels': levels,
        'level_after_minutes': [s['round_minutes_gt'] + 1 for s in schedule],
        'excluded_rawcodes': exclusions, 'excluded_ability': int(marker[1]),
        'damage': integer(weapon['attack1_min']), 'range': integer(weapon['attack1_range'], 1024),
        'speed_per_tick': int(Decimal(str(body['ua1z:0'])) * 1024 / 30),
        'dummy_ticks': lifetime, 'vision_radius': integer(re.search(r'tkr\[2\],([\d.]+),true,false', callback)[1], 1024),
        'vision_ticks': tick(re.search(r'doAfter\(([\d.]+),vkr\)', callback)[1])}
    cost = lambda field: next(p['runtime_value'] for p in protected if p['rawcode'] == parent_code and p['field'] == field)
    unit = units[row['building_rawcode']]
    amount = lambda k: 0 if unit[k].strip() in {'', '-', '_'} else integer(unit[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | integer(unit['mana_regen'], 10000)},
        'ability': {'id': raw(parent_code), 'mana_cost': integer(cost('mana_cost')),
            'cooldown_ticks': tick(cost('cooldown')), 'range': integer(parent['aran:1'], 1024),
            'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'BuildingAttackProxy': profile}}}
