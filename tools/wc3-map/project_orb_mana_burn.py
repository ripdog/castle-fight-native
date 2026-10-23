"""Free native Mana Burn invoked by a landed orb attack, independent of its parent mask."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (detail['native_base'] != 'AEmb' or not movement or not {'enemies', 'enemy'} & tokens
            or tokens - {'air', 'ground', 'enemies', 'enemy', 'ward'}):
        raise ValueError('orb Mana Burn requires native Mana Burn with its independent hostile unit mask')
    if number(protected.get('mana_cost', fields['cost1'])) != 0:
        raise ValueError('orb Mana Burn must be free')
    cooldown = number(protected.get('cooldown', fields['cool1']), 30)
    if cooldown > 1:
        raise ValueError('orb Mana Burn child needs at most one tick of native cooldown')
    if number(fields.get('cast1', 0)) != 0 or number(fields['dataa1']) <= 0:
        raise ValueError('orb Mana Burn needs an immediate cast and a positive mana cap')
    return {'kind': 'orb-mana-burn', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'profile': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
            'maximum_mana_drained': number(fields['dataa1']),
            'targets': sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement)},
        'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
            'targets': fields['targs1'], 'protected_resources': protected, 'native_child_cooldown_ticks': cooldown}}
