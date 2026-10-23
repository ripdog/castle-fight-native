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
    if (detail['all_bases'][child] != 'AUls' or semantics['effect_rawcodes'] != child or len(site) != 1
        or site[0]['callee'] != 'dummyCastImmediateFrom1' or lifetime is None or params is None
        or lifetime[1] != params[4] or lifetime[2] != params[5]
        or Decimal(lifetime[3]) != Decimal(swarm['dur1']) or Decimal(swarm['dur1']) != Decimal(swarm['herodur1'])
        or body['base_rawcode'] != 'uloc' or body['move_type'] != 'fly' or stats.get('override_attack1_dice_number') is None
        or set(body['attack1_targets'].split(',')) - {'air', 'debris', 'enemies', 'ground', 'structure'}
        or body['attack1_type'] != 'spells'
        or number(swarm['dataa1']) > 8 or number(swarm['datae1']) > (number(stats['override_attack1_base_damage']) + number(stats['override_attack1_dice_number']))):
        raise ValueError('Harpy Swarm control flow changed: dummy-owned stationary swarm at the Parasite target')
    return {'LocustSwarm': {'ability': raw(child), 'body_rawcode': raw(body_code), 'count': number(swarm['dataa1']),
        'release_interval_millis': number(swarm['datab1'], 1000), 'max_per_target': number(swarm['datac1']),
        'duration_ticks': number(swarm['dur1'], 30), 'radius': number(swarm['area1'], 1024)}}


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


def _landmine(recipe, detail, number, raw):
    row, parameters = detail['mechanics_row'], detail['semantic_parameters']
    functions = detail['functions']
    handler = functions[row['direct_calls']]
    callback = functions[row['delayed_callback_functions']]
    trained = functions['onUnitTrained']
    mine, carrier = parameters['landmine_unit_rawcode'], parameters['carrier_unit_rawcode']
    unit = int.from_bytes(recipe['unit_rawcode'].encode(), 'big')
    distance = re.search(r'GetRandomReal\(([\d.]+),([\d.]+)\)Osr=tupleCopy1\(vec2_polarOffset\(Msr,real_asAngleDegrees\(GetRandomReal\(([\d.]+),([\d.]+)\)\),Nsr\)\)', handler)
    speed = re.search(r'Rsr=\(Nsr/([\d.]+)\)', handler)
    mine_code, carrier_code = (int(parameters[key].encode().hex(), 16) for key in ('landmine_unit_rawcode', 'carrier_unit_rawcode'))
    if (row['mechanic_kind'] != 'delegated-helper' or row['direct_calls'] != 'mineLayerSpell'
        or distance is None or speed is None
        or [Decimal(distance[1]), Decimal(distance[2])] != [Decimal(parameters['placement_distance_min']), Decimal(parameters['placement_distance_max'])]
        or [Decimal(distance[3]), Decimal(distance[4])] != [Decimal(parameters['placement_angle_degrees'][0]), Decimal(parameters['placement_angle_degrees'][1])]
        or Decimal(speed[1]) != Decimal(parameters['carrier_speed'])
        or f'createUnit(Lsr,{carrier_code},Msr,{{0.}})' not in handler
        or 'unit_issuePointOrderById(Psr,851984,Osr)' not in handler
        or 'doAfter(Rsr,Qsr)' not in handler
        or f'createUnit(Yrn.p,{mine_code},Yrn.minePos,{{0.}})' not in callback.replace(' ', '')):
        raise ValueError('Mine Layer control flow changed: random-offset carrier, travel-time delay and mine creation')
    units = detail['all_units']
    abilities = units[mine]['abilities'].split(',')
    explosion_code, mine_code_ability = abilities[1], abilities[2]
    if (abilities[0] != 'Aloc' or len(abilities) != 3 or detail['all_bases'][explosion_code] != 'Amnx'
        or detail['all_bases'][mine_code_ability] != 'Amin'
        or units[carrier]['abilities'].split(',') != ['Aloc', 'Avul']):
        raise ValueError('Landmine inventory changed: locust body with native mine and death explosion')
    explosion_fields = dict(detail['all_fields'][explosion_code])
    if set(explosion_fields['targs1'].split(',')) != {'ground', 'structure', 'debris', 'enemy'}:
        raise ValueError('Landmine explosion needs its audited ground/structure enemy mask')
    explosion_fields['targs1'] = 'ground,enemies,structure,debris,ward'
    from project_death_explosion import project as project_explosion
    explosion = project_explosion({'source_key': explosion_code}, explosion_fields, 'Amnx')['profile']
    trigger = detail['all_fields'][mine_code_ability]
    train = detail.get('train_mana')
    if (train is None or train['mana_random_distribution'] != 'uniform-real'
        or f'elseif(unit_getTypeId(cgs)=={unit})then fgs=cgs ggs=UNIT_STATE_MANA hgs=GetRandomReal(.20,30.)__wurst_safe_SetUnitState(fgs,ggs,hgs)' not in trained
        or [Decimal(str(train['mana_random_min'])), Decimal(str(train['mana_random_max']))] != [Decimal('0.2'), Decimal('30')]):
        raise ValueError('Mine Layer train-event random initial mana changed')
    return {'LandMine': {'ability': raw(mine_code_ability), 'minimum_distance': number(distance[1], 1024),
        'maximum_distance': number(distance[2], 1024), 'carrier_speed_per_second': number(speed[1], 1024),
        'activation_ticks': number(trigger['dataa1'], 30), 'detection_radius': number(trigger['rng1'], 1024),
        'trigger_targets': 1, 'explosion': explosion,
        'initial_mana_minimum_10k': number(train['mana_random_min'], 10000),
        'initial_mana_maximum_10k': number(train['mana_random_max'], 10000)}}


