"""Source-scoped native Purge/Feral Spirit projection; recipes carry identities only."""
from decimal import Decimal, ROUND_CEILING
import re


def project_parasite(fields, version):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    tick = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    if any(Decimal(str(fields[k])) != 0 for k in ('dataa1','datab1','datac1','datad1')):
        raise ValueError('script parent requires zero native Parasite damage and slows')
    if fields['missilehoming'] != '1' or Decimal(str(fields['datae1'])) != 1:
        raise ValueError('script parent requires one native homing Parasite death summon')
    return {'buff_rawcode': raw(fields['buffid1']), 'map_version': version, 'speed_per_tick': int(Decimal(str(fields['missilespeed'])) * 1024 / 30),
        'duration_ticks': tick(fields['dur1']), 'hero_duration_ticks': tick(fields['herodur1']),
        'body_rawcode': raw(fields['unitid1']), 'count': int(fields['datae1']), 'body_duration_ticks': tick(fields['cast1'])}


def project(recipe, fields, unit, protected, detail):
    raw = lambda code: int.from_bytes(code.encode('ascii'), 'big')
    integer = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: integer(v, 30)
    source = recipe['source_key']
    child = recipe['source_kind'] == 'ability-effect'
    rate = integer(unit['mana_regen'], 10000)
    profile = {'mana': {'maximum': integer(unit['mana_max']), 'starting': integer(unit['mana_start']),
        'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {'id': raw(source),
        'mana_cost': 0 if child else integer(protected.get('mana_cost', fields['cost1'])),
        'cooldown_ticks': 0 if child else tick(protected.get('cooldown', fields['cool1'])),
        'range': integer(fields['rng1'], 1024), 'target_policy': 'ScriptedController'}}
    if recipe['kind'] == 'feral-spirit':
        if detail['native_base'] != 'AOsf' or fields['targs1'] != '_' or fields['levels'] != '1':
            raise ValueError('Feral Spirit requires an untargeted single-level native summon')
        body = fields['unitid1']
        if len(body) != 4 or integer(fields['datab1']) <= 0 or tick(fields['dur1']) <= 0:
            raise ValueError('Feral Spirit requires a resolved body, positive count and lifetime')
        effect = {'NativeSummon': {'map_version': detail['map_version'], 'ability': raw(source),
            'body_rawcode': raw(body), 'count': integer(fields['datab1']),
            'duration_ticks': tick(fields['dur1']), 'forward_distance': integer(fields['area1'], 1024)}}
        effect_raw = None
    else:
        spell = detail['mechanics_row']
        semantics = detail['semantics_row']
        if spell['target_mode_label'] != 'enemy-ground-combat-sapper' or semantics['effect_rawcodes'] != 'A02T':
            raise ValueError('Purge requires its independent ground-sapper parent and single child')
        funcs = detail['naga_functions']
        handler = funcs[spell['handler_function']]
        delayed = funcs[spell['delayed_callback_functions'].split(',')[0]]
        resume = funcs[spell['delayed_callback_functions'].split(',')[1]]
        delays = re.findall(r'doAfter\(([^,]+),', handler + delayed)
        if (f'dummyCastTargetFrom(ttn,{raw("A02T")},852111,utn,2.)' not in handler
            or 'if unit_issueImmediateOrderById(ctn.caster,852126)then' not in delayed
            or 'else orderCodeAttack(ctn.caster)end' not in delayed
            or 'orderCodeAttack(ftn.caster)' not in resume or len(delays) != 2):
            raise ValueError('Purge delayed manual Feral Spirit success/failure branch changed')
        native = detail['purge_fields']
        if detail['purge_base'] != 'ACpu' or set(native['targs1'].split(',')) != {'air','ground','tree','vulnerable','ward'}:
            raise ValueError('Purge requires its independent native child mask')
        if any(integer(detail['purge_protected'][k]) != 0 for k in ('mana_cost','cooldown')):
            raise ValueError('Purge dummy requires free native child resource fields')
        p = {'ability': raw('A02T'), 'range': integer(native['rng1'], 1024), 'targets': 3,
            'movement_factor_per_10k': integer(native['dataa1'], 10000),
            'minimum_speed_per_tick': integer(Decimal(detail['misc']['MinUnitSpeed']) * 1024 / 30),
            'duration_ticks': tick(native['dur1']), 'hero_duration_ticks': tick(native['herodur1']),
            'pause_ticks': tick(native['datad1']), 'hero_pause_ticks': tick(native['datae1']),
            'summoned_damage': integer(native['datac1'])}
        effect = {'SirenPurge': {'child': p, 'summon_ability': raw('A02S'),
            'summon_delay_ticks': tick(delays[0]), 'resume_delay_ticks': tick(delays[1]),
            'parent_nonhero_only': 'nonhero' in detail['parent_fields']['targs1'].split(','),
            'parent': project_parasite(detail['parent_fields'], detail['map_version'])}}
        profile['ability']['target_policy'] = 'RandomGroundEnemyUnit' if not child else 'ScriptedController'
        effect_raw = None if child else 'A02T'
    profile['ability']['effect'] = effect
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': source,
        'unit_rawcode': recipe['unit_rawcode'], 'spellcasting': profile,
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': effect_raw,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'protected': 'resolved/protected-ability-fields.tsv', 'script': 'resolved/unit-spell-mechanics.tsv',
            'functions': sorted(detail.get('naga_functions', {}))}}
