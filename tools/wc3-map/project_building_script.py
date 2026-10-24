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


def _raise_from_corpse(params, detail, number, raw):
    functions = detail['functions']
    names = detail['building_spell']['source_functions'].split(',')
    closure = functions[names[0]]
    body = functions['raiseFromCorpse']
    picker = functions['ForGroupCallback_forUnitsInRect_RaceUndeadAbilities_callback_forUnitsInRect_RaceUndeadAbilities']
    outcomes = params['summon_outcomes']
    called = re.fullmatch(r'function \w+\(\w+,\w+\)local (\w+)=nil \1=\w+ raiseFromCorpse\(\1,(\d+),(\d+),(\d+),(\d+)\)end', closure.strip())
    shape = (r'(\w+)=randomDyingSapper\(\)if\(\1==nil\)then return end (\w+)=tupleCopy1\(unit_getPos\(\1\)\).*?'
        r'(\w+)=(\w+) if\(GetRandomInt\(0,99\)>=(\d+)\)then (\w+)=GetRandomInt\(0,2\)(\w+)=\6 if\(\7==0\)then \3=(\w+) elseif\(\7==1\)then \3=(\w+) else \3=(\w+) end end '
        r'orderCodeAttack\(createUnit\(unit_getOwner\(\w+\),\3,\2,\{0\.\}\)\)(\w+)=\1 __wurst_safe_RemoveUnit\(\11\)')
    matched = re.search(shape, body)
    avul = raw('Avul')
    if (called is None or matched is None or len(outcomes) != 4 or outcomes[0]['probability_percent'] != 10
            or [o['probability_percent'] for o in outcomes[1:]] != [30, 30, 30]
            or [int(called[i]) for i in range(2, 6)] != [o['unit_id'] for o in outcomes]
            or int(matched[5]) != outcomes[0]['probability_percent']
            or not re.search(r'isDyingCombatSapper\(\w+\)and\(unit_getAbilityLevel\(\w+,%d\)<=0\)\)and\(not unit_isType\(\w+,UNIT_TYPE_UNDEAD\)\)' % avul, picker)
            or params['consumption_mode'] != 'selected-only' or params['requires_wc3_can_raise']
            or 'group_getRandom(' not in functions['randomDyingSapper']):
        raise ValueError('Skull Pile control flow changed: random dying non-undead combat body raised into a tiered skeleton')
    return {'RaiseFromCorpse': {'map_version': detail['map_version'], 'rare_body': outcomes[0]['unit_id'],
        'rare_chance_per_10k': number(outcomes[0]['probability_percent'], 100),
        'common_bodies': [o['unit_id'] for o in outcomes[1:]], 'excluded_ability': avul}}


def _death_pit(params, detail, number, raw):
    functions = detail['functions']
    body = functions['deathPitSpell']
    picker = functions['ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers']
    shape = (r'(\w+)=randomAliveEnemy\(\w+\)if\(\1==nil\)then return end if\(not checkForShield\(\1\)\)then '
        r'\w+=\w+ \w+=\1 (\w+)=ATTACK_TYPE_NORMAL (\w+)=DAMAGE_TYPE_DEATH (\w+)=WEAPON_TYPE_WHOKNOWS '
        r'__wurst_safe_UnitDamageTarget\(\w+,\w+,(\d+)\.,true,false,\2,\3,\4\)')
    matched = re.search(shape, body)
    filtered = re.search(r'isAliveCombatSapper\((\w+)\)and unit_isEnemyOf\(\1,\w+\.p\)\)and\(unit_getAbilityLevel\(\1,%d\)<=0\)\)and'
        r'\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(not\(unit_getTypeId\(\1\)==(\d+)\)\)' % raw('Avul'), picker)
    if (matched is None or filtered is None or int(matched[5]) != int(params['damage'])
            or params['attack_type'] != 'normal' or params['damage_type'] != 'death' or not params['attack_flag'] or params['ranged_flag']
            or 'group_getRandom(' not in functions['randomAliveEnemy'] or 'bj_mapInitialPlayableArea' in body):
        raise ValueError('Death Pit control flow changed: shield-checked random alive enemy dies to Death damage')
    return {'DeathPit': {'damage': int(params['damage']), 'excluded_ability': int(filtered[2]), 'excluded_type': int(filtered[3])}}


