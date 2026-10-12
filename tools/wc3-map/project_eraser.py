"""Independent native Parasite plus the source type-wide Chaos/Death damage callback."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(rows, fields, units, abilities, protected, traces, version):
    raw = lambda c: int.from_bytes(c.encode(), 'big')
    number = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: number(v, 30)
    source = lambda n: traces[n]['source']
    call = source('eraserSpell')
    callback = source('ForGroupCallback_forUnitsInRect_RaceChaosAbilities_callback_forUnitsInRect_RaceChaosAbilities')
    if ('bkr=randomAliveEnemy(akr)' not in call or 'ckr=unit_getTypeId(bkr)' not in call
            or 'dkr.targetType=ckr dkr.building=Zjr' not in call or 'ekr=uIb' not in call
            or 'unit_getTypeId(nkn)==mkn.targetType' not in callback
            or '(not Id(nkn))' not in callback or 'widget_getLife(nkn)>0.405' not in callback
            or 'unit_isEnemyOf(nkn,mkn.p)' not in callback):
        raise ValueError('Eraser must independently select a type and enumerate live battlefield matches')
    params = json.loads(rows[0]['parameters_json'])
    if any(json.loads(row['parameters_json']) != params or row['ability_rawcode'] != rows[0]['ability_rawcode'] for row in rows):
        raise ValueError('Eraser tiers must retain the same spell effect')
    if (f'__wurst_safe_UnitDamageTarget(pkn,qkn,{params["damage"]}.,true,false,rkn,skn,tkn)' not in callback
            or 'rkn=ATTACK_TYPE_CHAOS skn=DAMAGE_TYPE_DEATH' not in callback
            or f'unit_getAbilityLevel(nkn,{params["excluded_ability_id"]})<=0' not in callback
            or callback.index('addEffect1') > callback.index('unit_getAbilityLevel')):
        raise ValueError('Eraser must retain Chaos/Death packets and recipient art before A070 rejection')
    shield = source('Od')
    shield_ids = re.findall(r'GetUnitAbilityLevel\(\w+,(\d+)\)>(\d+)', shield)
    if len(shield_ids) != 1 or 'nd(bgo)return true' not in shield:
        raise ValueError('Eraser shield gate must remain distinct from ordinary negative-effect checks')
    shield_code = int(shield_ids[0][0])
    removal = source('nd')
    if (f'UnitRemoveAbility(Vfo,{shield_code})' not in removal
            or 'SetUnitState' in removal or 'SetWidgetLife' in removal
            or 'Od(beo)' not in source('Id')):
        raise ValueError('Eraser must remove the complete shield without native healing')
    native_art = lambda text: re.findall(r'AddSpecialEffectTarget\("(.*?)",\w+,"(.*?)"', text)
    if (native_art(removal) != native_art(source('Vd'))
            or native_art(source('Id')) != native_art(source('checkForShield'))):
        raise ValueError('Eraser shield interception no longer shares the projected native visual bindings')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(\w+,(\d+)\)<=0', selector)))
    excluded_type = re.findall(r'unit_getTypeId\(\w+\)==(\d+)', selector)
    if len(exclusions) != 2 or len(excluded_type) != 1 or 'isAliveCombatSapper' not in selector:
        raise ValueError('Eraser initial picker exclusions changed')
    base = lambda c: next(a['base_rawcode'] for a in abilities if a['rawcode'] == c)
    for code in ['A028', 'A029']:
        f = fields[code]
        if (base(code) != 'Aasl' or f['atar:1'] != 'self'
                or any(number(f[k]) != 0 for k in ['aare:1', 'Slo1:1', 'Slo2:1'])):
            raise ValueError('Eraser permanent self-aura decorations acquired a gameplay payload')
    parent_code = rows[0]['ability_rawcode']
    parent = fields[parent_code]
    if base(parent_code) != 'ACpa' or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}:
        raise ValueError('Eraser native parent class/mask changed')
    native_parent = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    p = {'map_version': version, 'ability': raw(parent_code), 'parent': native_parent,
        'trigger_targets': 5, 'trigger_mechanical_only': True, 'selector_exclusions': exclusions,
        'selector_excluded_rawcode': int(excluded_type[0]), 'excluded_ability': params['excluded_ability_id'],
        'shield_ability': shield_code, 'shield_minimum_level': int(shield_ids[0][1]) + 1,
        'damage': params['damage']}
    cost = lambda f: next(r['runtime_value'] for r in protected if r['rawcode'] == parent_code and r['field'] == f)
    casts = {}
    for row in rows:
        unit = units[row['building_rawcode']]
        amount = lambda key: 0 if unit[key].strip() in {'', '-', '_'} else number(unit[key])
        casts[row['building_rawcode']] = {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | number(unit['mana_regen'], 10000)},
            'ability': {'id': raw(parent_code), 'mana_cost': number(cost('mana_cost')), 'cooldown_ticks': tick(cost('cooldown')),
                'range': number(parent['aran:1'], 1024), 'target_policy': 'NativeBuildingSpellTrigger',
                'effect': {'BuildingEraser': p}}}
    caster_art = re.search(r'addEffect\("(.*?)",unit_getPos', call)
    target_art = re.search(r'addEffect1\("(.*?)",\w+,"(.*?)"', callback)
    if not caster_art or not target_art:
        raise ValueError('Eraser cast/recipient presentation changed')
    visuals = [{'role': 'caster', 'path': json.loads('"'+caster_art[1]+'"')},
               {'role': 'target', 'path': json.loads('"'+target_art[1]+'"'), 'attachment': target_art[2]}]
    return casts, visuals
