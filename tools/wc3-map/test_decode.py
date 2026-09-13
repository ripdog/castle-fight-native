#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("decode_map.py")
SPEC = importlib.util.spec_from_file_location("wc3_decode_map", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
DECODE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(DECODE)


class WtsTests(unittest.TestCase):
    def test_preserves_utf8_and_multiline_values(self) -> None:
        text = """STRING 12
{
Main Castle
}

STRING 19 // tooltip
{
Hazardous Munitions • splash damage
second line
}
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "war3map.wts"
            path.write_text(text, encoding="utf-8")
            parsed = DECODE.parse_wts(path)

        self.assertEqual(parsed["12"]["value"], "Main Castle")
        self.assertEqual(parsed["19"]["value"], "Hazardous Munitions • splash damage\nsecond line")
        self.assertEqual(parsed["19"]["comment"], "tooltip")

    def test_rejects_duplicate_ids(self) -> None:
        text = """STRING 1
{
one
}
STRING 1
{
two
}
"""
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "war3map.wts"
            path.write_text(text, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "duplicate trigger string"):
                DECODE.parse_wts(path)


class TextRepairTests(unittest.TestCase):
    def test_repairs_utf8_decoded_as_latin1(self) -> None:
        mojibake = bytes.fromhex("e280a2").decode("latin1")
        self.assertEqual(DECODE.repair_translator_text(f"ability {mojibake} effect"), "ability • effect")

    def test_leaves_legacy_single_byte_text_alone(self) -> None:
        self.assertEqual(DECODE.repair_translator_text("Jäger"), "Jäger")


class LuaIndexTests(unittest.TestCase):
    def test_indexes_method_scope_rawcodes_calls_and_mutators_without_string_matches(self) -> None:
        hfoo = int.from_bytes(b"hfoo", "big")
        source = (
            '--W3P\nlocal decoy="function Fake() BlzSetUnitMaxHP(1751543663) •" '
            "-- 1751543663 in comment\n"
            "function unitStats:apply(unit) "
            "if true then prepareSmokeUnit(1751543663) end "
            "__wurst_safe_BlzSetUnitMaxHP(unit,250) end"
        ).encode("utf-8")

        indexed = DECODE.analyze_lua(source, {hfoo})

        self.assertEqual([function["name"] for function in indexed["functions"]], ["unitStats:apply"])
        self.assertEqual(indexed["calls"]["prepareSmokeUnit"], 1)
        self.assertEqual(indexed["calls"]["__wurst_safe_BlzSetUnitMaxHP"], 1)
        self.assertNotIn("Fake", indexed["calls"])
        self.assertEqual(len(indexed["rawcode_sites"]), 1)
        site = indexed["rawcode_sites"][0]
        self.assertEqual(site["byte_offset"], source.find(b"1751543663", source.find(b"function unitStats")))
        self.assertEqual(site["function"], "unitStats:apply")
        self.assertEqual(site["call"], "prepareSmokeUnit")
        self.assertEqual(indexed["runtime_mutators"][0]["function"], "unitStats:apply")
        self.assertEqual(indexed["runtime_mutators"][0]["direct_map_rawcodes"], [hfoo])
        self.assertEqual(indexed["call_edges"][("unitStats:apply", "prepareSmokeUnit")], 1)

    def test_tracks_nested_function_and_block_ends(self) -> None:
        rawcode = int.from_bytes(b"A0HO", "big")
        source = (
            "function outer() if true then local callback=function() "
            "useAbility(1093683279) end callback() end return 1 end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {rawcode})

        self.assertEqual([function["name"] for function in indexed["functions"]], ["outer"])
        self.assertEqual(indexed["rawcode_sites"][0]["function"], "<anonymous@45>")
        self.assertEqual(indexed["rawcode_sites"][0]["call"], "useAbility")
        self.assertEqual(indexed["call_edges"][("outer", "callback")], 1)
        self.assertGreater(indexed["functions"][0]["end"], indexed["functions"][0]["start"])

    def test_extracts_protected_ability_table_and_jass_add_overlap(self) -> None:
        ability = int.from_bytes(b"A005", "big")
        source = (
            "function xD()"
            "AbilityLevelFields_AbilityLevelFields_cd(_I[_d[1]](1093677109,0),60.0)"
            "AbilityLevelFields_AbilityLevelFields_mana(_I[_d[2]](1093677109,0),0)"
            "end "
            "function applyProtectedAbilityFieldsForJassAdd(unit,abil) "
            "if(abil==1093677109)then "
            "__wurst_safe_BlzSetAbilityRealLevelField(unit,ABILITY_RLF_COOLDOWN,0,60.)"
            "__wurst_safe_BlzSetAbilityIntegerLevelField(unit,ABILITY_ILF_MANA_COST,0,0) end end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {ability})

        self.assertEqual(len(indexed["protected_ability_fields"]), 2)
        fields = {row["field"]: row for row in indexed["protected_ability_fields"]}
        self.assertEqual(fields["cooldown"]["value_text"], "60.0")
        self.assertEqual(fields["mana_cost"]["value_text"], "0")
        self.assertTrue(fields["cooldown"]["jass_add_restore"])
        self.assertEqual(
            {row["canonical_relation"] for row in indexed["jass_add_protected_fields"]},
            {"canonical-match"},
        )

    def test_allows_jass_add_restore_absent_from_canonical_table(self) -> None:
        source = (
            "function xD() end "
            "function applyProtectedAbilityFieldsForJassAdd(unit,abil) "
            "if(abil==1093677360)then "
            "__wurst_safe_BlzSetAbilityRealLevelField(unit,ABILITY_RLF_COOLDOWN,0,3.) end end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, set())

        self.assertEqual(indexed["protected_ability_fields"], [])
        self.assertEqual(indexed["jass_add_protected_fields"][0]["canonical_relation"], "jass-only")

    def test_rejects_disagreeing_protected_jass_add_restore(self) -> None:
        source = (
            "function xD()"
            "AbilityLevelFields_AbilityLevelFields_cd(_I[_d[1]](1093677109,0),60.0) end "
            "function applyProtectedAbilityFieldsForJassAdd(unit,abil) "
            "if(abil==1093677109)then "
            "__wurst_safe_BlzSetAbilityRealLevelField(unit,ABILITY_RLF_COOLDOWN,0,30.) end end"
        ).encode("ascii")

        with self.assertRaisesRegex(ValueError, "disagrees with xD table"):
            DECODE.analyze_lua(source, set())

    def test_extracts_generated_unit_object_metadata(self) -> None:
        source = (
            'function ensureUnitObjectMetadataRegistered() if(not OR)then OR=true '
            'PR:HashMap_put(1747988528,UnitObjectMeta_new_UnitObjectMeta(1751543663,"Barracks","bicon","tip","Footman","uicon",100,0,0,20,0,2,false,true,false,false)) '
            'PR:HashMap_put(1747988529,UnitObjectMeta_new_UnitObjectMeta(0,"Artillery","bicon","tip","","",200,380,0,0,2,(-1),false,false,false,false)) '
            'end end'
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747988528, 1747988529, 1751543663})

        self.assertEqual(len(indexed["unit_object_metadata"]), 2)
        barracks, artillery = indexed["unit_object_metadata"]
        self.assertEqual(barracks["building_id"], 1747988528)
        self.assertEqual(barracks["unit_id"], 1751543663)
        self.assertEqual(barracks["gold_cost"], 100)
        self.assertEqual(barracks["spawn_build_time"], 20)
        self.assertEqual(barracks["attack_index"], 0)
        self.assertEqual(barracks["defense_index"], 2)
        self.assertTrue(barracks["is_melee"])
        self.assertFalse(barracks["is_air"])
        self.assertEqual(artillery["unit_id"], 0)
        self.assertEqual(artillery["lumber_cost"], 380)
        self.assertEqual(artillery["defense_index"], -1)
        self.assertIsInstance(indexed["unit_object_metadata_fingerprint"], int)

    def test_extracts_race_membership_and_authored_upgrade_edges(self) -> None:
        source = (
            "function raceInit() local race=nil local building=nil "
            "race=Ke:create151() race.CFRace_builderId=(-1) race.CFRace_isCampaignOnly=false "
            "race.CFRace_builderId=1479563824 CFRace_CFRace_markCampaignOnly(race) "
            "building=_I[_d[1]](1747988528,1751543663) "
            "CFRace_CFRace_registerBuildings__w3p_vmProtect(race,building) end "
            "function ensureUnitObjectUpgradeMetadataRegistered() if(not CR)then CR=true "
            "IR[ER]=1747988528 HR[ER]=1747989305 ER=(ER+1) "
            "DR=authoredUpgradeMix(DR,1747988528) DR=authoredUpgradeMix(DR,1747989305) end end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747988528, 1751543663, 1747989305})

        self.assertEqual(indexed["race_buildings"], [{
            "race_index": 0,
            "race_function": "raceInit",
            "builder_id": 1479563824,
            "campaign_only": True,
            "building_order": 0,
            "building_id": 1747988528,
            "unit_id": 1751543663,
            "byte_offset": source.find(b"_I"),
        }])
        self.assertEqual(len(indexed["unit_object_upgrades"]), 1)
        upgrade = indexed["unit_object_upgrades"][0]
        self.assertEqual(upgrade["source_building_id"], 1747988528)
        self.assertEqual(upgrade["target_building_id"], 1747989305)

    def test_extracts_race_building_wrapper_semantics(self) -> None:
        source = (
            "function LE() gvb=0.2 fvb=0.18 evb=0.12 dvb=0.09 cvb=0.04 end "
            "function raceInit() local race=nil local a=nil local b=nil "
            "race=Ke:create151() race.CFRace_builderId=1479563824 "
            "a=CFBuilding_CFBuilding_tier(CFBuilding_CFBuilding_incomeFactor(_I[_d[1]](1747988528,1751543663),gvb),tierOne) "
            "b=CFBuilding_CFBuilding_isAntiAir(CFBuilding_CFBuilding_precursor(CFBuilding_CFBuilding_multiTarget(CFBuilding_CFBuilding_incomeFactor(_I[_d[2]](1747989305,1747989313),fvb),2.5),a)) "
            "CFBuilding_CFBuilding_extraTags(b,7,15) CFBuilding_CFBuilding_noPP(b) "
            "CFRace_CFRace_registerBuildings__w3p_vmProtect(race,a,b) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747988528, 1751543663, 1747989305, 1747989313})

        self.assertEqual(indexed["income_factor_constants"], {
            "gvb": "0.2", "fvb": "0.18", "evb": "0.12", "dvb": "0.09", "cvb": "0.04",
        })
        rows = {row["building_id"]: row for row in indexed["race_building_semantics"]}
        first = rows[1747988528]
        self.assertEqual(first["income_factor_symbol"], "gvb")
        self.assertEqual(first["income_factor"], "0.2")
        self.assertTrue(first["has_tier_assignment"])
        second = rows[1747989305]
        self.assertEqual(second["income_factor_symbol"], "fvb")
        self.assertEqual(second["precursor_building_id"], 1747988528)
        self.assertTrue(second["is_anti_air"])
        self.assertTrue(second["no_pp"])
        self.assertEqual(second["multi_target_mult"], "2.5")
        self.assertEqual(second["extra_tags"], (7, 15))

    def test_extracts_effective_unit_stat_catalog(self) -> None:
        source = (
            "function xO()local dcs=nil "
            "dcs=PB:create1139()"
            "dcs.UnitEffectiveStat_unitId=1751543663 "
            "dcs.UnitEffectiveStat_hp=int_toReal(250)"
            "dcs.UnitEffectiveStat_armor=(int_toReal(400)/100.)"
            "dcs.UnitEffectiveStat_dps=(int_toReal(1888)/100.)"
            "dcs.UnitEffectiveStat_attackRange=int_toReal(90)"
            "dcs.UnitEffectiveStat_moveSpeed=int_toReal(270)"
            "dcs.UnitEffectiveStat_spawnsPerCycle=1 "
            "dcs.UnitEffectiveStat_canHitAir=false "
            "ZR:HashMap_put(1747988528,dcs) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1751543663, 1747988528})

        self.assertEqual(len(indexed["effective_unit_stats"]), 1)
        row = indexed["effective_unit_stats"][0]
        self.assertEqual(row["unit_id"], 1751543663)
        self.assertEqual(row["building_id"], 1747988528)
        self.assertEqual(row["hp"], "250")
        self.assertEqual(row["armor"], "4")
        self.assertEqual(row["dps"], "18.88")
        self.assertEqual(row["attack_range"], "90")
        self.assertEqual(row["move_speed"], "270")
        self.assertEqual(row["spawns_per_cycle"], 1)
        self.assertFalse(row["can_hit_air"])

    def test_extracts_and_decodes_protected_unit_stat_row(self) -> None:
        wizard = int.from_bytes(b"h00W", "big")
        source = (
            "function jP()"
            "_I[_d[1]](1747988567,878085,3432,3941,2147483647,2277,1454,2147483647,3851,1803943,3534,2147483647,2147483647,2147483647,2147483647,2147483647)"
            "end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {wizard})

        self.assertEqual(len(indexed["protected_unit_stats"]), 1)
        row = indexed["protected_unit_stats"][0]
        self.assertEqual(row["unit_id"], wizard)
        self.assertEqual(row["source_fingerprint"], 878085)
        self.assertEqual(row["hp"], 850)
        self.assertEqual(row["armor"], 4)
        self.assertEqual(row["move_speed"], 290)
        self.assertEqual(row["attack1_base_damage"], 89)
        self.assertIsNone(row["attack1_dice_number"])
        self.assertEqual(row["attack1_dice_sides"], 21)
        self.assertEqual(row["attack1_cooldown_microseconds"], 1_800_000)
        self.assertEqual(row["attack1_cooldown"], "1.8")
        self.assertEqual(row["attack1_range"], 650)
        self.assertIsNone(row["attack2_base_damage"])

    def test_propagates_rawcode_context_to_runtime_mutator_through_named_calls(self) -> None:
        rawcode = int.from_bytes(b"ABCD", "big")
        source = (
            "function source() use(1094861636) helper() end "
            "function helper() mutate() end "
            "function mutate(unit) __wurst_safe_BlzSetUnitMaxHP(unit,500) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {rawcode})

        self.assertEqual(indexed["resolved_call_edges"], 2)
        self.assertEqual(len(indexed["rawcode_mutator_traces"]), 1)
        trace = indexed["rawcode_mutator_traces"][0]
        self.assertEqual(trace["rawcode_integer"], rawcode)
        self.assertEqual(trace["source_function"], "source")
        self.assertEqual(trace["mutation_function"], "mutate")
        self.assertEqual(trace["normalized_mutator"], "BlzSetUnitMaxHP")
        self.assertEqual(trace["hop_count"], 2)
        self.assertEqual(trace["call_path"], ("source", "helper", "mutate"))
        self.assertEqual(trace["evidence_kind"], "static-call-path")

    def test_mutator_trace_keeps_direct_same_function_evidence_distinct(self) -> None:
        rawcode = int.from_bytes(b"ABCD", "big")
        source = (
            "function apply(unit) use(1094861636) BlzSetUnitArmor(unit,4.0) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {rawcode})

        self.assertEqual(len(indexed["rawcode_mutator_traces"]), 1)
        trace = indexed["rawcode_mutator_traces"][0]
        self.assertEqual(trace["hop_count"], 0)
        self.assertEqual(trace["call_path"], ("apply",))
        self.assertEqual(trace["evidence_kind"], "direct-same-function")

    def test_mutator_trace_does_not_invent_indirect_callback_target(self) -> None:
        rawcode = int.from_bytes(b"ABCD", "big")
        source = (
            "function source(callback) use(1094861636) callback() end "
            "function mutate(unit) BlzSetUnitArmor(unit,4.0) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {rawcode})

        self.assertEqual(indexed["rawcode_mutator_traces"], [])

    def test_resolves_generated_filter_to_adjacent_predicate_function(self) -> None:
        source = (
            "function init() "
            "SX=Filter((function(...) local x=nil return x end)) "
            "RX=Filter((function(...) local x=nil return x end)) end "
            "function enemy() local u=nil return(isAliveCombatSapper(u)and unit_isEnemyOf(u,mIb))end "
            "function ally() local u=nil return(isAliveCombatSapper(u)and unit_isAllyOf(u,mIb))end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, set())

        bindings = {row["symbol"]: row for row in indexed["protected_filter_bindings"]}
        self.assertEqual(bindings["SX"]["resolved_function"], "enemy")
        self.assertEqual(bindings["SX"]["predicate"], "alive-combat-sapper;enemy-of-mIb")
        self.assertEqual(bindings["RX"]["resolved_function"], "ally")
        self.assertEqual(bindings["RX"]["predicate"], "alive-combat-sapper;ally-of-mIb")
        self.assertEqual(bindings["SX"]["resolution_status"], "resolved")

    def test_recovers_building_spell_registration_from_closure_dispatch(self) -> None:
        source = (
            "function handler(building) return building end "
            "fy.BuildingSpellClosure_cast=handler "
            "function bL() local closure=nil local ability=nil ABILITY=1093677393 ability=ABILITY "
            "closure=fy:create987() _I[_d[1]](1747988816,ability,closure) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747988816, 1093677393})

        self.assertEqual(indexed["building_spell_registrations"], [{
            "building_id": 1747988816,
            "ability_id": 1093677393,
            "closure_variable": "closure",
            "closure_class": "fy",
            "handler_function": "handler",
            "registration_function": "bL",
            "byte_offset": source.find(b"_I[_d[1]]"),
        }])

    def test_recovers_unit_spell_registration_from_closure_dispatch(self) -> None:
        source = (
            "function handler(caster,target) return target end "
            "qx.UnitSpellClosure_cast1=handler "
            "function init() local closure=nil closure=qx:create99() "
            "_I[_d[2]](1747989334,1093682481,2,852063,closure) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747989334, 1093682481})

        self.assertEqual(indexed["unit_spell_registrations"], [{
            "unit_id": 1747989334,
            "ability_id": 1093682481,
            "target_mode": 2,
            "target_mode_label": "ally-ground",
            "order_id": 852063,
            "order_expression_kind": "integer",
            "expected_immediate_unit_id": 0,
            "closure_variable": "closure",
            "closure_class": "qx",
            "handler_function": "handler",
            "registration_function": "init",
            "evidence_kind": "protected-registry-call",
            "byte_offset": source.find(b"_I[_d[2]]"),
        }])

    def test_unit_spell_mechanics_follow_named_calls_inside_anonymous_timer_callbacks(self) -> None:
        source = (
            "function processor() dummyCastImmediateFrom(nil,1094861636,852096,nil,1.) end "
            "function helper() TimerStart(nil,0.03,true,function() processor() end) end "
            "function handler(caster,target) helper() end "
            "qx.UnitSpellClosure_cast1=handler "
            "function init() local closure=nil closure=qx:create99() "
            "_I[_d[2]](1747989334,1093682481,0,852063,closure) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, {1747989334, 1093682481, 1094861636})

        self.assertEqual(len(indexed["unit_spell_mechanics"]), 1)
        mechanic = indexed["unit_spell_mechanics"][0]
        paths = {row["rawcode_integer"]: row for row in mechanic["reachable_map_rawcode_paths"]}
        self.assertEqual(paths[1094861636]["path"], ["handler", "helper", "processor"])
        self.assertEqual(paths[1094861636]["hops"], 2)
        effect_sites = [
            site for site in mechanic["semantic_effect_sites"]
            if site["function"] == "processor" and site["callee"] == "dummyCastImmediateFrom"
        ]
        self.assertEqual(len(effect_sites), 1)

    def test_indexes_generated_function_alias_assignments_without_making_call_edges(self) -> None:
        source = (
            "function handler(unit) BlzSetUnitArmor(unit,4.0) end "
            "Dispatch.Snowveil_onEvent=handler "
            "function runner() xpcall(handler,errorHandler) end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, set())

        self.assertEqual(indexed["function_aliases"], [{
            "alias": "Dispatch.Snowveil_onEvent",
            "target_function": "handler",
            "alias_byte_offset": source.find(b"Dispatch.Snowveil_onEvent"),
            "target_byte_offset": source.find(b"handler", source.find(b"Dispatch.Snowveil_onEvent")),
        }])
        self.assertNotIn(("<top-level>", "handler"), indexed["call_edges"])
        values = indexed["function_value_arguments"]
        self.assertEqual(len(values), 1)
        self.assertEqual(values[0]["target_function"], "handler")
        self.assertEqual(values[0]["containing_call"], "xpcall")
        self.assertEqual(values[0]["function"], "runner")

    def test_function_value_index_skips_strings_comments_and_function_declarations(self) -> None:
        source = (
            '-- Dispatch.fake=handler xpcall(handler,errorHandler)\n'
            'local text="Dispatch.fake=handler" '
            "function handler(value) return value end "
            "function other(handler) return handler end"
        ).encode("ascii")

        indexed = DECODE.analyze_lua(source, set())

        self.assertEqual(indexed["function_aliases"], [])
        self.assertEqual(indexed["function_value_arguments"], [])


if __name__ == "__main__":
    unittest.main()
