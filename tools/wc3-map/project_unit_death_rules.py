"""Native fJ death/kill callbacks, with retained object identity and execution order."""
import json
import re


def project(rows, functions, wisp=None):
    source = functions['fJ']
    if ('if isCombatSapper(M2q)then' not in source
        or 'if(not(N2q==nil))then if(not unit_isType(N2q,UNIT_TYPE_STRUCTURE))then' not in source):
        raise ValueError('native death callback eligibility changed')
    result = {}
    for row in rows:
        kind = row['mechanic_kind']
        if kind not in {'kill-heal-percent-max-hp', 'death-retaliation-damage-to-killer', 'organic-kill-eternal-servitude', 'source-damage-stacking-blood-corrosion', 'attack-stacking-corrosion'}:
            continue
        p = json.loads(row['parameters_json'])
        raw = int.from_bytes(row['unit_rawcode'].encode(), 'big')
        value = result.setdefault(raw, {'rawcode': raw, 'heal_percent': 0, 'retaliation_damage': 0})
        if kind == 'source-damage-stacking-blood-corrosion':
            levels = p['stack_levels']
            step = levels[0]['defense_bonus']
            reducer = functions['applyVampireArmorReduction']
            if (p['initial_stack_level'] != 1 or p['stack_increment_per_damage_event'] != 1 or not p['target_must_not_be_structure']
                    or [lv['level'] for lv in levels] != list(range(1, p['maximum_stack_level'] + 1))
                    or any(lv['defense_bonus'] != step * lv['level'] for lv in levels)
                    or f"unit_getTypeId(kqq)=={raw}" not in reducer
                    or f"addProtectedAbility(lqq,{p['target_stack_ability_id']})" not in reducer
                    or f"elseif(mqq<{p['maximum_stack_level']})then" not in reducer):
                raise ValueError('Blood Corrosion control flow changed')
            value['armor_stack'] = {'ability': p['target_stack_ability_id'],
                'armor_per_level_per_100': int(step * 100), 'maximum_level': p['maximum_stack_level']}
        elif kind == 'attack-stacking-corrosion':
            # The Emerald Dragon: its source-damage branch raises the victim's permanent armor
            # penalty one level per damage event, up to the retained maximum.
            levels = p['stack_levels']
            step = levels[0]['defense_bonus']
            stack = functions['tornadoStackProc']
            dispatcher = functions['handleSourceDamageEffects']
            marker = p['source_damage_marker_ability_id']
            if (p['initial_stack_level'] != 1 or p['stack_increment_per_hit'] != 1
                    or [lv['level'] for lv in levels] != list(range(1, p['maximum_stack_level'] + 1))
                    or any(lv['defense_bonus'] != step * lv['level'] for lv in levels)
                    or f"unit_getAbilityLevel(Qpq,{marker})<=0)then return end" not in dispatcher
                    or f"Spq=={raw})then tornadoStackProc(Rpq)" not in dispatcher
                    or f"addProtectedAbility(Joq,{p['target_stack_ability_id']})" not in stack
                    or f"elseif(Koq<{p['maximum_stack_level']})then" not in stack
                    or f"unit_getAbilityLevel(Joq,{p['target_stack_ability_id']})" not in stack):
                raise ValueError('Corrosive Breath control flow changed')
            value['armor_stack'] = {'ability': p['target_stack_ability_id'],
                'armor_per_level_per_100': int(step * 100), 'maximum_level': p['maximum_stack_level']}
        elif kind == 'organic-kill-eternal-servitude':
            tentacle = re.search(r'GetUnitAbilityLevel\(\w+,(\d+)\)<=0', functions['isNotTentacle'])
            branch = re.search(r'\(\(\(\(R2q==%d\)and isNotTentacle\(M2q\)\)and\(not unit_isType\(M2q,UNIT_TYPE_MECHANICAL\)\)\)and\(not unit_isType\(M2q,UNIT_TYPE_UNDEAD\)\)\)then (\w+)=M2q (\w+)=tupleCopy1\(unit_getPos\(\1\)\)pFb=\1 createUnit\(unit_getOwner\(N2q\),%d,\2,\{0\.\}\)(\w+)=\1 __wurst_safe_RemoveUnit\(\3\)' % (raw, p['spawned_unit_id']), source)
            if (tentacle is None or branch is None or not p['victim_requires_combat_sapper'] or not p['killer_must_not_be_structure']
                    or not p['victim_requires_not_mechanical'] or not p['victim_requires_not_undead'] or not p['victim_requires_not_tentacle']
                    or not p['remove_original_dead_unit'] or p['spawn_owner'] != 'killer-owner' or p['spawn_position'] != 'victim-position'):
                raise ValueError('Eternal Servitude control flow changed')
            value['servitude'] = {'spawn_rawcode': p['spawned_unit_id'], 'excluded_ability': int(tentacle[1])}
        elif kind == 'kill-heal-percent-max-hp':
            if (p['implementation'] != 'set-life-percent-current-plus-delta'
                or not p['victim_requires_combat_sapper'] or not p['killer_must_not_be_structure']
                or not re.search(r'R2q=='+str(raw)+r'\)then.*?GetUnitLifePercent\(N2q\)\+'+str(p['heal_percent_of_max_hp'])+r'\.', source)):
                raise ValueError('native kill-heal ownership/formula changed')
            value['heal_percent'] = p['heal_percent_of_max_hp']
        else:
            if (p['damage_source'] != 'dying-unit' or p['damage_target'] != 'killer'
                or p['attack_type'] != 'chaos' or p['damage_type'] != 'normal' or not p['is_attack']
                or p['is_ranged'] or not p['dying_unit_requires_combat_sapper']
                or not p['killer_required'] or not p['killer_must_not_be_structure']):
                raise ValueError('unsupported native death retaliation shape')
            match = re.search(r'if\(unit_getTypeId\(M2q\)=='+str(raw)+r'\)then __wurst_safe_UnitDamageTarget\(M2q,N2q,([\d.]+),true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_NORMAL,WEAPON_TYPE_WHOKNOWS\)end R2q=unit_getTypeId\(N2q\)', source)
            if not match or float(match[1]) != p['damage']:
                raise ValueError('native retaliation must precede killer heal/type branch')
            value['retaliation_damage'] = p['damage']
    if wisp is not None:
        _death_burst(result, wisp, functions)
    return [result[k] for k in sorted(result)]


