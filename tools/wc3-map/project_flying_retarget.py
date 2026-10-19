"""Shared damage-listener branch that orders listed unit types to attack a flying damage source."""
import json
import re
from decimal import Decimal, ROUND_CEILING

LISTENER = 'DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire'
BRANCH = re.compile(
    r'if\(\(\(unit_getTypeId\((?P<victim>\w+)\)==(?P<first>\d+)\)or\(unit_getTypeId\((?P=victim)\)==(?P<second>\d+)\)\)'
    r'and unit_isType\((?P<source>\w+),UNIT_TYPE_FLYING\)\)then (?P<body>.*?)'
    r'unit_issueTargetOrder\((?P=victim),"attack",(?P=source)\)end end')
THROTTLE = re.compile(
    r'(?P<index>\w+)=unit_getIndex\((?P<victim>\w+)\)if\(\((?P=index)>0\)and'
    r'\(\(getElapsedGameTime\(\)-__wurst_ensureReal\((?P<table>\w+)\[(?P=index)\]\)\)>(?P<seconds>[\d.]+)\)\)'
    r'then (?P=table)\[(?P=index)\]=getElapsedGameTime\(\)$')
WRONG_ORDER = re.compile(
    r'\(\(\(unit_getTypeId\(\w+\)==(?P<first>\d+)\)or\(unit_getTypeId\(\w+\)==(?P<second>\d+)\)\)'
    r'and unit_isType\(\w+,UNIT_TYPE_FLYING\)\)')


def project(rows, functions):
    listener = functions[LISTENER]
    branch = BRANCH.search(listener)
    if branch is None:
        raise ValueError('flying-attacker retarget branch changed')
    victim_types = {int(branch['first']), int(branch['second'])}
    # The per-unit stamp is written before the order, so a refused order still consumes the interval.
    throttle = THROTTLE.search(branch['body'])
    if throttle is None or throttle['victim'] != branch['victim']:
        raise ValueError('flying-attacker retarget throttle stamp must precede the order')
    guard = WRONG_ORDER.search(functions['checkForWrongOrder'])
    if guard is None or {int(guard['first']), int(guard['second'])} != victim_types:
        raise ValueError('flying-attacker attack orders must be exempt from the wrong-order guard for the same unit types')
    ticks = lambda value: int((Decimal(str(value)) * 30).to_integral_value(rounding=ROUND_CEILING))
    result = []
    seen = set()
    for row in rows:
        if row['mechanic_kind'] != 'retarget-flying-damage-source':
            continue
        p = json.loads(row['parameters_json'])
        raw = int.from_bytes(row['unit_rawcode'].encode(), 'big')
        if (raw not in victim_types or p['issued_order'] != 'attack' or p['retaliation_target'] != 'damage-source'
            or not p['trigger_source_requires_flying'] or not p['requires_positive_unit_index']
            or Decimal(str(p['per_unit_retarget_throttle_seconds'])) != Decimal(throttle['seconds'])
            or row['source_functions'] != LISTENER):
            raise ValueError('flying-attacker retarget row disagrees with the listener')
        seen.add(raw)
        result.append({'rawcode': raw, 'throttle_ticks': ticks(p['per_unit_retarget_throttle_seconds'])})
    if seen != victim_types:
        raise ValueError('every listed retarget unit type needs a retained mechanic row')
    return sorted(result, key=lambda rule: rule['rawcode'])


STRUCTURE_BRANCH = re.compile(
    r'if\(\(unit_getTypeId\((?P<source>\w+)\)==(?P<unit>\d+)\)and unit_isType\((?P<victim>\w+),UNIT_TYPE_STRUCTURE\)\)then '
    r'(?P<amount>\w+)=\(DamageEvent_getAmount\(\)\*(?P<factor>[\d.]+)\)DamageInstance_DamageInstance_setAmount\(\w+,(?P=amount)\)end')


def project_structure_damage(functions, unit_rawcodes):
    """Final damage a listed unit type deals to structures, rescaled by the shared listener."""
    listener = functions[LISTENER]
    result = []
    for rawcode in unit_rawcodes:
        raw = int.from_bytes(rawcode.encode(), 'big')
        branch = next((m for m in STRUCTURE_BRANCH.finditer(listener) if int(m['unit']) == raw), None)
        if branch is None:
            raise ValueError(f'{rawcode} structure damage branch changed')
        factor = Decimal(branch['factor'])
        if not 0 < factor <= 1:
            raise ValueError('structure damage reduction must be a fraction')
        result.append({'rawcode': raw, 'factor_per_10k': int(factor * 10000)})
    return result
