"""Desert building scripts released by an inert native Parasite trigger."""
import json
import re
from decimal import Decimal, ROUND_CEILING

PARENT_MASK = {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}


def _helpers():
    number = lambda value, scale=1: int((Decimal(str(value)) * scale).to_integral_value(rounding=ROUND_CEILING))
    raw = lambda value: int.from_bytes(value.encode('ascii'), 'big')
    return number, raw


def _replenish(params, detail, number, raw):
    functions = detail['functions']
    body = functions['spellReplenish']
    severe = re.search(r'\(\(\w+-(\d+)\)>\w+\)', functions['tK'])
    percent = re.search(r'\(\w+>0\.0\)\)and\(\w+<([\d.]+)\)', functions['uK'])
    order = re.findall(r'=([A-Z]0) __wurst_safe_GroupEnumUnitsInRect', body)
    gains = re.findall(r'SetUnitState\(\w+,\w+,\(unit_getState\(\w+,\w+\)\+(\d+)\.\)\)', body)
    if (params['selection_order'] != ['Y0', 'X0', 'W0'] or order != params['selection_order']
        or severe is None or percent is None or 'group_getRandom(' not in body
        or gains != [str(params['life_gain']), str(params['mana_gain'])]
        or 'unit_isAllyOf' not in functions['tK'] or 'isCombatSapper' not in functions['tK']
        or 'BlzGetUnitMaxHP' not in functions['sK']):
        raise ValueError('Beacon replenish control flow changed: ordered random tiers, ally combat bodies and fixed gains')
    return {'Replenish': {'life': params['life_gain'], 'mana': params['mana_gain'],
        'severe_missing_life': int(severe[1]), 'mana_below_per_10k': number(percent[1], 100)}}


def _hallucination(params, detail, number, raw):
    functions = detail['functions']
    body = functions['spellPeyote']
    child = params['effect_ability_id'].to_bytes(4, 'big').decode()
    fields = detail['all_fields'][child]
    excluded = re.search(r'GetUnitAbilityLevel\(\w+,(\d+)\)<=0', functions['isNotTentacle'])
    if (detail['all_bases'][child] != 'AIil' or 'checkForShield(' not in body or 'group_getRandom(' not in body
        or 'dummyCastTargetWithVision(' not in body or excluded is None
        or 'unit_isEnemyOf' not in functions['vK'] or 'isAliveCombatSapper' not in functions['vK']
        or set(fields['targs1'].split(',')) != {'ground', 'air'}
        or number(fields['dur1']) != number(fields['herodur1'])):
        raise ValueError('Peyote control flow changed: shield-checked random vulnerable enemy illusion cast')
    return {'Hallucination': {'illusion_ability': raw(child), 'illusion_ticks': number(fields['dur1'], 30),
        'damage_dealt_per_10k': number(fields['dataa1'], 10000), 'damage_taken_per_10k': number(fields['datab1'], 10000),
        'excluded_ability': int(excluded[1])}}


def _mana_burn(params, detail, number, raw):
    functions = detail['functions']
    body = functions['spellWitheringTouch']
    first, second = params['target_filters'][0], params['target_filters'][1]
    primary = functions[first['resolved_function']]
    fallback = functions[second['resolved_function']]
    excluded = re.search(r'GetUnitAbilityLevel\(\w+,(\d+)\)<=0', functions['isNotTentacle'])
    threshold = re.search(r'\(\w+>([\d.]+)\)\)$', primary.strip().rsplit('end', 1)[0])
    factor = re.search(r'=\(([\d.]+)\*\w+\)', body)
    burn = re.search(r'if\(\w+<(\w+)\)then \w+=\w+ else \w+=\1 end', body)
    if (params['attack_type'] != 'normal' or params['damage_type'] != 'sonic' or not params['attack_flag'] or params['ranged_flag']
        or params['damage_formula'] != '2 * min(target_mana, %d)' % params['mana_burn_cap']
        or first['symbol'] != 'G6' or second['symbol'] != 'F6' or threshold is None or factor is None or burn is None or excluded is None
        or not all(token in primary for token in ('isAliveCombatSapper', 'unit_isEnemyOf', 'isVulnerable', 'isNotTentacle'))
        or not all(token in fallback for token in ('isAliveCombatSapper', 'unit_isEnemyOf', 'isVulnerable', 'isNotTentacle'))
        or '(L4q>0.0)' not in fallback.replace('L4q', 'L4q') or 'checkForShield(' not in body
        or 'DAMAGE_TYPE_SONIC' not in body or 'bj_mapInitialPlayableArea' not in body):
        raise ValueError('Withering Touch control flow changed: shield-checked priority-random mana burn with doubled sonic damage')
    return {'ManaBurn': {'cap': params['mana_burn_cap'], 'priority_above': number(threshold[1]),
        'damage_per_mana': number(factor[1]), 'excluded_ability': int(excluded[1])}}


