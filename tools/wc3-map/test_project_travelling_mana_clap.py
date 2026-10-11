import unittest
from copy import deepcopy

from project_travelling_mana_clap import project


class TravellingManaClapProjection(unittest.TestCase):
    def fixture(self):
        raw = lambda s: int.from_bytes(s.encode(), 'big')
        handler = (f'Eln=tupleCopy1(unit_getPos(Cln))Fln=createUnit(Dln,{raw("MOVE")},unit_getPos(Bln),{{0.}})'
            'unit_issuePointOrderById(Fln,851984,Eln)__wurst_safe_UnitApplyTimedLife(Jln,1112820806,2.)'
            'Hln=(vec2_distanceTo(unit_getPos(Bln),Eln)/71.)'
            'Gln.p=Dln Gln.tpos=tupleCopy1(Eln)doAfter(Hln,Gln)')
        callback = (f'dummyCarrierCastImmediate(Akn.p,{raw("CLAP")},852096,Akn.tpos,3.)'
            'forUnitsInRange(Jkn,17.,false,Dkn)if(Gkn>0.)then Hkn=min1(Gkn,27.)Lkn=(Gkn-Hkn)'
            'addEffect1("Abilities\\\\Synthetic.mdl",Ekn,"origin")'
            'Tkn=ATTACK_TYPE_CHAOS Ukn=DAMAGE_TYPE_NORMAL '
            '__wurst_safe_UnitDamageTarget(Qkn,Rkn,Skn,true,false,Tkn,Ukn,Vkn)')
        picker = ('if((((isAliveCombatSapper(Ykn)and unit_isEnemyOf(Ykn,Xkn.p))and '
            f'unit_isType(Ykn,UNIT_TYPE_GROUND))and(unit_getAbilityLevel(Ykn,{raw("Avul")})<=0))'
            f'and(unit_getAbilityLevel(Ykn,{raw("SHLD")})<=0))then group_add(Xkn.g,Ykn)end')
        parent = {'buffid1': 'BUFF', 'unitid1': 'BODY', 'dataa1': '0', 'datab1': '0',
            'datac1': '0', 'datad1': '0', 'datae1': '1', 'missilehoming': '1',
            'missilespeed': '1200', 'dur1': '0.01', 'herodur1': '0.01', 'cast1': '0.01',
            'rng1': '31', 'targs1': 'enemies,ground,neutral,nonhero,vulnerable'}
        child = {'targs1': 'enemies,ground', 'datab1': '0', 'dataa1': '11', 'area1': '13',
            'dur1': '3', 'herodur1': '2', 'datac1': '0.33', 'datad1': '0.45'}
        return [{'source_kind': 'unit-ability', 'source_key': 'PARN', 'unit_rawcode': 'UNIT'},
            parent, {'mana_max': '-', 'mana_start': '-', 'mana_regen': '-'},
            {'mana_cost': '3', 'cooldown': '2'}, {'git_tree': 'synthetic',
            'map_version': {'major': 9, 'minor': 27},
            'mechanics_row': {'ability_rawcode': 'PARN', 'handler_function': 'handler',
                'target_mode_label': 'enemy-ground-combat-sapper',
                'delayed_callback_functions': 'callback', 'callback_functions': 'picker'},
            'functions': {'handler': handler, 'callback': callback, 'picker': picker},
            'all_fields': {'PARN': parent, 'CLAP': child},
            'all_bases': {'PARN': 'ACpa', 'CLAP': 'AHtc'},
            'all_units': {'MOVE': {'move_type': 'fly', 'abilities': 'Aloc,Avul'}},
            'all_unit_fields': {'MOVE': {'file': 'body.mdl', 'moveheight': '17'}},
            'all_unit_stats': {'MOVE': {'unitstat_move_speed': '45'}},
            'all_protected': {'CLAP': {'mana_cost': '0', 'cooldown': '0'}}}]

    def test_native_child_script_picker_and_mover_speed_remain_independent(self):
        output = project(*self.fixture())
        ability = output['spellcasting']['ability']
        self.assertEqual((ability['mana_cost'], ability['cooldown_ticks'], ability['range']), (3, 60, 31 * 1024))
        p = ability['effect']['TravellingManaClap']
        self.assertEqual((p['callback_speed_per_second'], p['mover_speed_per_tick']), (71 * 1024, 45 * 1024 // 30))
        self.assertEqual((p['clap']['radius'], p['burn_radius'], p['maximum_mana_burn']), (13 * 1024, 17 * 1024, 27))
        self.assertEqual(output['provenance']['script_visuals'][0]['model_path'], 'Abilities\\Synthetic.mdl')
        args = self.fixture()
        args[2].update(mana_max='23', mana_start='17', mana_regen='0.2')
        self.assertEqual(project(*args)['mana_regen_per_second_per_10k'], 2000)
        args[0].update(source_kind='ability-effect', source_key='CLAP')
        child = project(*args)
        self.assertIsNone(child['effect_ability_rawcode'])
        self.assertEqual(child['spellcasting']['ability']['effect']['FrostNova']['targets'], 1)
        self.assertEqual(child['provenance']['script_visuals'], [])

    def test_mutated_ownership_point_snapshot_damage_type_or_native_mask_is_rejected(self):
        mutations = [
            lambda d: d['functions'].update(handler=d['functions']['handler'].replace('Gln.p=Dln', 'Gln.p=nil')),
            lambda d: d['functions'].update(handler=d['functions']['handler'].replace('unit_getPos(Cln)', 'unit_getPos(Bln)')),
            lambda d: d['functions'].update(callback=d['functions']['callback'].replace('ATTACK_TYPE_CHAOS', 'ATTACK_TYPE_MAGIC')),
            lambda d: d['functions'].update(picker=d['functions']['picker'].replace('isAliveCombatSapper', 'unit_isAlive')),
            lambda d: d['all_fields']['CLAP'].update(targs1='enemies,air,ground'),
            lambda d: d['all_protected']['CLAP'].update(mana_cost='1'),
            lambda d: d['functions'].update(callback=d['functions']['callback'].replace('"origin"', '"head"')),
        ]
        for mutate in mutations:
            args = deepcopy(self.fixture())
            mutate(args[-1])
            with self.assertRaises(ValueError):
                project(*args)


if __name__ == '__main__':
    unittest.main()
