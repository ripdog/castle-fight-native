"""Native building parent plus the literal Forgotten One tentacle ring, from retained source only."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(row, fields, units, abilities, protected, traces, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda value: integer(value, 30)
    params = json.loads(row['parameters_json'])
    source = lambda name: traces[name]['source']
    parent = fields[row['ability_rawcode']]
    spell = source('forgottenOneSpell')
    mode = source('setArtilleryModeAbilityIds')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1')
    life = re.search(r'__wurst_safe_UnitApplyTimedLife\(Dlr,(\d+),(\d+)\.\)', spell)
    if (row['mechanic_kind'] != 'six-dummy-ring' or row['target_selector'] != 'random-alive-enemy-ground'
            or 'xlr=randomAliveEnemyGround(wlr)' not in spell or 'if(xlr==nil)then return end' not in spell
            or 'checkForShield' in spell or 'while true do if(zlr>5)then break end' not in spell
            or 'Alr=(60.*zlr)' not in spell
            or 'vec2_polarOffset(ylr,real_asAngleDegrees(Alr),64.)' not in spell
            or 'createUnit(wlr,eBb,Blr,real_asAngleDegrees((360.-Alr)))' not in spell or not life
            or params['dummy_count'] != 6 or params['angle_step_degrees'] != 60 or params['ring_radius'] != 64
            or str(params['timed_life_seconds']) != life[2]
            or f'if Z6o then eBb={params["normal_dummy_unit_id"]}' not in mode
            or f'else eBb={params["alternate_mode_dummy_unit_id"]}' not in mode):
        raise ValueError('Forgotten One requires a six-body 60-degree ring, ground selection and mode-selected bodies')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(COn,(\d+)\)<=0', selector)))
    if len(exclusions) != 2 or 'unit_getTypeId' in selector or 'UNIT_TYPE_GROUND' not in selector:
        raise ValueError('ground selector requires exactly its two native marker exclusions')
    a = next(a for a in abilities if a['rawcode'] == row['ability_rawcode'] and a['level'] == '1')
    if (a['base_rawcode'] != 'ACpa'
            or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}
            or parent['abuf:1'] != '' or any(Decimal(str(parent[f'Poi{i}:1'])) != 0 for i in range(1, 5))):
        raise ValueError('building trigger requires its empty, zero-effect native Parasite parent')
    cost = lambda ability, field: next(p['runtime_value'] for p in protected if p['rawcode'] == ability and p['field'] == field)
    profile = {'map_version': version, 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': False, 'selector_exclusions': exclusions,
        'artillery_body': params['normal_dummy_unit_id'], 'plain_body': params['alternate_mode_dummy_unit_id'],
        'count': params['dummy_count'], 'angle_step_degrees': params['angle_step_degrees'],
        'ring_radius': integer(params['ring_radius'], 1024), 'lifetime_ticks': tick(life[2])}
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
        'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'ForgottenOne': profile}}}
