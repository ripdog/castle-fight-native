"""Native passive ANic, with an explicit legacy native compatibility policy.

Nic1's original rawcode dependency and legacy three-hit limit are native observations,
not object fields. Keep that policy here, next to the version-scoped projection.
"""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, native_base):
    def number(key, scale=1):
        return int((Decimal(fields[key.lower()]) * scale).to_integral_value(rounding=ROUND_CEILING))
    if (native_base != 'ANic' or set(fields['targs1'].split(',')) != {'enemy', 'neutral', 'organic', 'nonancient'}
            or len(fields['buffid1']) != 4 or number('levels') != 1
            or any(number(k) != 0 for k in ['cost1', 'cool1', 'cast1'])
            or number('missilehoming') != 1 or number('missilearc') != 0
            or number('datae1', 1024) < number('datac1', 1024)):
        raise ValueError('passive Incinerate requires its audited free organic/nonancient native family and homing missile')
    p = {'ability': int.from_bytes(recipe['source_key'].encode(), 'big'),
         'buff_rawcode': int.from_bytes(fields['buffid1'].encode(), 'big'),
         'bonus_per_hit': number('dataa1') if recipe['source_key'] == 'ANic' else 0,
         'maximum_stacks': 3, 'duration_ticks': number('dur1', 30), 'hero_duration_ticks': number('herodur1', 30),
         'full_damage': number('datab1'), 'half_damage': number('datad1'),
         'full_radius': number('datac1', 1024), 'half_radius': number('datae1', 1024),
         'delay_ticks': number('dataf1', 30),
         'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30)}
    if p['duration_ticks'] <= 0 or p['hero_duration_ticks'] <= 0 or p['speed_per_tick'] <= 0:
        raise ValueError('Incinerate requires positive buff clocks and native missile speed')
    return {**recipe, 'profile': p, 'provenance': {'native_base': native_base,
        'source': 'resolved/object-fields.tsv', 'native_bonus_field': fields['dataa1'],
        'native_compatibility': 'legacy-original-rawcode-bonus-three-hit-cap-v1',
        'targets': fields['targs1'], 'missile_art': fields['missileart']}}
