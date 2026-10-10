import copy
import json
from pathlib import Path
import unittest
import build_native_tuning as tuning
import build_runtime_catalog as catalog
from project_starfall import project

class StarfallProjectionTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        root=Path(__file__).resolve().parents[2]
        evidence=json.loads((root/'crates/sim/data/castle-fight/9.27/building-mechanics-r1.json').read_text())
        release=catalog._load_release(catalog.DEFAULT_RELEASES,'9.27','r1')
        rows=lambda name:tuning.rows(catalog._retained_file_bytes(root,release['extraction']['git_tree'],'resolved/'+name))
        cls.evidence=evidence
        cls.args=[evidence['starfall'],evidence['fields'],{r['rawcode']:r for r in rows('units.tsv')},rows('abilities.tsv'),evidence['protected'],evidence['script_traces'],{'major':9,'minor':27}]
    def test_projection_reproduces_source_owned_variants(self):
        self.assertEqual(project(*self.args),self.evidence['starfall_spellcasting'])
    def test_script_control_flow_cannot_collapse_selector_dummy_or_cleanup(self):
        for name,token in [('starfallSpell','randomAliveEnemy(swr)'),('starfallSpell','{vwr;wwr}'),('starfallSpell','if(not NGb)then'),('starfallSpell','doAfter('),('dummyCastImmediateFrom1','SetUnitOwner(ZTr,aUr,false)'),('dummyCastImmediateFrom1','doAfter(TTr,YTr)'),('recycleSpellDummy','unit_removeAbility(tSr,uSr)'),('ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers','isAliveCombatSapper(zOn)')]:
            args=copy.deepcopy(self.args)
            args[5][name]['source']=args[5][name]['source'].replace(token,'changed')
            with self.subTest(token=token),self.assertRaises((ValueError,AttributeError)):project(*args)
        args=copy.deepcopy(self.args)
        body=args[5]['setArtilleryModeAbilityIds']['source']
        ids=json.loads(args[0]['parameters_json'])
        normal=str(ids['normal_ability_id']);alternate=str(ids['alternate_mode_ability_id'])
        args[5]['setArtilleryModeAbilityIds']['source']=body.replace('dBb='+normal,'dBb=SWAP').replace('dBb='+alternate,'dBb='+normal).replace('dBb=SWAP','dBb='+alternate)
        with self.assertRaises(ValueError):project(*args)
    def test_secondary_resources_and_native_duration_are_independent(self):
        args=copy.deepcopy(self.args)
        child=json.loads(args[0]['parameters_json'])['alternate_mode_ability_id'].to_bytes(4,'big').decode()
        cost=next(r for r in args[4] if r['rawcode']==child and r['field']=='mana_cost')
        dummy=self.evidence['starfall_spellcasting']['ability']['effect']['Starfall']['dummy_mana']
        cost['runtime_value']=str(dummy+1)
        with self.assertRaises(ValueError):project(*args)
        args=copy.deepcopy(self.args)
        args[1][child]['ahdu:1']=0
        with self.assertRaises(ValueError):project(*args)
