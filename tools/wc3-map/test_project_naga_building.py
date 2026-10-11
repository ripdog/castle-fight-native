import copy
import json
import unittest
from project_naga_building import project_pyramid, project_oracle, project_coral, project_tidal


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


class OracleProjectionTests(unittest.TestCase):
    def oracle(self):
        f = NagaBuildingProjectionTest().fixture()
        raw = lambda code: int.from_bytes(code.encode(), 'big')
        f[0].update(mechanic_kind='dummy-target-ability', target_selector='random-alive-enemy')
        f[1]['CHLD'] = {'atar:1': 'air,ground,nonhero,enemy,neutral', 'acas:1': 0, 'Nch1:1': 7, 'aran:1': 19}
        f[3][1]['base_rawcode'] = 'ACch'
        f[5].update({
            'oracleSpell': {'source': f'Btr=randomAliveEnemy(Atr) if((Btr==nil)or checkForShield(Btr))then return end dummyCastTargetWithVision(Atr,{raw("CHLD")},91,Btr,unit_getPos(ztr),2.)'},
            'ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers': {'source': 'isAliveCombatSapper(zOn)unit_isEnemyOf(zOn,yOn.p)unit_getAbilityLevel(zOn,3)<=0 unit_getAbilityLevel(zOn,4)<=0 not(unit_getTypeId(zOn)==8)'},
            'mJ': {'source': 'unit_getAbilityLevel(U3q,5)>0)or hasShield(U3q) unit_getTypeId(U3q)==9 doAfter(.1,Z3q) __wurst_safe_SetUnitOwner(U3q,W3q,true) orderCodeAttackIfAllowed(U3q)'},
            'orderCodeAttackIfAllowed': {'source': 'unit_getAbilityLevel(V9o,6)>=1'},
            'dummyCastTargetWithVision1': {'source': 'x[2],12.,true,false doAfter(.2,x)'},
        })
        return f

    def test_charm_preserves_native_parent_resources_and_ownership_controls(self):
        p = project_oracle(*self.oracle())['ability']
        self.assertEqual(p['mana_cost'], 7)
        e = p['effect']['NagaCharm']
        self.assertEqual((e['ownership_rejection'], e['order_block'], e['defender_delay_ticks']), (5, 6, 3))
        self.assertEqual((e['vision_radius'], e['vision_ticks']), (12 * 1024, 6))
        self.assertEqual((e['targets'], e['maximum_creep_level']), (3, 7))

    def test_missing_shield_rollback_or_independent_selector_is_rejected(self):
        for name, token in [('oracleSpell', 'randomAliveEnemy'), ('oracleSpell', 'checkForShield'),
                            ('mJ', '__wurst_safe_SetUnitOwner(U3q,W3q,true)'),
                            ('mJ', 'hasShield(U3q)'), ('orderCodeAttackIfAllowed', 'unit_getAbilityLevel')]:
            f = self.oracle(); f[5][name]['source'] = f[5][name]['source'].replace(token, 'changed')
            with self.subTest(token=token), self.assertRaises(ValueError): project_oracle(*f)

    def test_native_charm_mask_base_and_secondary_cost_cannot_drift(self):
        f = self.oracle(); f[1]['CHLD']['atar:1'] = 'air,ground,enemy'
        with self.assertRaises(ValueError): project_oracle(*f)
        f = self.oracle(); f[3][1]['base_rawcode'] = 'AOhx'
        with self.assertRaises(ValueError): project_oracle(*f)
        f = self.oracle(); f[4][-2]['runtime_value'] = '1'
        with self.assertRaises(ValueError): project_oracle(*f)


