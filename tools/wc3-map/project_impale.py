"""Native Impale and a source-traced fan of independent recycled dummy casts."""
from decimal import Decimal, ROUND_CEILING
import json
import re
from project_naga_unit import project_parasite


def project(recipe, fields, unit, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda value: number(value, 30)
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    row, semantic = detail['mechanics_row'], detail['semantics_row']
    parameters = json.loads(semantic['parameters_json'])
    helper = detail['functions'][row['helper_functions']]
    child = semantic['effect_rawcodes']
    native = detail['all_fields'][child]
    if (semantic['semantic_kind'] != 'four-lane-cone-proxy'
            or row['target_mode_label'] != 'enemy-ground-combat-sapper'
            or detail['all_bases'][child] != 'ACmp'
            or set(native['targs1'].split(',')) != {'enemies', 'ground', 'neutral'}
            or any(number(native[key]) != 0 for key in ['datae1', 'dataf1', 'cast1'])
            or number(detail['all_protected'][child]['mana_cost']) != 0
            or tick(native['datab1']) <= 0 or number(native['dataa1']) <= 0
            or number(native['datac1']) < 0):
        raise ValueError('Impale requires its native unit mask, protected free cost and protected airborne phase')
    profile = {'ability': raw(child), 'damage': number(native['datac1']),
        'distance': number(native['dataa1'], 1024), 'radius': number(native['area1'], 1024),
        'wave_ticks': tick(native['datab1']), 'air_ticks': max(0, tick(native['datad1'])),
        'stun_ticks': tick(native['dur1']), 'hero_stun_ticks': tick(native['herodur1']), 'targets': 1}
    # Compare the semantic projection with literal helper control flow and argument order.
    call = re.search(r'dummyCastPointFrom\(amr,(\d+),852555,vec2_polarOffset\(bmr,emr,([\d.]+)\),bmr,([\d.]+)\)', helper)
    loop = re.search(r'if\(dmr>(\d+)\)then break end', helper)
    angles = re.search(r'\(\(cmr-([\d.]+)\)\+\(([\d.]+)\*dmr\)\)', helper)
    if (not all([call, loop, angles]) or parameters['lane_count'] != 4
            or int(loop[1]) + 1 != parameters['lane_count']
            or [Decimal(str(a)) for a in parameters['relative_angles_degrees']] != [
                -Decimal(angles[1]) + Decimal(angles[2]) * i for i in range(parameters['lane_count'])]
            or int(call[1]) != raw(child)
            or Decimal(call[2]) != Decimal(str(parameters['cast_origin_offset']))
            or Decimal(call[3]) != Decimal(str(parameters['dummy_lifetime_seconds']))
            or 'bmr=tupleCopy1(unit_getPos(Ylr))' not in helper
            or 'cmr=angle_degrees(vec2_angleTo(unit_getPos(Ylr),unit_getPos(Zlr)))' not in helper
            or 'DummyRecycler_get(zTr,{0.})' not in detail['functions']['dummyCastPointFrom']
            or 'unit_issuePointOrderById(BTr,xTr,yTr)' not in detail['functions']['dummyCastPointFrom']
            or 'unit_removeAbility(tSr,uSr)DummyRecycler_recycle(tSr)' not in detail['functions']['recycleSpellDummy']):
        raise ValueError('Impale fan source origin, order, lanes or fresh native ability state changed')
    lifetime = tick(parameters['dummy_lifetime_seconds'])
    offset = number(parameters['cast_origin_offset'], 1024)
    if lifetime < profile['wave_ticks'] or offset <= 0 or offset > number(native['rng1'], 1024):
        raise ValueError('Impale dummy cannot finish its wave or issue its native point cast')
    provenance = {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
        'script': 'resolved/unit-spell-semantics.tsv', 'functions': [row['handler_function'], row['helper_functions'],
            'dummyCastPointFrom', 'recycleSpellDummy', 'QG'], 'native_child_cooldown': detail['all_protected'][child].get('cooldown', native['cool1']),
        'native_base': 'ACmp', 'dummy_ability_removed_before_recycling': True}
    if recipe['source_kind'] == 'ability-effect':
        if recipe['source_key'] != child:
            raise ValueError('Impale child identity differs from its traced parent')
        return {'kind': 'native-impale', 'source_kind': recipe['source_kind'], 'source_key': child,
            'profile': profile, 'provenance': provenance}
    parent = detail['all_fields'][row['ability_rawcode']]
    if (detail['all_bases'][row['ability_rawcode']] != 'ACpa'
            or set(parent['targs1'].split(',')) != {'enemies', 'ground', 'nonhero', 'vulnerable'}
            or detail['default_active_alias'] != recipe['source_key'] or parent['animnames'] != 'death'):
        raise ValueError('Impale fan requires its independent nonhero Parasite autocast parent')
    dummy = int(re.search(r'Tdb=__wurst_ensureInt\((\d+)\)', detail['functions']['QG'])[1]).to_bytes(4, 'big').decode()
    if number(detail['all_unit_fields'][dummy]['castpt']) != 0:
        raise ValueError('Impale dummy cast delay requires a retained secondary windup')
    rate = number(unit['mana_regen'], 10000)
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'], 'spellcasting': {
            'mana': {'maximum': number(unit['mana_max']), 'starting': number(unit['mana_start']), 'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected['mana_cost']),
                'cooldown_ticks': tick(protected['cooldown']), 'range': number(parent['rng1'], 1024),
                'target_policy': 'RandomGroundEnemyUnit', 'effect': {'ImpaleFan': {
                    'parent': project_parasite(parent, detail['map_version']), 'cast_animation': 'CastDeath', 'child': profile,
                    'relative_angles_millidegrees': [number(angle, 1000) for angle in parameters['relative_angles_degrees']],
                    'cast_point_distance': offset, 'dummy_lifetime_ticks': lifetime, 'dummy_rawcode': raw(dummy),
                    'dummy_flying': detail['all_units'][dummy]['move_type'] == 'fly'}}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': child, 'provenance': provenance}