def _death_burst(result, wisp, functions):
    """The Ancient of Wonders' wisp: a spell blast on death plus a lingering Slow Aura dummy."""
    from decimal import Decimal, ROUND_CEILING
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    params, fields = wisp['system'], wisp['abilities']
    raw = params['wisp_unit_id']
    death, callback = functions['fJ'], functions['ForGroupCallback_forUnitsInRange_RaceElementalAbilities_callback_forUnitsInRange_RaceElementalAbilities']
    branch = re.search(r'elseif\(unit_getTypeId\(M2q\)==%d\)then (\w+)=M2q (\w+)=unit_getOwner\(\1\)(\w+)=tupleCopy1\(unit_getPos\(\1\)\)'
        r'(\w+)=qu:create834\(\)\4\.p=\2 \4\.wisp=\1 (\w+)=tupleCopy1\(\3\)(\w+)=\4 forUnitsInRange\(\5,([\d.]+),false,\6\)'
        r'(\w+)=__wurst_safe_CreateUnit\(\2,(\w+),unit_getX\(\1\),unit_getY\(\1\),0\.\)(\w+)=\8 addProtectedAbility\(\10,%d\)'
        r'__wurst_safe_UnitApplyTimedLife\(\8,1112820806,([\d.]+)\)' % (raw, params['slow_aura_ability_id']), death)
    filtered = re.search(r'isAliveCombatSapper\((\w+)\)and unit_isEnemyOf\(\1,\w+\.p\)\)and\(unit_getAbilityLevel\(\1,%d\)<=0\)\)and'
        r'\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(not\(unit_getTypeId\(\1\)==%d\)\)' % (int.from_bytes(b'Avul', 'big'), raw), callback)
    damage = re.search(r'(\w+)=ATTACK_TYPE_NORMAL (\w+)=DAMAGE_TYPE_MAGIC (\w+)=WEAPON_TYPE_WHOKNOWS '
        r'__wurst_safe_UnitDamageTarget\(\w+,\w+,([\d.]+),true,false,\1,\2,\3\)', callback)
    code = params['slow_aura_ability_rawcode']
    slow = fields[code]
    tokens = set(slow['targs1'].split(','))
    if (branch is None or filtered is None or damage is None or Decimal(damage[4]) != params['death_damage_amount']
            or Decimal(branch[7]) != params['death_damage_radius'] or Decimal(branch[11]) != Decimal(params['slow_aura_timed_life_seconds'])
            or number(slow['area1']) != params['slow_aura_radius'] or params['death_damage_attack_type'] != 'normal'
            or params['death_damage_type'] != 'magic' or 'sapper' not in tokens or 'enemies' not in tokens
            or Decimal(slow['dataa1']) != Decimal(str(params['movement_speed_factor']))
            or Decimal(str(params['attack_speed_factor'])) != 0 or len(slow['buffid1']) != 4):
        raise ValueError('Wisp death control flow changed: spell blast on a dying combat wisp, then a timed Slow Aura dummy')
    movement = sum(bit for token, bit in (('ground', 1), ('air', 2)) if token in tokens)
    entry = result.setdefault(raw, {'rawcode': raw, 'heal_percent': 0, 'retaliation_damage': 0})
    entry['death_burst'] = {'damage': number(damage[4]), 'radius': number(branch[7], 1024),
        'excluded_ability': int(filtered[2]), 'excluded_type': raw,
        'slow': {'ability': params['slow_aura_ability_id'], 'buff': int.from_bytes(slow['buffid1'].encode(), 'big'),
            'radius': number(slow['area1'], 1024), 'movement_percent_delta': number(slow['dataa1'], 100),
            'attack_speed_percent_delta': number(params['attack_speed_factor'], 100), 'targets': movement,
            'affects_invulnerable': 'invulnerable' in tokens, 'duration_ticks': number(branch[11], 30)}}
