import unittest
from project_native_debuff import project


class NativeSlowProjectionTests(unittest.TestCase):
    def fixture(self):
        return [{'kind': 'native-debuff', 'source_kind': 'unit-ability', 'source_key': 'SLOW', 'unit_rawcode': 'BODY'},
            {'targs1': 'air,ground,enemy', 'buffid1': 'BUFF', 'dur1': '.11', 'herodur1': '.05',
                'dataa1': '.4', 'datab1': '.2', 'cost1': '7', 'cool1': '.5', 'rng1': '13'},
            {'mana_max': '9', 'mana_start': '8', 'mana_regen': '2'}, {},
            {'native_base': 'Aslo', 'reference_source_sha256': {'reference': 'hash'}, 'default_active_alias': 'Aslo'}]

    def test_native_resources_durations_and_reference_identity_are_projected(self):
        row = project(*self.fixture())
        ability = row['spellcasting']['ability']
        self.assertEqual((ability['mana_cost'], ability['cooldown_ticks']), (7, 15))
        profile = ability['effect']['NativeDebuff']
        self.assertEqual((profile['duration_ticks'], profile['hero_duration_ticks']), (4, 2))
        self.assertEqual((profile['movement_percent_delta'], profile['attack_speed_percent_delta']), (-40, -20))
        self.assertEqual(row['provenance']['default_active_alias'], 'Aslo')

    def test_unrepresentable_native_target_classes_are_rejected(self):
        f = self.fixture(); f[1]['targs1'] = 'ground,enemy,structure'
        with self.assertRaises(ValueError): project(*f)


if __name__ == '__main__':
    unittest.main()
