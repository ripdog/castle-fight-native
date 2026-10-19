"""Native Afak default-active weapon mode; primary AI range remains the ordinary weapon's."""
from decimal import Decimal, ROUND_CEILING


def project_searing_arrows(recipe, fields, unit, protected, detail):
    """Native ACsa: a default-active bonus-damage weapon mode with no collateral splash."""
    def number(key, scale=1):
        return int((Decimal(str(fields[key.lower()])) * scale).to_integral_value(rounding=ROUND_CEILING))
    cost = int(Decimal(protected.get('mana_cost', fields['cost1'])))
    cooldown = Decimal(protected.get('cooldown', fields['cool1']))
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground', 'structure'}
    if (detail['default_active_alias'] != recipe['source_key'] or not {'enemies', 'enemy'} & tokens
            or tokens - {'air', 'ground', 'structure', 'enemy', 'enemies', 'neutral'} or not movement
            or number('levels') != 1 or cooldown != 0 or cost < 0
            or any(number(k) != 0 for k in ['cast1', 'dur1', 'herodur1'])
            or number('missilehoming') != 1 or not unit['attack1_weapon_type'].startswith('missile')):
        raise ValueError('native Searing Arrows requires its default-active homing missile weapon mode and free cast clocks')
    mask = sum(bit for token, bit in [('ground', 1), ('air', 2), ('structure', 4)] if token in movement)
    p = {'primary_targets': mask, 'ability': int.from_bytes(recipe['source_key'].encode(), 'big'), 'mana_cost': cost,
        'bonus_damage': number('dataa1'), 'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30),
        # No area: an empty collateral mask keeps the orb machinery from touching bystanders.
        'splash': {'full_radius': 0, 'medium_radius': 0, 'outer_radius': 0,
            'medium_damage_per_10k': 0, 'outer_damage_per_10k': 0, 'targets': 0}}
    if p['bonus_damage'] < 0 or p['speed_per_tick'] <= 0:
        raise ValueError('native Searing Arrows requires nonnegative bonus damage and positive missile speed')
    return {**recipe, 'profile': p, 'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
        'default_active_alias': detail['default_active_alias'], 'targets': fields['targs1'],
        'protected_resources': protected, 'native_cast_range': fields['rng1'], 'missile_art': fields['missileart']}}


def project_poison_arrows(recipe, fields, unit, protected, detail):
    """Native AEpa: an organic-only default-active bonus-damage orb layered on the weapon's own delivery."""
    def number(key, scale=1):
        return int((Decimal(str(fields[key.lower()])) * scale).to_integral_value(rounding=ROUND_CEILING))
    cost = int(Decimal(protected.get('mana_cost', fields['cost1'])))
    cooldown = Decimal(protected.get('cooldown', fields['cool1']))
    if (detail['default_active_alias'] != recipe['source_key']
            or set(fields['targs1'].split(',')) != {'air', 'ground', 'organic'}
            or number('levels') != 1 or cooldown != 0 or cost < 0 or number('cast1') != 0
            or any(number(k) != 0 for k in ['datab1', 'datac1', 'datad1', 'datae1'])
            or unit['attack1_weapon_type'] != 'artillery' or number('dataa1') < 0):
        raise ValueError('native Poison Arrows requires its default-active free-cast organic orb, no poison payload and an artillery weapon')
    p = {'primary_targets': 3, 'ability': int.from_bytes(recipe['source_key'].encode(), 'big'), 'mana_cost': cost,
        'bonus_damage': number('dataa1'), 'speed_per_tick': int(Decimal(fields['missilespeed']) * 1024 / 30),
        # The artillery weapon keeps its own delivery and splash bands; the orb adds only payload.
        'splash': {'full_radius': 0, 'medium_radius': 0, 'outer_radius': 0,
            'medium_damage_per_10k': 0, 'outer_damage_per_10k': 0, 'targets': 0},
        'organic_only': True, 'preserve_delivery': True}
    return {**recipe, 'profile': p, 'provenance': {'source': 'resolved/object-fields.tsv', 'native_base': detail['native_base'],
        'default_active_alias': detail['default_active_alias'], 'targets': fields['targs1'],
        'protected_resources': protected, 'native_cast_range': fields['rng1'], 'area_field_unused_by_native_base': fields['area1'],
        'missile_art': fields['missileart']}}


def project(recipe, fields, unit, protected, detail):
    if detail['native_base'] == 'AEpa':
        return project_poison_arrows(recipe, fields, unit, protected, detail)
    if detail['native_base'] == 'ACsa':
        return project_searing_arrows(recipe, fields, unit, protected, detail)
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
