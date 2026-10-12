import unittest
from project_incinerate import project

class IncinerateProjectionTests(unittest.TestCase):
    def fixture(self):
        return [{'kind':'incinerate','source_kind':'unit-ability','source_key':'ANic','unit_rawcode':'TEST'},
            {'targs1':'enemy,neutral,organic,nonancient','levels':'1','buffid1':'BUFF',
             'cost1':'0','cool1':'0','cast1':'0','missilehoming':'1','missilearc':'0',
             'dataa1':'13','datab1':'51','datac1':'9','datad1':'27','datae1':'17','dataf1':'.11',
             'dur1':'.51','herodur1':'.21','missilespeed':'777','missileart':'native.mdl'},'ANic']
    def test_passive_keeps_fields_clocks_and_native_identity_policy(self):
        f=self.fixture(); p=project(*f)['profile']
        self.assertEqual((p['bonus_per_hit'],p['full_damage'],p['half_damage']),(13,51,27))
        self.assertEqual((p['duration_ticks'],p['hero_duration_ticks'],p['delay_ticks']),(16,7,4))
        f[0]['source_key']='CLON'; self.assertEqual(project(*f)['profile']['bonus_per_hit'],0)
        self.assertEqual(project(*f)['profile']['full_damage'],51)
    def test_active_arrows_resources_masks_levels_and_geometry_fail_closed(self):
        for key,value in [('cost1','1'),('cool1','1'),('cast1','.1'),('levels','2'),
            ('targs1','enemy,neutral,organic'),('missilehoming','0'),('datae1','1')]:
            f=self.fixture();f[1][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError): project(*f)
        f=self.fixture();f[2]='ANia'
        with self.assertRaises(ValueError):project(*f)

if __name__=='__main__':unittest.main()
