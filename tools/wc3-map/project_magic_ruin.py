"""Project the literal ten-way Chaos callback, including independently committed native children."""
import json
import re
from decimal import Decimal, ROUND_CEILING
from project_naga_unit import project_parasite


def project(row, fields, units, abilities, protected, traces, reference, version):
    raw = lambda c: int.from_bytes(c.encode(), 'big')
    code = lambda v: v.to_bytes(4, 'big').decode()
    number = lambda v, scale=1: int((Decimal(str(v)) * scale).to_integral_value(rounding=ROUND_CEILING))
    tick = lambda v: number(v, 30)
    source = lambda n: traces[n]['source']
    params = json.loads(row['parameters_json'])
    branches = params['branches']
    call = source('magicRuinSpell')
    base = lambda c: next(a['base_rawcode'] for a in abilities if a['rawcode'] == c)
    cost = lambda c, f: next((p['runtime_value'] for p in protected if p['rawcode'] == c and p['field'] == f), fields[c][{'mana_cost': 'amcs:1', 'cooldown': 'acdn:1'}[f]])
    if (row['mechanic_kind'] != 'uniform-random-effect-table'
            or [b['roll'] for b in branches] != list(range(10))
            or not re.search(r'GetRandomInt\(0,9\).*randomAliveEnemy\(\w+\).*if\(\w+==nil\).*checkForShield', call)):
        raise ValueError('Magic Ruin must roll before independent selection, nil handling and shield interception')
    for b in branches[4:7]:
        expected = f'chaosControlDummy(Bjr,{"Fjr" if b["roll"] == 6 else "Cjr"},{b["ability_id"]},{b["order_id"]},{b["dummy_lifetime_seconds"]}.,{str(b["reengage"]).lower()})'
        if expected not in call:
            raise ValueError('Magic Ruin child identity/order/lifetime/reengagement changed')
    required = [
        '__wurst_safe_ExplodeUnitBJ(Cjr)',
        '__wurst_safe_UnitDamageTarget(Jjr,Kjr,50000.,true,false,Ljr,Mjr,Njr)',
        '__wurst_safe_UnitDamageTarget(Ojr,Pjr,500.,true,false,Qjr,Rjr,Sjr)',
        'Ljr=ATTACK_TYPE_NORMAL Mjr=DAMAGE_TYPE_DEATH',
        'Qjr=ATTACK_TYPE_NORMAL Rjr=DAMAGE_TYPE_DEATH',
        '__wurst_safe_SetWidgetLife(Cjr,unit_getMaxHP(Cjr))',
        'if(not unit_isType(Fjr,UNIT_TYPE_GROUND))then Fjr=randomAliveEnemyGround(Bjr)end',
        f'orderCodeAttack(createUnit(Bjr,{branches[7]["unit_id"]},Gjr,unit_getFacingAngle(Cjr)))',
        '__wurst_safe_ShowUnit(Wjr,false)',
        'Gjr=tupleCopy1(unit_getPos(Cjr))',
        '__wurst_safe_RemoveUnit(Yjr)',
    ]
    if any(token not in call for token in required):
        raise ValueError('Magic Ruin outcome control flow changed')
    grants = list(map(int, re.findall(r'addProtectedAbility\(\w+,(\d+)\)', call)))
    if grants != branches[3]['ability_ids'] or [base(code(a)) for a in grants] != ['AIl2', 'AIth']:
        raise ValueError('Magic Ruin permanent native grant order/class changed')
    selector = source('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers')
    exclusions = list(map(int, re.findall(r'unit_getAbilityLevel\(\w+,(\d+)\)<=0', selector)))
    excluded = re.findall(r'unit_getTypeId\(\w+\)==(\d+)', selector)
    if len(exclusions) != 2 or len(excluded) != 1 or 'isAliveCombatSapper' not in selector:
        raise ValueError('Magic Ruin enemy selector exclusions changed')
    helper = source('chaosControlDummy')
    if ('dummyCastTargetWithVision(rjr,tjr,ujr,sjr,unit_getPos(sjr),vjr)' not in helper
            or 'if wjr then' not in helper or 'doAfter(1.,yjr)' not in helper):
        raise ValueError('Magic Ruin requires target-point dummies and unconditional delayed reengagement')
    control = params['control_reengage']
    callback = source('CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime')
    if (f'=={control["defender_unit_id"]}' not in callback
            or f'doAfter({control["defender_defend_restore_delay_seconds_after_initial_reengage"]},' not in callback):
        raise ValueError('Magic Ruin Defender callback changed')
    defend_callback = source('CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime')
    if (f'unit_issueImmediateOrderById(sBn.c,{control["defender_defend_order_id"]})' not in defend_callback
            or 'doAfter(.5,tBn)' not in defend_callback
            or 'orderCodeAttack' not in source('CallbackSingle_doAfter_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_doAfter_ReengageRuntime')):
        raise ValueError('Magic Ruin Defender restore/resume order changed')
    parent = fields[row['ability_rawcode']]
    if base(row['ability_rawcode']) != 'ACpa' or set(parent['atar:1'].split(',')) != {'enemies', 'ground', 'invulnerable', 'mechanical', 'neutral', 'structure'}:
        raise ValueError('Magic Ruin native parent mask/class changed')
    native_parent = project_parasite({'buffid1': parent['native_base_buff'].split(',')[0],
        'missilehoming': parent['amho:0'], 'missilespeed': parent['amsp:0'], 'targs1': parent['atar:1'],
        'dur1': parent['adur:1'], 'herodur1': parent['ahdu:1'], 'unitid1': parent['ipmu:1'],
        'cast1': parent['Npa6:1'], 'datae1': parent['Npa5:1'],
        **{f'data{letter}1': parent[f'Poi{i}:1'] for i, letter in enumerate('abcd', 1)}}, version)
    hex_code, banish_code, bolt_code = [code(b['ability_id']) for b in branches[4:7]]
    hex_fields, banish, bolt = [fields[c] for c in [hex_code, banish_code, bolt_code]]
    if base(hex_code) != 'AOhx' or set(hex_fields['atar:1'].split(',')) != {'air', 'enemies', 'ground', 'neutral'}:
        raise ValueError('Magic Ruin Hex native class/mask changed')
    if (base(banish_code) != 'AHbn' or set(banish['atar:1'].split(',')) != {'air', 'enemies', 'ground'}
            or base(bolt_code) != 'AHtb' or set(bolt['atar:1'].split(',')) != {'debris', 'enemies', 'ground', 'neutral'}
            or number(bolt['amho:0']) != 1):
        raise ValueError('Magic Ruin Banish/Storm Bolt native class or mask changed')
    if any(number(cost(c, k)) != 0 for c in [hex_code, banish_code, bolt_code] for k in ['mana_cost', 'cooldown']):
        raise ValueError('Magic Ruin native dummy children must remain protected free casts')
    ethereal = reference['ethereal']
    magic = ethereal['EtherealDamageBonus'].split(',')
    if magic[3] != magic[5]:
        raise ValueError('native Magic/spell ethereal factors diverged')
    dummy = int(re.search(r'Tdb=__wurst_ensureInt\((\d+)\)', source('QG'))[1]).to_bytes(4, 'big').decode()
    if number(fields[dummy]['ucpt:0']) != 0:
        raise ValueError('Magic Ruin native dummy has a nonzero cast point')
    def form(c):
        u = units[c]
        if u['armor_type'] != 'medium':
            raise ValueError('unsupported native Hex form armor')
        return {'rawcode': raw(c), 'speed_per_tick': number(u['move_speed'], 1024) // 30,
            'collision_radius': number(u['collision'], 1024),
            'armor': {'armor_points': number(u['armor']), 'armor_type': 'Medium'}}
    p = {'map_version': version, 'ability': raw(row['ability_rawcode']), 'parent': native_parent, 'trigger_targets': 5, 'trigger_mechanical_only': True,
        'selector_exclusions': exclusions, 'selector_excluded_rawcode': int(excluded[0]),
        'outcome_count': len(branches), 'death_damage': [b['damage'] for b in branches[1:3]],
        'grants': [{'ability': grants[0], 'effect': {'MaximumLife': number(fields[code(grants[0])]['Ilif:1'])}},
                   {'ability': grants[1], 'effect': {'AttackDamage': number(fields[code(grants[1])]['Iatt:1'])}}],
        'mutation_rawcode': branches[7]['unit_id'], 'dummy_rawcode': raw(dummy),
        'dummy_flying': fields[dummy]['umvt:0'] == 'fly',
        'control_dummy_ticks': [tick(b['dummy_lifetime_seconds']) for b in branches[4:7]],
        'hex': {'map_version': version,
            'duration_ticks': tick(hex_fields['adur:1']), 'hero_duration_ticks': tick(hex_fields['ahdu:1']),
            'initial_reengage_ticks': tick(control['initial_reengage_delay_seconds']),
            'defender_restore_ticks': tick(control['defender_defend_restore_delay_seconds_after_initial_reengage']),
            'defender_resume_ticks': tick(control['defender_attack_resume_delay_seconds_after_defend']),
            'defender_rawcode': control['defender_unit_id'],
            'ground': form(hex_fields['Ply2:1']), 'air': form(hex_fields['Ply3:1'])},
        'hex_ability': raw(hex_code), 'hex_range': number(hex_fields['aran:1'], 1024),
        'banish': {'ability': raw(banish_code), 'buff': raw(banish['abuf:1']), 'targets': 3, 'organic_only': False,
            'duration_ticks': tick(banish['adur:1']), 'hero_duration_ticks': tick(banish['ahdu:1']),
            'movement_percent_delta': -number(banish['Hbn1:1'], 100), 'attack_speed_percent_delta': -number(banish['Hbn2:1'], 100),
            'ethereal': {'magic_damage_per_10k': number(magic[3], 10000), 'healing_per_10k': number(ethereal['EtherealHealBonus'], 10000),
                'allied_damage_per_10k': number(ethereal['EtherealDamageBonusAlly'], 10000)}},
        'banish_range': number(banish['aran:1'], 1024),
        'bolt': {'ability': raw(bolt_code), 'damage': number(bolt['Htb1:1']), 'stun_ticks': tick(bolt['adur:1']),
            'hero_stun_ticks': tick(bolt['ahdu:1']), 'damage_per_second': 0, 'duration_ticks': 0,
            'speed_per_tick': number(bolt['amsp:0'], 1024) // 30, 'cleanse': False, 'targets': 1},
        'bolt_range': number(bolt['aran:1'], 1024)}
    vision = source('dummyCastTargetWithVision1')
    p['vision_radius'] = number(re.search(r'FOG_OF_WAR_VISIBLE,\w+\[1\],\w+\[2\],([\d.]+),', vision)[1], 1024)
    p['vision_ticks'] = tick(re.search(r'doAfter\(([\d.]+),', vision)[1])
    u = units[row['building_rawcode']]
    amount = lambda k: 0 if u[k].strip() in {'', '-', '_'} else number(u[k])
    return {'mana': {'maximum': amount('mana_max'), 'starting': amount('mana_start'),
        'regen_per_tick_per_10k': (1 << 31) | number(u['mana_regen'], 10000)},
        'ability': {'id': raw(row['ability_rawcode']), 'mana_cost': number(cost(row['ability_rawcode'], 'mana_cost')),
            'cooldown_ticks': tick(cost(row['ability_rawcode'], 'cooldown')), 'range': number(parent['aran:1'], 1024),
            'target_policy': 'NativeBuildingSpellTrigger', 'effect': {'BuildingMagicRuin': p}}}
