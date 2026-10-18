"""Native item-class life steal (AIva): a passive attacker heal that has no script consumer."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    stolen = Decimal(str(fields['dataa1']))
    if (detail['native_base'] != 'AIva' or not {'enemy', 'enemies'} & tokens or not movement
            or tokens - {'air', 'ground', 'enemy', 'enemies'} or number(fields['levels']) != 1
            or not 0 < stolen <= 1 or any(number(fields[key]) != 0 for key in ('cost1', 'cool1', 'cast1', 'dur1'))):
        raise ValueError('life steal requires the native item passive with a hostile unit mask, no clocks and a fractional rate')
    mask = sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement)
    return {**recipe, 'profile': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
        'stolen_per_10k': number(stolen, 10000), 'targets': mask},
        'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'], 'targets': fields['targs1']}}
