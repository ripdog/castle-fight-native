import copy
import json
import unittest
from pathlib import Path

from project_production_replication import project


class ProductionReplicationProjectionTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.data = json.loads((Path(__file__).resolve().parents[2] /
            'crates/sim/data/castle-fight/9.27/building-mechanics-r1.json').read_text())

    def test_generated_profile_retains_registration_and_training_control_flow(self):
        self.assertEqual(project(self.data['replication_source'], self.data['script_traces']),
                         self.data['production_replication'])

    def test_owner_setup_roll_and_marker_changes_are_rejected(self):
        mutations = [
            ('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning', 'CreateUnit(gYl,', 'CreateUnit(nil,'),
            ('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning', 'setupUnit(hYl,fYl)', 'onUnitTrained()'),
            ('spawnSyncedCompanions', 'unit_getOwner(bgs)', 'unit_getOwner(Wfs)'),
            ('spawnSyncedCompanions', 'GetRandomInt(0,99)<eR', 'GetRandomInt(0,99)<=eR'),
            ('spawnSyncedCompanions', 'Yfs=min(group_size(vGb[Xfs]),fR)', 'Yfs=group_size(vGb[Xfs])'),
            ('kP', f'eR={self.data["production_replication"]["chance_per_100"]}',
                   f'eR={self.data["production_replication"]["chance_per_100"] + 1}'),
            ('onUnitTrained', '<=0)and unit_isType', '>0)and unit_isType'),
            ('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning',
             f'CreateUnit(gYl,{self.data["production_replication"]["twins_children"][0]},',
             f'CreateUnit(nil,{self.data["production_replication"]["twins_children"][0]},'),
            ('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning',
             'setupUnit(jYl,fYl)', 'onUnitTrained()'),
            ('EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning',
             'setupUnit(iYl,fYl)', '__wurst_safe_RemoveUnit(eYl)setupUnit(iYl,fYl)'),
            ('spawnSyncedCompanions', 'cR=(cR+1)',
             f'cR=(cR+1)unit_getAbilityLevel(Vfs,{self.data["production_replication"]["twins_marker"]})'),
        ]
        for function, old, new in mutations:
            with self.subTest(function=function, mutation=new):
                traces = copy.deepcopy(self.data['script_traces'])
                self.assertIn(old, traces[function]['source'])
                traces[function]['source'] = traces[function]['source'].replace(old, new)
                with self.assertRaises(ValueError):
                    project(self.data['replication_source'], traces)


if __name__ == '__main__':
    unittest.main()
