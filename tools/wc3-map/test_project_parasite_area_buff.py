import unittest
from copy import deepcopy
from project_parasite_area_buff import project


class ParasiteAreaBuffProjection(unittest.TestCase):
    def fixture(self):
        parent = dict(buffid1='PBUF', unitid1='BODY', dataa1='0', datab1='0',
            datac1='0', datad1='0', datae1='1', missilehoming='1', missilespeed='73',
            dur1='0.01', herodur1='0.01', cast1='0.01', rng1='31',
            targs1='enemies,ground,neutral,nonhero,structure')
        child = dict(buffid1='CBUF', dataa1='0.17', datab1='2', datac1='0',
            datad1='0', datae1='0', dataf1='0', datag1='0', dur1='8',
            herodur1='4', area1='12', cool1='0', priority='1', targs1='air,ground,friend,self')
        return [dict(source_kind='unit-ability', source_key='PARN', unit_rawcode='UNIT'),
            parent, dict(mana_max='9', mana_start='7', mana_regen='0.2'),
            dict(mana_cost='3', cooldown='2'), dict(git_tree='synthetic',
            map_version=dict(major=9, minor=27), effect_key='CHLD', effect_fields=child,
            effect_protected=dict(mana_cost='0'), all_fields={'PARN': parent},
            all_bases={'PARN': 'ACpa', 'CHLD': 'ACro'},
            mechanics_row=dict(ability_rawcode='PARN', mechanic_kind='dummy-immediate-ability-from-caster',
                target_mode_label='enemy-ground-combat-sapper', scheduled_delays_json='[]', handler_function='handler'),
            functions={'handler': 'vln=tln dummyCastImmediateFrom(vln,1128811588,852164,1.)'})]

    def test_parent_geometry_resources_and_child_buff_are_independent(self):
        result = project(*self.fixture())
        ability = result['spellcasting']['ability']
        self.assertEqual((ability['mana_cost'], ability['cooldown_ticks'], ability['range']), (3, 60, 31 * 1024))
        profile = ability['effect']['ParasiteAreaBuff']
        self.assertEqual((profile['radius'], profile['damage_bonus_per_10k'], profile['duration_ticks']), (12 * 1024, 1700, 240))
        self.assertEqual(result['mana_regen_per_second_per_10k'], 2000)

    def test_changed_center_target_mask_delay_or_child_resources_are_rejected(self):
        for mutate in [
            lambda d: d['functions'].update(handler=d['functions']['handler'].replace('vln=tln', 'vln=uln')),
            lambda d: d['effect_fields'].update(targs1='air,ground,enemy'),
            lambda d: d['effect_protected'].update(mana_cost='1'),
            lambda d: d['mechanics_row'].update(scheduled_delays_json='[1]'),
        ]:
            args = deepcopy(self.fixture())
            mutate(args[-1])
            with self.assertRaises(ValueError):
                project(*args)
