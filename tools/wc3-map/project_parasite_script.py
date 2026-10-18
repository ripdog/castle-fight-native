"""Native targeted Parasite parents whose map-script children are projected separately."""
import re
from decimal import Decimal, ROUND_CEILING

from project_naga_unit import project_parasite

RANDOM_DELAY = re.compile(r'GetRandomReal\(([\d.]+),([\d.]+)\)')


def _helpers():
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    return number, raw


def _life_steal(recipe, detail, number):
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    parameters = detail['semantic_parameters']
    handler = detail['functions'][row['handler_function']]
    callback = detail['functions'][row['delayed_callback_functions']]
    delay = RANDOM_DELAY.search(handler)
    maximum = parameters['maximum_life_stolen']
    if (semantics['semantic_kind'] != 'delayed-life-steal' or row['mechanic_kind'] != 'scheduled-callback'
        or delay is None or [float(delay[1]), float(delay[2])] != parameters['random_delay_seconds']
        or parameters['damage_attack_type'] != 'chaos' or parameters['damage_type'] != 'normal'
        or not parameters['damage_equals_life_stolen']
        or f'Emn={maximum}.' not in callback.replace('=75.', '=75.').replace(' ', '') and f'={maximum}.' not in callback
        or f'if({maximum}.>Fmn)then Emn=Fmn end' not in callback
        or 'UnitDamageTarget(Gmn,Hmn,Imn,true,false,Jmn,Kmn,Lmn)' not in callback.replace('__wurst_safe_', '')
        or 'ATTACK_TYPE_CHAOS' not in callback or 'DAMAGE_TYPE_NORMAL' not in callback
        or '(widget_getLife(Dmn.caster)+Emn)' not in callback
        or 'UNIT_STATE_LIFE' not in callback):
        raise ValueError('Harpy life steal control flow changed: clamp, chaos attack damage and requested-amount heal')
    return {'LifeSteal': {'maximum_life': maximum,
        'minimum_delay_ticks': number(delay[1], 30), 'maximum_delay_ticks': number(delay[2], 30)}}


def project(recipe, fields, unit, protected, detail):
    number, raw = _helpers()
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    if detail['all_bases'][recipe['source_key']] != 'ACpa' or row['ability_rawcode'] != recipe['source_key']:
        raise ValueError('Parasite script parent must be the native Parasite ability')
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (not {'enemy', 'enemies'} & tokens or not movement or tokens - {'air', 'ground', 'enemy', 'enemies', 'organic', 'neutral', 'nonhero'}
        or number(fields['levels']) != 1):
        raise ValueError('Parasite script parent needs an audited hostile unit mask')
    detail = dict(detail, semantic_parameters=__import__('json').loads(semantics['parameters_json']))
    kind = semantics['semantic_kind']
    if kind == 'delayed-life-steal':
        child = _life_steal(recipe, detail, number)
    else:
        raise ValueError(f'unsupported Parasite script child {kind!r}')
    mana = lambda field, scale=1: 0 if unit[field].strip() in {'-', '_', ''} else number(unit[field], scale)
    rate = mana('mana_regen', 10000)
    profile = {'parent': project_parasite(fields, detail['map_version']),
        'parent_targets': sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement),
        'parent_organic_only': 'organic' in tokens, 'parent_nonhero_only': 'nonhero' in tokens,
        'ability': raw(recipe['source_key']), 'child': child}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': mana('mana_start'),
            'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {
            'id': raw(recipe['source_key']), 'mana_cost': number(protected.get('mana_cost', fields['cost1'])),
            'cooldown_ticks': number(protected.get('cooldown', fields['cool1']), 30),
            'range': number(fields['rng1'], 1024), 'target_policy': 'RandomEnemyUnit',
            'effect': {'ParasiteScript': profile}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'protected': 'resolved/protected-ability-fields.tsv', 'script': 'resolved/unit-spell-mechanics.tsv',
            'semantics': 'resolved/unit-spell-semantics.tsv', 'functions': [row['handler_function'], row['delayed_callback_functions']]}}
