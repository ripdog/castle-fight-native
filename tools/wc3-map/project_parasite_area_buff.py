"""Immediate native Parasite plus an independently targeted friendly Roar dummy."""
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(recipe, fields, unit, protected, detail):
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    row = detail['mechanics_row']
    child = detail['effect_key']
    parent = detail['all_fields'][row['ability_rawcode']]
    native = detail['effect_fields']
    handler = detail['functions'][row['handler_function']]
    if (row['mechanic_kind'] != 'dummy-immediate-ability-from-caster'
        or row['target_mode_label'] != 'enemy-ground-combat-sapper'
        or row['scheduled_delays_json'] != '[]'
        or f'dummyCastImmediateFrom(vln,{raw(child)},852164,1.)' not in handler
        or 'vln=tln' not in handler
        or detail['all_bases'][child] != 'ACro'
        or detail['all_bases'][row['ability_rawcode']] != 'ACpa'
        or set(parent['targs1'].split(',')) != {'enemies', 'ground', 'neutral', 'nonhero', 'structure'}
        or set(native['targs1'].split(',')) != {'air', 'ground', 'friend', 'self'}
        or any(number(native[key]) != 0 for key in ['datad1', 'datae1', 'dataf1', 'datag1'])
        or number(detail['effect_protected']['mana_cost']) != 0
        or number(native['cool1']) != 0):
        raise ValueError('Parasite/Roar requires independent immediate caster-centered free child control flow')
    buff = {'rawcode': raw(native['buffid1']), 'positive': True,
        'stealable': native.get('hero', '0') == '0', 'organic_only': False}
    profile = {'parent': project_parasite(parent, detail['map_version']),
        'parent_nonhero_only': True, 'ability': raw(child), 'buff': buff,
        'radius': number(native['area1'], 1024), 'targets': 3,
        'armor_bonus_per_100': number(native['datab1'], 100),
        'damage_bonus_per_10k': number(native['dataa1'], 10000),
        'regeneration_per_second_per_10k': number(native['datac1'], 10000),
        'duration_ticks': number(native['dur1'], 30), 'hero_duration_ticks': number(native['herodur1'], 30)}
    mana = lambda field, scale=1: 0 if unit[field].strip() in {'-', '_', ''} else number(unit[field], scale)
    child_only = recipe['source_kind'] == 'ability-effect'
    effect = {'StatBuff': {key: profile[key] for key in [
        'buff', 'armor_bonus_per_100', 'damage_bonus_per_10k', 'regeneration_per_second_per_10k',
        'duration_ticks', 'hero_duration_ticks']}}
    effect['StatBuff'].update(modifier=raw(child), autocast_range=0)
    if not child_only:
        effect = {'ParasiteAreaBuff': profile}
    rate = mana('mana_regen', 10000)
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'],
        'source_key': recipe['source_key'], 'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': mana('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {
            'id': raw(recipe['source_key']), 'mana_cost': 0 if child_only else number(protected['mana_cost']),
            'cooldown_ticks': 0 if child_only else number(protected['cooldown'], 30),
            'range': 0 if child_only else number(parent['rng1'], 1024),
            'target_policy': 'ScriptedController' if child_only else 'RandomGroundEnemyUnit', 'effect': effect}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None if child_only else child,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'protected': 'resolved/protected-ability-fields.tsv', 'script': 'resolved/unit-spell-mechanics.tsv',
            'functions': [row['handler_function']]}}
