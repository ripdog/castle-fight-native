"""Retained team charges trigger on attack attempts, before native damage/impact."""
import json
from decimal import Decimal, ROUND_CEILING


def project(row, fields, units, abilities, protected, traces, version):
    params = json.loads(row['parameters_json'])
    raw = lambda code: int.from_bytes(code.encode(), 'big')
    source = lambda name: traces[name]['source']
    if (row['mechanic_kind'] != 'team-charge-next-attack-proc'
        or params['trigger_event'] != 'EVENT_PLAYER_UNIT_ATTACKED'
        or 'EVENT_PLAYER_UNIT_ATTACKED' not in source('OK')
        or 'isCombatSapper(Wwr)' not in source('PK')
        or f'unit_getAbilityLevel(Wwr,{params["excluded_ability_id"]})<=0' not in source('PK')
        or f'dummyCastImmediateFrom1(Xwr,{params["effect_ability_id"]},{params["order_id"]},unit_getPos(Wwr),{params["dummy_lifetime_seconds"]}.)' not in source('PK')
        or 'R0[Uvr]=(__wurst_ensureInt(R0[Uvr])+1)' not in source('addSilverGladeChargeForTeam')
        or 'R0[Vvr]=(__wurst_ensureInt(R0[Vvr])-1)' not in source('NK')
        or 'R0[Wvr]=0' not in source('resetSilverGladeCounterForTeam')
        or 'resetSilverGladeCounterForTeam(0)resetSilverGladeCounterForTeam(1)' not in source('Action_watch_RaceNelfAbilities_run_watch_RaceNelfAbilities')
        or not params['round_end_signal_resets_both_team_counters']):
        raise ValueError('Silver Glade requires source attack-attempt, team charge and native target-origin proc')
    if set(fields[row['ability_rawcode']]['atar:1'].split(',')) != {'enemies','ground','invulnerable','mechanical','neutral','structure'}:
        raise ValueError('Silver Glade native parent trigger mask changed')
    child = params['effect_ability_id'].to_bytes(4,'big').decode()
    a = next(a for a in abilities if a['rawcode']==child and a['level']=='1')
    if a['base_rawcode'] != 'AIdb' or set(a['targets'].split(',')) != {'air','friend','ground','self','vulnerable'}:
        raise ValueError('Silver Glade requires native area armor bonus mask')
    native = json.loads(a['data_fields_json'])
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    unit=units[row['building_rawcode']]
    cost=lambda name: next(p['runtime_value'] for p in protected if p['rawcode']==row['ability_rawcode'] and p['field']==name)
    amount=lambda key: 0 if unit[key].strip() in {'','-','_'} else int(Decimal(unit[key]))
    rate=int(Decimal(unit['mana_regen'])*10000)
    profile={'map_version':version,'trigger_targets':5,'trigger_invulnerable':True,'trigger_spell_immune':False,'trigger_mechanical_only':False,'effect':raw(child),'excluded':params['excluded_ability_id'],
        'radius':int(Decimal(a['area'])*1024),'health':int(native['DataB']),'mana':int(native['DataC']),
        'armor_per_100':int(Decimal(str(native['DataA']))*100),'buff':{'rawcode':raw(a['buffs']),'positive':True,'stealable':True,'organic_only':False},
        'duration_ticks':tick(fields[child]['adur:1']),'hero_duration_ticks':tick(fields[child]['ahdu:1'])}
    return {'mana':{'maximum':amount('mana_max'),'starting':amount('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},
        'ability':{'id':raw(row['ability_rawcode']),'mana_cost':int(cost('mana_cost')),'cooldown_ticks':tick(cost('cooldown')),
        'range':int(Decimal(fields[row['ability_rawcode']]['aran:1'])*1024),'target_policy':'NativeBuildingSpellTrigger','effect':{'SilverGlade':profile}}}