def _infest(recipe, fields, detail, number, raw):
    """The stock damaging Parasite plus the map's two-step autocast script (`RaceNatureAbilities`)."""
    parameters = detail['semantic_parameters']
    names = detail['semantics_row']['source_functions'].split(',')
    cast, first, second = (detail['functions'][name] for name in names)
    source = recipe['source_key']
    web = parameters['first_followup_ability_rawcode']
    web_fields = detail['all_fields'][web]
    wait = lambda text: re.search(r'doAfter\(([\d.]+),\w+\)end$', text)
    web_order = re.search(r'^function \w+\((\w+)\)local \w+=nil unit_issueImmediateOrderById\(\1\.caster,(\d+)\)\w+=\w+:create\d+\(\)\w+\.caster=\1\.caster doAfter\(([\d.]+),\w+\)end$', first)
    own_order = re.search(r'^function \w+\((\w+)\)unit_issueImmediateOrderById\(\1\.caster,(\d+)\)orderCodeAttack\(\1\.caster\)end$', second)
    opening = re.search(r'^function \w+\(\w+,(\w+),\w+\).*?\.caster=\w+ doAfter\(([\d.]+),\w+\)end$', cast)
    if (detail['all_bases'][source] != 'ACpa' or web_order is None or own_order is None or opening is None
        or int(web_order[2]) != parameters['first_followup_order_id']
        or int(own_order[2]) != parameters['second_followup_order_id']
        or Decimal(opening[2]) != Decimal(str(parameters['first_followup_delay_seconds']))
        or Decimal(web_order[3]) != Decimal(str(parameters['second_followup_delay_seconds']))
        or web_fields['orderon'] != parameters['first_followup_order_name']
        or fields['orderon'] != parameters['second_followup_order_name']
        or parameters['second_followup_ability_rawcode'] != source
        or not parameters['resume_attack_after_second_followup']
        or detail['all_bases'][web] != 'Aweb'):
        raise ValueError('Infest control flow changed: cast-time wait, Web autocast on, Infest autocast on, attack resume')
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    if (detail['all_bases'][source] != 'ACpa' or number(fields['datab1']) != 0 or number(fields['datac1']) != 0
        or len(fields['buffid1']) != 4 or len(fields['unitid1']) != 4 or fields['missilehoming'] != '1'
        or number(fields['dataa1']) <= 0 or number(fields['datae1']) <= 0
        or parameters['primary_effect_spawn_unit_rawcode'] != fields['unitid1']
        or parameters['primary_effect_spawn_count_on_kill'] != number(fields['datae1'])
        or parameters['primary_effect_damage_per_second'] != number(fields['dataa1'])):
        raise ValueError('Infest requires the stock damaging Parasite with an audited summon')
    ticks = lambda field: number(fields[field], 30)
    parent = {'buff_rawcode': raw(fields['buffid1']), 'map_version': detail['map_version'],
        'speed_per_tick': int(Decimal(str(fields['missilespeed'])) * 1024 / 30),
        'affects_invulnerable': 'invulnerable' in tokens, 'duration_ticks': ticks('dur1'),
        'hero_duration_ticks': ticks('herodur1'), 'body_rawcode': raw(fields['unitid1']),
        'count': number(fields['datae1']), 'body_duration_ticks': ticks('cast1')}
    poison = {'ability': raw(source), 'buff_rawcode': raw(fields['buffid1']), 'damage_per_second': number(fields['dataa1']),
        'movement_percent_delta': -number(fields['datab1'], 100), 'attack_speed_percent_delta': -number(fields['datac1'], 100),
        'duration_ticks': ticks('dur1'), 'hero_duration_ticks': ticks('herodur1'),
        'targets': sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement),
        'stack_damage': False, 'can_kill': True, 'organic_only': 'organic' in tokens, 'ward': None}
    return {'Infest': {'poison': poison, 'web': raw(web),
        'web_delay_ticks': number(opening[2], 30), 'reenable_delay_ticks': number(web_order[3], 30)}}, parent


