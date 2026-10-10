"""Immediate owner-owned dummy Roots, separate from the parent Parasite order."""
import json
import re
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
    bounded = row['mechanic_kind'] == 'delegated-helper'
    if site['effect_rawcodes'] != [child] or site['callee'] != ('dummyCastTargetWithVision' if bounded else 'dummyCastTargetFrom1'):
        raise ValueError('unsupported Roots dummy origin or callback')
    radius, cap, exclusions, priorities, vision_radius, vision_ticks = 0, 1, [0, 0], [0, 0], 0, 0
    if bounded:
        functions = detail['functions']
        helper = functions['assassinSpell']
        candidates = functions['ForGroupCallback_forUnitsInRange_RaceNelfAbilities_callback_forUnitsInRange_RaceNelfAbilities']
        vision = functions['dummyCastTargetWithVision1']
        args = site['arguments']
        if (not re.search(re.escape(args[0]) + r'=unit_getOwner\(', helper)
                or not re.search(re.escape(args[4]) + r'=tupleCopy1\(unit_getPos\(', helper)
                or 'isAliveCombatSapper(' not in candidates or 'unit_isEnemyOf(' not in candidates
                or 'UNIT_TYPE_GROUND' not in candidates or 'LinkedList_LinkedList_add(gwn.priority' not in candidates
                or 'LinkedList_LinkedList_add(gwn.fallback' not in candidates
                or 'LinkedList_LinkedList_add(Fwr,__wurst_objectToIndex(Jwr))' not in helper
                or 'Kwr=(Kwr-1)dummyCastTargetWithVision(' not in helper):
            raise ValueError('unsupported bounded Roots selection priority or ownership')
        checks = re.findall(r'unit_getAbilityLevel\(hwn,(\d+)\)<=0', candidates)
        if len(checks) != 4:
            raise ValueError('bounded Roots exclusion and buff gates changed')
        exclusions, priorities = list(map(int, checks[:2])), list(map(int, checks[2:]))
        radius = number(re.search(r'forUnitsInRange\(Nwr,([0-9.]+),false,Owr\)', helper)[1], 1024)
        cap = int(re.search(r'Kwr=(\d+) Lwr=LinkedList', helper)[1])
        if cap != trace['max_targets'] or radius != number(trace['search_radius'], 1024):
            raise ValueError('bounded Roots summary disagrees with script')
        if 'if player_hasVisibility(kTr,oTr)then return dummyCastTargetFrom2(' not in vision or 'FOG_OF_WAR_VISIBLE' not in vision:
            raise ValueError('Roots temporary visibility must be conditional on hidden recipients')
        vision_radius = number(re.search(r'rTr\[2\],([0-9.]+),true,false', vision)[1], 1024)
        vision_ticks = tick(re.search(r'doAfter\(([0-9.]+),uTr\)', vision)[1])
    else:
        if row['mechanic_kind'] != 'dummy-target-ability-from-owner' or 'unit_getOwner(' not in site['arguments'][0] or 'unit_getPos(' not in site['arguments'][4]:
            raise ValueError('Roots dummy must use caster ownership and selected recipient origin')
    if Decimal(site['arguments'][5].rstrip('.')) < Decimal(child_fields['dur1']):
        raise ValueError('Roots damage outlives the audited dummy owner lifetime')
    mana = lambda key, scale=1: 0 if unit[key].strip() in {'-', '', '_'} else number(unit[key], scale)
    rate = mana('mana_regen', 10000)
    profile = {'map_version': detail['map_version'],
        'roots': {'ability': raw(child), 'damage_per_second': number(child_fields['dataa1']),
                  'duration_ticks': tick(child_fields['dur1']), 'hero_duration_ticks': tick(child_fields['herodur1']),
                  'nonhero_only': False, 'targets': 1},
        'primary_nonhero_only': True, 'search_radius': radius, 'maximum_targets': cap,
        'child_range': number(child_fields['rng1'], 1024), 'excluded_abilities': exclusions,
        'priority_buffs': priorities, 'vision_radius': vision_radius, 'vision_ticks': vision_ticks}
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
