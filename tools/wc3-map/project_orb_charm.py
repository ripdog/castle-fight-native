"""Free native Charm invoked by a landed orb attack, independent of its parent mask."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tokens = set(fields['targs1'].split(','))
    if (detail['native_base'] != 'ANch'
            or tokens != {'air', 'enemies', 'ground', 'neutral', 'nonhero', 'ward'}):
        raise ValueError('orb Charm requires native Charm with its independent nonhero hostile unit mask')
    if any(number(protected.get(key, fields[field])) != 0 for key, field in [('mana_cost', 'cost1'), ('cooldown', 'cool1')]):
        raise ValueError('orb Charm resource clocks need a richer native proc contract')
    if number(fields.get('cast1', 0)) != 0:
        raise ValueError('delayed orb Charm needs an authoritative native cast state')
    return {'kind': 'orb-charm', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'profile': {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
            'map_version': detail['map_version'], 'targets': 3, 'maximum_creep_level': number(fields['dataa1'])},
        'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
            'targets': fields['targs1'], 'protected_resources': protected}}
