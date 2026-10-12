import unittest
from project_death_explosion import project

class DeathExplosionProjectionTests(unittest.TestCase):
    def fixture(self):
        return [{'kind':'death-explosion','source_kind':'unit-ability','source_key':'TEST','unit_rawcode':'UNIT'},
            {'levels':'1','targs1':'ground,enemies,structure,debris,ward','cost1':'0','cool1':'0','cast1':'0',
             'dur1':'.11','herodur1':'.11','dataa1':'13','datab1':'41','datac1':'27','datad1':'19'},'Amnx']
    def test_native_duration_is_a_post_death_deadline_and_preserves_both_rings(self):
        row=project(*self.fixture());p=row['profile']
        self.assertEqual((p['full_radius'],p['partial_radius']),(13*1024,27*1024))
        self.assertEqual((p['full_damage'],p['partial_damage'],p['delay_ticks'],p['targets']),(41,19,4,5))
        self.assertEqual(row['provenance']['delay_field'],'Dur1')
    def test_other_target_clocks_and_nonpassive_resources_fail_closed(self):
        for key,value in [('cost1','1'),('cast1','.1'),('herodur1','.2'),('targs1','ground,enemies'),('datac1','1')]:
            f=self.fixture();f[1][key]=value
            with self.subTest(key=key),self.assertRaises(ValueError):project(*f)
if __name__=='__main__':unittest.main()
