import unittest
from project_cripple import project


class NativeCrippleProjectionTests(unittest.TestCase):
    def fixture(self):
        return [
            {'kind': 'native-target-debuff', 'source_kind': 'ability-effect', 'source_key': 'CHLD'},
            {'targs1': 'air,enemies,ground,neutral,ward', 'buffid1': 'BUFF',
             'dur1': '.11', 'herodur1': '.05', 'dataa1': '0', 'datab1': '0',
             'datac1': '.4', 'cost1': '9999', 'cool1': '99', 'cast1': '0'},
            {'mana_cost': '0', 'cooldown': '0'}, {'native_base': 'Acri'},
        ]

    def test_free_child_keeps_its_native_family_damage_and_independent_hero_duration(self):
        row = project(*self.fixture())
        p = row['profile']
        self.assertEqual((p['duration_ticks'], p['hero_duration_ticks']), (4, 2))
        self.assertEqual(p['damage_bonus_per_10k'], -4000)
        self.assertFalse(p['organic_only'])
        self.assertIsNone(p['ethereal'])
        self.assertNotIn('unit_rawcode', row)
        self.assertEqual(row['provenance']['native_base'], 'Acri')

    def test_unmodeled_child_cost_cast_delay_and_target_classes_fail_closed(self):
        for field, value in [('cast1', '.1'), ('targs1', 'air,enemies,ground,organic'), ('buffid1', 'ONE1,TWO2')]:
            f = self.fixture(); f[1][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError): project(*f)
        for field in ['mana_cost', 'cooldown']:
            f = self.fixture(); f[2][field] = '1'
            with self.subTest(field=field), self.assertRaises(ValueError): project(*f)


if __name__ == '__main__':
    unittest.main()
