import unittest
import json
from project_impale import project


class ImpaleProjectionTests(unittest.TestCase):
    def fixture(self):
        child_id = int.from_bytes(b'CHLD', 'big')
        native = {'targs1': 'enemies,ground,neutral', 'dataa1': '17', 'datab1': '.11',
            'datac1': '13', 'datad1': '.05', 'datae1': '0', 'dataf1': '0', 'cast1': '0',
            'area1': '3', 'dur1': '.1', 'herodur1': '.02', 'rng1': '19', 'cool1': '9'}
        parent = {'animnames': 'death', 'targs1': 'enemies,ground,nonhero,vulnerable', 'buffid1': 'BUFF',
            'unitid1': 'BODY', 'dataa1': '0', 'datab1': '0', 'datac1': '0', 'datad1': '0',
            'datae1': '1', 'cast1': '.01', 'missilehoming': '1', 'missilespeed': '60',
            'dur1': '.01', 'herodur1': '.01', 'rng1': '23'}
        trace = {'lane_count': 4, 'relative_angles_degrees': [-37, -26, -15, -4],
            'cast_origin_offset': 2, 'dummy_lifetime_seconds': 1}
        detail = {'mechanics_row': {'helper_functions': 'helper', 'handler_function': 'handler',
            'ability_rawcode': 'PARN', 'target_mode_label': 'enemy-ground-combat-sapper'},
            'semantics_row': {'semantic_kind': 'four-lane-cone-proxy', 'effect_rawcodes': 'CHLD',
                'parameters_json': json.dumps(trace)},
            'functions': {'helper': f'bmr=tupleCopy1(unit_getPos(Ylr))cmr=angle_degrees(vec2_angleTo(unit_getPos(Ylr),unit_getPos(Zlr)))if(dmr>3)then break end ((cmr-37.)+(11.*dmr))dummyCastPointFrom(amr,{child_id},852555,vec2_polarOffset(bmr,emr,2.),bmr,1.)',
                'dummyCastPointFrom': 'DummyRecycler_get(zTr,{0.})unit_issuePointOrderById(BTr,xTr,yTr)',
                'recycleSpellDummy': 'unit_removeAbility(tSr,uSr)DummyRecycler_recycle(tSr)',
                'QG': f'Tdb=__wurst_ensureInt({int.from_bytes(b"DUMY", "big")})'},
            'all_fields': {'CHLD': native, 'PARN': parent}, 'all_bases': {'CHLD': 'ACmp', 'PARN': 'ACpa'},
            'all_protected': {'CHLD': {'mana_cost': '0'}}, 'default_active_alias': 'PARN',
            'all_unit_fields': {'DUMY': {'castpt': '0'}}, 'all_units': {'DUMY': {'move_type': 'fly'}},
            'git_tree': 'tree', 'map_version': {'major': 1, 'minor': 2}}
        return [{'source_kind': 'unit-ability', 'source_key': 'PARN', 'unit_rawcode': 'UNIT'},
            parent, {'mana_max': '9', 'mana_start': '7', 'mana_regen': '.1'},
            {'mana_cost': '3', 'cooldown': '2'}, detail]

    def test_primary_resources_and_independent_native_wave_air_and_hero_deadlines(self):
        row = project(*self.fixture())
        ability = row['spellcasting']['ability']; p = ability['effect']['ImpaleFan']
        self.assertEqual((ability['mana_cost'], ability['cooldown_ticks']), (3, 60))
        self.assertEqual(p['relative_angles_millidegrees'], [-37000, -26000, -15000, -4000])
        self.assertEqual((p['child']['wave_ticks'], p['child']['air_ticks'], p['child']['hero_stun_ticks']), (4, 2, 1))
        self.assertEqual(p['cast_point_distance'], 2 * 1024)
        self.assertTrue(p['dummy_flying'])
        self.assertTrue(row['provenance']['dummy_ability_removed_before_recycling'])

    def test_child_owns_its_native_identity_and_no_caster_mana_profile(self):
        f = self.fixture(); f[0].update(source_kind='ability-effect', source_key='CHLD')
        row = project(*f)
        self.assertEqual(row['kind'], 'native-impale')
        self.assertNotIn('spellcasting', row)

    def test_source_origin_argument_order_lanes_and_native_restrictions_fail_closed(self):
        for key, value in [('datae1', '1'), ('dataf1', '1'), ('cast1', '.1'), ('datab1', '0'),
                           ('targs1', 'enemies,ground,organic')]:
            f = self.fixture(); f[4]['all_fields']['CHLD'][key] = value
            with self.subTest(field=key), self.assertRaises(ValueError): project(*f)
        for key, before, after in [('helper', 'bmr=tupleCopy1(unit_getPos(Ylr))', 'bmr=tupleCopy1(unit_getPos(Zlr))'),
            ('helper', 'bmr,1.)', 'emr,1.)'), ('helper', '37.', '38.'),
            ('dummyCastPointFrom', 'DummyRecycler_get(zTr', 'DummyRecycler_get(yTr'),
            ('recycleSpellDummy', 'unit_removeAbility(tSr,uSr)', '')]:
            f = self.fixture(); f[4]['functions'][key] = f[4]['functions'][key].replace(before, after)
            with self.subTest(function=key), self.assertRaises(ValueError): project(*f)
        f = self.fixture(); f[4]['all_protected']['CHLD']['mana_cost'] = '1'
        with self.assertRaises(ValueError): project(*f)
        f = self.fixture(); f[4]['all_unit_fields']['DUMY']['castpt'] = '.1'
        with self.assertRaises(ValueError): project(*f)


if __name__ == '__main__':
    unittest.main()
