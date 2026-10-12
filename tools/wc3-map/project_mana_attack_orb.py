"""Native Afak default-active weapon mode; primary AI range remains the ordinary weapon's."""
from decimal import Decimal, ROUND_CEILING


def project(recipe, fields, unit, protected, detail):
    def number(key, scale=1):
        return int((Decimal(str(fields[key.lower()])) * scale).to_integral_value(rounding=ROUND_CEILING))
    cost = int(Decimal(protected.get('mana_cost', fields['cost1'])))
    cooldown = Decimal(protected.get('cooldown', fields['cool1']))
    if (detail['native_base'] != 'Afak' or detail['default_active_alias'] != recipe['source_key']
            or unit['attack1_weapon_type'] != 'msplash' or unit['attack1_type'] != 'magic'
            or set(fields['targs1'].split(',')) != {'air', 'enemies', 'ground'}
            or number('levels') != 1 or cooldown != 0 or cost < 0
            or any(number(k) != 0 for k in ['cast1', 'dur1', 'herodur1'])
            or number('missilehoming') != 1 or Decimal(fields.get('missilearc', '0')) != 0):
        raise ValueError('native annihilation orb requires its source default-active Magic homing weapon and free cast clocks')
    full, medium, outer = (number(k, 1024) for k in ['datad1', 'datae1', 'area1'])
    if not 0 <= full <= medium <= outer or not all(0 <= number(k, 10000) <= 10000 for k in ['datab1','datac1']):
        raise ValueError('native annihilation orb requires nested splash bands and bounded falloff')
    splash_tokens = set(unit['attack1_splash_targets'].split(','))
    if 'enemies' not in splash_tokens or splash_tokens - {'air','ground','enemies','structure','ward'}:
        raise ValueError('native orb collateral requires the audited weapon enemy mask')
    movement = splash_tokens & {'air','ground','structure'}
    splash_mask = sum(bit for token,bit in [('ground',1),('air',2),('structure',4)] if token in movement) if movement else 7
    p = {'primary_targets': 3, 'ability': int.from_bytes(recipe['source_key'].encode(), 'big'), 'mana_cost': cost,
        'bonus_damage': number('dataa1'), 'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30),
        'splash': {'full_radius': full, 'medium_radius': medium, 'outer_radius': outer,
            'medium_damage_per_10k': number('datab1',10000), 'outer_damage_per_10k': number('datac1',10000), 'targets':splash_mask}}
    if p['bonus_damage'] < 0 or p['speed_per_tick'] <= 0:
        raise ValueError('native attack orb requires nonnegative bonus and positive missile speed')
    return {**recipe, 'profile':p, 'provenance':{'source':'resolved/object-fields.tsv', 'native_base':detail['native_base'],
        'default_active_alias': detail['default_active_alias'], 'targets':fields['targs1'],
        'unit_splash_targets':unit['attack1_splash_targets'],
        'protected_resources':protected, 'native_cast_range':fields['rng1'], 'missile_art':fields['missileart']}}