def _thunder(params, detail, number, raw):
    functions = detail['functions']
    cast = functions['BuildingSpellClosure_registerBuildingSpell_ThunderpawSpire_cast_registerBuildingSpell_ThunderpawSpire']
    listener = functions['DamageListener_addListener_doAfter_ThunderpawSpire_onEvent_addListener_doAfter_ThunderpawSpire']
    callback = functions['ForGroupCallback_forUnitsInRange_addListener_doAfter_ThunderpawSpire_callback_forUnitsInRange_addListener_doAfter_ThunderpawSpire']
    watch = functions['Action_watch_doAfter_ThunderpawSpire_run_watch_doAfter_ThunderpawSpire']
    special = params['special_source_ability_id']
    gate = re.search(r'\(\(\(\(\(__wurst_ensureInt\(RS\[\w+\]\)>0\)and\(not\(\w+==QS\)\)\)and\(DamageEvent_getType\(\)==0\)\)and\(DamageEvent_getAmount\(\)>([\d.]+)\)\)and\(not unit_isInvulnerable\(\w+\)\)\)', listener)
    ranges = re.search(r'\(\(\w+>([\d.]+)\)and\(\w+<([\d.]+)\)\)and\(\w+<([\d.]+)\)\)', listener)
    strike = re.search(r'=max\(([\d.]+),min1\(([\d.]+),\(DamageEvent_getAmount\(\)\*([\d.]+)\)\)\)', listener)
    if (params['attack_type'] != 'normal' or params['damage_type'] != 'universal' or params['charges_granted_per_cast'] != 1
        or params['charges_consumed_per_proc'] != 1 or not params['round_end_signal_resets_all_charge_slots']
        or gate is None or ranges is None or strike is None or ranges[2] != ranges[3]
        or f'unit_hasAbility(' not in listener or f'{special})then' not in listener
        or f'(F6n*{params["special_source_multiplier"]})' not in listener.replace('0.35', params['special_source_multiplier'])
        or 'forUnitsInRange(' not in listener or ',%s.,false,' % params['effect_radius'] not in listener
        or 'RS[' not in cast or '+1)' not in cast
        or not all(token in callback for token in ('isCombatSapper', 'unit_isEnemyOf', 'unit_isAlive', 'UNIT_TYPE_FLYING', 'DAMAGE_TYPE_UNIVERSAL'))
        or '*0.5' not in callback or 'RS[t6n]=0' not in watch.replace(' ', ' ')):
        raise ValueError('Call of Thunder control flow changed: team charge cast and damage-listener lightning')
    if (Decimal(strike[1]) != Decimal(params['minimum_damage']) or Decimal(strike[2]) != Decimal(params['maximum_damage'])
        or Decimal(strike[3]) != Decimal(params['damage_multiplier'])
        or Decimal(params['flying_recipient_multiplier']) != Decimal('0.5')):
        raise ValueError('Call of Thunder constants disagree with the retained trace')
    world = lambda value: number(value, 1024)
    return {'ThunderCharge': {'ability': raw(params_ability(detail)),
        'minimum_event_damage_exclusive': number(gate[1]), 'weapon_range_lower_exclusive': world(ranges[1]),
        'weapon_range_upper_exclusive': world(ranges[2]), 'strike_minimum': number(strike[1]), 'strike_maximum': number(strike[2]),
        'strike_multiplier': number(strike[3]), 'radius': world(params['effect_radius']),
        'flying_per_10k': number(params['flying_recipient_multiplier'], 10000), 'special_source_ability': special,
        'special_source_per_10k': number(params['special_source_multiplier'], 10000)}}


