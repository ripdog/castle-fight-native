"""Project the Echofoot Mystic's damage-listener Echo Step and its Amin remnant mine."""
import re
from decimal import Decimal, ROUND_CEILING

TIMED_LIFE_BUFF = 1112820806  # 'BTLF', the Warcraft timed-life buff the script applies


def project(recipe, fields, unit, protected, detail):
    trace, functions = detail['echo'], detail['functions']
    fields_of = detail['all_fields']
    marker = int.from_bytes(recipe['source_key'].encode(), 'big')
    unit_id = int.from_bytes(recipe['unit_rawcode'].encode(), 'big')
    remnant_unit = trace['remnant_unit_id']
    mine = trace['remnant_activation_ability_id'].to_bytes(4, 'big').decode()
    mine_fields = fields_of[mine]
    listener = functions['DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire']
    death = functions['fJ']
    mark = functions['UC']
    tentacle = functions['isNotTentacle']
    ticks = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    world = lambda value: int(Decimal(str(value)) * 1024)
    if (trace['trigger_ability_id'] != marker or detail['native_base'] != 'ANcl'
        or fields_of[mine]['targs1'] not in ('_', '-') or detail['all_bases'][mine] != 'Amin'
        or not (trace['trigger_source_requires_combat_sapper'] and trace['trigger_target_requires_alive'] and trace['resume_attack_immediately']
            and trace['script_proc_cooldown_overrides_protected_ability_cooldown'])
        or trace['remnant_explosion_attack_type'] != 'normal' or trace['remnant_explosion_damage_type'] != 'magic'
        or trace['remnant_target_filter_function'] != 'UC'):
        raise ValueError('Echo Step trace changed')
    trigger = (rf'unit_getTypeId\(\w+\)=={unit_id}\)and unit_isAlive\(\w+\)\)and isCombatSapper\(\w+\)\)and'
        rf'\(__wurst_safe_BlzGetUnitAbilityCooldownRemaining\(\w+,{marker}\)<=0\.\)\)and'
        rf'\(vec2_distanceToSq\(unit_getPos\(\w+\),unit_getPos\(\w+\)\)<{int(trace["trigger_range_squared"])}(?:\.0)?\)')
    if not re.search(trigger, listener):
        raise ValueError('Echo Step trigger predicates changed')
    if (f'createUnit(unit_getOwner(' not in listener or f',{remnant_unit},unit_getPos(' not in listener
        or f'__wurst_safe_UnitApplyTimedLife(' not in listener
        or not re.search(rf'UnitApplyTimedLife\(\w+,{TIMED_LIFE_BUFF},{Decimal(str(trace["remnant_timed_life_seconds"]))}\.?0*\)', listener)
        or not re.search(rf'\(unit_getX\(\w+\)\+\({Decimal(str(trace["blink_distance_toward_own_castle"]))}\.\*Cos\(', listener)
        or 'Atan2((__wurst_safe_GetLocationY(' not in listener
        or not re.search(rf'BlzStartUnitAbilityCooldown\(\w+,{marker},{Decimal(str(trace["runtime_cooldown_seconds"]))}\.?0*\)', listener)
        or not re.search(r'SetUnitPosition\(\w+,\w+,\w+\)orderCodeAttack\(\w+\)', listener)):
        raise ValueError('Echo Step effect control flow changed')
    if (not re.search(rf'elseif\(\w+=={remnant_unit}\)then', death)
        or not re.search(rf'GroupEnumUnitsInRange\(\w+,\w+\[1\],\w+\[2\],{Decimal(str(trace["remnant_explosion_radius"]))}\.,\w+\)', death)
        or not re.search(rf'UnitDamageTarget\(\w+,\w+,{Decimal(str(trace["remnant_explosion_damage"]))}\.,true,false,ATTACK_TYPE_NORMAL,DAMAGE_TYPE_MAGIC,', death)):
        raise ValueError('remnant death handler changed')
    excluded_unit = re.search(r'not\(__wurst_safe_GetUnitTypeId\(\w+\)==(\d+)\)', mark)
    excluded_ability = re.search(r'GetUnitAbilityLevel\(\w+,(\d+)\)<=0', tentacle)
    # The importer's predicate label names the neighbouring rawcode; the script's literal governs.
    if (excluded_unit is None or excluded_ability is None
        or not trace['remnant_target_predicate'].startswith('life>0.405;enemy-of-mIb;sapper;vulnerable;not-tentacle;unit-type-not-')):
        raise ValueError('remnant recipient predicate changed')
    return {**recipe, 'profile': {'marker': marker, 'trigger_range': world(trace['trigger_range']),
        'blink_distance': world(trace['blink_distance_toward_own_castle']), 'cooldown_ticks': ticks(trace['runtime_cooldown_seconds']),
        'remnant': {'ability': trace['remnant_activation_ability_id'], 'unit_rawcode': remnant_unit,
            'lifetime_ticks': ticks(trace['remnant_timed_life_seconds']),
            'activation_ticks': ticks(mine_fields['dataa1']), 'detection_radius': world(mine_fields['rng1']),
            'trigger_targets': 1, 'damage': int(trace['remnant_explosion_damage']),
            'radius': world(trace['remnant_explosion_radius']),
            'excluded_ability': int(excluded_ability[1]), 'excluded_unit_rawcode': int(excluded_unit[1])}},
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/production-unit-special-mechanics.tsv',
            'fields': 'resolved/object-fields.tsv', 'script': 'DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire,fJ,UC',
            'unit': recipe['unit_rawcode']}}