def _tornado(params, detail, number, raw):
    """Ancient of Gale: a random alive enemy receives a dummy-cast Summon Tornado at its position."""
    functions = detail['functions']
    body = functions['galeSpell']
    picker = functions['ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers']
    selector = functions['setArtilleryModeAbilityIds']
    normal, alternate = params['normal_ability_id'], params['alternate_mode_ability_id']
    shape = re.fullmatch(r'function galeSpell\((\w+)\)local (\w+)=nil local (\w+)=nil \2=unit_getOwner\(\1\)\3=randomAliveEnemy\(\2\)'
        r'if\(\3==nil\)then return end dummyCastPointFrom\(\2,(\w+),%d,unit_getPos\(\3\),unit_getPos\(\3\),([\d.]+)\)end' % params['order_id'], body.strip())
    chosen = re.fullmatch(r'function setArtilleryModeAbilityIds\(\w+\)if \w+ then .*?%s=(\d+) .*? else .*?%s=(\d+) .*? end end' % ((shape[4],) * 2) if shape else r'$^', selector.strip())
    filtered = re.search(r'isAliveCombatSapper\((\w+)\)and unit_isEnemyOf\(\1,\w+\.p\)\)and\(unit_getAbilityLevel\(\1,%d\)<=0\)\)and'
        r'\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(not\(unit_getTypeId\(\1\)==(\d+)\)\)' % raw('Avul'), picker)
    if (shape is None or chosen is None or filtered is None or 'checkForShield' in body
            or [int(chosen[1]), int(chosen[2])] != [normal, alternate]
            or 'group_getRandom(' not in functions['randomAliveEnemy']):
        raise ValueError('Ancient of Gale control flow changed: unshielded random alive enemy and a dummy-cast Summon Tornado')
    fields, bases, unit_fields = detail['all_fields'], detail['all_bases'], detail['all_unit_fields']
    second = number(1, 30)

    def damage(code):
        aura = fields[code]
        tokens = set(aura['targs1'].split(','))
        if (bases[code] != 'Atdg' or 'enemies' not in tokens or tokens - {'air', 'ground', 'enemies', 'sapper', 'ward', 'mechanical', 'structure', 'invulnerable'}
                or number(aura['dataa1']) != number(aura['datac1']) or number(aura['datab1']) > number(aura['area1'])
                or number(aura['datad1']) != 0 or number(aura['datae1']) != 0):
            raise ValueError('Tornado damage aura must be one flat per-second rate inside its radius (no distinct small tier)')
        movement = sum(bit for token, bit in (('ground', 1), ('air', 2)) if token in tokens) or 3
        return {'ability': raw(code), 'radius': number(aura['area1'], 1024), 'damage_per_second': number(aura['dataa1']),
            'combat_bodies': movement if 'sapper' in tokens else 0, 'mechanical_units': 'mechanical' in tokens,
            'structures': 'structure' in tokens, 'affects_invulnerable': 'invulnerable' in tokens}

    options = []
    for code in (normal, alternate):
        ability = code.to_bytes(4, 'big').decode()
        summon = fields[ability]
        body_code = summon['unitid1']
        unit = unit_fields[body_code]
        abilities = [a for a in unit['abillist'].split(',') if a not in ('Aloc', 'Atwa')]
        auras = [a for a in abilities if bases.get(a) == 'Atdg']
        slows = [a for a in abilities if bases.get(a) == 'Aasl']
        if (detail['all_bases'][ability] != 'ANto' or number(summon['dur1']) != number(summon['herodur1'])
                or number(unit['spd']) != 0 or len(slows) != 1 or len(auras) != len(abilities) - 1
                or 'Aloc' not in unit['abillist'].split(',')):
            raise ValueError('Tornado body must be a stationary locust with damage auras and one slow aura')
        damages = [damage(a) for a in auras]
        units = [d for d in damages if not d['structures']]
        structures = [d for d in damages if d['structures']]
        slow = fields[slows[0]]
        slow_tokens = set(slow['targs1'].split(','))
        if len(units) != 1 or len(structures) > 1 or not {'enemies', 'sapper'} <= slow_tokens or len(slow['buffid1']) != 4:
            raise ValueError('Tornado body needs one unit damage aura, at most one structure aura and a hostile combat slow')
        options.append({'ability': raw(ability), 'body_rawcode': raw(body_code), 'duration_ticks': number(summon['dur1'], 30),
            'pulse_ticks': second, 'unit_damage': units[0], 'structure_damage': structures[0] if structures else None,
            'slow': {'ability': raw(slows[0]), 'buff': raw(slow['buffid1']), 'radius': number(slow['area1'], 1024),
                'movement_percent_delta': number(slow['dataa1'], 100), 'attack_speed_percent_delta': number(slow['datab1'], 100),
                'targets': sum(bit for token, bit in (('ground', 1), ('air', 2)) if token in slow_tokens),
                'affects_invulnerable': 'invulnerable' in slow_tokens}})
    return {'Tornado': {'options': options, 'excluded_ability': int(filtered[2]), 'excluded_type': int(filtered[3])}}