def params_ability(detail):
    return detail['building_spell']['ability_rawcode']


def _sandstorm(params, detail, number, raw):
    functions = detail['functions']
    handler = functions['BuildingSpellClosure_registerBuildingSpell_RaceDesertAbilities_cast_registerBuildingSpell_RaceDesertAbilities2']
    debuff_code, pulse_code = (code.to_bytes(4, 'big').decode() for code in params['carrier_ability_ids'])
    carrier = re.search(r'dummyCarrierWithAbilities\(unit_getOwner\(\w+\),%d,%d,\{0\.;3500\.\},([\d.]+)\)unit_issuePointOrderById\(\w+,%d,\{16\.;3500\.\}\)'
        % (params['carrier_ability_ids'][0], params['carrier_ability_ids'][1], params['order_id']), handler)
    fields = detail['all_fields']
    debuff, pulse = fields[debuff_code], fields[pulse_code]
    tokens = set(debuff['targs1'].split(','))
    if (carrier is None or Decimal(carrier[1]) != Decimal(params['duration_seconds'])
        or detail['all_bases'][debuff_code] != 'ACsi' or detail['all_bases'][pulse_code] != 'ANpi'
        or tokens - {'air', 'ground', 'enemies', 'vulnerable'} or tokens != set(pulse['targs1'].split(','))
        or number(debuff['dataa1']) != 0 or number(pulse['datab1']) != 0 or number(pulse['datac1']) != 0
        or Decimal(debuff['area1']) < 20000 or Decimal(pulse['area1']) < 20000
        or len(debuff['buffid1']) != 4 or number(debuff['cast1']) != 0):
        raise ValueError('Sandstorm control flow changed: owner dummy with a map-wide silence and immolation pair')
    movement = tokens & {'air', 'ground'}
    mask = sum(bit for token, bit in [('ground', 1), ('air', 2)] if token in movement)
    return {'Sandstorm': {'debuff': {'ability': raw(debuff_code), 'buff': raw(debuff['buffid1']), 'targets': mask,
        'organic_only': False, 'duration_ticks': number(debuff['dur1'], 30), 'hero_duration_ticks': number(debuff['herodur1'], 30),
        'movement_percent_delta': number(debuff['datac1'], 100), 'attack_speed_percent_delta': number(debuff['datad1'], 100),
        'damage_bonus_per_10k': 0, 'ethereal': None, 'miss_chance_per_10k': number(debuff['datab1'], 10000)},
        'pulse_ability': raw(pulse_code), 'pulse_damage': number(pulse['dataa1']), 'pulse_interval_ticks': number(pulse['dur1'], 30),
        'duration_ticks': number(carrier[1], 30), 'targets': mask}}