def _treant_sprout(recipe, fields, detail, number, raw):
    """`keeperSpell`: displace nearby ground bodies, wall the caster in with trees, then raise Treants."""
    parameters = detail['semantic_parameters']
    names = detail['semantics_row']['source_functions'].split(',')
    closure, spell, transform, spawn, remove = (detail['functions'][name] for name in names)
    angles = parameters['tree_angles_degrees']
    tree = parameters['tree_rawcode']
    row = detail['destructable'](tree)
    texture = detail['pathing_textures'][row['pathTex'].split('\\')[-1].lower()]
    rows = texture['hex_rows']
    scatter = re.search(r'randomAliveGroundSapperNear\(\w+,([\d.]+)\)', spell)
    distance = re.search(r'=GetRandomReal\(([\d.]+),([\d.]+)\)\w+=tupleCopy2\(real_asAngleDegrees\(\(GetRandomReal\(([\d.]+),([\d.]+)\)\+\(180\.\*__wurst_ensureInt\(lGb\[player_getId\(unit_getOwner\(\w+\)\)\]\)\)\)\)\)', spell)
    walls = re.findall(r'CreateDestructable\((\d+),\(\w+\[1\]\+\(([\d.]+)\*Cos\(\(([\d.]+)\*bj_DEGTORAD\)\)\)\)', spell)
    wait = re.search(r'doAfter\(([\d.]+),\w+\)end$', spell)
    births = re.findall(r'spawnTreantFromWall\(\w+\.p,\w+\.d\d,\w+\.pos,([\d.]+)\)', transform)
    child = re.search(r'createUnit\(\w+,(\d+),\w+,real_asAngleDegrees\(\w+\)\)__wurst_safe_UnitApplyTimedLife\(\w+,1110454833,([\d.]+)\)', spawn)
    ring = re.search(r'vec2_polarOffset\(\w+,real_asAngleDegrees\(\w+\),([\d.]+)\)', spawn)
    if (closure.strip().count('keeperSpell(') != 1 or scatter is None or distance is None or wait is None or child is None
            or ring is None or len(walls) != len(angles) or len(births) != len(angles)
            or [Decimal(a) for _, _, a in walls] != [Decimal(a) for a in angles]
            or [Decimal(a) for a in births] != [Decimal(a) for a in angles]
            or any(int(code) != raw(tree) or Decimal(radius) != Decimal(parameters['tree_radius']) for code, radius, _ in walls)
            or Decimal(scatter[1]) != Decimal(parameters['nearby_ground_clearance_radius'])
            or [Decimal(distance[1]), Decimal(distance[2])] != [Decimal(v) for v in parameters['nearby_unit_displacement_radius']]
            or Decimal(wait[1]) != Decimal(parameters['transform_delay_seconds'])
            or int(child[1]) != raw(parameters['treant_rawcode']) or Decimal(child[2]) != Decimal(parameters['treant_timed_life_seconds'])
            or Decimal(ring[1]) != Decimal(parameters['treant_spawn_radius'])
            or 'orderCodeAttack(' not in transform or 'KillDestructable' not in transform
            or parameters['tree_count'] != len(angles) or len(angles) > 8
            or row['targType'] != 'tree' or texture['width_cells'] != texture['height_cells']
            or any(set(r) != {'5'} for r in rows)):
        raise ValueError('Treant Sprout control flow changed: displaced bystanders, tree ring, transform delay, timed Treants')
    cells = texture['width_cells']
    padded = list(angles) + [0] * (8 - len(angles))
    return {'TreantSprout': {'ability': raw(recipe['source_key']), 'map_version': detail['map_version'],
        'clearance_radius': number(scatter[1], 1024),
        'displacement_minimum': number(distance[1], 1024), 'displacement_maximum': number(distance[2], 1024),
        'bearing_minimum_degrees': number(distance[3]), 'bearing_maximum_degrees': number(distance[4]),
        'tree_count': len(angles), 'tree_angles_degrees': padded, 'tree_radius': number(parameters['tree_radius'], 1024),
        'tree_cells': cells, 'transform_delay_ticks': number(wait[1], 30), 'treant_rawcode': raw(parameters['treant_rawcode']),
        'treant_radius': number(ring[1], 1024), 'treant_life_ticks': number(child[2], 30)}}


