import unittest
from copy import deepcopy
from project_training_variants import project


class TrainingVariantProjection(unittest.TestCase):
    def fixture(self):
        raw = lambda code: int.from_bytes(code.encode(), 'big')
        groups = [f'{name}_o=GetRandomInt(0,9)if({name}_o<2)then I_o=C_o addProtectedAbility(I_o,{raw("G001")})end'
            for name in 'DEF']
        groups.append(f'if(GetRandomInt(0,9)<3)then I_o=C_o addProtectedAbility(I_o,{raw("G002")})end')
        groups.extend(f'{name}_o=GetRandomInt(0,9)if({name}_o<4)then I_o=C_o addProtectedAbility(I_o,{raw("G003")})end'
            for name in 'GH')
        return [{'onUnitTrained': f'if(unit_getAbilityLevel(cgs,{raw("MARK")})>0)then cgs=randomizeBloodFiend(cgs)elseif',
            'rollBody': 'A_o=GetRandomInt(0,11)if(A_o<2)then return replaceUnitWithType(z_o,qAb)elseif(A_o<5)then return replaceUnitWithType(z_o,pAb)end return replaceUnitWithType(z_o,__wurst_ensureInt(oAb[GetRandomInt(0,1)]))',
            'EE': f'qAb={raw("B001")} pAb={raw("B002")} oAb[0]={raw("B003")} oAb[1]={raw("B004")}',
            'randomizeBloodFiend': 'C_o=rollBody(B_o)' + ''.join(groups) + 'return C_o',
            'replaceUnitWithType': '__wurst_safe_ShowUnit(x_o,false)pFb=u_o __wurst_safe_CreateUnit(unit_getOwner(u_o),v_o,unit_getX(u_o),unit_getY(u_o),angle_degrees(unit_getFacingAngle(u_o)))y_o=u_o __wurst_safe_RemoveUnit(y_o)'},
            {'ROOT': {'abilities': 'MARK'}, **{code: {'abilities': ''} for code in ['B001', 'B002', 'B003', 'B004']}},
            {'G001': 'ACce', 'G002': 'ANde', 'G003': 'ACev'}]

    def test_cold_body_selection_and_each_independent_grant_branch_retain_source_identity(self):
        profile = project(*self.fixture())
        self.assertEqual((profile['body_minimum'], profile['body_maximum']), (0, 11))
        self.assertEqual([body['threshold_exclusive'] for body in profile['rare_bodies']], [2, 5])
        self.assertEqual(len(profile['common_bodies']), 2)
        self.assertEqual(len(profile['grant_groups']), 6)
        self.assertEqual(len(profile['grant_sources']), 3)
        self.assertEqual(profile['grant_sources'][1]['kind'], 'ability-effect')

    def test_changed_replacement_owner_removal_grant_recipient_or_hidden_control_flow_rejected(self):
        for mutate in [
            lambda f: f.update(replaceUnitWithType=f['replaceUnitWithType'].replace('unit_getOwner(u_o)', 'GetTriggerPlayer()')),
            lambda f: f.update(replaceUnitWithType=f['replaceUnitWithType'].replace('RemoveUnit', 'KillUnit')),
            lambda f: f.update(randomizeBloodFiend=f['randomizeBloodFiend'].replace('I_o=C_o', 'I_o=B_o', 1)),
            lambda f: f.update(randomizeBloodFiend=f['randomizeBloodFiend'].replace('return C_o', '__wurst_safe_KillUnit(C_o)return C_o')),
        ]:
            args = deepcopy(self.fixture())
            mutate(args[0])
            with self.assertRaises(ValueError): project(*args)