class CoralProjectionTests(unittest.TestCase):
    def fixture(self):
        raw = lambda c: int.from_bytes(c.encode(), 'big')
        fields = {'BLDG': {'uaen:0': 0, 'uabi:0': 'A07V,A09A'},
            'ARMR': {'atar:1': 'friend,invulnerable,self,structure,vulnerable', 'adur:1': 0, 'ahdu:1': 0,
                'Had2:1': 0, 'Had1:1': 3.5, 'aare:1': 11, 'abuf:1': 'BUFF'},
            'REGN': {'atar:1': 'air,friend,ground,invulnerable,vulnerable', 'adur:1': 0, 'ahdu:1': 0,
                'Oar2:1': 0, 'Oar1:1': 2.25, 'aare:1': 19}}
        abilities = [{'rawcode': c, 'base_rawcode': base} for c, base in [('ARMR', 'ACav'), ('REGN', 'ACnr'), ('A07V', 'Amgl')]]
        trace = {'iD': {'source': f'elseif(mho=={raw("BLDG")})then safeUnitAddAbility(jho,{raw("ARMR")})safeUnitAddAbility(jho,{raw("REGN")})'}}
        return [fields, abilities, trace, {'major': 1, 'minor': 2}]

    def test_hidden_construction_auras_keep_independent_masks_ranges_and_rates(self):
        p = project_coral(*self.fixture())
        self.assertEqual((p['armor_targets'], p['regeneration_targets']), (4, 3))
        self.assertEqual((p['armor_radius'], p['regeneration_radius']), (11*1024, 19*1024))
        self.assertEqual((p['armor_bonus_per_100'], p['regeneration_per_second_per_10k']), (350, 22500))

    def test_changed_construction_and_native_bases_are_rejected(self):
        f = self.fixture(); f[2]['iD']['source'] = 'changed'
        with self.assertRaises(ValueError): project_coral(*f)
        f = self.fixture(); f[1][1]['base_rawcode'] = 'AOhw'
        with self.assertRaises(ValueError): project_coral(*f)

    def test_percentage_linger_or_ordinary_attack_cannot_be_silently_added(self):
        for code, key, value in [('BLDG', 'uaen:0', 1), ('ARMR', 'Had2:1', 1), ('REGN', 'Oar2:1', 1), ('REGN', 'adur:1', 3)]:
            f = self.fixture(); f[0][code][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError): project_coral(*f)


class TidalProjectionTests(unittest.TestCase):
    def fixture(self):
        f = NagaBuildingProjectionTest().fixture()
        f[0].update(mechanic_kind='dummy-target-ability')
        f[1]['CAST'].update({'atar:1': 'enemies,ground,neutral,nonhero,sapper,vulnerable', 'abuf:1': 'BUFF'})
        f[1]['CHLD'] = {'atar:1': 'enemies,ground,neutral', 'acas:1': 0, 'aran:1': 300,
            'Ucs1:1': 17, 'Ucs2:1': 99, 'Ucs3:1': 70, 'Ucs4:1': 9, 'aare:1': 3, 'amsp:0': 60}
        f[1]['BLDG'] = {'ua1g:0': 'enemies,ground,sapper,vulnerable'}
        f[3][1]['base_rawcode'] = 'ACcv'
        f[5]['tidalSpell'] = {'source': f'ytr=randomAliveEnemyGround(xtr)if(ytr==nil)then return end dummyCastTargetWithVision(xtr,{int.from_bytes(b"CHLD", "big")},91,ytr,unit_getPos(wtr),2.)'}
        f[5]['dummyCastTargetWithVision1'] = {'source': 'x[2],12.,true,false doAfter(.2,x)'}
        return f

    def test_native_wave_is_projected_with_independent_parent_and_weapon_guards(self):
        e = project_tidal(*self.fixture())['ability']['effect']['NagaTidalWave']
        self.assertTrue(e['trigger_nonhero'] and e['trigger_sapper'] and e['weapon_vulnerable'])
        self.assertEqual((e['wave']['damage'], e['wave']['initial_radius'], e['wave']['final_radius']), (17, 3*1024, 9*1024))
        self.assertEqual(e['wave']['speed_per_tick'], 2*1024)

    def test_source_origin_native_masks_and_secondary_resources_cannot_drift(self):
        for code, key, value in [('CAST', 'atar:1', 'enemies,ground'), ('CHLD', 'atar:1', 'enemies,air,ground'), ('BLDG', 'ua1g:0', 'enemies,ground')]:
            f = self.fixture(); f[1][code][key] = value
            with self.subTest(code=code), self.assertRaises(ValueError): project_tidal(*f)
        f = self.fixture(); f[5]['tidalSpell']['source'] = f[5]['tidalSpell']['source'].replace('unit_getPos(wtr)', 'unit_getPos(ytr)')
        with self.assertRaises(ValueError): project_tidal(*f)
        f = self.fixture(); f[4][-1]['runtime_value'] = '1'
        with self.assertRaises(ValueError): project_tidal(*f)


if __name__ == '__main__': unittest.main()