def _wisp_release(params, detail, number, raw):
    """Ancient of Wonders: six pooled forest wisps are given to the owner over successive periods."""
    functions = detail['functions']
    names = detail['building_spell']['source_functions'].split(',')
    cast, periodic, take, pool = (functions[name] for name in names[:2] + ['sA', 'MC'])
    wisp, removed = params['pool_unit_id'], params['removed_ability_id']
    cast_shape = re.search(r'(\w+)=\(2\*__wurst_ensureInt\(lGb\[player_getId\((\w+)\)\]\)\)(\w+)=Ov:create908\(\)\3\.iteration=0 \3\.p=\2 \3\.baseSlot=\1 '
        r'\3\.building=\w+ doPeriodically\(([\d.]+),\3\)end$', cast.strip())
    release = re.search(r'(\w+)=__wurst_modInt\((\w+)\.iteration,2\)(\w+)=\2\.p (\w+)=\(\2\.baseSlot\+\1\)(\w+)=sA\(\4\)unit_removeAbility\(\5,%d\)'
        r'(\w+)=\5 (\w+)=\3 __wurst_safe_SetUnitOwner\(\6,\7,false\)__wurst_safe_UnitAddType\(\5,UNIT_TYPE_SAPPER\)orderCodeAttack\(\5\)'
        r'(\w+)=\5 (\w+)=\8 __wurst_safe_UnitApplyTimedLife\(\9,1112820806,([\d.]+)\)' % removed, periodic)
    iterations = re.search(r'\w+\.iteration=\(\w+\.iteration\+1\)if\(\w+\.iteration>=(\d+)\)then', periodic)
    x_limit = re.search(r'CreateUnit\(\w+,%d,\(_b\[.*?\]\(\(Seo<10\),\(-1\),1\)\*_b\[.*?\]\(([\d.]+),wHb\)\)' % wisp, pool)
    y_inset = re.search(r'\(Seo,10\)<5\),\(-1\),1\)\*\(uHb-([\d.]+)\)\),270\.\)', pool)
    if (cast_shape is None or release is None or iterations is None or x_limit is None or y_inset is None
            or Decimal(cast_shape[4]) != Decimal(params['period_seconds']) or int(iterations[1]) != params['iterations']
            or Decimal(release[10]) != Decimal(params['timed_life_seconds']) or not params['sets_owner_to_building_owner']
            or not params['issues_attack_order'] or params['adds_unit_type'] != 'UNIT_TYPE_SAPPER'
            or params['pool_group_count'] != 4 or params['pool_slots_per_group'] != 5 or 'WHb[deo]=nil return UHb' not in take):
        raise ValueError('Ancient of Wonders control flow changed: periodic release of pooled wisps for the owner')
    return {'WispRelease': {'map_version': detail['map_version'], 'wisp_rawcode': wisp, 'releases': params['iterations'],
        'period_ticks': number(params['period_seconds'], 30), 'life_ticks': number(params['timed_life_seconds'], 30),
        'x_limit': number(x_limit[1], 1024), 'y_inset': number(y_inset[1], 1024)}}


