"""Unmodified native Slow, resolved from registered base inputs rather than copied tuning."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    tokens = set(fields['targs1'].split(','))
    if detail['native_base'] != 'Aslo' or tokens != {'air', 'ground', 'enemy'} or len(fields['buffid1']) != 4:
        raise ValueError('native Slow requires its inherited ability class and independent enemy unit mask')
    rate = number(unit['mana_regen'], 10000)
    amount = lambda key: 0 if unit[key].strip() in {'', '-', '_'} else number(unit[key])
    profile = {'ability': raw(recipe['source_key']), 'buff': raw(fields['buffid1']), 'targets': 3,
        'organic_only': False, 'duration_ticks': number(fields['dur1'], 30),
        'hero_duration_ticks': number(fields['herodur1'], 30),
        'movement_percent_delta': -number(fields['dataa1'], 100),
        'attack_speed_percent_delta': -number(fields['datab1'], 100), 'damage_bonus_per_10k': 0, 'ethereal': None}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'], 'spellcasting': {
            'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'), 'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected.get('mana_cost', fields['cost1'])),
                'cooldown_ticks': number(protected.get('cooldown', fields['cool1']), 30),
                'range': number(fields['rng1'], 1024), 'target_policy': 'NearestEnemyInCombat',
                'effect': {'NativeDebuff': profile}}}, 'mana_regen_per_second_per_10k': rate,
        'effect_ability_rawcode': None, 'provenance': {'source': 'resolved/base-data-manifest.json',
            'reference_source_sha256': detail['reference_source_sha256'], 'native_base': detail['native_base'],
            'unit': 'resolved/units.tsv', 'default_active_alias': detail['default_active_alias']}}
