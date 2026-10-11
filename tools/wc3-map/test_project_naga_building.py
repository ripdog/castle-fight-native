import copy
import json
import unittest
from project_naga_building import project_pyramid


class NagaBuildingProjectionTest(unittest.TestCase):
    def fixture(self):
        raw = lambda code: int.from_bytes(code.encode(), 'big')
        row = {'building_rawcode': 'BLDG', 'ability_rawcode': 'CAST',
            'mechanic_kind': 'dummy-immediate-ability-at-target-point', 'target_selector': 'random-alive-enemy-ground',
            'parameters_json': json.dumps({'effect_ability_id': raw('CHLD'), 'order_id': 91, 'dummy_lifetime_seconds': '2'})}
        fields = {'CAST': {'atar:1': 'enemies,ground,invulnerable,mechanical,neutral,structure', 'abuf:1': '',
            'aran:1': 17, 'native_base_buff': 'BASE,MINI', 'amho:0': '1', 'amsp:0': 200, 'adur:1': .01, 'ahdu:1': 0, 'ipmu:1': 'BODY', 'Npa6:1': .01, 'Npa5:1': 1, **{f'Poi{i}:1': 0 for i in range(1, 5)}},
            'CHLD': {'atar:1': 'enemies,ground', 'Htc1:1': 23, 'Htc2:1': 0, 'Htc3:1': 0.25, 'Htc4:1': 0.75,
                'acas:1': 0, 'abuf:1': 'BUFF', 'aare:1': 19, 'adur:1': 0.15, 'ahdu:1': 0.05}}
        units = {'BLDG': {'mana_max': '9', 'mana_start': '-', 'mana_regen': '2'}}
        abilities = [{'rawcode': 'CAST', 'level': '1', 'base_rawcode': 'ACpa'}, {'rawcode': 'CHLD', 'level': '1', 'base_rawcode': 'AHtc'}]
        protected = [{'rawcode': code, 'field': field, 'runtime_value': value} for code, field, value in
            [('CAST', 'mana_cost', '7'), ('CAST', 'cooldown', '.2'), ('CHLD', 'mana_cost', '0'), ('CHLD', 'cooldown', '0')]]
        traces = {'pyramidSpell': {'source': f'Etr=randomAliveEnemyGround(Dtr)if(Etr==nil)then return end dummyCastImmediateFrom1(Dtr,{raw("CHLD")},91,unit_getPos(Etr),2.)'},
            'ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1': {'source': 'isAliveCombatSapper(COn)unit_isEnemyOf(COn,BOn.p)UNIT_TYPE_GROUND unit_getAbilityLevel(COn,3)<=0 unit_getAbilityLevel(COn,4)<=0'}}
        return [row, fields, units, abilities, protected, traces, {'major': 1, 'minor': 2}]

    def test_protected_resources_and_independent_native_duration_are_projected(self):
        p = project_pyramid(*self.fixture())
        self.assertEqual((p['ability']['mana_cost'], p['ability']['cooldown_ticks'], p['mana']['starting']), (7, 6, 0))
        effect = p['ability']['effect']['NagaEarthquake']
        self.assertEqual((effect['damage'], effect['duration_ticks'], effect['hero_duration_ticks']), (23, 5, 2))
        self.assertEqual((effect['movement_percent_delta'], effect['attack_speed_percent_delta']), (-25, -75))

    def test_selector_and_effect_origin_changes_are_rejected(self):
        fixture = self.fixture()
        for name, token in [('pyramidSpell', 'randomAliveEnemyGround'), ('pyramidSpell', 'if(Etr==nil)then return end'),
                ('pyramidSpell', 'unit_getPos(Etr)'), ('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1', 'UNIT_TYPE_GROUND'),
                ('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers1', 'isAliveCombatSapper(COn)')]:
            f = copy.deepcopy(fixture)
            f[5][name]['source'] = f[5][name]['source'].replace(token, 'changed')
            with self.subTest(token=token), self.assertRaises(ValueError): project_pyramid(*f)

    def test_parent_child_masks_payload_and_child_resource_changes_are_rejected(self):
        for key, value in [('abuf:1', 'BUFF'), ('Poi1:1', 1), ('atar:1', 'enemies,ground')]:
            f = self.fixture(); f[1]['CAST'][key] = value
            with self.subTest(parent=key), self.assertRaises(ValueError): project_pyramid(*f)
        for key, value in [('atar:1', 'enemies,ground,air'), ('Htc2:1', 5), ('acas:1', 1)]:
            f = self.fixture(); f[1]['CHLD'][key] = value
            with self.subTest(child=key), self.assertRaises(ValueError): project_pyramid(*f)
        f = self.fixture(); f[4][-1]['runtime_value'] = '1'
        with self.assertRaises(ValueError): project_pyramid(*f)


if __name__ == '__main__': unittest.main()
