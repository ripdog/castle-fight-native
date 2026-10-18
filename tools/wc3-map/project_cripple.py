"""Free native Cripple invoked by a landed orb attack, independent of its parent mask."""
from decimal import Decimal, ROUND_CEILING


def project_drunken_haze(recipe, fields, protected, detail, number):
    """Native ANdh child of an Orb of Darkness: a free miss-chance and movement debuff."""
    tokens = set(fields['targs1'].split(','))
    stats = detail['source_unit_stats']
    weapon_cooldown = Decimal(stats['override_attack1_cooldown'] if stats.get('override_attack1_cooldown') else stats['static_attack1_cooldown'])
    cooldown = Decimal(str(protected.get('cooldown', fields['cool1'])))
    if (tokens != {'air', 'enemies', 'ground'} or len(fields['buffid1']) != 4
            or number(fields['dataa1']) != 0 or number(fields['datad1']) != 0
            or number(fields.get('cast1', 0)) != 0 or number(protected.get('mana_cost', fields['cost1'])) != 0
            or not 0 <= cooldown <= weapon_cooldown or number(fields['levels']) != 1):
        raise ValueError('orb Drunken Haze needs the audited free hostile mask, no silence and a child clock within the weapon cooldown')
    return {'kind': 'native-target-debuff', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'profile': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
            'buff': int.from_bytes(fields['buffid1'].encode(), 'big'), 'targets': 3, 'organic_only': False,
            'duration_ticks': number(fields['dur1'], 30), 'hero_duration_ticks': number(fields['herodur1'], 30),
            'movement_percent_delta': -number(fields['datac1'], 100), 'attack_speed_percent_delta': 0,
            'damage_bonus_per_10k': 0, 'ethereal': None, 'miss_chance_per_10k': number(fields['datab1'], 10000)},
        'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
            'targets': fields['targs1'], 'protected_resources': protected}}


def project(recipe, fields, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    if detail['native_base'] == 'ANdh':
        return project_drunken_haze(recipe, fields, protected, detail, number)
    tokens = set(fields['targs1'].split(','))
    if (detail['native_base'] not in {'Acri', 'ACcr'}
            or tokens != {'air', 'enemies', 'ground', 'neutral', 'ward'}
            or len(fields['buffid1']) != 4):
        raise ValueError('native Cripple requires its independent hostile nonorganic unit mask and buff')
    if any(number(protected.get(key, fields[field])) != 0 for key, field in [('mana_cost', 'cost1'), ('cooldown', 'cool1')]):
        raise ValueError('orb Cripple resource clocks need a richer native proc contract')
    if number(fields.get('cast1', 0)) != 0:
        raise ValueError('delayed orb Cripple needs an authoritative native cast state')
    return {'kind': 'native-target-debuff', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'profile': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
            'buff': int.from_bytes(fields['buffid1'].encode(), 'big'), 'targets': 3, 'organic_only': False,
            'duration_ticks': number(fields['dur1'], 30), 'hero_duration_ticks': number(fields['herodur1'], 30),
            'movement_percent_delta': -number(fields['dataa1'], 100),
            'attack_speed_percent_delta': -number(fields['datab1'], 100),
            'damage_bonus_per_10k': -number(fields['datac1'], 10000), 'ethereal': None},
        'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
            'targets': fields['targs1'], 'protected_resources': protected}}
