"""Source-owned point Flame Strike, independent of the native Parasite trigger."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(row, fields, units, abilities, protected, traces, version):
    raw = lambda c: int.from_bytes(c.encode(), 'big')
    number = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: number(v, 30)
    source = lambda n: traces[n]['source']
    params = json.loads(row['parameters_json'])
    codes = [params[k].to_bytes(4, 'big').decode() for k in ['normal_ability_id', 'alternate_mode_ability_id']]
    call = source('volcanoSpell')
    if (row['mechanic_kind'] != 'dummy-point-ability' or row['target_selector'] != 'random-alive-enemy-ground'
            or 'mkr=randomAliveEnemyGround(lkr)if(mkr==nil)then return end' not in call
            or f'dummyCastPointFrom(lkr,cBb,{params["order_id"]},unit_getPos(mkr),unit_getPos(mkr),{params["dummy_lifetime_seconds"]}.)' not in call):
        raise ValueError('Flame Strike must retain independent ground selection and selected-point origin')
    mode = source('setArtilleryModeAbilityIds').split(' else ')
    if len(mode) != 2 or any(f'cBb={raw(c)}' not in mode[i] for i, c in enumerate(codes)):
        raise ValueError('Flame Strike mode ability order changed')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(COn,(\d+)\)<=0', selector)))
    if len(exclusions) != 2 or not all(t in selector for t in ['isAliveCombatSapper(COn)', 'unit_isEnemyOf(COn,BOn.p)', 'UNIT_TYPE_GROUND']):
        raise ValueError('Flame Strike selector classification changed')
    dummy = int(re.search(r'Tdb=__wurst_ensureInt\((\d+)\)', source('QG'))[1]).to_bytes(4, 'big').decode()
    if (f'createUnit(uAb,Tdb,Nvq,Ovq)' not in source('createDummy')
            or 'SetUnitState(JTr,KTr,1000000.)' not in source('dummyCastPointFrom')
            or 'recycleSpellDummy(IOn.dummy,IOn.abilityId)' not in source('CallbackSingle_doAfter_SpellHelpers_call_doAfter_SpellHelpers2')
            or 'unit_removeAbility(tSr,uSr)DummyRecycler_recycle(tSr)' not in source('recycleSpellDummy')
            or number(fields[dummy]['ucpt:0']) != 0):
        raise ValueError('Flame Strike requires the zero-cast-point recycled native dummy and delayed ability removal')
    cost = lambda c, f: next(p['runtime_value'] for p in protected if p['rawcode'] == c and p['field'] == f)
    base = lambda c: next(a['base_rawcode'] for a in abilities if a['rawcode'] == c)
    parent_code = row['ability_rawcode']
    parent = fields[parent_code]
    if (base(parent_code) != 'ACpa' or parent['abuf:1'] != ''
            or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}):
        raise ValueError('Flame Strike parent requires its independent native mechanical Parasite mask')
    options = []
    for c in codes:
        f = fields[c]
        targets = set(f['atar:1'].split(','))
        if (base(c) != 'AHfs' or targets - {'enemies', 'ground', 'neutral', 'self', 'structure', 'debris', 'tree'}
                or not {'enemies', 'ground', 'self'} <= targets
                or any(number(cost(c, k)) != 0 for k in ['mana_cost', 'cooldown'])):
            raise ValueError('Flame Strike child native mask/resources changed')
        p = {'ability': raw(c), 'buff': raw(f['abuf:1']), 'radius': number(f['aare:1'], 1024),
            'cast_ticks': tick(f['acas:1']), 'full_damage': number(f['Hfs1:1']),
            'full_interval_millis': number(f['Hfs2:1'], 1000), 'half_damage': number(f['Hfs3:1']),
            'half_interval_millis': number(f['Hfs4:1'], 1000), 'building_damage_per_10k': number(f['Hfs5:1'], 10000),
            'maximum_damage': number(f['Hfs6:1']), 'full_duration_millis': number(f['ahdu:1'], 1000),
            'duration_millis': number(f['adur:1'], 1000), 'targets': 1 | (4 if 'structure' in targets else 0)}
        if p['cast_ticks'] >= tick(params['dummy_lifetime_seconds']) or any(p[k] <= 0 for k in ['radius', 'full_interval_millis', 'half_interval_millis', 'maximum_damage']):
            raise ValueError('Flame Strike must release before recycling, with positive clocks and native cap')
        options.append(p)
    native_parent = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    p = {'map_version': version, 'parent': native_parent, 'trigger_targets': 5, 'trigger_mechanical_only': False,
        'selector_exclusions': exclusions, 'dummy_rawcode': raw(dummy), 'dummy_flying': fields[dummy]['umvt:0'] == 'fly',
        'dummy_ticks': tick(params['dummy_lifetime_seconds']), 'options': options}
    u = units[row['building_rawcode']]
    amount = lambda k: 0 if u[k].strip() in {'', '-', '_'} else number(u[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | number(u['mana_regen'], 10000)},
        'ability': {'id': raw(parent_code), 'mana_cost': number(cost(parent_code, 'mana_cost')),
            'cooldown_ticks': tick(cost(parent_code, 'cooldown')), 'range': number(parent['aran:1'], 1024),
            'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'BuildingFlameStrike': p}}}
