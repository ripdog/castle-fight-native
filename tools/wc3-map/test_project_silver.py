import json
import unittest
from project_silver import project
class SilverProjectionTest(unittest.TestCase):
    def test_attack_attempt_and_owner_target_origin_are_required(self):
        raw=lambda c:int.from_bytes(c.encode(),'big')
        p={'trigger_event':'EVENT_PLAYER_UNIT_ATTACKED','effect_ability_id':raw('CHLD'),'order_id':3,'dummy_lifetime_seconds':'1','excluded_ability_id':7,'round_end_signal_resets_both_team_counters':True}
        row={'building_rawcode':'UNIT','ability_rawcode':'CAST','mechanic_kind':'team-charge-next-attack-proc','parameters_json':json.dumps(p)}
        strings={'OK':'EVENT_PLAYER_UNIT_ATTACKED','PK':f'isCombatSapper(Wwr)unit_getAbilityLevel(Wwr,7)<=0 dummyCastImmediateFrom1(Xwr,{raw("CHLD")},3,unit_getPos(Wwr),1.)','NK':'R0[Vvr]=(__wurst_ensureInt(R0[Vvr])-1)','addSilverGladeChargeForTeam':'R0[Uvr]=(__wurst_ensureInt(R0[Uvr])+1)','resetSilverGladeCounterForTeam':'R0[Wvr]=0','Action_watch_RaceNelfAbilities_run_watch_RaceNelfAbilities':'resetSilverGladeCounterForTeam(0)resetSilverGladeCounterForTeam(1)'}
        traces={k:{'source':s} for k,s in strings.items()}
        fields={'CAST':{'aran:1':100,'atar:1':'enemies,ground,invulnerable,mechanical,neutral,structure'},'CHLD':{'adur:1':0.15,'ahdu:1':0.15}}
        units={'UNIT':{'mana_max':'19','mana_start':'-','mana_regen':'1'}}
        abilities=[{'rawcode':'CHLD','level':'1','base_rawcode':'AIdb','targets':'air,friend,ground,self,vulnerable','data_fields_json':'{"DataA":3,"DataB":17,"DataC":5}','area':'9','buffs':'BUFF'}]
        protected=[{'rawcode':'CAST','field':'mana_cost','runtime_value':'5'},{'rawcode':'CAST','field':'cooldown','runtime_value':'0.1'}]
        effect=project(row,fields,units,abilities,protected,traces,{'major':1,'minor':2})['ability']['effect']['SilverGlade']
        self.assertEqual((effect['health'],effect['mana'],effect['duration_ticks']),(17,5,5))
        for name,token in [('OK','EVENT_PLAYER_UNIT_ATTACKED'),('PK','isCombatSapper(Wwr)'),('PK','unit_getPos(Wwr)'),('NK','-1'),('resetSilverGladeCounterForTeam','R0[Wvr]=0')]:
            with self.subTest(token=token),self.assertRaises(ValueError):
                project(row,fields,units,abilities,protected,{**traces,name:{'source':strings[name].replace(token,'changed')}},{'major':1,'minor':2})