def _banish(params, detail, number, raw):
    """Ancient Guardian: shield-checked random enemy receives a dummy-cast native Banish."""
    functions = detail['functions']
    body = functions['banishGuardianSpell']
    picker = functions['ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers']
    child = params['effect_ability_id']
    code = child.to_bytes(4, 'big').decode()
    banish = detail['all_fields'][code]
    shape = (r'(\w+)=randomAliveEnemy\(\w+\)if\(\(\1==nil\)or checkForShield\(\1\)\)then return end \w+=\w+ \w+=\1 '
        r'dummyCastTargetWithVision\(\w+,%d,%d,\w+,unit_getPos\(\w+\),([\d.]+)\)end$' % (child, params['order_id']))
    matched = re.search(shape, body)
    filtered = re.search(r'isAliveCombatSapper\((\w+)\)and unit_isEnemyOf\(\1,\w+\.p\)\)and\(unit_getAbilityLevel\(\1,%d\)<=0\)\)and'
        r'\(unit_getAbilityLevel\(\1,(\d+)\)<=0\)\)and\(not\(unit_getTypeId\(\1\)==(\d+)\)\)' % raw('Avul'), picker)
    vision = functions['dummyCastTargetWithVision1']
    sight = re.search(r'CreateFogModifierRadius\(\w+,FOG_OF_WAR_VISIBLE,\w+\[1\],\w+\[2\],([\d.]+),', vision)
    sight_time = re.search(r'doAfter\(([\d.]+),\w+\)return \w+ end$', vision)
    tokens = set(banish['targs1'].split(','))
    ethereal = detail['ethereal']
    magic = ethereal['EtherealDamageBonus'].split(',')
    if (matched is None or filtered is None or sight is None or sight_time is None
            or Decimal(matched[2]) != Decimal(params['dummy_lifetime_seconds'])
            or detail['all_bases'][code] != 'AHbn' or tokens != {'air', 'enemies', 'ground'}
            or number(banish['cost1']) != 9999 or len(banish['buffid1']) != 4 or magic[3] != magic[5]
            or 'group_getRandom(' not in functions['randomAliveEnemy']):
        raise ValueError('Ancient Guardian control flow changed: shield-checked random alive enemy and a dummy-cast Banish with vision')
    protected = detail['all_protected'].get(code, {})
    if number(protected.get('mana_cost', 0)) != 0 and code in detail['all_protected']:
        raise ValueError('Banish dummy must be a free cast')
    return {'Banish': {'debuff': {'ability': raw(code), 'buff': raw(banish['buffid1']), 'targets': 3, 'organic_only': False,
        'duration_ticks': number(banish['dur1'], 30), 'hero_duration_ticks': number(banish['herodur1'], 30),
        'movement_percent_delta': -number(banish['dataa1'], 100), 'attack_speed_percent_delta': -number(banish['datab1'], 100),
        'damage_bonus_per_10k': 0, 'ethereal': {'magic_damage_per_10k': number(magic[3], 10000),
            'healing_per_10k': number(ethereal['EtherealHealBonus'], 10000),
            'allied_damage_per_10k': number(ethereal['EtherealDamageBonusAlly'], 10000)}},
        'excluded_ability': int(filtered[2]), 'excluded_type': int(filtered[3]),
        'vision_radius': number(sight[1], 1024), 'vision_ticks': number(sight_time[1], 30)}}


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
    elif kind == 'scripted-raise-random':
        child = _raise_from_corpse(params, detail, number, raw)
    elif kind == 'dummy-target-ability':
        child = _banish(params, detail, number, raw)
    elif kind == 'periodic-preplaced-unit-release':
        child = _wisp_release(params, detail, number, raw)
    elif kind == 'dummy-point-ability':
        child = _tornado(params, detail, number, raw)
    elif kind == 'direct-random-target-damage':
        child = _death_pit(params, detail, number, raw)
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