def _taunt(recipe, fields, detail, number, raw):
    """The Mountain Giant: the registered parasite spell issues the native Taunt order, then resumes the lane."""
    parameters = detail['semantic_parameters']
    closure, callback = (detail['functions'][name] for name in
        (detail['semantics_row']['source_functions'], detail['mechanics_row']['delayed_callback_functions']))
    taunt = parameters['taunt_ability_rawcode']
    row = detail['all_fields'][taunt]
    tokens = set(row['targs1'].split(','))
    shape = (r'function \w+\(\w+,(\w+),\w+\)local \w+=nil local \w+=nil \w+=\1 unit_issueImmediateOrderById\(\w+,(\d+)\)'
        r'(\w+)=\w+:create\d+\(\)\3\.caster=\w+ doAfter\(([\d.]+),\3\)end')
    matched = re.fullmatch(shape, closure.strip())
    if (detail['all_bases'][taunt] != 'Atau' or tokens != {'air', 'ground', 'enemy', 'vuln', 'invu'} or matched is None
            or int(matched[2]) != parameters['immediate_order_id']
            or Decimal(matched[4]) != Decimal(str(parameters['resume_attack_delay_seconds']))
            or not re.fullmatch(r'function \w+\((\w+)\)orderCodeAttack\(\1\.caster\)end', callback.strip())
            or number(row['area1']) != parameters['taunt_area'] or number(row['cost1']) != 0):
        raise ValueError('Taunt control flow changed: immediate native Taunt at the caster and the resume callback')
    return {'Taunt': {'ability': raw(taunt), 'radius': number(row['area1'], 1024),
        'resume_delay_ticks': number(matched[4], 30)}}


def project(recipe, fields, unit, protected, detail):
    number, raw = _helpers()
    row, semantics = detail['mechanics_row'], detail['semantics_row']
    if detail['all_bases'][recipe['source_key']] != 'ACpa' or row['ability_rawcode'] != recipe['source_key']:
        raise ValueError('Parasite script parent must be the native Parasite ability')
    tokens = set(fields['targs1'].split(','))
    movement = tokens & {'air', 'ground'}
    kind = semantics['semantic_kind']
    allowed = {'air', 'ground', 'enemy', 'enemies', 'organic', 'neutral', 'nonhero', 'sapper', 'ward'}
    if kind == 'random-offset-landmine-placement':
        # The Mine Layer's autocast mask also names structures, mechanical bodies and invulnerable
        # targets. The parent fires on the unit set below; the discrepancy is recorded in the
        # race's implementation notes rather than silently flattened.
        allowed |= {'invulnerable', 'mechanical', 'structure'}
    if (not {'enemy', 'enemies'} & tokens or not movement or tokens - allowed
        or number(fields['levels']) != 1):
        raise ValueError('Parasite script parent needs an audited hostile unit mask')
    detail = dict(detail, semantic_parameters=__import__('json').loads(semantics['parameters_json']))
    parent = None
    if kind == 'taunt-plus-resume':
        child = _taunt(recipe, fields, detail, number, raw)
    elif kind == 'five-tree-wall-to-treants':
        child = _treant_sprout(recipe, fields, detail, number, raw)
    elif kind == 'registered-infest-then-enable-autocasts':
        child, parent = _infest(recipe, fields, detail, number, raw)
    elif kind == 'delayed-life-steal':
        child = _life_steal(recipe, detail, number)
    elif kind == 'delayed-berserk-rage':
        child = _berserk(recipe, detail, number, raw)
    elif kind == 'periodic-sandcloud-carrier':
        child = _sand_cloud(recipe, detail, number, raw)
    elif kind == 'proxy-object-effect':
        child = _locust_swarm(recipe, detail, number, raw)
    elif kind == 'random-offset-landmine-placement':
        child = _landmine(recipe, detail, number, raw)
    else:
        raise ValueError(f'unsupported Parasite script child {kind!r}')
    mana = lambda field, scale=1: 0 if unit[field].strip() in {'-', '_', ''} else number(unit[field], scale)
    rate = mana('mana_regen', 10000)
    profile = {'parent': parent if parent is not None else project_parasite(fields, detail['map_version']),
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
