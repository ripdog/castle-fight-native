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


def _locust_swarm(recipe, detail, number, raw):
    semantics, row = detail['semantics_row'], detail['mechanics_row']
    parameters = detail['semantic_parameters']
    handler = detail['functions'][row['handler_function']]
    fields = detail['all_fields']
    child = 'A0GM'
    swarm = fields[child]
    body_code = swarm['unitid1']
    body, stats = detail['all_units'][body_code], detail['all_unit_stats'][body_code]
    site = parameters['delivery_sites']
    lifetime = re.search(r'dummyCastImmediateFrom1\(unit_getOwner\((\w+)\),%d,OrderId\("locustswarm"\),unit_getPos\((\w+)\),([\d.]+)\)orderCodeAttack\(\1\)end$' % raw(child), handler)
    params = re.search(r'\((\w+),(\w+),(\w+)\)local (\w+)=nil local (\w+)=nil (\4)=\2 (\5)=\3 ', handler)
    listener = detail['functions']['DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire']
    structure = re.search(r'if\(\(unit_getTypeId\((\w+)\)==%d\)and unit_isType\((\w+),UNIT_TYPE_STRUCTURE\)\)then (\w+)=\(DamageEvent_getAmount\(\)\*([\d.]+)\)' % raw(body_code), listener)
    structure = structure or re.search(r'unit_getTypeId\((\w+)\)==%d\)and unit_isType\((\w+),UNIT_TYPE_STRUCTURE\)\)then (\w+)=\(DamageEvent_getAmount\(\)\*([\d.]+)\)' % raw(body_code), listener)
    if (detail['all_bases'][child] != 'AUls' or semantics['effect_rawcodes'] != child or len(site) != 1
        or site[0]['callee'] != 'dummyCastImmediateFrom1' or lifetime is None or params is None
        or lifetime[1] != params[4] or lifetime[2] != params[5]
        # the Parasite handler's second argument is the caster, the third its target
        or structure is None
        or Decimal(lifetime[3]) != Decimal(swarm['dur1']) or Decimal(swarm['dur1']) != Decimal(swarm['herodur1'])
        or body['base_rawcode'] != 'uloc' or body['move_type'] != 'fly' or stats.get('override_attack1_dice_number') is None
        or set(body['attack1_targets'].split(',')) - {'air', 'debris', 'enemies', 'ground', 'structure'}
        or body['attack1_type'] != 'spells'
        or number(swarm['dataa1']) > 8 or number(swarm['datae1']) > (number(stats['override_attack1_base_damage']) + number(stats['override_attack1_dice_number']))):
        raise ValueError('Harpy Swarm control flow changed: dummy-owned stationary swarm at the Parasite target')
    tokens = set(body['attack1_targets'].split(','))
    mask = sum(bit for token, bit in [('ground', 1), ('air', 2), ('structure', 4)] if token in tokens)
    speed = number(stats['override_move_speed'], 1024) // 30
    return {'LocustSwarm': {'ability': raw(child), 'body_rawcode': raw(body_code), 'count': number(swarm['dataa1']),
        'release_interval_millis': number(swarm['datab1'], 1000), 'max_per_target': number(swarm['datac1']),
        'duration_ticks': number(swarm['dur1'], 30), 'radius': number(swarm['area1'], 1024),
        'base_damage': number(stats['override_attack1_base_damage']),
        'dice_number': number(stats['override_attack1_dice_number']), 'dice_sides': number(stats['override_attack1_dice_sides']),
        'attack_cooldown_ticks': number(body['attack1_cooldown'], 30), 'flight_speed_per_tick': speed,
        'structure_damage_per_10k': number(structure[4], 10000), 'targets': mask}}


def _berserk(recipe, detail, number, raw):
    row, parameters = detail['mechanics_row'], detail['semantic_parameters']
    handler = detail['functions'][row['handler_function']]
    first, second = (detail['functions'][name] for name in row['delayed_callback_functions'].split(',')[:2])
    child = parameters['berserk_ability_rawcode']
    swarm = detail['all_fields'][child]
    delay = RANDOM_DELAY.search(handler)
    if (detail['all_bases'][child] != 'Absk' or parameters['berserk_order_id'] != 852100
        or delay is None or [float(delay[1]), float(delay[2])] != parameters['random_activation_delay_seconds']
        or 'unit_issueImmediateOrderById(ymn.caster,852100)' not in first
        or 'orderCodeAttack(' not in handler or 'orderCodeAttack(Bmn.caster)' not in second
        or swarm['targs1'] not in ('_', '-') or number(swarm['dur1']) != parameters['duration_seconds']
        or Decimal(swarm['dataa1']) != Decimal(str(parameters['movement_speed_increase']))
        or Decimal(swarm['datab1']) != Decimal(str(parameters['attack_speed_increase']))
        or Decimal(swarm['datac1']) != Decimal(str(parameters['damage_taken_increase']))):
        raise ValueError('Ogre Lord rage control flow changed: random delay, native instant Berserk on the caster, attack resume')
    return {'Berserk': {'berserk': {'ability': raw(child), 'duration_ticks': number(swarm['dur1'], 30),
        'movement_percent_delta': number(swarm['dataa1'], 100), 'attack_speed_percent_delta': number(swarm['datab1'], 100),
        'damage_taken_bonus_per_10k': number(swarm['datac1'], 10000)},
        'minimum_delay_ticks': number(delay[1], 30), 'maximum_delay_ticks': number(delay[2], 30)}}


