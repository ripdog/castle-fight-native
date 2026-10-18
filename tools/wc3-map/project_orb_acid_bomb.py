"""Free native Acid Bomb (ANab) cast on a landed orb attack, as an independent homing missile."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (detail['native_base'] != 'ANab' or not {'enemies', 'enemy'} & tokens or not movement
            or tokens - {'air', 'ground', 'enemies', 'enemy'} or number(fields['levels']) != 1
            or number(fields.get('cast1', 0)) != 0 or number(protected.get('mana_cost', fields['cost1'])) != 0):
        raise ValueError('orb Acid Bomb needs the audited free hostile unit mask and no cast time')
    # The child's protected cooldown is recorded in provenance; orb procs are not gated by it.
    raw = int.from_bytes(recipe['source_key'].encode(), 'big')
    profile = {'ability': raw, 'range': number(fields['rng1'], 1024), 'radius': number(fields['area1'], 1024),
        'speed_per_tick': number(fields['missilespeed'], 1024) // 30, 'homing': number(fields['missilehoming']) != 0,
        'armor_penalty_per_100': number(fields['datac1'], 100), 'movement_percent_delta': number(fields['dataa1'], 100),
        'attack_speed_percent_delta': number(fields['datab1'], 100), 'primary_damage': number(fields['datad1']),
        'secondary_damage': number(fields['datae1']), 'interval_ticks': number(fields['dataf1'], 30),
        'duration_ticks': number(fields['dur1'], 30), 'hero_duration_ticks': number(fields['herodur1'], 30),
        'targets': sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement)}
    return {'kind': 'orb-acid-bomb', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'profile': profile, 'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
            'targets': fields['targs1'], 'protected_resources': protected}}
