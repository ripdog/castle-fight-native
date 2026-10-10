from __future__ import annotations

import json
import unittest
from pathlib import Path

import build_native_tuning as native
import build_runtime_catalog as catalog


class NativeTuningProjectionTest(unittest.TestCase):
    def test_scripted_roots_reconciles_parent_child_resources_and_origin(self):
        recipe = {"kind":"scripted-roots", "source_kind":"unit-ability", "source_key":"ORDR", "unit_rawcode":"UNIT"}
        fields = {"targs1":"enemies,ground,neutral,nonhero,vulnerable", "cost1":"999", "cool1":"999", "rng1":"71"}
        child = {"targs1":"enemies,ground,neutral", "cost1":"999", "cool1":"999", "dur1":"3", "herodur1":"0.15", "dataa1":"7", "rng1":"91"}
        unit = {"mana_max":"19", "mana_start":"11", "mana_regen":"0.2"}
        site = {"effect_rawcodes":["ROOT"], "callee":"dummyCastTargetFrom1", "arguments":["unit_getOwner(caster)","ROOT","order","target","unit_getPos(target)","4."]}
        detail = {"native_base":"ACpa", "all_bases":{"ROOT":"AEer"}, "effect_key":"ROOT", "effect_fields":child, "effect_protected":{"mana_cost":"0","cooldown":"0"}, "map_version":{"major":9,"minor":27}, "git_tree":"synthetic", "mechanics_row":{"mechanic_kind":"dummy-target-ability-from-owner","handler_function":"roots"}, "semantics_row":{"parameters_json":json.dumps({"delivery_sites":[site]})}}
        ability = native.project_effect(recipe, fields, unit, {"mana_cost":"5","cooldown":"2"}, detail)["spellcasting"]["ability"]
        self.assertEqual((ability["mana_cost"], ability["cooldown_ticks"]), (5,60))
        roots = ability["effect"]["ScriptedRoots"]
        self.assertEqual(roots["roots"]["hero_duration_ticks"], 5)
        self.assertTrue(roots["primary_nonhero_only"])
        self.assertFalse(roots["roots"]["nonhero_only"])
        self.assertLess(ability["range"], roots["child_range"])
        for changes in ({"effect_protected":{"mana_cost":"1","cooldown":"0"}}, {"native_base":"AEer"}, {"effect_fields":{**child,"targs1":"enemies,air,ground"}}, {"semantics_row":{"parameters_json":json.dumps({"delivery_sites":[{**site,"callee":"dummyCastTargetFrom"}]})}}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                native.project_effect(recipe, fields, unit, {}, {**detail,**changes})

    def test_bounded_roots_projects_selection_before_order_and_conditional_vision(self):
        recipe = {"kind":"scripted-roots","source_kind":"unit-ability","source_key":"ORDR","unit_rawcode":"UNIT"}
        fields = {"targs1":"enemies,ground,neutral,nonhero,vulnerable","cost1":"0","cool1":"1","rng1":"17"}
        child = {"targs1":"enemies,ground,neutral","cost1":"0","cool1":"0","dur1":"2","herodur1":"0.1","dataa1":"7","rng1":"23"}
        helper = 'Dwr=unit_getOwner(caster)Ewr=tupleCopy1(unit_getPos(primary))forUnitsInRange(Nwr,19.,false,Owr)LinkedList_LinkedList_add(Fwr,__wurst_objectToIndex(Jwr))Kwr=2 Lwr=LinkedList Kwr=(Kwr-1)dummyCastTargetWithVision('
        candidates = 'isAliveCombatSapper(hwn)unit_isEnemyOf(hwn,gwn.p)UNIT_TYPE_GROUND unit_getAbilityLevel(hwn,101)<=0 unit_getAbilityLevel(hwn,102)<=0 unit_getAbilityLevel(hwn,103)<=0 unit_getAbilityLevel(hwn,104)<=0 LinkedList_LinkedList_add(gwn.priority LinkedList_LinkedList_add(gwn.fallback'
        vision = 'if player_hasVisibility(kTr,oTr)then return dummyCastTargetFrom2(FOG_OF_WAR_VISIBLE rTr[2],13.,true,false doAfter(0.15,uTr)'
        functions = {'assassinSpell':helper,'ForGroupCallback_forUnitsInRange_RaceNelfAbilities_callback_forUnitsInRange_RaceNelfAbilities':candidates,'dummyCastTargetWithVision1':vision}
        trace = {"max_targets":2,"search_radius":19,"delivery_sites":[{"effect_rawcodes":["ROOT"],"callee":"dummyCastTargetWithVision","arguments":["Dwr","ROOT","order","target","Ewr","3."]}]}
        detail = {"native_base":"ACpa","all_bases":{"ROOT":"AEer"},"effect_key":"ROOT","effect_fields":child,"effect_protected":{},"map_version":{"major":1,"minor":2},"git_tree":"synthetic","mechanics_row":{"mechanic_kind":"delegated-helper","handler_function":"cast"},"semantics_row":{"parameters_json":json.dumps(trace)},"functions":functions}
        unit = {"mana_max":"-","mana_start":"-","mana_regen":"-"}
        profile = native.project_effect(recipe,fields,unit,{},detail)['spellcasting']['ability']['effect']['ScriptedRoots']
        self.assertEqual(profile['maximum_targets'],2)
        self.assertEqual(profile['excluded_abilities'],[101,102])
        self.assertEqual(profile['priority_buffs'],[103,104])
        self.assertEqual((profile['vision_radius'],profile['vision_ticks']),(13*1024,5))
        for key, token in [('assassinSpell','Kwr=(Kwr-1)'),('assassinSpell','unit_getOwner('),('dummyCastTargetWithVision1','if player_hasVisibility(kTr,oTr)'),('ForGroupCallback_forUnitsInRange_RaceNelfAbilities_callback_forUnitsInRange_RaceNelfAbilities','UNIT_TYPE_GROUND')]:
            with self.subTest(token=token),self.assertRaises(ValueError):
                native.project_effect(recipe,fields,unit,{}, {**detail,'functions':{**functions,key:functions[key].replace(token,'changed')}})

    def test_aftershock_requires_health_formula_and_disabled_nested_dispatch(self):
        recipe = {"kind":"health-scaled-aftershock", "source_kind":"unit-ability", "source_key":"TEST", "unit_rawcode":"UNIT"}
        trace = {"aftershock_marker_ability_id":1,"source_damage_marker_ability_id":2,
            "bonus_damage_formula":"17 * unit_level * current_hp / max_hp", "object_unit_level":3,"unit_level":3,
            "maximum_bonus_damage_at_full_hp":51,"recursion_guard_trigger_disabled_during_bonus_hit":True,
            "attack_type":"siege","damage_type":"demolition","is_attack":True,"is_ranged":False}
        handler = "unit_getAbilityLevel(Qpq,2)<=0 elseif(unit_getAbilityLevel(Qpq,1)>0) dqq=hhb __wurst_safe_DisableTrigger(dqq) (((17.*unit_getLevel(Qpq))*widget_getLife(Qpq))/unit_getMaxHP(Qpq)) true,false,ATTACK_TYPE_SIEGE,DAMAGE_TYPE_DEMOLITION,WEAPON_TYPE_WHOKNOWS eqq=hhb __wurst_safe_EnableTrigger(eqq)"
        detail = {"aftershock":trace,"aftershock_handler":handler,"damage_runtime":"handleSourceDamageEffects(sqq,tqq)handleTargetDamageEffects(sqq,tqq)",
            "sapper_registration":"if(not isCombatSapper(ocs))then return end registerSapperDamage(ocs)",
            "damage_registration":"TriggerRegisterUnitEvent(hhb,vqq,EVENT_UNIT_DAMAGED)","git_tree":"synthetic"}
        result = native.project_effect(recipe, {}, None, {}, detail)["profile"]
        self.assertEqual(result, {"marker":1,"dispatcher":2,"maximum_damage":51})
        for replacement in ["unit_getLevel(Qpq)","widget_getLife(Qpq)","__wurst_safe_DisableTrigger(dqq)","ATTACK_TYPE_SIEGE"]:
            with self.assertRaises(ValueError):
                native.project_effect(recipe, {}, None, {}, {**detail,"aftershock_handler":handler.replace(replacement,"changed")})

    def test_lava_split_projects_generation_limit_healing_and_converted_child(self):
        recipe = {"kind":"lava-spawn", "source_kind":"unit-ability", "source_key":"TEST", "unit_rawcode":"UNIT"}
        fields = {"unitid1":"BODY", "dataa1":"1", "dur1":"0", "datae1":"0", "datac1":"7",
            "datab1":"1.5", "dataf1":"4", "datad1":"0.3", "targs1":"enemy,neutral,air,ground,ward"}
        trace = {"runtime_combat_unit_rawcode":"BODY", "runtime_combat_unit_id":1,
            "nested_h030_child_replacement_unit_id":2, "nested_h030_child_replace_method":2,
            "trained_proxy_is_gameplay_combat_body":False, "remove_summoned_type_from_h030_child":True,
            "post_child_attack_order_delay_seconds":0.2}
        detail = {"native_base":"ANlm", "lava_spawn":trace, "git_tree":"synthetic"}
        result = native.project_effect(recipe, fields, None, {"mana_cost":"0"}, detail)["profile"]
        self.assertEqual((result["maximum_splits"], result["healing_per_10k"], result["split_delay_ticks"], result["child_rawcode"]), (3,3000,45,2))
        for changes in ({"dur1":"1"}, {"datae1":"1"}, {"dataa1":"2"}, {"targs1":"enemy,ground,structure"}):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, **changes}, None, {"mana_cost":"0"}, detail)

    def test_native_cone_preserves_distinct_geometry_cap_and_speed_and_rejects_ignition(self):
        recipe = {"kind":"cone-wave", "source_kind":"ability-effect", "source_key":"TEST"}
        fields = {"dataa1":"19", "datab1":"53", "datac1":"100", "datad1":"12",
            "area1":"3", "missilespeed":"300", "datae1":"0", "targs1":"air,enemies,ground"}
        detail = {"native_base":"ACbc"}
        protected = {"mana_cost":"0", "cooldown":"0.01"}
        result = native.project_effect(recipe, fields, None, protected, detail)["profile"]
        self.assertEqual((result["damage"], result["maximum_damage"], result["initial_radius"], result["final_radius"], result["speed_per_tick"]), (19,53,3*1024,12*1024,10*1024))
        for changes in ({"datae1":"1"}, {"targs1":"enemies,ground,structure"}):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, **changes}, None, protected, detail)

    def test_mirror_image_reconciles_script_order_with_protected_native_fields(self):
        recipe = {"kind":"mirror-image", "source_kind":"unit-ability", "source_key":"TEST", "unit_rawcode":"UNIT"}
        fields = {"cost1":"9999", "cool1":"99", "datad1":"0.2", "rng1":"51", "missilespeed":"300",
            "dur1":"7", "dataa1":"2", "datab1":"0.4", "datac1":"1.5"}
        trace = {"proc_chance_percent":17, "mirror_image_order_id":852123}
        handler = 'function randomN02YOrder(upq)if(GetRandomInt(0,99)<17)then jY=true unit_issueImmediateOrderById(upq,852123)end end'
        detail = {"mirror_image":trace, "mirror_image_handler":handler, "native_base":"AOmi", "git_tree":"synthetic"}
        output = native.project_effect(recipe, fields, None, {"mana_cost":"0", "cooldown":"2"}, detail)["profile"]
        self.assertEqual((output["split_ticks"], output["cooldown_ticks"], output["images"]), (12, 60, 2))
        for changed in (handler.replace('<17','<=17'), handler.replace('jY=true ',''), handler.replace('852123','852124')):
            with self.assertRaisesRegex(ValueError, "control flow"):
                native.project_effect(recipe, fields, None, {"mana_cost":"0"}, {**detail,"mirror_image_handler":changed})


    def test_sphere_uses_protected_cooldown_and_independent_native_visual_speed(self):
        recipe = {"kind":"sphere", "source_kind":"unit-ability", "source_key":"TEST", "unit_rawcode":"UNIT"}
        fields = {"targetattachcount":"3", "cool1":"99", "missilespeed":"450"}
        profile = native.project_effect(recipe, fields, None, {"cooldown":"0.11"}, {"native_base":"Asph"})["profile"]
        self.assertEqual((profile["attachment_count"], profile["cooldown_ticks"], profile["speed_per_tick"]), (3,4,15*1024))
        for count in ('0', '4'):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, "targetattachcount":count}, None, {}, {"native_base":"Asph"})

    def test_speed_aura_projects_signed_native_payload_and_rejects_unsupported_semantics(self):
        recipe = {"kind":"speed-aura", "source_kind":"unit-ability", "source_key":"TEST", "unit_rawcode":"UNIT"}
        fields = {"targs1":"air,enemies,ward", "dur1":"0", "herodur1":"0", "buffid1":"BUFF", "area1":"71", "dataa1":"-0.17", "datab1":"-0.29"}
        profile = native.project_effect(recipe, fields, None, {}, {"native_base":"AOae"})["profile"]
        self.assertEqual((profile["movement_percent_delta"], profile["attack_speed_percent_delta"]), (-17, -29))
        self.assertEqual(profile["radius"], 71 * 1024)
        for changes in ({"dur1":"1"}, {"targs1":"air,ground,enemies,ward"}, {"buffid1":"BUFF,TEST"}):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, **changes}, None, {}, {"native_base":"AOae"})

    def test_native_damage_aura_retains_weapon_eligibility_and_linger(self) -> None:
        recipe = {"kind": "native-aura", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"targs1": "air,ground,friend,self,vuln,invu", "area1": "77", "dataa1": "0.17", "datab1": "1", "datac1": "0", "datad1": "0", "dur1": "0.25"}
        output = native.project_effect(recipe, fields, None, {}, {"native_base": "AIar"})
        self.assertEqual((output["damage_bonus_per_10k"], output["affects_melee"], output["affects_ranged"], output["linger_ticks"]), (1700, True, False, 8))
        with self.assertRaisesRegex(ValueError, "weapon classes"):
            native.project_effect(recipe, {**fields, "datad1": "1"}, None, {}, {"native_base": "AIar"})

    def test_poison_projects_class_mask_short_hero_expiry_and_nonlethal_damage_stacking(self) -> None:
        recipe = {"kind": "poison", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"targs1": "air,ground,organic", "dataa1": "19", "datab1": "0.15", "datac1": "0.1", "datad1": "1", "dur1": "4", "herodur1": "0.25", "buffid1": "BUFF,SLOW"}
        profile = native.project_effect(recipe, fields, None, {}, {"native_base": "ACvs"})["profile"]
        self.assertEqual(profile["buff_rawcode"], int.from_bytes(b"SLOW", "big"))
        for base in ("Aven", "ACvs"):
            projected = native.project_effect(recipe, {**fields, "buffid1": "BUFF"}, None, {}, {"native_base": base})
            self.assertEqual(projected["profile"]["buff_rawcode"], int.from_bytes(b"BUFF", "big"))
            self.assertEqual(projected["provenance"]["native_base"], base)
        self.assertTrue(profile["stack_damage"])
        self.assertFalse(profile["can_kill"])
        self.assertEqual((profile["hero_duration_ticks"], profile["movement_percent_delta"], profile["attack_speed_percent_delta"]), (8, -10, -15))
        for override in ({"datad1": "3"}, {"targs1": "air,ground,mechanical"}):
            with self.assertRaisesRegex(ValueError, "organic mask"):
                native.project_effect(recipe, {**fields, **override}, None, {}, {})

    def test_damage_controller_preserves_native_grant_vs_failed_level_assignment(self) -> None:
        recipe = {"kind": "damage-threshold", "source_kind": "ability-effect", "source_key": "BONU", "unit_rawcode": "UNIT"}
        handler = f"unit_getTypeId(o8n)=={int.from_bytes(b'UNIT', 'big')} p8n<0.4)then SetUnitAbilityLevel(a,b,3) SetUnitVertexColor(a,77,88,99,255) elseif(p8n<0.6)then SetUnitAbilityLevel(a,b,2) SetUnitVertexColor(a,44,55,66,255) elseif(p8n<0.8)then addProtectedAbility(a,b) SetUnitVertexColor(a,11,22,33,255) end end end end"
        thresholds = [{"ability_level": level, "attack_bonus": bonus, "hp_ratio_below": ratio, "vertex_rgba": color}
            for level, bonus, ratio, color in [(1, 7, 0.8, [11,22,33,255]), (2, 13, 0.6, [44,55,66,255]), (3, 29, 0.4, [77,88,99,255])]]
        mechanics = {"damage_threshold": {"bonus_ability_rawcode": "BONU", "levels_only_increase": True,
            "healing_does_not_downgrade_reached_level": True, "thresholds": thresholds},
            "damage_threshold_handler": handler, "ability_levels": {"dataa1": "7", "dataa2": "13", "dataa3": "29"}, "git_tree": "synthetic"}
        output = native.project_effect(recipe, {}, None, {}, mechanics)
        self.assertEqual([row["adds_if_absent"] for row in output["thresholds"]], [True, False, False])
        with self.assertRaisesRegex(ValueError, "disagrees"):
            native.project_effect(recipe, {}, None, {}, {**mechanics, "ability_levels": {"dataa1": "7", "dataa2": "13", "dataa3": "31"}})

    def test_net_projects_separate_trigger_range_native_flight_and_hero_duration(self) -> None:
        recipe = {"kind": "ensnare", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"rng1": "100", "cost1": "9999", "cool1": "99"}
        child = {"rng1": "200", "dur1": "4", "herodur1": "0.15", "missilespeed": "300", "missilehoming": "1",
            "dataa1": "0.1", "datab1": "80", "datac1": "60", "targs1": "air,enemies,mechanical,neutral,organic"}
        unit = {"mana_max": "-", "mana_start": "-", "mana_regen": "-"}
        mechanics = {"mechanics_row": {"target_mode_label": "enemy-flying-combat-sapper", "scheduled_delays_json": "[]",
            "mechanic_kind": "dummy-target-ability-from-caster"}, "semantics_row": {"parameters_json": json.dumps({"delivery_sites": [{"callee": "dummyCastTargetFrom"}]})},
            "effect_key": "NET1", "effect_fields": child}
        ability = native.project_effect(recipe, fields, unit, {"cooldown": "2", "mana_cost": "0"}, mechanics)["spellcasting"]["ability"]
        net = ability["effect"]["Ensnare"]["profile"]
        self.assertLess(ability["range"], net["range"])
        self.assertEqual((net["hero_duration_ticks"], net["lowering_ticks"], net["speed_per_tick"]), (5, 3, 10 * 1024))
        self.assertEqual((ability["mana_cost"], ability["cooldown_ticks"]), (0, 60))
        with self.assertRaisesRegex(ValueError, "delivery/mask"):
            native.project_effect(recipe, fields, unit, {}, {**mechanics, "effect_fields": {**child, "targs1": "ground,enemies"}})

    def test_kill_berserk_reconciles_death_handler_predicates_and_native_resources(self) -> None:
        recipe = {"kind": "kill-berserk", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"dur1": "7", "dataa1": "0.25", "datab1": "2", "datac1": "0.1", "cost1": "0", "cool1": "99"}
        trace = {"victim_requires_combat_sapper": True, "killer_must_not_be_structure": True,
            "resume_attack_immediately": True, "berserk_ability_id": int.from_bytes(b"TEST", "big"), "berserk_base_order": "berserk"}
        projected = native.project_effect(recipe, fields, None, {"cooldown": "0"}, {"kill_mechanics": trace})
        self.assertEqual(projected["duration_ticks"], 210)
        self.assertEqual(projected["damage_taken_bonus_per_10k"], 1000)
        with self.assertRaisesRegex(ValueError, "non-free orders"):
            native.project_effect(recipe, fields, None, {"cooldown": "0", "mana_cost": "1"}, {"kill_mechanics": trace})
        with self.assertRaisesRegex(ValueError, "death-handler predicates"):
            native.project_effect(recipe, fields, None, {"cooldown": "0"}, {"kill_mechanics": {**trace, "victim_requires_combat_sapper": False}})

    def test_native_transfer_projects_independent_geometry_resources_and_buff_provenance(self) -> None:
        recipe = {"kind": "spell-steal", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"targs1": "air,ground,enemies,friend,self,sapper,ward", "rng1": "120", "area1": "80", "cost1": "9999", "cool1": "99"}
        unit = {"mana_max": "20", "mana_start": "10", "mana_regen": "0.8"}
        profile = native.project_effect(recipe, fields, unit, {"mana_cost": "4", "cooldown": "3"}, {})["spellcasting"]
        self.assertEqual(profile["ability"]["mana_cost"], 4)
        self.assertEqual(profile["ability"]["cooldown_ticks"], 90)
        self.assertLess(profile["ability"]["effect"]["SpellSteal"]["recipient_radius"], profile["ability"]["range"])
        with self.assertRaisesRegex(ValueError, "native unit mask"):
            native.project_effect(recipe, {**fields, "targs1": fields["targs1"] + ",structure"}, unit, {}, {})
        buff_fields = {"buffid1": "BUFF", "targs1": "air,ground,friend,organic", "hero": "1"}
        identity = native.buff_identity(buff_fields, positive=True)
        self.assertFalse(identity["stealable"])
        self.assertTrue(identity["organic_only"])
        self.assertEqual(identity["rawcode"], int.from_bytes(b"BUFF", "big"))

    def test_native_stat_buff_projection_preserves_exact_mana_clock_and_rejects_extra_target_qualifiers(self) -> None:
        recipe = {"kind": "inner-fire", "source_kind": "unit-ability", "source_key": "TEST", "unit_rawcode": "UNIT"}
        fields = {"targs1": "air,ground,friend,neutral,self", "buffid1": "BUFF", "cost1": "4", "cool1": "9",
            "rng1": "120", "dataa1": "0.25", "datab1": "2", "datac1": "80", "datad1": "3",
            "dur1": "7", "herodur1": "2"}
        unit = {"mana_max": "20", "mana_start": "10", "mana_regen": "0.8"}
        effect = native.project_effect(recipe, fields, unit, {"cooldown": "3"}, {})
        mana = effect["spellcasting"]["mana"]
        self.assertEqual(mana["regen_per_tick_per_10k"], (1 << 31) | effect["mana_regen_per_second_per_10k"])
        buff = effect["spellcasting"]["ability"]["effect"]["StatBuff"]
        self.assertLess(buff["autocast_range"], effect["spellcasting"]["ability"]["range"])
        self.assertLess(buff["hero_duration_ticks"], buff["duration_ticks"])
        for qualifier in ("structure", "organic", "invulnerable"):
            with self.assertRaisesRegex(ValueError, "friendly unit mask"):
                native.project_effect(recipe, {**fields, "targs1": fields["targs1"] + "," + qualifier}, unit, {}, {})

    def test_damage_charge_preserves_signed_mana_and_rejects_changed_threshold_branch(self) -> None:
        marker, child = int.from_bytes(b"PARN", "big"), int.from_bytes(b"CHLD", "big")
        fields = {"targs1":"air,enemies,ward,structure,debris", "rng1":"0", "cool1":"0", "cost1":"0",
            "dataa1":"10", "datab1":"17", "datac1":"20", "datad1":"9", "datae1":"0.6", "dataf1":"1"}
        trace = {"charge_resource":"mana", "charge_delta":"event-damage-amount", "remove_damage_marker_on_threshold":True,
            "kaboom_order_id":852041, "kaboom_ability_id":child, "detonation_threshold":19, "detonation_move_speed":90}
        handler = (f'function vampireCharge(vpq,wpq)local xpq=nil local ypq=nil local zpq=nil '
            f'xpq=(unit_getState(vpq,UNIT_STATE_MANA)+wpq)if(xpq>=19.)then unit_removeAbility(vpq,{marker})'
            f'__wurst_safe_SetUnitMoveSpeed(vpq,90.)addProtectedAbility(vpq,{child})jY=true '
            'unit_issueImmediateOrderById(vpq,852041)else ypq=UNIT_STATE_MANA zpq=xpq __wurst_safe_SetUnitState(vpq,ypq,zpq)end end ')
        mechanics = {"damage_charge":trace, "damage_charge_handler":handler, "charge_child_fields":fields, "charge_child_base":"Asds"}
        unit = {"mana_max":"50", "mana_start":" - ", "mana_regen":"-1.5", "move_type":"fly"}
        projected = native.damage_charge_profile({"source_key":"PARN"}, unit, mechanics)
        self.assertEqual((projected["mana_starting"], projected["mana_regen_per_second_per_10k"]), (0, -15000))
        self.assertEqual(projected["explosion"]["targets"], 6)
        for changed in (handler.replace('852041', '852040'), handler.replace('jY=true', '__wurst_safe_SetUnitState(vpq,UNIT_STATE_MANA,xpq)jY=true')):
            with self.assertRaises(ValueError):
                native.damage_charge_profile({"source_key":"PARN"}, unit, {**mechanics,"damage_charge_handler":changed})
        with self.assertRaises(ValueError):
            native.self_destruct_profile("CHLD", {**fields,"targs1":"air,ground,enemies,structure"}, "Asds")

    def test_committed_tuning_is_a_projection_of_retained_evidence(self) -> None:
        root = Path(__file__).resolve().parents[2]
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        directory = root / release["runtime_content"]["path"]
        recipes = json.loads((directory / "native-effect-recipes.json").read_text())
        generated = native.build_tuning(release, root, recipes)
        self.assertEqual(json.loads((directory / "native-effect-tuning.json").read_text()), generated)

    def test_critical_strike_retains_structure_permission_without_flattening_classes(self) -> None:
        recipe = {"kind": "critical-strike", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "17", "datab1": "1.7", "targs1": "air,ground,enemies,structure"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual(effect["targets"], "air-ground-units-and-buildings")
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,enemies,structure,nonhero"}, None, {}, {})

    def test_cleave_projects_geometry_and_rejects_unrepresented_classes(self) -> None:
        recipe = {"kind": "cleave", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"area1": "123", "dataa1": "0.37", "targs1": "ground,enemy,structure"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["radius_world"], effect["damage_per_10k"], effect["targets"]), (123, 3700, 5))
        for mask in ("ground,air,enemy", "ground,enemy,organic", "ground,enemy,nonhero"):
            with self.subTest(mask=mask), self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, "targs1": mask}, None, {}, {})

    def test_frost_attack_retains_hero_duration_and_uses_supplied_map_misc(self) -> None:
        recipe = {"kind": "frost-attack", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dur1": "1.25", "herodur1": "0.1", "targs1": "air,ground"}
        misc = {"FrostMoveSpeedDecrease": "0.17", "FrostAttackSpeedDecrease": "0.23"}
        effect = native.project_effect(recipe, fields, None, {}, {"misc": misc})
        self.assertEqual((effect["duration_millis"], effect["hero_duration_millis"], effect["movement_percent_delta"], effect["attack_speed_percent_delta"]), (1250, 100, -17, -23))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,nonhero"}, None, {}, {"misc": misc})

    def test_pulverize_projects_distinct_full_and_half_radii_without_class_flattening(self) -> None:
        recipe = {"kind": "pulverize", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "17", "datab1": "31", "datac1": "41", "datad1": "73", "targs1": "ground,enemy"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["chance_per_10k"], effect["damage"], effect["full_radius_world"], effect["half_radius_world"]), (1700, 31, 41, 73))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "ground,enemy,nonhero"}, None, {}, {})

    def test_proxy_nova_retains_specific_and_area_damage_and_target_delivery(self) -> None:
        recipe = {"kind": "frost-nova", "source_key": "PARN", "source_kind": "unit-ability", "unit_rawcode": "UNIT"}
        parent = {"cost1": "0", "cool1": "1", "rng1": "8"}
        child = {"cost1": "0", "cool1": "0", "dataa1": "15", "datab1": "23", "area1": "7", "dur1": "0.4", "herodur1": "0.2", "targs1": "air,ground,enemies,neutral"}
        mechanics = {"effect_key": "CHLD", "effect_fields": child, "effect_protected": {},
            "mechanics_row": {"mechanic_kind": "dummy-target-ability-from-caster", "scheduled_delays_json": "[]"},
            "misc": {"FrostMoveSpeedDecrease": "0.17", "FrostAttackSpeedDecrease": "0.23"}}
        effect = native.project_effect(recipe, parent, {"mana_max": "20", "mana_start": "12", "mana_regen": "1.3"}, {}, mechanics)
        nova = effect["spellcasting"]["ability"]["effect"]["FrostNova"]
        self.assertEqual((nova["primary_damage"], nova["area_damage"], nova["duration_ticks"], nova["hero_duration_ticks"]), (23, 15, 12, 6))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, parent, {"mana_max": "20", "mana_start": "12", "mana_regen": "1.3"}, {}, {**mechanics, "mechanics_row": {"mechanic_kind": "dummy-immediate-ability-from-caster", "scheduled_delays_json": "[]"}})

    def test_fixed_point_conversion_is_exact_not_float_rounded(self) -> None:
        self.assertEqual(native.scaled("0.1234", 10_000), 1234)
        self.assertEqual(native.scaled("0.123", 1000), 123)
        for value in ("0.12345", "NaN", "Infinity"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                native.scaled(value, 10_000)

    def test_native_proc_projection_does_not_silently_drop_unsupported_targets(self) -> None:
        with self.assertRaises(ValueError):
            native.unit_targets("air,ground,enemy,structure")
        with self.assertRaises(ValueError):
            native.unit_targets("enemy,ward")

    def test_bash_keeps_hero_duration_and_rejects_unimplemented_native_fields(self) -> None:
        recipe = {"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "33", "datab1": "0", "datac1": "7", "datad1": "0", "datae1": "0", "dur1": "0.5", "herodur1": "0.25", "targs1": "air,ground"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["stun_duration_millis"], effect["hero_stun_duration_millis"]), (500, 250))
        for field in ("datab1", "datad1", "datae1"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                native.project_effect(recipe, {**fields, field: "1"}, None, {}, {})
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "targs1": "air,ground,nonhero"}, None, {}, {})

    def test_class_specific_orb_chances_require_explicit_native_coverage(self) -> None:
        recipe = {"kind": "orb-spell-proc", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"datab1": "10", "datac1": "20", "datad1": "10", "unitid1": "CHLD", "targs1": "ground"}
        with self.assertRaises(ValueError):
            native.project_effect(recipe, fields, None, {}, {})

    def test_feedback_projects_native_fields_and_rejects_unequal_class_tuning(self) -> None:
        recipe = {"kind": "feedback", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "7", "datab1": "0.5", "datac1": "7", "datad1": "0.5", "datae1": "3", "targs1": "air,ground,enemy"}
        effect = native.project_effect(recipe, fields, None, {}, {})
        self.assertEqual((effect["maximum_mana_drained"], effect["damage_per_mana_per_10k"], effect["summoned_damage"]), (7, 5000, 3))
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "dataa1": "8"}, None, {}, {})

    def test_faerie_fire_uses_protected_costs_and_keeps_distinct_hero_duration(self) -> None:
        recipe = {"kind": "faerie-fire", "source_key": "TEST", "source_kind": "unit-ability"}
        fields = {"dataa1": "2", "datab1": "1", "cost1": "999", "cool1": "99", "rng1": "123", "dur1": "13", "herodur1": "2.5", "targs1": "air,ground,enemy"}
        unit = {"mana_max": "50", "mana_start": "30", "mana_regen": "1.2"}
        effect = native.project_effect(recipe, fields, unit, {"mana_cost": "4", "cooldown": "3"}, {})
        self.assertEqual((effect["mana_cost"], effect["cooldown_millis"]), (4, 3000))
        self.assertEqual((effect["duration_millis"], effect["hero_duration_millis"]), (13000, 2500))
        self.assertEqual(effect["mana_regen_per_second_per_10k"], 12000)
        self.assertTrue(effect["always_autocast"])
        combat_only = native.project_effect(recipe, {**fields, "datab1": "0"}, unit, {}, {})
        self.assertFalse(combat_only["always_autocast"])
        with self.assertRaises(ValueError):
            native.project_effect(recipe, {**fields, "datab1": "2"}, unit, {}, {})

    def test_automatic_projection_keeps_exact_rate_and_native_delivery_fields(self) -> None:
        recipe = {"kind": "phoenix-fire", "source_key": "FIRE", "source_kind": "unit-ability", "unit_rawcode": "UNIT"}
        fields = {"dataa1": "7", "datab1": "3", "dur1": "4", "herodur1": "2",
                  "missilespeed": "700", "targs1": "ground,air,structure,enemies", "area1": "123", "cool1": "3", "cost1": "0"}
        for rate in ("1", "3.5", "0.0001"):
            unit = {"mana_max": "100", "mana_start": "17", "mana_regen": rate}
            projected = native.project_effect(recipe, fields, unit, {}, {})
            profile = projected["spellcasting"]
            self.assertEqual(profile["mana"]["regen_per_tick_per_10k"], (1 << 31) | native.scaled(rate, 10_000))
            self.assertEqual(profile["ability"]["effect"]["PhoenixFire"]["targets"], 7)
            self.assertEqual(profile["ability"]["effect"]["PhoenixFire"]["damage_per_second"], 3)
            self.assertEqual(profile["ability"]["range"], 123 * 1024)
        for rate in ("-1", "1000.0001", "214748.3648"):
            with self.subTest(rate=rate), self.assertRaises(ValueError):
                native.project_effect(recipe, fields, {"mana_max": "100", "mana_start": "17", "mana_regen": rate}, {}, {})

    def test_non_free_proxy_child_cannot_silently_bypass_native_resources(self) -> None:
        recipe = {"kind": "healing-wave", "source_key": "HEAL", "source_kind": "ability-effect", "unit_rawcode": "UNIT"}
        fields = {"dataa1": "7", "datab1": "2", "datac1": "0.25", "area1": "123", "cost1": "9999", "cool1": "99"}
        unit = {"mana_max": "100", "mana_start": "17", "mana_regen": "1"}
        mechanics = {"mechanics_row": {"scheduled_delays_json": '["0.8"]'}}
        for protected in ({}, {"mana_cost": "1", "cooldown": "0"}, {"mana_cost": "0", "cooldown": "1"}):
            with self.assertRaises(ValueError):
                native.project_effect(recipe, fields, unit, protected, mechanics)
        projected = native.project_effect(recipe, fields, unit, {"mana_cost": "0", "cooldown": "0"}, mechanics)
        self.assertEqual(projected["spellcasting"]["ability"]["mana_cost"], 0)
        self.assertEqual(projected["spellcasting"]["ability"]["effect"]["HealingWave"]["recovery_ticks"], 24)

    def test_recipes_cannot_become_a_second_tuning_database(self) -> None:
        root = catalog.REPO_ROOT
        release = catalog._load_release(catalog.DEFAULT_RELEASES, "9.27", "r1")
        recipes = {"schema_version": 1, "effects": [{"kind": "bash", "source_key": "TEST", "source_kind": "unit-ability", "bonus_damage": 123}]}
        with self.assertRaises(ValueError):
            native.build_tuning(release, root, recipes)


if __name__ == "__main__":
    unittest.main()
