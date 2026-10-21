"""Native Repair on the Gobbo Machinist, driven by the shared support-order task queue."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from fractions import Fraction

from project_wind_walk import support_queue_rules


def project(recipe, fields, unit, protected, detail):
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    f = detail['functions']
    system = detail['support_system']
    actors = {actor['unit_rawcode']: actor for actor in system['actors']}
    actor = actors[recipe['unit_rawcode']]
    gobbo = raw(recipe['unit_rawcode'])
    heap = actor['production_building_rawcode']
    order, repairable, rally = f['orderGoboW'], f['isGobboRepairableTarget'], f['isGobboRallyTarget']
    prepare, timed, construct = f['prepareGobboTargets'], f['applyGobboTimedLife'], f['EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight']
    index_hook, setup = f['tH'], f['nH']
    if (detail['native_base'] != 'Ahrp' or actor['repair_ability_rawcode'] != recipe['source_key']
            or Decimal(fields['dataa1']) != 0
            or set(fields['targs1'].split(',')) != {'air', 'friend', 'ground', 'mechanical', 'structure'}
            or number(fields['levels']) != 1):
        raise ValueError('Gobbo repair requires the free native Repair with its audited mechanical/structure mask')
    if (not all(token in order for token in ('unit_issueTargetOrder(KBq,"repair",QBq)', 'unit_issueTargetOrder(KBq,"smart",OBq)',
            'GetUnitLifePercent(PX[LBq])>=100.', 'group_getRandom(Acb[NBq])', 'stopGobboIfBusy(KBq)', 'unit_getRallyUnit('))
            or not all(token in repairable for token in ('unit_isAllyOf', 'UNIT_TYPE_MECHANICAL', 'UNIT_TYPE_STRUCTURE', '<=1.', 'unit_isInConstruction'))
            or not all(token in rally for token in ('unit_isAllyOf', 'unit_isInConstruction'))
            or 'GroupEnumUnitsInRect(' not in prepare
            or 'ucb=45.' not in f['sH'] or 'UnitApplyTimedLife(pCq,1112820806,ucb)' not in timed
            or 'doAfter(0.0,' not in index_hook
            or f'unit_getTypeId(s8m)=={raw(heap)}' not in construct
            or f'createUnit1(unit_getOwner(s8m),{gobbo},unit_getPos(s8m))' not in construct
            or 'Dcb=%d' % gobbo not in setup):
        raise ValueError('Gobbo controller control flow changed: target groups, repair/smart orders, timed life and construction spawn')
    ratio = Fraction(Decimal(fields['datab1']))
    if actor['timed_life_seconds'] != 45 or Decimal(str(actor['repair_object_data']['Repair Time Ratio'])) != Decimal(fields['datab1']):
        raise ValueError('Gobbo repair constants disagree with the retained system trace')
    return {'kind': 'gobbo-repair', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'],
        'profile': {'ability': raw(recipe['source_key']), 'rules': support_queue_rules(detail),
            'repair_range': number(fields['rng1'], 1024),
            'repair_time_ratio_numerator': ratio.numerator, 'repair_time_ratio_denominator': ratio.denominator,
            'timed_life_ticks': number(actor['timed_life_seconds'], 30), 'construction_spawn_building': raw(heap)},
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'script': 'resolved/runtime-system-mechanics.tsv', 'functions': ['orderGoboW', 'prepareGobboTargets',
                'isGobboRepairableTarget', 'isGobboRallyTarget', 'applyGobboTimedLife', 'sH', 'nH',
                'EventListener_add_doAfter_ObeliskOfLight_onEvent_add_doAfter_ObeliskOfLight']}}
