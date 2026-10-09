"""Project Master of Elements' native/script composition from retained identities."""
from decimal import Decimal, ROUND_CEILING
import re
from build_native_tuning import scaled


def project(recipe, fields, unit, protected, mechanics):
    trace = mechanics['master_trace']
    lightning, frost = trace['lightning'], trace['frost']
    functions = mechanics['master_functions']
    guards = {
        'processMasterLightningBolts': ['findMasterLightningHit(JOq,i_[IOq],LOq)', 'i_[IOq]=MOq',
            'j_[SOq]=1', 'j_[f_]=(-1)', 'i_[f_]=nil', 'g_[f_]=false',
            'elseif((__wurst_ensureInt(j_[IOq])>0)and(not __wurst_ensureBool(g_[IOq])))',
            'm_[NOq]=m_[f_]', 'IOq=(IOq+1)', 'ATTACK_TYPE_NORMAL,DAMAGE_TYPE_LIGHTNING'],
        'onLightningBolt': ['g_[(f_-1)]=true', 'LPq=(f_==0)', 'NPq=((-(50.*I2R(__wurst_intDiv(FPq,2))))+((50.*ModuloInteger((FPq+1),2))/2.))'],
        'spawnSnowMissileFanStep': ['widget_getLife(sPq)<0.405', 'unit_getX(sPq)-(26.*Cos', 'doAfter(.04,BPq)'],
        'processSnowMissiles': ['ATTACK_TYPE_NORMAL,DAMAGE_TYPE_COLD', 'if unit_isAlive(hPq)then',
            'dummyCastTargetWithVision(unit_getOwner(s_[dPq]),1093678131,852226,hPq,{ePq;fPq},1.)',
            '__wurst_safe_SetUnitAbilityLevel(iPq,1093678131,__wurst_ensureInt(p_[dPq]))'],
    }
    if any(snippet not in functions[name] for name, snippets in guards.items() for snippet in snippets):
        raise ValueError('Master projectile timer, asymmetric branch, last-hit or secondary cast control flow changed')
    if mechanics['native_base'] != 'ACpa' or trace['branch_roll'] != {'frost_otherwise': True, 'lightning_if_less_than': 50, 'max': 99, 'min': 0}:
        raise ValueError('Master requires audited Parasite parent and branch comparison')
    if any(scaled(fields[k]) != 0 for k in ['dataa1','datab1','datac1','datad1']) or scaled(fields['datae1']) != 1:
        raise ValueError('Master native Parasite payload or death summon count changed')
    if set(fields['targs1'].split(',')) != {'air','enemies','ground','neutral','nonhero','vulnerable'}:
        raise ValueError('Master native Parasite mask changed')
    nova = mechanics['master_nova']
    if scaled(nova['levels']) != 1 or scaled(mechanics['master_nova_protected']['mana_cost']) != 0 or scaled(mechanics['master_dummy']['castpt']) != 0:
        raise ValueError('Master Nova is a free, instantaneous one-level dummy cast; requested levels cannot create invented damage')
    def formula(value, pattern):
        m = re.fullmatch(pattern, value)
        if m is None: raise ValueError('unrecognized Master source scaling formula')
        return tuple(map(int, m.groups()))
    initial, divisor = formula(lightning['projectile_count_formula'], r'(\d+) \+ floor\(lightning_building_count / (\d+)\)')
    splits, split_divisor = formula(lightning['split_budget_formula'], r'(\d+) \+ floor\(wind_building_count / (\d+)\)')
    frost_count, frost_factor = formula(frost['projectile_count_formula'], r'(\d+) \+ (\d+) \* water_building_count')
    damage, cap = formula(lightning['damage_formula'], r'(\d+) \* min\(1 \+ fire_building_count, (\d+)\)')
    if formula(frost['damage_formula'], r'(\d+) \* min\(1 \+ earth_building_count, (\d+)\)') != (damage, cap):
        raise ValueError('Master branches no longer share damage scaling')
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    identity = lambda value: int.from_bytes(value.encode(), 'big')
    def mover(p):
        obj = mechanics['master_units'][p['projectile_unit_rawcode']]
        if obj['abillist'] != 'Avul,Aloc' or scaled(obj['weapson']) != 0:
            raise ValueError('Master mover must remain a harmless invulnerable locust with no ordinary weapon')
        return {'rawcode': identity(p['projectile_unit_rawcode']), 'step': scaled(p['movement_per_tick'],1024),
            'radius': scaled(p['hit_radius'],1024), 'budget': scaled(p.get('initial_segment_budget', p.get('initial_range_budget')),1024),
            'decrement': scaled(p.get('segment_budget_decrement_per_tick',p.get('range_budget_decrement_per_tick')),1024),
            'interval_millis': scaled(p['tick_seconds'],1000), 'height': scaled(obj['moveheight'])}
    buildings = []
    for element in ['fire','earth','lightning','water','wind']:
        ids = [identity(code) for code in trace['element_building_buckets'][element]['building_rawcodes']]
        buildings.append(ids + [0]*(3-len(ids)))
    profile = {'ability':identity(recipe['source_key']), 'buildings':buildings, 'lightning':mover(lightning), 'frost':mover(frost),
        'damage':damage, 'damage_count_cap':cap, 'branch_roll_size': trace['branch_roll']['max']-trace['branch_roll']['min']+1,
        'lightning_roll_below':trace['branch_roll']['lightning_if_less_than'], 'lightning_initial_count':initial,
        'lightning_count_divisor':divisor, 'split_initial_count':splits, 'split_count_divisor':split_divisor,
        'lane_spacing':scaled(lightning['lane_spacing'],1024), 'forward_offset':scaled(lightning['initial_origin_forward_offset'],1024),
        'split_angle_millidegrees':scaled(lightning['split_angle_degrees'],1000), 'stub_budget':scaled(lightning['split_stub_budget'],1024),
        'frost_initial_count':frost_count, 'frost_count_factor':frost_factor,
        'fan_spread_millidegrees':scaled(frost['fan_max_relative_angle_degrees'],1000),
        'fan_interval_millis':scaled(frost['spawn_interval_seconds'],1000),
        'nova':{'ability':identity(frost['frost_nova_ability_rawcode']), 'radius':scaled(nova['area1'],1024),
            'primary_damage':scaled(nova['datab1']), 'area_damage':scaled(nova['dataa1']),
            'duration_ticks':tick(nova['dur1']), 'hero_duration_ticks':tick(nova['herodur1']),
            # Native Frost Nova slow semantics shared with the audited area-spell projection.
            'movement_percent_delta':-50,'attack_speed_percent_delta':-25},
        'nova_reveal_radius':scaled(re.search(r'FOG_OF_WAR_VISIBLE,rTr\[1\],rTr\[2\],([0-9.]+),',functions['dummyCastTargetWithVision1'])[1],1024),
        'nova_reveal_ticks':tick(re.search(r'doAfter\(([0-9.]+),uTr\)',functions['dummyCastTargetWithVision1'])[1]),
        'parasite_speed':int(Decimal(fields['missilespeed'])*1024/30),
        'parasite_duration_ticks':tick(fields['dur1']), 'parasite_child_rawcode':identity(fields['unitid1']),
        'parasite_child_duration_ticks':tick(fields['cast1'])}
    scalar = lambda value: 0 if value in ['-','_',' - '] else scaled(value)
    rate=scaled(unit['mana_regen'],10_000) if unit['mana_regen'] not in ['-','_',' - '] else 0
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],
        'unit_rawcode':recipe['unit_rawcode'], 'spellcasting':{'mana':{'maximum':scalar(unit['mana_max']),
            'starting':scalar(unit['mana_start']),'regen_per_tick_per_10k':(1<<31)|rate},
        'ability':{'id':identity(recipe['source_key']),'mana_cost':scaled(protected['mana_cost']),
            'cooldown_ticks':tick(protected['cooldown']),'range':scaled(fields['rng1'],1024),
            'target_policy':'RandomGroundEnemyUnit','effect':{'MasterProjectiles':mechanics['map_version']}}},
        'master_profile':profile,'mana_regen_per_second_per_10k':rate,'effect_ability_rawcode':frost['frost_nova_ability_rawcode'],
        'provenance':{'git_tree':mechanics['git_tree'],'semantics':'resolved/unit-spell-semantics.tsv',
            'script_functions':sorted(functions),'fields':'resolved/object-fields.tsv','protected':'resolved/protected-ability-fields.tsv',
            'nova_requested_level_formula':frost['frost_nova_level_formula'],'nova_actual_levels':scaled(nova['levels']),
            'script_visuals':[{'owner_rawcode':lightning['projectile_unit_rawcode'],'source_unit_rawcode':None,'source_entity_rawcode':recipe['unit_rawcode'],'role':'target',
                'model_path':__import__('json').loads(re.search(r'addEffect\(("(?:[^"\\]|\\.)*")',functions['processMasterLightningBolts'])[1])}]}}
