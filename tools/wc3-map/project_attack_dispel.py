"""Nature's Abolish Magic: a source-damage listener that dispels positive buffs after a landed attack."""
import re
from decimal import Decimal


def project(recipe, detail):
    parameters = detail['attack_dispel']
    functions = detail['functions']
    dispatcher = functions['handleSourceDamageEffects']
    unit = int.from_bytes(recipe['unit_rawcode'].encode(), 'big')
    marker = int.from_bytes(recipe['source_key'].encode(), 'big')
    sources = [name for name in detail['attack_dispel_sources'] if name != 'handleSourceDamageEffects']
    if parameters['unit_remove_buffs_ex_args'] != [True, False, True, False, False, False, False]:
        raise ValueError('Abolish Magic must remove positive magical buffs only')
    flags = ','.join('true' if flag else 'false' for flag in parameters['unit_remove_buffs_ex_args'])
    removal = r'__wurst_safe_UnitRemoveBuffsEx\(%s,' + flags + r'\)'
    chance = Decimal(parameters['proc_chance_percent'])
    if (parameters['center'] != 'attacked-target' or parameters['effect_radius'] < 0
            or f'unit_getAbilityLevel(Qpq,{marker})<=0)then return end' not in dispatcher):
        raise ValueError('Abolish Magic control flow changed: marker gate and attacked-target center')
    area = None
    if parameters['direct_target_only']:
        shape = (r'function \w+\((\w+)\)if\(GetRandomInt\(0,99\)<(\d+)\)then ' + removal % r'\1' +
            r'safeDestroyEffect\(__wurst_safe_AddSpecialEffectTarget\("[^"]+",\1,"overhead"\)\)end end')
        matched = re.fullmatch(shape, functions[sources[0]].strip())
        reaches = f'Spq=={unit})then {sources[0]}(Rpq)' in dispatcher
        radius = 0
        if len(sources) != 1 or parameters['effect_radius'] != 0:
            raise ValueError('direct Abolish Magic has no area callback')
    else:
        if len(sources) != 2 or parameters['aoe_target_predicate'] != 'isDispellableEnemyOf':
            raise ValueError('area Abolish Magic needs its proc and group callback')
        proc, callback = functions[sources[0]], functions[sources[1]]
        predicate = functions['isDispellableEnemyOf']
        matched = re.fullmatch(r'function \w+\((\w+),(\w+)\)(?:local \w+=nil |local \w+=\{0\.;0\.\}'
            r')+(\w+)=unit_getOwner\(\1\)if\(GetRandomInt\(0,99\)<(\d+)\)then (\w+)=tupleCopy1\(unit_getPos\(\2\)\)'
            r'(\w+)=\w+:create\d+\(\)\6\.p=\3 (\w+)=tupleCopy1\(\5\)(\w+)=\6 forUnitsInRange\(\7,([0-9.]+),false,\8\)end end',
            proc.strip())
        hit = re.fullmatch(r'function \w+\((\w+),(\w+)\)local \w+=nil if isDispellableEnemyOf\(\2,\1\.p\)then (\w+)=\2 '
            + removal % r'\3' + r'safeDestroyEffect\(__wurst_safe_AddSpecialEffectTarget\("[^"]+",\3,"overhead"\)\)end end',
            callback.strip())
        rule = re.fullmatch(r'function isDispellableEnemyOf\((\w+),(\w+)\)return\(\(\(\(isAliveCombatSapper\(\1\)and unit_isEnemyOf'
            r'\(\1,\2\)\)and\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(not\(unit_getTypeId'
            r'\(\1\)==(\d+)\)\)\)end', predicate.strip())
        if matched is None or hit is None or rule is None or Decimal(matched[4]) != chance or Decimal(matched[9]) != Decimal(parameters['effect_radius']):
            raise ValueError('area Abolish Magic control flow changed: one roll, target-centered group of dispellable enemies')
        reaches = f'Spq=={unit})then {sources[0]}(Qpq,Rpq)' in dispatcher
        radius = int(Decimal(parameters['effect_radius']) * 1024)
        area = {'map_version': detail['map_version'], 'excluded_abilities': [int(rule[3]), int(rule[4])],
            'excluded_unit_rawcode': int(rule[5])}
        matched = [None, None, None, matched[4]] if matched else None
    if matched is None or not reaches or Decimal(matched[2] if parameters['direct_target_only'] else matched[3]) != chance:
        raise ValueError('Abolish Magic control flow changed: type dispatch and one roll')
    profile = {'ability': marker, 'chance_per_10k': int(chance * 100), 'radius': radius}
    if area is not None:
        profile['area'] = area
    return {'unit_rawcode': recipe['unit_rawcode'], 'rule': {'AttackDispel': profile},
        'provenance': {'source': 'resolved/production-unit-special-mechanics.tsv', 'handler': sources[0],
            'dispatcher': 'handleSourceDamageEffects', 'remove_buffs_ex': parameters['unit_remove_buffs_ex_args']}}
