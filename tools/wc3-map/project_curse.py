"""Native autocast Curse; retained object fields own all parameters."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, protected, detail):
    integer = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    if (detail['native_base'] != 'Acrs'
        or set(fields['targs1'].split(',')) != {'air', 'ground', 'enemies', 'neutral'}
        or len(fields['buffid1']) != 4 or fields['order'] != 'curse'
        or fields['orderon'] != 'curseon' or fields['orderoff'] != 'curseoff'
        or not 0 <= integer(fields['dataa1'], 10000) <= 10000):
        raise ValueError('Curse needs its native autocast orders and audited unit mask')
    mana = lambda field, scale=1: 0 if unit[field].strip() in {'-', '_', ''} else integer(unit[field], scale)
    rate = mana('mana_regen', 10000)
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'],
        'source_key': recipe['source_key'], 'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': mana('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {
            'id': raw(recipe['source_key']), 'mana_cost': integer(protected.get('mana_cost', fields['cost1'])),
            'cooldown_ticks': integer(protected.get('cooldown', fields['cool1']), 30),
            'range': integer(fields['rng1'], 1024), 'target_policy': 'NearestEnemyInCombat',
            'effect': {'Curse': {'ability': raw(recipe['source_key']),
                'buff': {'rawcode': raw(fields['buffid1']), 'positive': False,
                    'stealable': fields.get('hero', '0') == '0', 'organic_only': False},
                'chance_to_miss_per_10k': integer(fields['dataa1'], 10000), 'targets': 3,
                'duration_ticks': integer(fields['dur1'], 30), 'hero_duration_ticks': integer(fields['herodur1'], 30)}}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'source': 'resolved/object-fields.tsv', 'unit': 'resolved/units.tsv',
            'protected': 'resolved/protected-ability-fields.tsv'}}
