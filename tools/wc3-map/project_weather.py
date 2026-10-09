"""Native point weather, selected by retained Elemental callbacks.

Blizzard/Rain of Fire Cast is the wave interval. The native falling-shard
impact delay is a fixed 900 ms interpretation of published native experiments,
kept here separately from authored map tuning.
"""
import json


def wave(code, fields, bases, scaled):
    f, base = fields[code], bases[code]
    if base not in {'AHbz', 'ACrg', 'ACmo'}:
        raise ValueError('weather requires Blizzard, Rain of Fire or Monsoon native base')
    tokens = set(f['targs1'].split(','))
    if 'enemies' not in tokens or tokens - {'enemies', 'ground', 'air', 'structure'}:
        raise ValueError('weather requires independently hostile native recipients')
    monsoon = base == 'ACmo'
    interval = scaled(f['datab1'] if monsoon else f['cast1'], 30)
    if interval <= 0:
        raise ValueError('weather pulse interval must be positive')
    return {'ability':int.from_bytes(code.encode(), 'big'), 'radius':scaled(f['area1'], 1024),
        'damage':scaled(f['dataa1'] if monsoon else f['datab1']),
        'maximum_damage':2147483647 if monsoon else scaled(f['dataf1']),
        'building_damage_per_10k':scaled(f['datac1'] if monsoon else f['datad1'],10000),
        'waves':scaled(f['dur1'],30)//interval if monsoon else scaled(f['dataa1']),
        'shards':0 if monsoon else scaled(f['datac1']),
        'interval_ticks':interval, 'first_impact_ticks':interval if monsoon else 27,
        'damage_per_second':0 if monsoon else scaled(f['datae1']),
        'duration_ticks':scaled(f['dur1'],30), 'hero_duration_ticks':scaled(f['herodur1'],30),
        'buff_rawcode':int.from_bytes(f['buffid1'].split(',')[0].encode(),'big'),
        'wave_buff_rawcode':int.from_bytes(f['buffid1'].split(',')[-1].encode(),'big'),
        'targets':(1 if 'ground' in tokens else 0)|(2 if 'air' in tokens else 0)|(4 if 'structure' in tokens else 0)}


def project(recipe, fields, unit, protected, mechanics, scaled):
    if recipe['kind'] == 'native-weather-wave':
        return dict(recipe, profile=wave(recipe['source_key'],mechanics['all_fields'],mechanics['all_bases'],scaled))
    row = mechanics['building_spell']; params = json.loads(row['parameters_json'])
    functions = mechanics['functions']
    ground = row['mechanic_kind'] == 'uniform-random-point-ability'
    if ground:
        body = functions['lightningSpell']
        selector = functions['ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1']
        if not all(s in body for s in ['randomAliveEnemyGround','GetRandomInt(0,1)','aBb,852089','ZAb,852591']):
            raise ValueError('weather requires uniform independent point choice')
        if not all(s in selector for s in ['isAliveCombatSapper','UNIT_TYPE_GROUND','1098282348','1093679176','unit_isEnemyOf']):
            raise ValueError('ground weather selector marker exclusions changed')
        codes = [[o['ability_id'].to_bytes(4,'big').decode() for o in params[k]] for k in ['normal_options','alternate_mode_options']]
        mode = functions['setArtilleryModeAbilityIds']
        branches=mode.split(' else ')
        if len(branches)!=2 or not all(f'{symbol}={int.from_bytes(code.encode(),"big")}' in branches[index]
            for index, row_codes in enumerate(codes) for symbol,code in zip(['aBb','ZAb'],row_codes)):
            raise ValueError('weather mode objects must retain ordered source switch')
    else:
        body=functions['frostSpell'];selector=functions['ForGroupCallback_forUnitsInRect_RaceElementalAbilities_callback_forUnitsInRect_RaceElementalAbilities']
        if not all(s in body for s in ['dummyCastPointFrom',str(params['effect_ability_id']),str(params['order_id'])]):
            raise ValueError('meteor point dummy identity changed')
        if not all(s in selector for s in ['isAliveCombatSapper','UNIT_TYPE_FLYING','1098282348','unit_isEnemyOf']) or '1093679176' in selector:
            raise ValueError('meteor independently selects flying combat sappers')
        code=params['effect_ability_id'].to_bytes(4,'big').decode();codes=[[code,code],[code,code]]
    if mechanics['native_base']!='ACpa' or any(scaled(fields[k]) != 0 for k in ['dataa1','datab1','datac1','datad1']):
        raise ValueError('weather parent Parasite must be inert')
    tokens=set(fields['targs1'].split(','));rate=scaled(unit['mana_regen'],10000)
    profile={'map_version':mechanics['map_version'],'trigger_targets':(1 if 'ground' in tokens else 0)|(2 if 'air' in tokens else 0)|(4 if 'structure' in tokens else 0),
        'trigger_invulnerable':'invulnerable' in tokens,'trigger_spell_immune':'magicimmune' in tokens,
        'select_ground':ground,'selector_excluded_ability':1093679176 if ground else 0,
        'option_count':2 if ground else 1,'dummy_lifetime_ticks':scaled(params['dummy_lifetime_seconds'],30),
        'options':[[wave(c,mechanics['all_fields'],mechanics['all_bases'],scaled) for c in cs] for cs in codes]}
    return {'kind':'scripted-automatic','source_kind':recipe['source_kind'],'source_key':recipe['source_key'],'unit_rawcode':recipe['unit_rawcode'],
        'spellcasting':{'mana':{'maximum':scaled(unit['mana_max']),'starting':0,'regen_per_tick_per_10k':(1<<31)|rate},
            'ability':{'id':int.from_bytes(recipe['source_key'].encode(),'big'),'mana_cost':scaled(protected['mana_cost']),'cooldown_ticks':scaled(protected['cooldown'],30),'range':scaled(fields['rng1'],1024),
                'target_policy':'NativeBuildingSpellTrigger','effect':{'BuildingWeather':profile}}},
        'mana_regen_per_second_per_10k':rate,'effect_ability_rawcode':None,'dependencies':sorted({c for cs in codes for c in cs}),
        'provenance':{'git_tree':mechanics['git_tree'],'source':'resolved/building-spell-mechanics.tsv','functions':row['source_functions'],
            'native_timing_insight':'https://github.com/Cokemonkey11/wc3-ability-doc/blob/main/sources/screwthetrees-wc3-editor-ability-insight-document/2023-11-14.md'}}
