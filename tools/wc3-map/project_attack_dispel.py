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
    if len(sources) not in (1, 2) or parameters['unit_remove_buffs_ex_args'] != [True, False, True, False, False, False, False]:
        raise ValueError('Abolish Magic must remove positive magical buffs only')
    proc = functions[sources[0]]
    flags = ','.join(['true' if flag else 'false' for flag in parameters['unit_remove_buffs_ex_args']])
    chance = Decimal(parameters['proc_chance_percent'])
    direct = parameters['direct_target_only']
    if direct:
        shape = (r'function \w+\((\w+)\)if\(GetRandomInt\(0,99\)<(\d+)\)then __wurst_safe_UnitRemoveBuffsEx\(\1,' + flags +
            r'\)safeDestroyEffect\(__wurst_safe_AddSpecialEffectTarget\("[^"]+",\1,"overhead"\)\)end end')
        matched = re.fullmatch(shape, proc.strip())
        reaches = f'Spq=={unit})then {sources[0]}(Rpq)' in dispatcher
    else:
        matched = None
        reaches = False
    if (not direct or matched is None or Decimal(matched[2]) != chance or not reaches
            or f'unit_getAbilityLevel(Qpq,{marker})<=0)then return end' not in dispatcher
            or parameters['center'] != 'attacked-target' or parameters['effect_radius'] != 0):
        raise ValueError('Abolish Magic control flow changed: marker gate, type dispatch, one roll, direct unit-only dispel')
    return {'unit_rawcode': recipe['unit_rawcode'], 'rule': {'AttackDispel': {
        'ability': marker, 'chance_per_10k': int(chance * 100), 'radius': 0}},
        'provenance': {'source': 'resolved/production-unit-special-mechanics.tsv', 'handler': sources[0],
            'dispatcher': 'handleSourceDamageEffects', 'remove_buffs_ex': parameters['unit_remove_buffs_ex_args']}}