def _sand_cloud(recipe, detail, number, raw):
    sand = detail['functions']['spellSandCloud']
    callback = detail['functions']['CallbackPeriodic_doPeriodically_RaceDesertAbilities_call_doPeriodically_RaceDesertAbilities']
    match = re.search(
        r'(\w+)=unit_getX\((\w+)\)(\w+)=unit_getY\(\2\)(\w+)=(\d+) if\(unit_getTypeId\((\w+)\)==(\d+)\)then \4=(\d+) end '
        r'(\w+)=dummyCarrierWithAbility\(unit_getOwner\(\6\),\4,\{\1;\3\},([\d.]+)\)orderCodeAttack\(\6\)(\w+)=\w+:create\d+\(\)'
        r'\w+\.ca=\9 \w+\.x=\1 \w+\.y=\3 \w+\.i=0 doPeriodically\(([\d.]+),\11\)end$', sand)
    again = re.search(r'unit_issuePointOrderById\(\w+\.ca,P6,\{\w+\.x;\w+\.y\}\)\w+\.i=\(\w+\.i\+1\)if\(\w+\.i>=(\d+)\)then CallbackPeriodic_destroyCallbackPeriodic', callback)
    if match is None or again is None:
        raise ValueError('Sandcloud control flow changed: target-position carrier, caster-type variant and periodic point order')
    standard, special_code, special = match[5], int(match[7]), match[8]
    variants = []
    for ability in (f'{standard}', f'{special}'):
        pass
    codes = [(int(match[5]), 'standard'), (int(match[8]), 'special')]
    names = [value.to_bytes(4, 'big').decode() for value, _ in codes]
    profiles = []
    for name in names:
        fields = detail['all_fields'][name]
        tokens = set(fields['targs1'].split(','))
        if (detail['all_bases'][name] != 'ACsi' or number(fields['dataa1']) != 0 or number(fields['datac1']) != 0
            or number(fields['datad1']) != 0 or number(fields['cast1']) != 0 or len(fields['buffid1']) != 4
            or not {'enemies', 'enemy'} & tokens or not tokens & {'air', 'ground'}
            or tokens - {'air', 'ground', 'enemies', 'enemy', 'structure', 'neutral', 'invulnerable', 'vulnerable'}):
            raise ValueError('Sandcloud child requires the audited free miss-chance Silence variant')
        movement = tokens & {'air', 'ground'}
        profiles.append({'radius': number(fields['area1'], 1024), 'debuff': {
            'ability': raw(name), 'buff': raw(fields['buffid1']),
            'targets': sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement),
            'organic_only': False, 'duration_ticks': number(fields['dur1'], 30), 'hero_duration_ticks': number(fields['herodur1'], 30),
            'movement_percent_delta': 0, 'attack_speed_percent_delta': 0, 'damage_bonus_per_10k': 0, 'ethereal': None,
            'miss_chance_per_10k': number(fields['datab1'], 10000)}})
    period = Decimal(match[12])
    if period != Decimal(1) or int(again[1]) != number(detail['semantic_parameters']['cast_iterations']):
        raise ValueError('Sandcloud period/iterations disagree with the retained semantics')
    return {'SandCloud': {'carrier_ticks': number(match[10], 30), 'period_ticks': number(period, 30),
        'pulses': int(again[1]), 'special_caster_rawcode': special_code,
        'standard': profiles[0], 'special': profiles[1]}}


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
    elif kind == 'delayed-berserk-rage':
        child = _berserk(recipe, detail, number, raw)
    elif kind == 'periodic-sandcloud-carrier':
        child = _sand_cloud(recipe, detail, number, raw)
    elif kind == 'proxy-object-effect':
        child = _locust_swarm(recipe, detail, number, raw)
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
        'spellcasting': {'mana': {'maximum': mana('mana_max'), 'starting': min(mana('mana_start'), mana('mana_max')),
            'regen_per_tick_per_10k': (1 << 31) | rate}, 'ability': {
            'id': raw(recipe['source_key']), 'mana_cost': number(protected.get('mana_cost', fields['cost1'])),
            'cooldown_ticks': number(protected.get('cooldown', fields['cool1']), 30),
            'range': number(fields['rng1'], 1024), 'target_policy': 'RandomEnemyUnit',
            'effect': {'ParasiteScript': profile}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/object-fields.tsv',
            'protected': 'resolved/protected-ability-fields.tsv', 'script': 'resolved/unit-spell-mechanics.tsv',
            'semantics': 'resolved/unit-spell-semantics.tsv', 'functions': [row['handler_function'], row['delayed_callback_functions']]}}
