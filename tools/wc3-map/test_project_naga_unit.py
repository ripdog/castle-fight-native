import unittest
from copy import deepcopy
from project_naga_unit import project

class NagaUnitProjection(unittest.TestCase):
    def fixture(self):
        raw=lambda s:int.from_bytes(s.encode(),'big')
        handler='UnitSpellClosure_registerUnitSpellWithTargetOrder_RaceNagaAbilities_cast_registerUnitSpellWithTargetOrder_RaceNagaAbilities'
        callback='CallbackSingle_doAfter_RaceNagaAbilities_call_doAfter_RaceNagaAbilities'
        resume='CallbackSingle_doAfter_doAfter_RaceNagaAbilities_call_doAfter_doAfter_RaceNagaAbilities'
        funcs={handler:f'dummyCastTargetFrom(ttn,{raw("A02T")},852111,utn,2.)doAfter(2.,vtn)',
            callback:'if unit_issueImmediateOrderById(ctn.caster,852126)then doAfter(3.,dtn)else orderCodeAttack(ctn.caster)end',
            resume:'orderCodeAttack(ftn.caster)'}
        return {'source_kind':'unit-ability','source_key':'A02U','unit_rawcode':'n00S','kind':'siren-purge'}, \
            {'cost1':'9999','cool1':'99','rng1':'7'}, {'mana_regen':'0.2','mana_max':'50','mana_start':'20'}, \
            {'mana_cost':'3','cooldown':'4'}, {'git_tree':'synthetic','map_version':{'major':9,'minor':27},
            'native_base':'ACpa','naga_functions':funcs,'misc':{'MinUnitSpeed':'2'},
            'mechanics_row':{'target_mode_label':'enemy-ground-combat-sapper','handler_function':handler,
                'delayed_callback_functions':callback+','+resume},
            'semantics_row':{'effect_rawcodes':'A02T'}, 'purge_base':'ACpu',
            'purge_protected':{'mana_cost':'0','cooldown':'0'}, 'parent_fields':{'targs1':'ground,nonhero','buffid1':'TEST', 'dataa1':'0','datab1':'0','datac1':'0','datad1':'0','datae1':'1','missilehoming':'1','missilespeed':'1200','dur1':'0.01','herodur1':'0.01','unitid1':'nfbr','cast1':'0.01'},
            'purge_fields':{'targs1':'air,ground,tree,vulnerable,ward','rng1':'11','dataa1':'8','dur1':'2','herodur1':'1',
                'datad1':'0.2','datae1':'0','datac1':'17','datab1':'99','dataf1':'300'}}
    def test_independent_child_and_delayed_order_use_source_values(self):
        args=self.fixture(); p=project(*args)
        ability=p['spellcasting']['ability'];self.assertEqual(ability['mana_cost'],3);self.assertEqual(ability['cooldown_ticks'],120)
        effect=ability['effect']['SirenPurge'];self.assertEqual(effect['child']['range'],11*1024)
        self.assertEqual(effect['child']['summoned_damage'],17);self.assertEqual(effect['summon_delay_ticks'],60)
        self.assertEqual(effect['resume_delay_ticks'],90);self.assertNotIn('mana_loss',effect['child'])
    def test_changed_native_resources_mask_or_callback_rejected(self):
        for mutate in (lambda d:d['purge_protected'].update(mana_cost='1'),
            lambda d:d['purge_fields'].update(targs1='ground'), lambda d:d.update(purge_base='AHbn'),
            lambda d:d['naga_functions'].update({next(k for k in d['naga_functions'] if k.startswith('CallbackSingle_doAfter_Race')):'orderCodeAttack(ctn.caster)'})):
            args=deepcopy(self.fixture());mutate(args[-1])
            with self.assertRaises(ValueError):project(*args)
    def test_feral_spirit_retains_native_forward_offset_and_timed_body(self):
        args=list(self.fixture());args[0].update(source_key='A02S',kind='feral-spirit')
        args[1].update(targs1='_',levels='1',unitid1='TEST',datab1='3',dur1='2',area1='-4')
        args[-1]['native_base']='AOsf'
        p=project(*args)['spellcasting']['ability']['effect']['NativeSummon']
        self.assertEqual(p['body_rawcode'],int.from_bytes(b'TEST','big'));self.assertEqual(p['count'],3)
        self.assertEqual(p['duration_ticks'],60);self.assertEqual(p['forward_distance'],-4096)

if __name__=='__main__':unittest.main()
