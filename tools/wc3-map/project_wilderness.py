"""Permanent native grants reconcile the exact selection and conditional source branch."""
import json
import re
from decimal import Decimal, ROUND_CEILING

def project(row, fields, units, abilities, protected, traces, version):
    p=json.loads(row['parameters_json']); raw=lambda code:int.from_bytes(code.encode(),'big')
    source=lambda name:traces[name]['source']
    conditional=p['conditional_spell_resistance_ability_id']; always=p['always_ability_ids']; exclusions=p['spell_resistance_exclusion_ability_ids']
    body=source('applyWildernessObeliskBuff')
    if (row['mechanic_kind']!='persistent-buff-bundle' or list(map(int,re.findall(r'addProtectedAbility\(\w+,(\d+)\)',body))) != [conditional,*always]
        or list(map(int,re.findall(r'unit_getAbilityLevel\(\w+,(\d+)\)<=0',body)))!=exclusions
        or 'isAliveCombatSapper(Yvr)' not in source('isWildernessObeliskTarget')
        or 'unit_isAllyOf(Yvr,Zvr)' not in source('isWildernessObeliskTarget')
        or '(not hasWildernessObeliskBuff(Yvr))' not in source('isWildernessObeliskTarget')
        or 'group_getRandom(gwr)' not in source('wildernessObeliskSpell')
        or 'jwr=uIb' not in source('wildernessObeliskSpell')):
        raise ValueError('Wilderness requires permanent conditional grants and its non-retry allied selection')
    expected_buff = f'unit_getAbilityLevel(Xvr,{conditional})>0)or(unit_getAbilityLevel(Xvr,{always[0]})>0'
    conditional_block = re.search(r'if\(\(\(\(unit_getAbilityLevel.*?then(.*?)end', body)
    if (expected_buff not in source('hasWildernessObeliskBuff')
        or '(awr==nil)or(widget_getLife(awr)<=0.405)' not in body
        or conditional_block is None
        or re.findall(r'addProtectedAbility\(\w+,(\d+)\)',conditional_block[1]) != [str(conditional)]
        or 'isWildernessObeliskTarget' not in source('ForGroupCallback_forUnitsInRect_RaceNelfAbilities_callback_forUnitsInRect_RaceNelfAbilities')
        or source('wildernessObeliskSpell').index('addEffect(') > source('wildernessObeliskSpell').index('if(iwr==nil)then return end')
        or source('wildernessObeliskSpell').index('if(iwr==nil)then return end') > source('wildernessObeliskSpell').index('applyWildernessObeliskBuff(iwr)')):
        raise ValueError('Wilderness conditional branch, OR retry exclusion, life guard or caster-art order changed')
    locust=int(re.search(r'unit_getAbilityLevel\(Yvr,(\d+)\)<=0',source('isWildernessObeliskTarget'))[1])
    codes=[v.to_bytes(4,'big').decode() for v in [conditional,*always]]
    a={r['rawcode']:r for r in abilities if r['level']=='1'}
    native=[json.loads(a[c]['data_fields_json']) for c in codes]
    if (a[codes[0]]['base_rawcode']!='AIsr' or native[0]['DataA']!=0
        or a[codes[1]]['base_rawcode']!='Assk' or native[1]['DataD']!=1 or native[1]['DataE']!=1
        or set(a[codes[1]]['targets'].split(','))!={'enemy','ally'}
        or a[codes[2]]['base_rawcode']!='AOae' or native[2]['DataA']!=0 or native[2]['DataB']!=0
        or a[codes[2]]['area']!='0' or a[codes[2]]['targets']!='self'):
        raise ValueError('Wilderness native resistance/skin/self-badge fields changed')
    grants=[{'ability':conditional,'effect':{'SpellResistance':int((1-Decimal(str(native[0]['DataB'])))*10000)}},
        {'ability':always[0],'effect':{'HardenedSkin':{'chance_per_10k':int(Decimal(str(native[1]['DataA']))*100),'ignored_damage':int(native[1]['DataC']),'minimum_damage':int(native[1]['DataB'])}}},
        {'ability':always[1],'effect':'AuraMarker'}]
    parent=row['ability_rawcode'];unit=units[row['building_rawcode']]
    if set(fields[parent]['atar:1'].split(','))!={'enemies','ground','invulnerable','mechanical','neutral','structure'}:
        raise ValueError('Wilderness native parent trigger mask changed')
    tick=lambda v:int((Decimal(str(v))*30).to_integral_value(rounding=ROUND_CEILING))
    cost=lambda k:next(p['runtime_value'] for p in protected if p['rawcode']==parent and p['field']==k)
    amount=lambda k:0 if unit[k].strip() in {'','-','_'} else int(Decimal(unit[k]))
    rate=int(Decimal(unit['mana_regen'])*10000)
    profile={'map_version':version,'trigger_targets':5,'trigger_invulnerable':True,'trigger_spell_immune':False,'trigger_mechanical_only':True,'grants':grants,'exclusions':exclusions,'locust':locust}
    return {'mana':{'maximum':amount('mana_max'),'starting':amount('mana_start'),'regen_per_tick_per_10k':(1<<31)|rate},'ability':{'id':raw(parent),'mana_cost':int(cost('mana_cost')),'cooldown_ticks':tick(cost('cooldown')),'range':int(Decimal(fields[parent]['aran:1'])*1024),'target_policy':'NativeBuildingSpellTrigger','effect':{'Wilderness':profile}}}
