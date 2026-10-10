import json
import unittest
from project_unit_death_rules import project

class DeathRuleProjectionTest(unittest.TestCase):
    def test_retaliation_is_source_owned_and_precedes_killer_heal(self):
        raw = int.from_bytes(b'UNIT', 'big')
        retaliation = {'damage_source':'dying-unit','damage_target':'killer','attack_type':'chaos', 'damage_type':'normal','is_attack':True,'is_ranged':False,'dying_unit_requires_combat_sapper':True,'killer_required':True,'killer_must_not_be_structure':True,'damage':23}
        heal = {'implementation':'set-life-percent-current-plus-delta', 'victim_requires_combat_sapper':True,'killer_must_not_be_structure':True,'heal_percent_of_max_hp':17}
        rows = [{'unit_rawcode':'UNIT','mechanic_kind':kind,'parameters_json':json.dumps(p)} for kind,p in [('death-retaliation-damage-to-killer',retaliation),('kill-heal-percent-max-hp',heal)]]
        source = f'if isCombatSapper(M2q)then if(not(N2q==nil))then if(not unit_isType(N2q,UNIT_TYPE_STRUCTURE))then if(unit_getTypeId(M2q)=={raw})then __wurst_safe_UnitDamageTarget(M2q,N2q,23.,true,false,ATTACK_TYPE_CHAOS,DAMAGE_TYPE_NORMAL,WEAPON_TYPE_WHOKNOWS)end R2q=unit_getTypeId(N2q)if(R2q=={raw})then native(N2q,(__wurst_safe_GetUnitLifePercent(N2q)+17.))end'
        self.assertEqual(project(rows, {'fJ':source}), [{'rawcode':raw,'heal_percent':17,'retaliation_damage':23}])
        for changed in ['UNIT_TYPE_STRUCTURE','ATTACK_TYPE_CHAOS','end R2q=unit_getTypeId(N2q)','GetUnitLifePercent(N2q)']:
            with self.assertRaises(ValueError):project(rows, {'fJ':source.replace(changed,'changed')})
