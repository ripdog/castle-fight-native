import copy
import unittest

from project_air_attack_bonus import project


class AirAttackBonusTests(unittest.TestCase):
    def fixture(self):
        raw = int.from_bytes(b'BODY', 'big')
        rows = [{'mechanic_kind': 'attack-damage-flying-target-multiplier', 'unit_rawcode': 'BODY',
            'parameters_json': '{"bonus_fraction":0.125,"damage_multiplier":1.125,"required_damage_event_type":0,"applies_to_any_qualifying_damage_recipient":true,"modifies_current_damage_instance":true,"requires_positive_damage":true,"target_requires_flying":true}'}]
        functions = {
            'hL': f'function hL()local TDr=nil u0={raw} q0=0.125 TDr.DamageListener_unreduced=false DamageEvent_addListener(TDr)end',
            'DamageEvent_addListener1': 'doq.DamageListener_priority=coq',
            'DamageEvent_addListener': 'return DamageEvent_addListener1(0,boq)',
            'shouldApplyRiptideAirBonus': 'function shouldApplyRiptideAirBonus(UDr,VDr,WDr,XDr)return((((UDr==u0)and VDr)and(WDr==0))and(XDr>0.))end',
            'addRiptideAirDamageBonus': 'function addRiptideAirDamageBonus(YDr)return(YDr+(YDr*q0))end',
            'DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack': (
                'function DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack(qCn)'
                'local rCn=nil local sCn=nil local tCn=nil local uCn=nil '
                'rCn=DamageEvent_getSource()sCn=DamageEvent_getTarget()tCn=DamageEvent_getAmount()'
                'if((not(rCn==nil))and(not(sCn==nil)))then if '
                'shouldApplyRiptideAirBonus(unit_getTypeId(rCn),unit_isType(sCn,UNIT_TYPE_FLYING),DamageEvent_getType(),tCn)'
                'then uCn=addRiptideAirDamageBonus(tCn)DamageInstance_DamageInstance_setAmount(khb,uCn)end end end'),
        }
        return rows, functions

    def test_projection_uses_script_fraction_and_source_identity(self):
        rows, functions = self.fixture()
        self.assertEqual(project(rows, functions), [{'rawcode': 'BODY', 'bonus_per_10k': 1250}])
        self.assertEqual(project([], functions), [])

    def test_rejects_unreduced_spell_zero_damage_or_first_recipient_shortcuts(self):
        rows, functions = self.fixture()
        mutations = [
            ('hL', 'unreduced=false', 'unreduced=true'),
            ('DamageEvent_addListener', '(0,boq)', '(1,boq)'),
            ('shouldApplyRiptideAirBonus', 'WDr==0', 'WDr==1'),
            ('shouldApplyRiptideAirBonus', 'XDr>0.', 'XDr>=0.'),
            ('addRiptideAirDamageBonus', 'YDr+(YDr*q0)', 'YDr*q0'),
            ('DamageListener_addListener_RiptideAttack_onEvent_addListener_RiptideAttack', 'then uCn=', 'and isFirstHit()then uCn='),
        ]
        for key, old, new in mutations:
            with self.subTest(key=key, mutation=new):
                changed = copy.copy(functions)
                changed[key] = changed[key].replace(old, new)
                with self.assertRaises(ValueError):
                    project(rows, changed)


if __name__ == '__main__':
    unittest.main()
