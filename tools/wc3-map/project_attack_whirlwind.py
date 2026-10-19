"""Project the damage-listener Whirlwind: a marker-gated, rolled, universal area hit at the damaged target."""
import re
from decimal import Decimal


def project(recipe, fields, unit, protected, detail):
    trace, functions = detail['whirlwind'], detail['functions']
    marker = int.from_bytes(recipe['source_key'].encode(), 'big')
    init = functions['qP']
    listener = functions['DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind']
    callback = functions['ForGroupCallback_forUnitsInRange_addListener_doAfter_Whirlwind_callback_forUnitsInRange_addListener_doAfter_Whirlwind']
    constants = re.search(r'(\w+)=(\d+) (\w+)=([0-9.]+) (\w+)=([0-9.]+) (\w+)=([0-9.]+) ', init)
    if constants is None:
        raise ValueError('Whirlwind constants changed')
    marker_symbol, chance_symbol, damage_symbol, radius_symbol = constants[1], constants[3], constants[5], constants[7]
    if (int(constants[2]) != marker or marker != trace['marker_ability_id']
        or Decimal(constants[4]) != Decimal(str(trace['proc_probability']))
        or Decimal(constants[6]) != Decimal(str(trace['damage']))
        or Decimal(constants[8]) != Decimal(str(trace['radius']))
        or trace['proc_comparison'] != 'roll <= 0.20' or trace['required_damage_event_type'] != 0
        or trace['damage_type'] != 'universal' or trace['attack_type'] != 'normal' or trace['is_attack'] or trace['is_ranged']
        or trace['center'] != 'damaged-target-position'
        or trace['target_predicate'] != 'enemy-of-source;alive;combat-sapper;not-flying'):
        raise ValueError('Whirlwind trace disagrees with its script constants')
    if (not re.search(r'\(DamageEvent_getType\(\)==0\)and unit_hasAbility\(\w+,' + marker_symbol + r'\)', listener)
        or f'{chance_symbol}>=GetRandomReal(0.,1.)' not in listener
        or not re.search(r'=tupleCopy1\(unit_getPos\(\w+\)\)\w+=' + radius_symbol + r' ', listener)
        or 'forUnitsInRange(' not in listener
        or 'DamageEvent_getTarget()' not in listener):
        raise ValueError('Whirlwind listener must roll on landed attack damage and spin at the damaged target')
    if (not re.search(r'unit_isEnemyOf1\(\w+,\w+\.sunit\)and unit_isAlive\(\w+\)\)and isCombatSapper\(\w+\)\)and\(not unit_isType\(\w+,UNIT_TYPE_FLYING\)', callback)
        or damage_symbol not in callback or 'ATTACK_TYPE_NORMAL' not in callback or 'DAMAGE_TYPE_UNIVERSAL' not in callback
        or 'UnitDamageTarget(' not in callback):
        raise ValueError('Whirlwind recipients are not the expected non-flying combat sappers')
    chance = Decimal(constants[4]) * 10_000
    if chance != chance.to_integral_value():
        raise ValueError('Whirlwind chance is not exactly representable')
    return {**recipe, 'chance_per_10k': int(chance), 'trigger_targets': 'air-ground-units-and-buildings',
        'profile': {'ability': marker, 'damage': int(Decimal(constants[6])), 'radius': int(Decimal(constants[8]) * 1024),
            'combat_sappers_only': True, 'targets': 1},
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/production-unit-special-mechanics.tsv',
            'script': 'DamageListener_addListener_doAfter_Whirlwind_onEvent_addListener_doAfter_Whirlwind',
            'marker': recipe['source_key']}}
