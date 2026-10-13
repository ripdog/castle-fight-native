import unittest
from project_orb_charm import project


class OrbCharmProjectionTests(unittest.TestCase):
    def fixture(self):
        return [
            {'kind': 'orb-charm', 'source_kind': 'ability-effect', 'source_key': 'CHLD'},
            {'targs1': 'air,enemies,ground,neutral,nonhero,ward', 'dataa1': '10',
             'cost1': '9999', 'cool1': '99', 'cast1': '0'},
            {'mana_cost': '0', 'cooldown': '0'},
            {'native_base': 'ANch', 'map_version': {'major': 9, 'minor': 27}},
        ]

    def test_free_child_keeps_creep_limit_and_version(self):
        p = project(*self.fixture())['profile']
        self.assertEqual((p['maximum_creep_level'], p['targets'], p['map_version']['minor']), (10, 3, 27))

    def test_unmodeled_masks_costs_delay_and_base_fail_closed(self):
        for part, field, value in [(1, 'targs1', 'air,ground,enemies'), (1, 'cast1', '.1'),
                                   (2, 'mana_cost', '1'), (2, 'cooldown', '1'), (3, 'native_base', 'ACch')]:
            f = self.fixture(); f[part][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError): project(*f)


if __name__ == '__main__':
    unittest.main()
