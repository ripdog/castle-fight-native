import unittest
from project_mana_attack_orb import project

class ManaAttackOrbProjectionTests(unittest.TestCase):
    def fixture(self):
        return [{'kind':'mana-attack-orb','source_kind':'unit-ability','source_key':'TEST','unit_rawcode':'UNIT'},
            {'targs1':'air,enemies,ground','levels':'1','cost1':'9999','cool1':'99','cast1':'0','dur1':'0','herodur1':'0',
             'missilehoming':'1','missilespeed':'777','missileart':'native.mdl','rng1':'99',
             'dataa1':'13','datab1':'.71','datac1':'.31','datad1':'9','datae1':'17','area1':'27'},
            {'attack1_weapon_type':'msplash','attack1_type':'magic','attack1_splash_targets':'enemies'}, {'mana_cost':'8','cooldown':'0'},
            {'native_base':'Afak','default_active_alias':'TEST'}]
    def test_native_mode_uses_protected_cost_exact_bonus_bands_and_its_own_missile(self):
        row=project(*self.fixture());p=row['profile']
        self.assertEqual((p['mana_cost'],p['bonus_damage']),(8,13))
        self.assertEqual((p['splash']['full_radius'],p['splash']['medium_radius'],p['splash']['outer_radius']),(9*1024,17*1024,27*1024))
        self.assertEqual(p['splash']['outer_damage_per_10k'],3100)
        self.assertEqual((p['primary_targets'],p['splash']['targets']),(3,7))
        self.assertEqual(row['provenance']['native_cast_range'],'99')
    def test_unmodeled_toggle_clocks_weapons_and_target_masks_fail_closed(self):
        for key,value in [('cast1','.1'),('dur1','1'),('targs1','air,enemies,ground,structure'),('datae1','2'),('missilehoming','0')]:
            f=self.fixture();f[1][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):project(*f)
        for i,key,value in [(2,'attack1_weapon_type','normal'),(3,'cooldown','1'),(4,'default_active_alias','NONE')]:
            f=self.fixture();f[i][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):project(*f)

if __name__=='__main__':unittest.main()
