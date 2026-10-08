import importlib.util
import json
import unittest
from pathlib import Path
from unittest import mock

SPEC = importlib.util.spec_from_file_location(
    "building_mechanics", Path(__file__).with_name("build_building_mechanics.py")
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BuildingMechanicsProjectionTests(unittest.TestCase):
    def test_complete_artifact_is_reproducible_from_the_registered_tree(self):
        expected = json.loads(MODULE.OUT.read_text())
        with mock.patch.object(
            MODULE, "_retained_file_bytes", wraps=MODULE._retained_file_bytes
        ) as read_source:
            self.assertEqual(MODULE.build(), expected)
        self.assertTrue(read_source.call_args_list)
        self.assertTrue(all(
            call.args[1] == expected["extraction_git_tree"]
            for call in read_source.call_args_list
        ))

    def test_targeted_native_child_and_delayed_animations_retain_script_provenance(self):
        import re
        data = MODULE.build()
        row = data["serpent"]
        child = json.loads(row["parameters_json"])["dummy_ability_id"].to_bytes(4, "big").decode()
        linked = json.loads(row["effect_objects_json"])[0]
        self.assertEqual(child, linked["rawcode"])
        fields = data["fields"][child]
        for native, extracted in (("aare:1", "area"), ("aran:1", "range"),
                                  ("adur:1", "duration_normal"), ("ahdu:1", "duration_hero")):
            self.assertEqual(float(fields[native]), float(linked["ability_level1"][extracted]))
        self.assertEqual(fields["atar:1"], linked["ability_level1"]["targets"])
        traces = data["script_traces"]
        attack = re.search(r"doAfter\(([\d.]+),", traces["serpentRockShoot"]["source"])[1]
        stand_callback = traces["CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities3"]["source"]
        stand = re.search(r"doAfter\(([\d.]+),", stand_callback)[1]
        self.assertEqual(data["serpent_animation_delays"], {"attack": attack, "stand": stand})
        self.assertIn('"attack"', stand_callback)
        self.assertIn('"stand"', traces["CallbackSingle_doAfter_doAfter_RaceOrcAbilities_call_doAfter_doAfter_RaceOrcAbilities"]["source"])
        self.assertIn("randomAliveEnemy", traces["serpentRockShoot"]["source"])
        self.assertIn("isAliveCombatSapper", traces["ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers"]["source"])

    def test_permanent_grants_retain_literal_add_order_options_retry_gate_and_art(self):
        import re
        data = MODULE.build()
        params = json.loads(data['ceremonial']['parameters_json'])
        source = data['script_traces']['ceremonialTotemSpell']['source']
        calls = list(map(int, re.findall(r'addProtectedAbility\(\w+,(\d+)\)', source)))
        self.assertEqual(calls, params['always_ability_ids'] +
                         [value for option in params['option_ability_ids'] for value in option])
        self.assertLess(source.index('GetRandomInt'), source.index('ceremonialPick'))
        self.assertRegex(source, r'ceremonialPick\(\w+,false\).*ceremonialPick\(\w+,true\)')
        predicate = data['script_traces']['isCeremonialTotemTarget']['source']
        gate = int(re.findall(r'unit_getAbilityLevel\(\w+,(\d+)\)', predicate)[-1])
        self.assertEqual(data['ceremonial_retry_marker'], gate)
        code = gate.to_bytes(4, 'big').decode()
        self.assertIn(code, data['fields'])
        self.assertNotIn(gate, calls)
        self.assertIn('isAliveCombatSapper', predicate)
        self.assertIn('unit_isAllyOf', predicate)
        self.assertIn('Zcs or ads', data['script_traces']['passesRetryTargetGate']['source'])
        for identity in calls:
            self.assertIn(identity.to_bytes(4, 'big').decode(), data['fields'])
        target, caster = data['ceremonial_cast_visuals']
        self.assertEqual([target['role'], caster['role']], ['target', 'caster'])
        self.assertIn(target['attachment'], source)
        self.assertEqual([target['path'], caster['path']], data['script_art']['ceremonialTotemSpell'])

    def test_native_trap_closure_keeps_separate_clocks_hidden_identity_and_type_caps(self):
        import re
        data = MODULE.build()
        row = data['stasis']
        params = json.loads(row['parameters_json'])
        child = params['effect_ability_id'].to_bytes(4, 'big').decode()
        fields = data['fields'][child]
        control = data['stasis_control']
        self.assertEqual(control['hidden_ward_rawcode'], fields['Stau:1'])
        callback = data['script_traces'][row['source_functions']]['source']
        self.assertIn(str(params['effect_ability_id']), callback)
        self.assertEqual(float(control['dummy_seconds']), float(params['carrier_or_dummy_lifetime_seconds']))
        self.assertIn('unit_getPos', callback)
        self.assertIn('false', data['script_traces']['code__onEnter_doAfter_StasisTrap']['source'])
        self.assertIn('"stand"', data['script_traces']['CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities']['source'])
        limits = data['building_type_limits']
        predicate = data['script_traces']['isSingleLimitBuilding']['source']
        self.assertEqual(set(limits), {int(n).to_bytes(4, 'big').decode() for n in re.findall(r'==([0-9]+)', predicate)})
        self.assertNotIn('h02O', limits)
        linked = json.loads(row['effect_objects_json'])[0]['ability_level1']
        self.assertEqual(fields['atar:1'], linked['targets'])
        self.assertEqual(str(fields['ahdu:1']), linked['duration_hero'])

    def test_unsupported_version_does_not_reuse_an_older_recipe(self):
        with self.assertRaisesRegex(ValueError, "do not support"):
            MODULE.build("9.32")

    def test_missing_retained_tree_is_rejected_before_any_projection(self):
        with mock.patch.object(
            MODULE, "_load_release", return_value={"extraction": {"git_tree": None}}
        ), mock.patch.object(MODULE, "_retained_file_bytes") as read_source:
            with self.assertRaisesRegex(ValueError, "retained extraction"):
                MODULE.build()
            read_source.assert_not_called()


if __name__ == "__main__":
    unittest.main()
