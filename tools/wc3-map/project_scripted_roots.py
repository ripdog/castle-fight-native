"""Immediate owner-owned dummy Roots, separate from the parent Parasite order."""
import json
from decimal import Decimal


def project(recipe, fields, unit, protected, detail):
    tick = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding='ROUND_CEILING'))
    number = lambda value, scale=1: int(Decimal(str(value)) * scale)
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    row, semantic = detail['mechanics_row'], detail['semantics_row']
    child, child_fields = detail['effect_key'], detail['effect_fields']
    child_protected = detail['effect_protected']
    if detail['native_base'] != 'ACpa' or detail['all_bases'][child] != 'AEer':
        raise ValueError('Roots order must retain its separate Parasite parent and native Roots child')
    if set(fields['targs1'].split(',')) != {'enemies', 'ground', 'neutral', 'nonhero', 'vulnerable'}:
        raise ValueError('unsupported native Roots parent targeting')
    if set(child_fields['targs1'].split(',')) != {'enemies', 'ground', 'neutral'}:
        raise ValueError('unsupported native Roots effect targeting')
    if number(child_protected.get('mana_cost', child_fields['cost1'])) != 0 or number(child_protected.get('cooldown', child_fields['cool1'])) != 0:
        raise ValueError('Roots child requires audited free native resource state')
    trace = json.loads(semantic['parameters_json'])
    site = trace['delivery_sites'][0]
    if site['effect_rawcodes'] != [child] or site['callee'] != 'dummyCastTargetFrom1' or row['mechanic_kind'] != 'dummy-target-ability-from-owner':
        raise ValueError('unsupported Roots dummy origin or callback')
    if 'unit_getOwner(' not in site['arguments'][0] or 'unit_getPos(' not in site['arguments'][4]:
        raise ValueError('Roots dummy must use caster ownership and selected recipient origin')
    if Decimal(site['arguments'][5].rstrip('.')) < Decimal(child_fields['dur1']):
        raise ValueError('Roots damage outlives the audited dummy owner lifetime')
    mana = lambda key, scale=1: 0 if unit[key].strip() in {'-', '', '_'} else number(unit[key], scale)
    rate = mana('mana_regen', 10000)
    profile = {'map_version': detail['map_version'],
        'roots': {'ability': raw(child), 'damage_per_second': number(child_fields['dataa1']),
                  'duration_ticks': tick(child_fields['dur1']), 'hero_duration_ticks': tick(child_fields['herodur1']),
                  'nonhero_only': False, 'targets': 1},
        'primary_nonhero_only': True, 'search_radius': 0, 'maximum_targets': 1,
        'child_range': number(child_fields['rng1'], 1024), 'excluded_abilities': [0, 0],
        'priority_buffs': [0, 0], 'vision_radius': 0, 'vision_ticks': 0}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'],
        'source_key': recipe['source_key'], 'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': mana('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected.get('mana_cost', fields['cost1'])),
                'cooldown_ticks': tick(protected.get('cooldown', fields['cool1'])),
                'range': number(fields['rng1'], 1024), 'target_policy': 'RandomGroundEnemyUnit',
                'effect': {'ScriptedRoots': profile}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': child,
        'provenance': {'git_tree': detail['git_tree'], 'script': row['handler_function'],
            'semantics': semantic['parameters_json'], 'native_child': child}}
