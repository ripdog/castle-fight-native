import copy
import json
import unittest
from project_wilderness import project
class WildernessProjectionTest(unittest.TestCase):
    def test_conditional_resistance_and_unconditional_skin_are_distinct(self):
        raw=lambda c:int.from_bytes(c.encode(),'big')
        ids=[raw(c) for c in ('RESI','SKIN','BADG')]
        exclusions=[11,12,13,14]
        row={'mechanic_kind':'persistent-buff-bundle','building_rawcode':'UNIT','ability_rawcode':'CAST','parameters_json':json.dumps({'conditional_spell_resistance_ability_id':ids[0],'always_ability_ids':ids[1:],'spell_resistance_exclusion_ability_ids':exclusions})}
        body=f'if((awr==nil)or(widget_getLife(awr)<=0.405))then return false end if((((unit_getAbilityLevel(awr,11)<=0)and(unit_getAbilityLevel(awr,12)<=0))and(unit_getAbilityLevel(awr,13)<=0))and(unit_getAbilityLevel(awr,14)<=0))then bwr=awr addProtectedAbility(bwr,{ids[0]})end cwr=awr addProtectedAbility(cwr,{ids[1]})dwr=awr addProtectedAbility(dwr,{ids[2]})return true end'
        strings={'applyWildernessObeliskBuff':body,'hasWildernessObeliskBuff':f'unit_getAbilityLevel(Xvr,{ids[0]})>0)or(unit_getAbilityLevel(Xvr,{ids[1]})>0','isWildernessObeliskTarget':'isAliveCombatSapper(Yvr)unit_isAllyOf(Yvr,Zvr)(not hasWildernessObeliskBuff(Yvr))unit_getAbilityLevel(Yvr,15)<=0','wildernessObeliskSpell':'jwr=uIb addEffect( group_getRandom(gwr) if(iwr==nil)then return end applyWildernessObeliskBuff(iwr)','ForGroupCallback_forUnitsInRect_RaceNelfAbilities_callback_forUnitsInRect_RaceNelfAbilities':'isWildernessObeliskTarget'}
        traces={k:{'source':s} for k,s in strings.items()}
        fields={'CAST':{'atar:1':'enemies,ground,invulnerable,mechanical,neutral,structure','aran:1':10}}
        units={'UNIT':{'mana_max':'9','mana_start':'-','mana_regen':'1'}}
        abilities=[{'rawcode':c,'level':'1','base_rawcode':base,'targets':targets,'area':'0','data_fields_json':json.dumps(data)} for c,base,targets,data in [('RESI','AIsr','self',{'DataA':0,'DataB':0.3}),('SKIN','Assk','enemy,ally',{'DataA':70,'DataB':2,'DataC':5,'DataD':1,'DataE':1}),('BADG','AOae','self',{'DataA':0,'DataB':0})]]
        protected=[{'rawcode':'CAST','field':'mana_cost','runtime_value':'3'},{'rawcode':'CAST','field':'cooldown','runtime_value':'0.1'}]
        args=(row,fields,units,abilities,protected,traces,{'major':1,'minor':2})
        grants=project(*args)['ability']['effect']['Wilderness']['grants']
        self.assertEqual(grants[0]['effect'],{'SpellResistance':7000})
        self.assertEqual(grants[1]['effect']['HardenedSkin'],{'chance_per_10k':7000,'minimum_damage':2,'ignored_damage':5})
        for name,token in [('applyWildernessObeliskBuff','end cwr'),('hasWildernessObeliskBuff',')or('),('isWildernessObeliskTarget','(not hasWildernessObeliskBuff(Yvr))'),('wildernessObeliskSpell','jwr=uIb'),('applyWildernessObeliskBuff','widget_getLife(awr)<=0.405')]:
            with self.subTest(token=token),self.assertRaises(ValueError):
                altered={**traces,name:{'source':strings[name].replace(token,'changed')}}
                project(row,fields,units,abilities,protected,altered,args[-1])
        for index,field,value in [(1,'DataD',0),(2,'DataA',0.1)]:
            mutated=copy.deepcopy(abilities)
            data=json.loads(mutated[index]['data_fields_json']);data[field]=value
            mutated[index]['data_fields_json']=json.dumps(data)
            with self.assertRaises(ValueError):project(row,fields,units,mutated,protected,traces,args[-1])
