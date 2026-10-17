"""Native Starfall channels retain dummy resources, selector and independent cleanup clocks."""
import json
import re
from decimal import Decimal, ROUND_CEILING

def project(row, fields, units, abilities, protected, traces, version):
    p = json.loads(row['parameters_json'])
    src = lambda name: traces[name]['source']
    body = src('starfallSpell')
    selector = src('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers')
    child_ids = [p['normal_ability_id'], p['alternate_mode_ability_id']]
    if (row['mechanic_kind'] != 'dummy-immediate-area-ability'
        or 'twr=randomAliveEnemy(swr)if(twr==nil)then return end' not in body
        or 'vwr=unit_getX(twr)wwr=unit_getY(twr)' not in body
        or f'dummyCastImmediateFrom1(swr,dBb,{p["order_id"]},{{vwr;wwr}},{p["dummy_lifetime_seconds"]}.)' not in body
        or f'doAfter({p["visual_and_fog_cleanup_delay_seconds"]}.,Awr)' not in body
        or f'if(not {p["vision_skipped_when_symbol_true"]})then' not in body
        or f'FOG_OF_WAR_VISIBLE,vwr,wwr,{p["temporary_vision_radius"]}.,true,false' not in body
        or 'jwr=uIb' in body
        or 'dSr=uIb' not in src('randomAliveEnemy')
        or 'bSr=Uz:create1066()' not in src('randomAliveEnemy')
        or 'isAliveCombatSapper(zOn)and unit_isEnemyOf(zOn,yOn.p)' not in selector
        or 'UNIT_TYPE_GROUND' in selector
        or re.search(r'if Z6o then[^\n]*?dBb=(\d+).*?else[^\n]*?dBb=(\d+)',src('setArtilleryModeAbilityIds')).groups() != tuple(map(str,child_ids))
        or 'unit_getOwner(rwr)' not in body
        or 'DummyRecycler_get(STr,{0.})' not in src('dummyCastImmediateFrom1')
        or 'SetUnitOwner(ZTr,aUr,false)' not in src('dummyCastImmediateFrom1')
        or 'unit_issueImmediateOrderById(UTr,RTr)' not in src('dummyCastImmediateFrom1')
        or 'doAfter(TTr,YTr)' not in src('dummyCastImmediateFrom1')
        or 'recycleSpellDummy(IOn.dummy,IOn.abilityId)' not in src('CallbackSingle_doAfter_SpellHelpers_call_doAfter_SpellHelpers2')
        or 'unit_removeAbility(tSr,uSr)DummyRecycler_recycle(tSr)' not in src('recycleSpellDummy')
        or 'DestroyFogModifier(dwn.fogCapture)' not in src('CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities')
        or 'DestroyEffect(ewn)' not in src('CallbackSingle_doAfter_RaceNelfAbilities_call_doAfter_RaceNelfAbilities')):
        raise ValueError('Starfall selector, dummy origin/owner/resources or independent cleanup flow changed')
    raw = lambda c: int.from_bytes(c.encode(), 'big')
    tick = lambda value: int((Decimal(str(value))*30).to_integral_value(rounding=ROUND_CEILING))
    native = {r['rawcode']:r for r in abilities if r['level']=='1'}
    resource = lambda code,key, fallback: next((r['runtime_value'] for r in protected if r['rawcode']==code and r['field']==key), fallback)
    dummy_code = int(re.search(r'Tdb=__wurst_ensureInt\((\d+)\)',src('QG'))[1]).to_bytes(4,'big').decode()
    if ('return createDummy(fwq,gwq)' not in src('DummyRecycler_get')
        or 'Pvq=createUnit(uAb,Tdb,Nvq,Ovq)' not in src('createDummy')
        or set(units[dummy_code]['abilities'].split(',')) != {'Avul','Aloc','AM03'}):
        raise ValueError('Starfall recycler dummy identity or permanent protection changed')
    requested_mana = int(Decimal(re.search(r'SetUnitState\(cUr,dUr,([\d.]+)\)',src('dummyCastImmediateFrom1'))[1]))
    dummy_mana = min(requested_mana,int(Decimal(units[dummy_code]['mana_max'])))
    effects=[]
    for index, id in enumerate(child_ids):
        code=id.to_bytes(4,'big').decode();a=native[code];data=json.loads(a['data_fields_json'])
        expected={'air','ground','enemy','neutral','structure'} if index==0 else {'air','enemies','ground','neutral'}
        if (a['base_rawcode']!='AEsf' or set(a['targets'].split(','))!=expected
            or fields[code]['adur:1']!=fields[code]['ahdu:1'] or len(a['buffs'].split(','))!=1
            or 'Pierces spell immunity.' not in a['ubertip']):
            raise ValueError('Starfall native variant, duration or immunity contract changed')
        cost=int(resource(code,'mana_cost',a['mana_cost']))
        effects.append({'ability':id,'buff':raw(a['buffs']),'radius':int(Decimal(a['area'])*1024),
            'damage':int(data['DataA']),'interval_ticks':tick(data['DataB']),
            'duration_ticks':tick(fields[code]['adur:1']),
            'targets':7 if 'structure' in expected else 3,
            'building_damage_per_10k':int(Decimal(str(data['DataC']))*10000),
            'mana_cost':cost,'cooldown_ticks':tick(resource(code,'cooldown',a['cooldown']))})
        if cost>dummy_mana or cost<0 or effects[-1]['interval_ticks']<=0 or effects[-1]['damage']<0 or not 0<=effects[-1]['building_damage_per_10k']<=10000:raise ValueError('Starfall native dummy order cannot succeed')
    parent=row['ability_rawcode'];u=units[row['building_rawcode']]
    if set(fields[parent]['atar:1'].split(','))!={'enemies','ground','invulnerable','mechanical','neutral','structure'}:raise ValueError('Starfall primary mask changed')
    exclusions=list(map(int,re.findall(r'unit_getAbilityLevel\(zOn,(\d+)\)<=0',selector)))
    if len(exclusions)!=2:raise ValueError('Starfall selector exclusions changed')
    amount=lambda key:0 if u[key].strip() in {'','-','_'} else int(Decimal(u[key]))
    profile={'map_version':version,'trigger_targets':5,'trigger_invulnerable':True,'trigger_spell_immune':False,'trigger_mechanical_only':False,
        'options':effects,'selector_exclusions':exclusions,
        'selector_excluded_unit':int(re.search(r'unit_getTypeId\(zOn\)==(\d+)',selector)[1]),
        'dummy_rawcode':raw(dummy_code),'dummy_mana':dummy_mana,'dummy_lifetime_ticks':tick(p['dummy_lifetime_seconds']),
        'visual_duration_ticks':tick(p['visual_and_fog_cleanup_delay_seconds']),
        'vision_radius':int(Decimal(p['temporary_vision_radius'])*1024)}
    return {'mana':{'maximum':amount('mana_max'),'starting':amount('mana_start'),'regen_per_tick_per_10k':(1<<31)|int(Decimal(u['mana_regen'])*10000)},
        'ability':{'id':raw(parent),'mana_cost':int(resource(parent,'mana_cost',native[parent]['mana_cost'])),'cooldown_ticks':tick(resource(parent,'cooldown',native[parent]['cooldown'])),'range':int(Decimal(fields[parent]['aran:1'])*1024),'target_policy':'NativeBuildingSpellTrigger','effect':{'Starfall':profile}}}
