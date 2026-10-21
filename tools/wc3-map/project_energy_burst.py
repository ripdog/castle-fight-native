"""Energy Tower's script-dispatched Chain Lightning proc on landed weapon damage."""
import json
import re
from decimal import Decimal, ROUND_CEILING


def project(row, fields, protected, traces):
    params = json.loads(row['parameters_json'])
    source = params['source_unit_id']
    marker = int.from_bytes(b'A03Q', 'big')
    child = params['effect_ability_id']
    code = child.to_bytes(4, 'big').decode()
    dispatch = traces['handleSourceDamageEffects']['source']
    proc = traces['h05LAttackProc']['source']
    if f'unit_getAbilityLevel(Qpq,{marker})<=0' not in dispatch or f'Spq=={source})then h05LAttackProc(Qpq,Rpq)' not in dispatch:
        raise ValueError('Energy Burst must preserve the dispatcher marker and source-type gates')
    roll = re.search(r'GetRandomInt\(0,99\)<(\d+)', proc)
    life = re.search(r'UnitApplyTimedLife\(\w+,1112820806,([\d.]+)\)', proc)
    if (not roll or not life or int(roll[1]) != params['proc_probability_percent']
            or Decimal(life[1]) != Decimal(str(params['dummy_timed_life_seconds']))
            or f'addProtectedAbility(Coq,{child})' not in proc
            or f'unit_issueTargetOrderById(Boq,{params["target_order_id"]},yoq)' not in proc
            or 'unit_getPos(xoq)' not in proc or 'CreateUnit(zoq,dV' not in proc):
        raise ValueError('Energy Burst random threshold, origin, child, target order or lifetime drifted')
    f = fields[code]
    ticks = lambda v: int((Decimal(str(v)) * 30).to_integral_value(rounding=ROUND_CEILING))
    runtime = {r['field']: r['runtime_value'] for r in protected if r['rawcode'] == code and r['level'] == '1'}
    tokens = set(f['atar:1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (Decimal(runtime['mana_cost']) != 0 or not {'enemy', 'enemies'} & tokens or not movement
            or tokens - {'air', 'ground', 'enemy', 'enemies', 'neutral'}
            or int(f['Ocl1:1']) != params['effect_damage'] or int(f['Ocl2:1']) != params['effect_target_count']
            or Decimal(str(f['Ocl3:1'])) != Decimal(str(params['effect_damage_reduction_per_bounce']))):
        raise ValueError('Energy Burst requires a free hostile native Chain Lightning that matches its runtime trace')
    return {'source_rawcode': source, 'helper': marker, 'dispatcher': marker, 'granted_at_construction': False,
        'chance_per_10k': int(roll[1]) * 100, 'range': int(Decimal(str(f['aran:1'])) * 1024),
        'dummy_lifetime_ticks': ticks(life[1]), 'child_cooldown_ticks': ticks(runtime['cooldown']),
        'chain': {'ability': child, 'initial_damage': int(f['Ocl1:1']), 'maximum_targets': int(f['Ocl2:1']),
            'jump_radius': int(Decimal(str(f['aare:1'])) * 1024),
            'damage_reduction_per_10k': int(Decimal(str(f['Ocl3:1'])) * 10000),
            'targets': sum(bit for token, bit in (('ground', 1), ('air', 2)) if token in movement)}}