def _energy_shield(params, detail, number, raw):
    functions = detail['functions']
    handlers = {name: functions[name] for name in detail['building_spell']['source_functions'].split(',')
        if name in ('blueShieldSpell', 'redShieldSpell')}
    shield, level = params['shield_ability_id'], params['shield_level']
    picker = functions['ForGroupCallback_forUnitsInRect_RaceMechAbilities_callback_forUnitsInRect_RaceMechAbilities']
    chooser = functions['randomAllySapper']
    avul = raw('Avul')
    if (detail['building_spell']['mechanic_kind'] == 'apply-shield-level-1') != (list(handlers) == ['blueShieldSpell']):
        raise ValueError('shield generator handler does not match its retained mechanic kind')
    if 'blueShieldSpell' in handlers:
        body = handlers['blueShieldSpell']
        upgrade = False
        if (level != 1 or not re.search(r'=randomAllySapper\(\w+,true\)if\(\w+==nil\)then return end if\(widget_getLife\(\w+\)>0\.405\)then \w+=\w+ addProtectedAbility\(\w+,%d\)' % shield, body)
                or 'SetUnitAbilityLevel' in body):
            raise ValueError('Blue shield control flow changed: random unshielded ally receives a level-one shield')
    else:
        body = handlers['redShieldSpell']
        upgrade = True
        if (level != 2 or not params.get('fallback_can_upgrade_existing_shield')
                or not re.search(r'(\w+)=randomAllySapper\(\w+,true\)if\(\1==nil\)then \1=randomAllySapper\(\w+,false\)if\(\1==nil\)then return end '
                    r'elseif\(widget_getLife\(\1\)>0\.405\)then \w+=\1 addProtectedAbility\(\w+,%d\)end \w+=\1 __wurst_safe_SetUnitAbilityLevel\(\w+,%d,2\)' % (shield, shield), body)):
            raise ValueError('Red shield control flow changed: unshielded-first selection, any-ally fallback and level-two shield')
    if (not re.search(r'isAliveCombatSapper\(\w+\)and unit_isAllyOf\(\w+,\w+\.p\)\)and\(unit_getAbilityLevel\(\w+,%d\)<=0\)\)and\(\(not \w+\.requireUnshielded\)or\(unit_getAbilityLevel\(\w+,%d\)<=0\)\)' % (avul, shield), picker)
            or 'GroupEnumUnitsInRect(' not in chooser or 'group_getRandom(' not in chooser):
        raise ValueError('shield recipient picker changed: ally combat bodies, no invulnerable marker, optional unshielded filter')
    return {'EnergyShield': {'shield_ability': shield, 'level': level, 'upgrade_existing': upgrade}}


def project(recipe, fields, unit, protected, detail):
    number, raw = _helpers()
    row = detail['building_spell']
    params = json.loads(row['parameters_json'])
    tokens = set(fields['targs1'].split(','))
    if detail['native_base'] != 'ACpa' or tokens != PARENT_MASK or any(number(fields[key]) != 0 for key in ('dataa1', 'datab1', 'datac1', 'datad1')):
        raise ValueError('building script parent must be the inert native Parasite with the audited hostile mask')
    kind = row['mechanic_kind']
    if kind == 'priority-random-replenish':
        child = _replenish(params, detail, number, raw)
    elif kind == 'dummy-target-illusion':
        child = _hallucination(params, detail, number, raw)
    elif kind == 'armed-next-melee-attack-splash':
        child = _thunder(params, detail, number, raw)
    elif kind == 'mana-burn-random-target':
        child = _mana_burn(params, detail, number, raw)
    elif kind == 'global-weather-ability-carrier':
        child = _sandstorm(params, detail, number, raw)
    elif kind in ('apply-shield-level-1', 'apply-or-upgrade-shield-level-2'):
        child = _energy_shield(params, detail, number, raw)
    else:
        raise ValueError(f'unsupported building script {kind!r}')
    rate = number(unit['mana_regen'], 10000)
    profile = {'ability': raw(recipe['source_key']), 'trigger_targets': 5, 'trigger_invulnerable': True,
        'trigger_spell_immune': False, 'trigger_mechanical_only': False, 'child': child}
    return {'kind': 'scripted-automatic', 'source_kind': recipe['source_kind'], 'source_key': recipe['source_key'],
        'unit_rawcode': recipe['unit_rawcode'],
        'spellcasting': {'mana': {'maximum': number(unit['mana_max']), 'starting': 0, 'regen_per_tick_per_10k': (1 << 31) | rate},
            'ability': {'id': raw(recipe['source_key']), 'mana_cost': number(protected['mana_cost']),
                'cooldown_ticks': number(protected['cooldown'], 30), 'range': number(fields['rng1'], 1024),
                'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'BuildingScript': profile}}},
        'mana_regen_per_second_per_10k': rate, 'effect_ability_rawcode': None,
        'provenance': {'git_tree': detail['git_tree'], 'source': 'resolved/building-spell-mechanics.tsv',
            'fields': 'resolved/object-fields.tsv', 'protected': 'resolved/protected-ability-fields.tsv',
            'functions': row['source_functions']}}
