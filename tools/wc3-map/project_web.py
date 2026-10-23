"""Native Web (`Aweb`), an object-owned default-active autocast that immobilizes and grounds its target."""
from decimal import Decimal, ROUND_CEILING

TARGET_GROUND, TARGET_AIR = 1, 2


def project(recipe, fields, unit, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (detail['native_base'] != 'Aweb' or not movement or tokens - {'air', 'ground', 'enemy', 'neutral'}
            or not {'enemy', 'neutral'} <= tokens or fields['missilehoming'] != '1'
            or detail['default_active_alias'] != recipe['source_key']):
        raise ValueError('native Web requires its enemy unit mask, homing missile and default-active autocast')
    if number(fields['dur1']) <= 0 or number(fields['herodur1']) <= 0:
        raise ValueError('native Web requires positive normal and hero durations')
    bits = (TARGET_GROUND if 'ground' in movement else 0) | (TARGET_AIR if 'air' in movement else 0)
    amount = lambda key, scale=1: 0 if unit[key].strip() in {'', '-', '_'} else number(unit[key], scale)
    rate = amount('mana_regen', 10000)
    profile = {'ability': raw(recipe['source_key']), 'duration_ticks': number(fields['dur1'], 30),
        'hero_duration_ticks': number(fields['herodur1'], 30), 'range': number(fields['rng1'], 1024),
        'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30),
        'lowering_ticks': number(fields['dataa1'], 30), 'air_height': number(fields['datab1'], 1024),
        'melee_range': number(fields['datac1'], 1024), 'targets': bits}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'], 'spellcasting': {
            'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'), 'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected.get('mana_cost', fields['cost1'])),
                'cooldown_ticks': number(protected.get('cooldown', fields['cool1']), 30),
                'range': number(fields['rng1'], 1024), 'target_policy': 'NearestEnemyInCombat',
                'effect': {'Ensnare': {'profile': profile, 'search_radius': 0, 'maximum_targets': 1}}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'source': 'resolved/object-fields.tsv', 'protected': 'resolved/protected-ability-fields.tsv',
            'native_base': detail['native_base'], 'unit': 'resolved/units.tsv',
            'default_active_alias': detail['default_active_alias']}}
