import unittest
from project_curse import project


class CurseProjection(unittest.TestCase):
    def fixture(self):
        return [dict(source_kind='unit-ability', source_key='CURS', unit_rawcode='UNIT'),
            dict(targs1='air,ground,enemies,neutral', buffid1='BUFF', hero='0',
                order='curse', orderon='curseon', orderoff='curseoff', dataa1='0.17',
                cost1='9999', cool1='99', rng1='31', dur1='8', herodur1='4'),
            dict(mana_max='9', mana_start='7', mana_regen='0.2'),
            dict(mana_cost='3', cooldown='2'), dict(native_base='Acrs')]

    def test_native_mask_fraction_duration_and_protected_resources(self):
        result = project(*self.fixture())
        ability = result['spellcasting']['ability']
        self.assertEqual((ability['mana_cost'], ability['cooldown_ticks'], ability['range']), (3, 60, 31 * 1024))
        self.assertEqual(ability['effect']['Curse']['chance_to_miss_per_10k'], 1700)
        self.assertFalse(ability['effect']['Curse']['buff']['organic_only'])
        self.assertEqual(result['mana_regen_per_second_per_10k'], 2000)

    def test_unsupported_orders_target_mask_or_fraction_are_rejected(self):
        for changed in [dict(orderon=''), dict(targs1='air,ground,enemies,organic'), dict(dataa1='1.1')]:
            args = self.fixture()
            args[1].update(changed)
            with self.assertRaises(ValueError): project(*args)
