#!/usr/bin/env python3
from __future__ import annotations

import csv
from collections import Counter
import importlib.util
import json
import struct
import sys
import tempfile
import unittest
from pathlib import Path


MODULE_PATH = Path(__file__).with_name("resolve-base-data.py")
SPEC = importlib.util.spec_from_file_location("resolve_base_data", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
resolve = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = resolve
SPEC.loader.exec_module(resolve)


class SylkTests(unittest.TestCase):
    def test_split_record_keeps_quoted_semicolon(self) -> None:
        self.assertEqual(
            resolve.split_semicolon_record('C;X2;Y3;K"alpha;beta"'),
            ["C", "X2", "Y3", 'K"alpha;beta"'],
        )

    def test_parse_slk_tracks_sparse_coordinates(self) -> None:
        text = "\r\n".join([
            "ID;PWXL;N;E",
            "B;X3;Y3;D0",
            'C;X1;Y1;K"id"',
            'C;X2;K"hp"',
            'C;X3;K"name"',
            'C;X1;Y2;K"hfoo"',
            'C;X2;K250',
            'C;X3;K"Foot; Man"',
            'C;X1;Y3;K"hrif"',
            'C;X2;K270',
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "tiny.slk"
            path.write_text(text, encoding="utf-8")
            rows = resolve.parse_slk(path)
        self.assertEqual(rows["hfoo"]["hp"], 250)
        self.assertEqual(rows["hfoo"]["name"], "Foot; Man")
        self.assertEqual(rows["hrif"]["hp"], 270)
        self.assertNotIn("name", rows["hrif"])


class ProfileTests(unittest.TestCase):
    def test_selected_profile_variant_wins(self) -> None:
        profile = {
            "hbar": {
                "Ubertip": "generic",
                "Ubertip:custom,V0": "custom zero",
                "Ubertip:custom,V1": "custom one",
                "Buttonpos": "1,2",
            }
        }
        self.assertEqual(resolve.profile_value(profile, "hbar", "Ubertip", "custom,V1"), "custom one")
        self.assertEqual(resolve.profile_value(profile, "hbar", "Ubertip", "custom,V0"), "custom zero")
        self.assertEqual(resolve.selected_index(resolve.profile_value(profile, "hbar", "Buttonpos", "custom,V1"), 1), "2")

    def test_w3i_default_tft_selects_custom_v1(self) -> None:
        selection = resolve.select_game_data({
            "game_data_set_version": 0,
            "game_data_version": {"raw": 1},
        })
        self.assertEqual(selection.overlay_dir, "custom_v1")
        self.assertEqual(selection.profile_variant, "custom,V1")
        self.assertEqual(selection.label, "Default (TFT)")

    def test_w3i_explicit_data_sets_map_to_balance_overlays(self) -> None:
        custom = resolve.select_game_data({"game_data_set_version": 1, "game_data_version": {"raw": 1}})
        melee = resolve.select_game_data({"game_data_set_version": 2, "game_data_version": {"raw": 1}})
        self.assertEqual((custom.overlay_dir, custom.profile_variant), ("custom_v0", "custom,V0"))
        self.assertEqual((melee.overlay_dir, melee.profile_variant), ("melee_v0", "melee,V0"))

    def test_single_quoted_csv_value_is_unquoted(self) -> None:
        self.assertEqual(resolve.selected_index('"Shop Sharing, Allied Bldg."', 0), "Shop Sharing, Allied Bldg.")


class ProtectionRecoveryTests(unittest.TestCase):
    def candidate(self, index: int, value):
        return resolve.MapCandidate(index=index, value=value, value_type="int")

    def test_numeric_zero_one_sentinels_recover_first_value(self) -> None:
        value, reason = resolve.choose_recovered([
            self.candidate(0, 280),
            self.candidate(1, 1),
            self.candidate(2, 1),
        ])
        self.assertEqual(value, 280)
        self.assertEqual(reason, "w3p-recovered-first-numeric-sentinel")

    def test_conflicting_strings_keep_last_write(self) -> None:
        value, reason = resolve.choose_recovered([
            resolve.MapCandidate(0, "A0HO", "string"),
            resolve.MapCandidate(1, "A0HO,AM0{", "string"),
        ])
        self.assertEqual(value, "A0HO,AM0{")
        self.assertEqual(reason, "ambiguous-string-last-write")

    def test_repeated_final_string_write_is_classified_stable(self) -> None:
        value, reason = resolve.choose_recovered([
            resolve.MapCandidate(0, "A0HO", "string"),
            resolve.MapCandidate(1, "A0HO,AM0{", "string"),
            resolve.MapCandidate(2, "A0HO,AM0{", "string"),
        ])
        self.assertEqual(value, "A0HO,AM0{")
        self.assertEqual(reason, "map-stable-final-write")


class PathingTextureTests(unittest.TestCase):
    def test_tga_channels_map_to_pathing_bits(self) -> None:
        # 2x2, bottom-origin 24-bit TGA. File pixels are BGR.
        header = bytearray(18)
        header[2] = 2
        struct.pack_into("<HH", header, 12, 2, 2)
        header[16] = 24
        # Source bottom row then top row: blue, green / red, blue+red.
        pixels = bytes([
            255, 0, 0,
            0, 255, 0,
            0, 0, 255,
            255, 0, 255,
        ])
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "mask.tga"
            path.write_bytes(bytes(header) + pixels)
            decoded = resolve.parse_tga_pathing(path)
        self.assertEqual(decoded["hex_rows"], ["15", "42"])
        self.assertEqual(decoded["width_world_units"], 64)
        self.assertEqual(decoded["counts"], {"unwalkable": 2, "unflyable": 1, "unbuildable": 2})


class ResolvedEvidenceTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.root = Path(__file__).resolve().parents[2]
        cls.extracted = cls.root / "docs" / "original_map" / "extracted"
        cls.resolved = cls.extracted / "resolved"
        cls.script = cls.extracted / "script"

    def test_protected_perk_registry_and_damage_listener_audits_prove_registration_without_hiding_semantic_gaps(self) -> None:
        with (self.script / "protected-perk-registry-audit.tsv").open(encoding="utf-8") as handle:
            perks = list(csv.DictReader(handle, delimiter="\t"))
        self.assertEqual(len(perks), 19)
        self.assertEqual([row["perk_id"] for row in perks], [f"perk_{index:02d}" for index in range(1, 20)])
        self.assertEqual({row["protected_registry_slot_count"] for row in perks}, {"19"})
        self.assertEqual({row["runtime_registry_path_status"] for row in perks}, {"reachable-protected-initializer"})
        self.assertEqual({row["individual_factory_registration_status"] for row in perks}, {"exact-protected-vm-call"})
        self.assertEqual({row["individual_factory_registration_proven"] for row in perks}, {"1"})
        self.assertEqual([int(row["protected_registry_slot"]) for row in perks], list(range(19)))
        breaching = next(row for row in perks if row["perk_id"] == "perk_10")
        self.assertEqual(breaching["perk_name"], "Breaching Doctrine")
        self.assertEqual(breaching["factory_function"], "createBreachingDoctrinePerk")
        self.assertEqual(
            breaching["damage_listener_function"],
            "DamageListener_perkListenDamage_PerkBreachingDoctrine_onEvent_perkListenDamage_PerkBreachingDoctrine",
        )

        with (self.script / "damage-listener-coverage.tsv").open(encoding="utf-8") as handle:
            listeners = {row["listener_function"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(listeners), 20)
        self.assertEqual(
            dict(sorted(Counter(row["coverage_status"] for row in listeners.values()).items())),
            {
                "e2e-only": 1,
                "normalized-ai-runtime-semantics": 2,
                "normalized-gameplay-semantics": 16,
                "telemetry-only": 1,
            },
        )
        self.assertEqual(
            listeners["DamageListener_addListener_RaceCorrupted_onEvent_addListener_RaceCorrupted"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertEqual(
            listeners[breaching["damage_listener_function"]]["candidate_perk_factory"],
            "createBreachingDoctrinePerk",
        )
        self.assertEqual(breaching["protected_registry_slot"], "9")
        self.assertEqual(breaching["factory_value_evidence"], "factory-result-via-local")
        summary = json.loads((self.extracted / "summary.json").read_text(encoding="utf-8"))["script"]
        self.assertEqual(summary["protected_perk_registry_audit_rows"], 19)
        self.assertEqual(summary["perk_mechanics"], 19)
        self.assertEqual(summary["runtime_ai_mechanics"], 5)
        self.assertEqual(summary["runtime_session_mechanics"], 7)
        self.assertEqual(summary["runtime_mode_mechanics"], 2)
        self.assertEqual(summary["runtime_campaign_mechanics"], 5)
        self.assertEqual(summary["runtime_draft_mechanics"], 3)
        self.assertEqual(summary["damage_listener_coverage_rows"], 20)
        self.assertEqual(summary["action_watch_coverage_rows"], 43)
        self.assertEqual(summary["callback_periodic_coverage_rows"], 29)
        self.assertEqual(summary["event_listener_coverage_rows"], 70)
        self.assertEqual(
            listeners["DamageListener_addListener_AiEngagement_onEvent_addListener_AiEngagement"]["coverage_status"],
            "normalized-ai-runtime-semantics",
        )
        self.assertEqual(
            listeners["DamageListener_addListener_CustomAI_onEvent_addListener_CustomAI"]["coverage_status"],
            "normalized-ai-runtime-semantics",
        )

    def test_event_listener_coverage_is_explicit_and_fail_loud(self) -> None:
        with (self.script / "event-listener-coverage.tsv").open(encoding="utf-8") as handle:
            rows = {row["listener_function"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 70)
        self.assertEqual(
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
            {
                "command-framework-infrastructure": 1,
                "e2e-only": 13,
                "gameplay-framework-infrastructure": 3,
                "normalized-ai-runtime-semantics": 1,
                "normalized-campaign-runtime-semantics": 2,
                "normalized-mode-runtime-semantics": 1,
                "normalized-session-runtime-semantics": 4,
                "normalized-gameplay-dispatch": 10,
                "normalized-gameplay-semantics": 21,
                "presentation-only": 13,
                "telemetry-only": 1,
            },
        )
        self.assertEqual(
            rows["EventListener_add_PerkUtils_onEvent_add_PerkUtils"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertIn(
            "native-lava-spawn-split-with-child-conversion",
            rows["EventListener_add_PerkUtils_onEvent_add_PerkUtils"]["normalized_sources"],
        )
        self.assertEqual(
            rows["EventListener_add_doAfter_SpamPrevention_onEvent_add_doAfter_SpamPrevention"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_AiItemBuying_onEvent_add_AiItemBuying"]["coverage_status"],
            "normalized-ai-runtime-semantics",
        )
        lifecycle = rows["EventListener_add_BuildingLifecycle_onEvent_add_BuildingLifecycle"]
        self.assertEqual(lifecycle["coverage_status"], "normalized-gameplay-dispatch")
        self.assertEqual(
            lifecycle["dispatch_path"],
            "EventListener_add_BuildingLifecycle_onEvent_add_BuildingLifecycle -> onBuildingFinished -> acquireEleBuilding",
        )
        self.assertEqual(
            rows["EventListener_add_CampaignChallenges_onEvent_add_CampaignChallenges"]["coverage_status"],
            "normalized-campaign-runtime-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_ShopAnnouncements_onEvent_add_ShopAnnouncements"]["coverage_status"],
            "normalized-campaign-runtime-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_Campaign_onEvent_add_Campaign"]["coverage_status"],
            "presentation-only",
        )
        self.assertEqual(
            rows["EventListener_add_Commands_onEvent_add_Commands"]["coverage_status"],
            "presentation-only",
        )
        self.assertEqual(
            rows["EventListener_add_WurstCommand_onEvent_add_WurstCommand"]["coverage_status"],
            "command-framework-infrastructure",
        )
        self.assertEqual(
            rows["EventListener_add_PlayerLeave_onEvent_add_PlayerLeave"]["coverage_status"],
            "normalized-session-runtime-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_IdleDetectionRuntime_onEvent_add_IdleDetectionRuntime"]["coverage_status"],
            "normalized-session-runtime-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_ModeParser_onEvent_add_ModeParser"]["coverage_status"],
            "normalized-mode-runtime-semantics",
        )
        self.assertEqual(
            rows["EventListener_add_BuildingAttachments_onEvent_add_BuildingAttachments"]["coverage_status"],
            "presentation-only",
        )
        self.assertEqual(
            rows["EventListener_add_BuildingCatalogE2E_onEvent_add_BuildingCatalogE2E"]["coverage_status"],
            "e2e-only",
        )
        summary = json.loads((self.extracted / "summary.json").read_text(encoding="utf-8"))["script"]
        self.assertEqual(summary["event_listener_coverage_rows"], 70)
        self.assertEqual(
            summary["event_listener_coverage_status_counts"],
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
        )

    def test_action_watch_coverage_is_explicit_and_fail_loud(self) -> None:
        with (self.script / "action-watch-coverage.tsv").open(encoding="utf-8") as handle:
            rows = {row["callback_function"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 43)
        self.assertEqual(
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
            {
                "e2e-only": 4,
                "normalized-campaign-runtime-semantics": 1,
                "normalized-draft-runtime-semantics": 2,
                "gameplay-framework-infrastructure": 1,
                "normalized-gameplay-semantics": 10,
                "normalized-mode-runtime-semantics": 2,
                "normalized-session-runtime-semantics": 3,
                "presentation-only": 17,
                "telemetry-only": 3,
            },
        )
        self.assertEqual(
            rows["Action_watch_IdleDetectionRuntime_run_watch_IdleDetectionRuntime"]["coverage_status"],
            "normalized-session-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_RoundEndRuntime_run_watch_RoundEndRuntime"]["coverage_status"],
            "normalized-session-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_ModeRaceRuntime_run_watch_ModeRaceRuntime"]["coverage_status"],
            "normalized-mode-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_UltimateRoll_run_watch_UltimateRoll"]["coverage_status"],
            "normalized-mode-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_RaceNelfAbilities_run_watch_RaceNelfAbilities"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertEqual(
            rows["Action_watch_SnowveilFountain_run_watch_SnowveilFountain"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertEqual(
            rows["Action_watch_doAfter_ThunderpawSpire_run_watch_doAfter_ThunderpawSpire"]["coverage_status"],
            "normalized-gameplay-semantics",
        )
        self.assertEqual(
            rows["Action_watch_CampaignRuntime_run_watch_CampaignRuntime"]["coverage_status"],
            "normalized-campaign-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_DraftOrchestrator_run_watch_DraftOrchestrator"]["coverage_status"],
            "normalized-draft-runtime-semantics",
        )
        self.assertEqual(
            rows["Action_watch_DraftPerkRegistry_run_watch_DraftPerkRegistry"]["coverage_status"],
            "normalized-draft-runtime-semantics",
        )
        summary = json.loads((self.extracted / "summary.json").read_text(encoding="utf-8"))["script"]
        self.assertEqual(summary["action_watch_coverage_rows"], 43)
        self.assertEqual(
            summary["action_watch_coverage_status_counts"],
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
        )

    def test_callback_periodic_coverage_is_explicit_and_fail_loud(self) -> None:
        with (self.script / "callback-periodic-coverage.tsv").open(encoding="utf-8") as handle:
            rows = {row["callback_function"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 29)
        self.assertEqual(
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
            {
                "callback-framework-infrastructure": 3,
                "e2e-only": 2,
                "gameplay-framework-infrastructure": 1,
                "normalized-ai-runtime-semantics": 2,
                "normalized-campaign-runtime-semantics": 2,
                "normalized-draft-runtime-semantics": 3,
                "normalized-gameplay-semantics": 2,
                "normalized-mode-runtime-semantics": 4,
                "normalized-session-runtime-semantics": 2,
                "normalized-unit-spell-semantics": 1,
                "presentation-only": 5,
                "telemetry-only": 2,
            },
        )
        self.assertEqual(
            rows["CallbackPeriodic_doPeriodically_RaceDesertAbilities_call_doPeriodically_RaceDesertAbilities"]["coverage_status"],
            "normalized-unit-spell-semantics",
        )
        self.assertEqual(
            rows["CallbackPeriodic_doPeriodically_CampaignChallenges_call_doPeriodically_CampaignChallenges"]["coverage_status"],
            "normalized-campaign-runtime-semantics",
        )
        self.assertEqual(
            rows["CallbackPeriodic_doPeriodically_doAfter_AiExecutor_CustomAI_call_doPeriodically_doAfter_AiExecutor_CustomAI"]["coverage_status"],
            "normalized-ai-runtime-semantics",
        )
        self.assertEqual(
            rows["CallbackPeriodic_doPeriodically_doAfter_CfCastlePathing_call_doPeriodically_doAfter_CfCastlePathing"]["coverage_status"],
            "gameplay-framework-infrastructure",
        )
        self.assertEqual(
            rows["CallbackPeriodic_doPeriodically_watch_doAfter_IncomeUI_call_doPeriodically_watch_doAfter_IncomeUI"]["coverage_status"],
            "presentation-only",
        )
        summary = json.loads((self.extracted / "summary.json").read_text(encoding="utf-8"))["script"]
        self.assertEqual(summary["callback_periodic_coverage_rows"], 29)
        self.assertEqual(
            summary["callback_periodic_coverage_status_counts"],
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
        )

    def test_callback_single_coverage_is_explicit_and_fail_loud(self) -> None:
        with (self.script / "callback-single-coverage.tsv").open(encoding="utf-8") as handle:
            rows = {row["callback_function"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 276)
        self.assertEqual(
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
            {
                "ai-runtime-infrastructure": 1,
                "callback-framework-infrastructure": 2,
                "e2e-only": 98,
                "gameplay-framework-infrastructure": 19,
                "integrity-infrastructure": 3,
                "normalized-ai-runtime-semantics": 11,
                "normalized-campaign-runtime-semantics": 2,
                "normalized-draft-runtime-dispatch": 2,
                "normalized-gameplay-dispatch": 4,
                "normalized-gameplay-semantics": 41,
                "normalized-mode-runtime-semantics": 10,
                "normalized-session-runtime-dispatch": 2,
                "normalized-session-runtime-semantics": 6,
                "normalized-unit-spell-semantics": 32,
                "normalized-unresolved-reachability-evidence": 1,
                "presentation-only": 37,
                "sync-framework-infrastructure": 2,
                "telemetry-infrastructure": 3,
            },
        )
        self.assertEqual(
            rows["CallbackSingle_doAfter_RuntimeLedger_call_doAfter_RuntimeLedger"]["coverage_status"],
            "normalized-unresolved-reachability-evidence",
        )
        self.assertEqual(
            rows["CallbackSingle_doAfter_MMDData_call_doAfter_MMDData"]["coverage_status"],
            "normalized-campaign-runtime-semantics",
        )
        for callback in (
            "CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime",
            "CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime",
            "CallbackSingle_doAfter_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_doAfter_ReengageRuntime",
        ):
            self.assertEqual(rows[callback]["coverage_status"], "normalized-gameplay-semantics")
            self.assertIn("building-spell-mechanics", rows[callback]["normalized_sources"])
        summary = json.loads((self.extracted / "summary.json").read_text(encoding="utf-8"))["script"]
        self.assertEqual(summary["callback_single_coverage_rows"], 276)
        self.assertEqual(
            summary["callback_single_coverage_status_counts"],
            dict(sorted(Counter(row["coverage_status"] for row in rows.values()).items())),
        )

    def test_runtime_draft_lifecycle_is_normalized(self) -> None:
        with (self.resolved / "runtime-draft-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(
            set(rows),
            {"draft-round-restart-lifecycle", "draft-controller-periodic-timers", "draft-perk-reminder-reapply"},
        )

        restart = json.loads(rows["draft-round-restart-lifecycle"]["parameters_json"])
        self.assertTrue(restart["requires_existing_draft_controller"])
        self.assertTrue(restart["resets_unit_pool"])
        self.assertTrue(restart["reinitializes_default_draft_tiers"])
        self.assertTrue(restart["reinitializes_proven_19_perk_registry"])
        self.assertTrue(restart["resets_existing_draft_controller"])
        self.assertTrue(restart["restarts_controller_with_warmup"])

        timers = json.loads(rows["draft-controller-periodic-timers"]["parameters_json"])
        self.assertEqual(timers["period_seconds"], 1)
        self.assertEqual(timers["initializer_defaults"]["rerolls_per_player"], 5)
        self.assertEqual(timers["initializer_defaults"]["total_rounds"], 5)
        self.assertEqual(timers["initializer_defaults"]["seconds_per_round"], 20)
        self.assertEqual(timers["initializer_defaults"]["warmup_seconds_when_Pcb"], 60)
        self.assertEqual(timers["initializer_defaults"]["warmup_seconds_otherwise"], 10)

        reminder = json.loads(rows["draft-perk-reminder-reapply"]["parameters_json"])
        self.assertEqual(reminder["player_ids_scanned"], [0, 11])
        self.assertTrue(reminder["skips_perks_with_zero_reminder_ability_id"])
        self.assertTrue(reminder["adds_missing_reminder_ability_only"])
        self.assertTrue(reminder["makes_reminder_ability_permanent"])
        self.assertEqual(reminder["sets_ability_real_level_field"], "ABILITY_RLF_CHANCE_TO_CRITICAL_STRIKE")
        self.assertEqual(reminder["sets_field_level_index"], 0)
        self.assertEqual(reminder["sets_field_value"], 0.0)

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_draft_mechanic_rows"], 3)

    def test_runtime_campaign_restriction_hooks_are_normalized(self) -> None:
        with (self.resolved / "runtime-campaign-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(
            set(rows),
            {
                "campaign-star-restriction-failure-hooks",
                "campaign-star-periodic-objectives",
                "campaign-survival-countdown",
                "campaign-match-result-bridge",
                "campaign-mission-supply-application",
            },
        )
        parameters = json.loads(rows["campaign-star-restriction-failure-hooks"]["parameters_json"])
        self.assertEqual(parameters["active_restriction_scope"], ["second-star", "third-star"])
        building_loss = parameters["tracked_building_loss"]
        self.assertEqual(building_loss["restriction_id"], "challenge_no_buildings_lost")
        self.assertTrue(building_loss["requires_unit_in_campaign_tracked_building_group"])
        self.assertTrue(building_loss["removes_lost_unit_from_tracking_group"])
        self.assertTrue(building_loss["sets_hard_failure_flag"])
        item_purchase = parameters["challenge_bound_item_purchase"]
        self.assertEqual(item_purchase["restriction_id"], "challenge_no_items")
        self.assertTrue(item_purchase["requires_buying_unit_owner_is_challenge_bound_player"])
        self.assertTrue(item_purchase["sets_hard_failure_flag"])
        periodic = json.loads(rows["campaign-star-periodic-objectives"]["parameters_json"])
        self.assertEqual(periodic["period_seconds"], 0.25)
        self.assertEqual(periodic["protected_setup_vm_index"], 23)
        self.assertEqual(periodic["fast_win_failure_comparison"], "elapsed_seconds > configured_fast_win_seconds")
        self.assertTrue(periodic["fast_win_comparison_is_strict"])
        self.assertEqual(periodic["castle_health_failure_comparison"], "health_percent < configured_minimum_castle_hp_percent")
        self.assertTrue(periodic["castle_health_comparison_is_strict"])

        survival = json.loads(rows["campaign-survival-countdown"]["parameters_json"])
        self.assertEqual(survival["period_seconds"], 1)
        self.assertEqual(survival["completion_condition"], "remaining_seconds <= 0")
        self.assertTrue(survival["completion_kills_owner_team_castle"])

        result_bridge = json.loads(rows["campaign-match-result-bridge"]["parameters_json"])
        self.assertEqual(result_bridge["result_bridge_delay_seconds"], 5.0)
        self.assertTrue(result_bridge["delayed_callback_stops_survival_timer"])
        self.assertEqual(result_bridge["delayed_callback_writes_player_won_symbol"], "Bpb")
        self.assertEqual(result_bridge["delayed_callback_executes_trigger_symbol"], "Cpb")
        self.assertFalse(result_bridge["protected_result_trigger_payload_normalized"])

        supplies = json.loads(rows["campaign-mission-supply-application"]["parameters_json"])
        self.assertEqual(supplies["retry_interval_seconds"], 0.1)
        self.assertEqual(supplies["maximum_retry_attempt_index"], 80)
        self.assertEqual(supplies["maximum_retry_horizon_seconds_nominal"], 8.0)
        self.assertTrue(supplies["cloud_machine_presence_grants_or_refills_cloud_staff_for_every_party_player"])
        self.assertTrue(supplies["player_assigned_perk_applies_to_selected_party_slot"])
        self.assertTrue(supplies["positive_speed_bonus_applies_to_own_added_ai_players"])
        self.assertTrue(supplies["this_function_does_not_directly_add_accumulated_gold_or_lumber"])

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_campaign_mechanic_rows"], 5)

    def test_runtime_mode_registry_and_host_chat_parser_are_normalized(self) -> None:
        with (self.resolved / "runtime-mode-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(set(rows), {"mode-selection-controller-and-registry", "w3champions-ladder-bootstrap"})
        parameters = json.loads(rows["mode-selection-controller-and-registry"]["parameters_json"])
        self.assertEqual(parameters["host_player_id"], 0)
        self.assertTrue(parameters["requires_leading_dash"])
        self.assertTrue(parameters["ultimate_draft_conflict_is_rejected_before_application"])
        self.assertEqual(parameters["multiple_modes_separator"], "-")
        self.assertEqual(parameters["registered_mode_count"], 44)
        modes = parameters["registered_modes"]
        self.assertEqual(
            [mode["mode_id"] for mode in modes],
            [
                "r", "p", "m", "d", "cr", "sr", "um", "ud", "na", "ntb", "nb", "ns", "ni", "la", "nrs",
                "ur", "norb", "desync", "du", "nch", "co", "cc", "dom", "ult", "nca", "noai", "nfow", "it", "lt",
                "glw", "gld", "mp", "emp", "ll", "ban", "rban", "bal", "fow", "fill", "nt", "ht", "mt", "skip", "w3c",
            ],
        )
        by_id = {mode["mode_id"]: mode for mode in modes}
        self.assertEqual(by_id["r"]["choices"], ["r", "g"])
        self.assertEqual((by_id["r"]["minimum_value"], by_id["r"]["maximum_value"]), (1, 6))
        self.assertEqual(by_id["sr"]["choices"], ["g", "l", "u"])
        self.assertEqual(by_id["sr"]["choice_minimum_values"], {"g": 100, "l": 100, "u": 0})
        self.assertEqual(by_id["sr"]["maximum_value"], 100000)
        self.assertIn("applyArtilleryModeEnabled", by_id["na"]["callback_direct_calls"])
        self.assertIn("applyNoAfkMode", by_id["noai"]["callback_direct_calls"])
        self.assertEqual((by_id["bal"]["minimum_value"], by_id["bal"]["maximum_value"]), (0, 2))
        self.assertIn("applyAutobalanceMode", by_id["bal"]["callback_direct_calls"])
        self.assertIn("ModeParser_applyModeString__w3p_vmProtect", by_id["w3c"]["callback_direct_calls"])
        self.assertIn({"symbol": "qX", "value": "false"}, by_id["norb"]["callback_simple_assignments"])
        self.assertIn({"symbol": "EGb", "value": "false"}, by_id["desync"]["callback_simple_assignments"])
        self.assertIn({"symbol": "zGb", "value": "1"}, by_id["co"]["callback_simple_assignments"])
        self.assertIn({"symbol": "zGb", "value": "10"}, by_id["cc"]["callback_simple_assignments"])
        self.assertIn({"symbol": "tX", "value": "true"}, by_id["dom"]["callback_simple_assignments"])
        self.assertTrue(parameters["round_end_signal_starts_next_round_via_mode_runtime"])
        self.assertEqual(parameters["ultimate_round_end_restores_all_building_availability_for_player_ids"], [0, 11])
        self.assertTrue(parameters["ultimate_round_end_clears_roll_texttags"])
        round_limit = parameters["round_limit_runtime"]
        self.assertEqual(round_limit["enabled_only_when_absolute_minutes_gt"], 5)
        self.assertEqual(round_limit["warning_cascade_minutes_before_limit"], [5, 3, 1])
        self.assertEqual(round_limit["warning_delays_seconds"], [120, 120, 60])
        self.assertEqual(round_limit["positive_limit_primary_tiebreak"], "remaining Rescue Strike count per team")
        self.assertEqual(round_limit["positive_limit_secondary_tiebreak"], "sum of taxedIncomeRealForPlayer over each team")
        self.assertEqual(round_limit["negative_limit_result"], "draw signal XW")
        self.assertTrue(parameters["delayed_mode_start_callbacks_call_protected_start_selected_modes_or_default"])

        w3c = json.loads(rows["w3champions-ladder-bootstrap"]["parameters_json"])
        self.assertEqual(w3c["detection_player_id"], 23)
        self.assertEqual(w3c["detection_player_name"], "FLO")
        self.assertEqual(w3c["global_draw_player_removal_delay_seconds"], 3)
        self.assertEqual(w3c["post_mode_start_delay_seconds"], 2)
        self.assertEqual(w3c["protected_auto_mode_payload_status"], "protected-vm-not-normalized-here")
        self.assertEqual(w3c["protected_default_draft_payload_status"], "protected-vm-not-normalized-here")

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_mode_mechanic_rows"], 2)

    def test_runtime_session_away_control_and_leave_autobalance_are_normalized(self) -> None:
        with (self.resolved / "runtime-session-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(
            set(rows),
            {
                "away-control-and-idle-detection",
                "player-leave-autobalance-and-team-empty-resolution",
                "unanimous-draw-round-restart",
                "round-end-review-countdown",
                "team-afk-confirmation-check",
                "team-forfeit-vote",
                "global-draw-vote-match-cancel",
            },
        )

        away = json.loads(rows["away-control-and-idle-detection"]["parameters_json"])
        self.assertTrue(away["automatic_idle_detection_enabled_by_no_afk_mode"])
        self.assertEqual(away["idle_check_interval_seconds"], 1)
        self.assertEqual(away["player_slots_checked"], [0, 11])
        self.assertEqual(away["round_start_fast_window_seconds"], 40)
        self.assertEqual(away["round_start_warning_idle_seconds"], 20)
        self.assertEqual(away["round_start_auto_away_idle_seconds"], 30)
        self.assertEqual(away["general_warning_idle_seconds"], 60)
        self.assertEqual(away["general_auto_away_idle_seconds"], 120)
        self.assertTrue(away["player_activity_clears_away_immediately"])
        self.assertTrue(away["away_grants_allied_advanced_unit_control"])
        self.assertTrue(away["away_restores_prior_shared_control_state_when_cleared"])
        self.assertTrue(away["round_end_signal_stops_automatic_detection"])

        leave = json.loads(rows["player-leave-autobalance-and-team-empty-resolution"]["parameters_json"])
        self.assertEqual(leave["default_mode_value_observed_in_runtime_initializer"], 1)
        self.assertEqual(
            leave["autobalance_modes"],
            {
                "0": "redistribute all leaver-controlled player assets across remaining team",
                "1": "share dependent player slots among remaining team members",
                "2": "enable AI control for the leaver",
            },
        )
        self.assertEqual(leave["mmd_leave_flag_before_300_seconds"], 3)
        self.assertEqual(leave["mmd_leave_flag_at_or_after_300_seconds"], 1)
        self.assertEqual(leave["mmd_leave_record_delay_seconds"], 0.25)
        self.assertEqual(leave["team_empty_check_delay_seconds"], 1)
        self.assertTrue(leave["team_empty_sets_opponent_match_winner"])
        self.assertTrue(leave["team_empty_stops_round_and_cleans_round_units"])

        draw = json.loads(rows["unanimous-draw-round-restart"]["parameters_json"])
        self.assertTrue(draw["ignored_after_match_end"])
        self.assertTrue(draw["ignored_when_round_not_started"])
        self.assertTrue(draw["cleans_applied_perks"])
        self.assertTrue(draw["cleans_round_units"])
        self.assertEqual(draw["restart_delay_seconds"], 1)
        self.assertTrue(draw["does_not_set_match_winner"])

        review = json.loads(rows["round-end-review-countdown"]["parameters_json"])
        self.assertEqual(review["duration_seconds"], 15)
        self.assertEqual(review["period_seconds"], 1)
        self.assertEqual(review["next_round_signal_when_PGb"], "YW")
        self.assertEqual(review["next_round_signal_otherwise"], "bX")

        afk_check = json.loads(rows["team-afk-confirmation-check"]["parameters_json"])
        self.assertEqual(afk_check["cooldown_seconds_per_side"], 120)
        self.assertEqual(afk_check["confirmation_window_seconds"], 7.5)
        self.assertTrue(afk_check["disabled_during_campaign"])
        self.assertTrue(afk_check["timeout_only_marks_still_pending_non_afk_players"])
        self.assertTrue(afk_check["afk_effect_includes_ai_control"])

        forfeit = json.loads(rows["team-forfeit-vote"]["parameters_json"])
        self.assertEqual(forfeit["vote_timeout_seconds"], 60)
        self.assertEqual(forfeit["ineligible_voter_states"], ["not-in-game", "away", "afk"])
        self.assertEqual(forfeit["success_effect"], "kill forfeiting team's own castle")
        self.assertTrue(forfeit["vote_state_resets_after_success"])
        self.assertTrue(forfeit["vote_state_resets_on_timeout"])

        game_draw = json.loads(rows["global-draw-vote-match-cancel"]["parameters_json"])
        self.assertEqual(game_draw["vote_timeout_seconds"], 60)
        self.assertEqual(game_draw["default_player_removal_delay_seconds"], 15)
        self.assertTrue(game_draw["success_sets_match_end_flag_dY"])
        self.assertEqual(game_draw["success_flags_both_team_forces_in_MMD_with_value"], 0)
        self.assertEqual(game_draw["removes_all_ingame_players_with_result"], "PLAYER_GAME_RESULT_NEUTRAL")
        self.assertTrue(game_draw["remove_player_preserves_units"])

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_session_mechanic_rows"], 7)

    def test_runtime_ai_damage_signals_rescue_strike_and_strategic_aura_observer_are_normalized(self) -> None:
        with (self.resolved / "runtime-ai-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(
            set(rows),
            {
                "ai-engagement-damage-signals", "ai-executor-periodic-fsm",
                "ai-rescue-strike-controller", "ai-strategic-aura-purchase-observer",
                "ai-team-production-sync-targeting",
            },
        )

        engagement = json.loads(rows["ai-engagement-damage-signals"]["parameters_json"])
        self.assertFalse(engagement["rewrites_damage"])
        self.assertEqual(engagement["decay_period_seconds"], 2.0)
        self.assertEqual(engagement["decay_factor_per_period"], 0.8)
        self.assertEqual(engagement["engagement_sample_position"], "midpoint(source-position,target-position)")
        self.assertEqual(engagement["engagement_sample_weight"], "damage-amount")
        self.assertFalse(engagement["structure_target_contributes_to_engagement_centroid"])
        self.assertEqual(engagement["engagement_axis_invalid_value"], -1.0)
        self.assertEqual(
            engagement["structure_push_dominance_formula"],
            "clamp((team-damage-other-team-damage)/(team-damage+other-team-damage),-1,1)",
        )

        executor = json.loads(rows["ai-executor-periodic-fsm"]["parameters_json"])
        self.assertEqual(executor["ai_start_delay_seconds"], 0.1)
        self.assertEqual(executor["builder_idle_order_id"], 852064)
        self.assertEqual(executor["pending_build_order_timeout_seconds"], 90.0)
        self.assertEqual(executor["pending_build_order_stale_threshold_seconds"], 8.0)
        self.assertTrue(executor["pending_build_timeout_requires_matching_building_id_and_token"])
        self.assertEqual(executor["quick_build_disabled_below_base_speed"], 0.45)
        self.assertEqual(executor["quick_build_guaranteed_at_or_above_base_speed"], 0.9)
        self.assertEqual(executor["quick_build_delay_no_profile_seconds"], 0.45)
        self.assertEqual(executor["quick_build_delay_formula_seconds"], "0.45 + (1.0-campaign_ai_speed)*0.8")
        self.assertEqual(executor["rescue_strike_state_point_order_id"], 852488)
        self.assertEqual(executor["fsm_update_period_seconds"], 1.0)
        self.assertEqual(executor["startup_delay_min_seconds"], 0.02)
        self.assertEqual(executor["startup_delay_max_seconds"], 0.845)
        self.assertTrue(executor["startup_generation_must_still_match"])
        self.assertTrue(executor["does_not_start_second_periodic_callback_if_one_exists"])

        rescue = json.loads(rows["ai-rescue-strike-controller"]["parameters_json"])
        self.assertFalse(rescue["rewrites_damage"])
        self.assertEqual(rescue["target_hp_ratio_below_exclusive"], 0.65)
        self.assertEqual(rescue["evaluation_debounce_seconds_per_team"], 0.25)
        self.assertEqual(rescue["global_commit_lock_seconds"], 3.0)
        self.assertEqual(rescue["effect_radius"], 700.0)
        self.assertEqual(rescue["candidate_search_radius_around_damaged_structure"], 900.0)
        self.assertEqual(rescue["maximum_candidate_units_scored"], 16)
        self.assertEqual(rescue["tower_target_vs_siege_attacker_score_delta"], -4)
        self.assertEqual(rescue["required_score_hp_ratio_clamp"], [0.2, 0.65])
        self.assertEqual(
            rescue["required_score_formula"],
            "2 + 14*((clamp(hp_ratio,0.20,0.65)-0.20)/0.45)",
        )
        self.assertTrue(rescue["comparison_is_strict"])
        self.assertEqual(rescue["non_castle_recent_commit_throttle_seconds"], 28.0)
        self.assertEqual(rescue["same_area_distance_threshold"], 900.0)
        self.assertEqual(rescue["same_area_repeat_throttle_seconds"], 75.0)
        self.assertTrue(rescue["castle_targets_bypass_repeat_throttles"])
        self.assertEqual(rescue["target_filter"], "alive-combat-sapper;enemy-of-mIb")
        ability = rescue["rescue_strike_ability"]
        self.assertEqual(ability["rawcode"], "A005")
        self.assertEqual(ability["effective_mana_cost"], 0.0)
        self.assertEqual(ability["effective_cooldown_seconds"], 60.0)
        self.assertEqual(ability["static_mana_cost"], 9999.0)
        self.assertEqual(ability["static_cooldown_seconds"], 99.0)
        self.assertEqual(ability["range"], 1000.0)
        self.assertEqual(ability["area"], 700.0)

        sync = json.loads(rows["ai-team-production-sync-targeting"]["parameters_json"])
        self.assertEqual(sync["target_order_id"], 851980)
        self.assertEqual(sync["team_indices_used"], [0, 1])
        self.assertTrue(sync["orders_are_deferred_with_null_timer"])
        self.assertEqual(sync["team_representative_selection"], "highest CFBuilding_spawnTime within each partition")
        self.assertEqual(sync["team_partition"], "normal and legendary production buildings are synchronized independently")
        self.assertTrue(sync["candidate_rejects_cage_buildings"])

        strategic = json.loads(rows["ai-strategic-aura-purchase-observer"]["parameters_json"])
        self.assertFalse(strategic["rewrites_gameplay_event"])
        self.assertTrue(strategic["human_and_ai_purchases_are_observed"])
        self.assertTrue(strategic["already_bought_item_is_removed_from_future_strategic_candidates"])
        self.assertTrue(strategic["state_resets_with_ai_round_state"])
        self.assertEqual(
            [(item["rawcode"], item["name"], item["gold_cost"]) for item in strategic["strategic_aura_items"]],
            [
                ("I00A", "Drum'n'Bass Bassline Generator", 600.0),
                ("I003", "Drum'n'Bass Bass Drums", 650.0),
            ],
        )

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_ai_mechanic_rows"], 5)

    def test_all_proven_live_perk_mechanics_are_importer_ready(self) -> None:
        with (self.resolved / "perk-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["perk_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(set(rows), {f"perk_{index:02d}" for index in range(1, 20)})

        arcane = json.loads(rows["perk_01"]["parameters_json"])
        self.assertEqual(arcane["unit_mana_regen_delta"], 0.5)
        self.assertEqual(arcane["special_building_income_factors"], [0.12, 0.09])
        self.assertEqual(arcane["special_building_mana_regen_delta"], -0.3)

        cloudchaser = json.loads(rows["perk_02"]["parameters_json"])
        self.assertEqual(cloudchaser["builder_item_charges"], 4)
        self.assertEqual(cloudchaser["siege_unit_base_damage_factor"], 0.8)
        self.assertEqual(cloudchaser["portable_cloud_item"]["rawcode"], "IcfS")
        self.assertEqual(cloudchaser["portable_cloud_item"]["uses"], 4.0)
        self.assertEqual(cloudchaser["portable_cloud_effect_ability"]["rawcode"], "AcfS")
        self.assertEqual(cloudchaser["portable_cloud_effect_ability"]["area"], 225.0)

        feathered = json.loads(rows["perk_03"]["parameters_json"])
        self.assertEqual(feathered["food_intervals_seconds"], [600, 450, 300, 150, 120, 60])
        self.assertEqual(feathered["food_consumption_times_from_round_start_seconds"], [600, 1050, 1350, 1500, 1620, 1680])
        self.assertEqual(feathered["bird_base_damage_initial"], 25)
        self.assertEqual(feathered["bird_base_damage_gain"], 12)
        self.assertEqual(feathered["bird_damage_step_seconds"], 300)
        self.assertEqual(feathered["bird_food_item"]["rawcode"], "IM01")
        self.assertEqual(feathered["bird_unit"]["rawcode"], "x002")
        self.assertEqual(feathered["bird_unit"]["name"], "Feathered Friend")

        towerless = json.loads(rows["perk_04"]["parameters_json"])
        self.assertEqual(towerless["builder_item_id"], 1229795378)
        self.assertEqual(towerless["tiny_tower_unit_id"], 1747990577)
        self.assertEqual(towerless["towerless_item"]["rawcode"], "IM02")
        self.assertEqual(towerless["tiny_tower_unit"]["name"], "Tiny Watch Tower")
        self.assertEqual(towerless["tiny_tower_unit"]["attack1_range"], 900.0)
        self.assertEqual(towerless["tiny_tower_unit"]["attack1_dps"], 53.0)
        self.assertEqual(towerless["tiny_tower_build_ability"]["rawcode"], "AM04")
        self.assertFalse(towerless["initializer_related_multishot_runtime_attachment_proven"])
        self.assertEqual(towerless["initializer_related_multishot_ability"]["rawcode"], "AM05")

        glass = json.loads(rows["perk_05"]["parameters_json"])
        self.assertEqual(glass["max_hp_factor"], 0.85)
        self.assertEqual(glass["primary_weapon_base_damage_factor"], 1.25)
        self.assertFalse(glass["secondary_weapon_modified"])

        transcendent = json.loads(rows["perk_06"]["parameters_json"])
        self.assertEqual(transcendent["base_damage_delta_on_non_structure_index"], -15)
        self.assertEqual(transcendent["flat_triggering_damage_bonus"], 45)
        self.assertEqual(transcendent["bonus_target_defense_types"], ["none", "hero", "divine"])

        ground_control = json.loads(rows["perk_07"]["parameters_json"])
        self.assertEqual(ground_control["flying_damage_factor"], 1.18)
        self.assertEqual(ground_control["structure_damage_factor"], 0.85)
        self.assertTrue(ground_control["factors_multiply_when_target_matches_both"])

        caged = json.loads(rows["perk_08"]["parameters_json"])
        self.assertEqual(caged["spell_damage_factor_while_currently_caged"], 0.75)
        self.assertEqual(caged["base_damage_delta_on_qualifying_index"], -50)

        stance = json.loads(rows["perk_09"]["parameters_json"])
        self.assertEqual(stance["low_hp_threshold_exclusive"], 0.35)
        self.assertEqual(stance["high_hp_threshold_exclusive"], 0.70)
        self.assertEqual(stance["modes"]["execute"]["target_hp_ratio_below_0_35_factor"], 1.25)
        self.assertEqual(stance["modes"]["open-fire"]["target_hp_ratio_above_0_70_factor"], 1.18)
        self.assertEqual(stance["toggle_cooldown_seconds"], 10.0)
        self.assertEqual(
            [(ability["rawcode"], ability["name"], ability["cooldown"]) for ability in stance["toggle_abilities"]],
            [
                ("AM06", "Combat Stance: Execute", 10.0),
                ("AM07", "Combat Stance: Open Fire", 10.0),
            ],
        )

        breaching = json.loads(rows["perk_10"]["parameters_json"])
        self.assertEqual(breaching["structure_damage_factor"], 1.2)
        self.assertEqual(breaching["flying_damage_factor"], 0.75)
        self.assertTrue(breaching["factors_multiply_when_target_matches_both"])

        bulwark = json.loads(rows["perk_11"]["parameters_json"])
        self.assertEqual(bulwark["max_hp_factor"], 1.12)
        self.assertEqual(bulwark["move_speed_factor"], 0.93)
        self.assertEqual(bulwark["attack_speed_factor"], 0.93)
        self.assertEqual(bulwark["affected_weapon_indices"], [0, 1])

        forward = json.loads(rows["perk_12"]["parameters_json"])
        self.assertEqual(forward["outside_own_castle_armor_bonus"], 3)
        self.assertEqual(forward["inside_own_castle_armor_bonus"], -2)
        self.assertEqual(
            [(ability["rawcode"], ability["object_data"]["Defense Bonus"]) for ability in forward["armor_abilities"]],
            [("AM08", 3), ("AM09", -2)],
        )

        longline = json.loads(rows["perk_13"]["parameters_json"])
        self.assertEqual(longline["advertised_attack_range_delta"], 90)
        self.assertEqual(longline["script_range_reference_weapon_index"], 0)
        self.assertEqual(longline["script_range_write_weapon_index"], 1)
        self.assertTrue(longline["primary_weapon_range_is_not_directly_written"])
        self.assertEqual(longline["acquisition_range_update_condition"], "current-acquisition < weapon0 + 90")
        self.assertEqual(longline["acquisition_range_on_update"], "weapon0 + 140")
        self.assertEqual(longline["acquisition_range_otherwise"], "unchanged")
        self.assertEqual(longline["attack_speed_factor"], 0.88)

        production_enchantment = json.loads(rows["perk_14"]["parameters_json"])
        self.assertEqual(production_enchantment["all_owned_trained_combat_sapper_max_hp_factor"], 0.95)
        self.assertEqual(production_enchantment["selected_building_spawn_max_hp_factor"], 1.15)
        self.assertEqual(production_enchantment["selected_building_spawn_nominal_net_hp_factor_after_global_penalty"], 1.0925)
        self.assertEqual(production_enchantment["selected_bonus_deferred_via_nested_zero_timers"], 2)
        self.assertTrue(production_enchantment["each_scaling_step_rounds_independently"])
        self.assertEqual(production_enchantment["max_hp_current_life_when_increasing"], "old-life + (new-max-old-max)")
        self.assertEqual(production_enchantment["max_hp_current_life_when_decreasing"], "min(old-life,new-max)")
        self.assertEqual(
            [ability["rawcode"] for ability in production_enchantment["enchantment_abilities"]],
            ["AM0a", "AM0b", "AM0c"],
        )

        rampant = json.loads(rows["perk_15"]["parameters_json"])
        self.assertEqual(rampant["hp_regeneration_delta_per_second"], 7.0)
        self.assertEqual(rampant["base_damage_factor"], 0.9)
        self.assertEqual(rampant["base_damage_weapon_indices"], [0, 1])

        mana = json.loads(rows["perk_16"]["parameters_json"])
        self.assertEqual(mana["damage_per_mana"], 3.0)
        self.assertTrue(mana["applies_to_attack_and_non_attack_damage"])
        self.assertTrue(mana["eligible_excludes_peon"])

        spell_enchantment = json.loads(rows["perk_17"]["parameters_json"])
        self.assertEqual(spell_enchantment["selected_spell_building_mana_regen_delta"], 0.2)
        self.assertEqual(spell_enchantment["other_owned_spell_building_mana_regen_delta"], -0.05)
        self.assertTrue(spell_enchantment["unit_deindex_clears_stored_delta_for_reused_index"])
        self.assertEqual(
            [ability["rawcode"] for ability in spell_enchantment["enchantment_abilities"]],
            ["AM0d", "AM0e"],
        )

        spells_edge = json.loads(rows["perk_18"]["parameters_json"])
        self.assertEqual(spells_edge["per_owned_source_factor"], 1.15)
        self.assertEqual(spells_edge["per_owned_target_factor"], 1.15)
        self.assertEqual(spells_edge["owned_source_and_owned_target_factor"], 1.3225)
        self.assertTrue(spells_edge["enemy_relationship_not_required"])

        containment = json.loads(rows["perk_19"]["parameters_json"])
        self.assertEqual(containment["caged_target_damage_factor"], 1.2)
        self.assertEqual(containment["uncaged_target_damage_factor"], 0.9)
        self.assertEqual(containment["cage_test_owner"], "damage-target-owner")

        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["perk_mechanic_rows"], 19)

    def test_wurst_generated_object_marker_is_classified_as_compiler_provenance(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["unknown_map_field_ids"], {})
        self.assertEqual(summary["wurst_generated_object_marker_rows"], 1309)
        self.assertEqual(summary["wurst_generated_object_marker_value"], 42)
        with (self.resolved / "object-fields.tsv").open(encoding="utf-8") as handle:
            marker = next(row for row in csv.DictReader(handle, delimiter="\t") if row["field_id"] == "wurs")
        self.assertEqual(marker["field_name"], "GENERATED_BY_WURST")
        self.assertEqual(marker["display_name"], "Wurst generated-object marker")
        self.assertEqual(marker["source_table"], "WurstCompiler")
        self.assertEqual(marker["recovered_value_json"], "42")

    def test_resolved_items_keep_helper_recipe_and_attached_ability_multiplicity(self) -> None:
        with (self.resolved / "items.tsv").open(encoding="utf-8") as handle:
            items = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(items["I001"]["name"], "Blast Staff")
        self.assertEqual(items["I001"]["abilities"], "A02D")
        self.assertEqual(items["I001"]["gold_cost"], "150")
        self.assertEqual(items["I001"]["lumber_cost"], "300")
        self.assertEqual(items["I002"]["name"], "Multi Blast Staff")
        self.assertEqual(items["I002"]["abilities"], "A02D,A02D,A02D,A02D")
        self.assertEqual(items["I002"]["gold_cost"], "600")
        self.assertEqual(items["I002"]["lumber_cost"], "1200")

    def test_castle_shop_and_scripted_item_mechanics_are_native_ready(self) -> None:
        with (self.resolved / "castle-shop-items.tsv").open(encoding="utf-8") as handle:
            shop = {row["item_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(shop), 10)
        self.assertEqual(shop["I004"]["gold_cost"], "1750")
        self.assertEqual(shop["I004"]["stock_regen"], "210")
        orb_abilities = json.loads(shop["I006"]["ability_objects_json"])
        self.assertEqual(orb_abilities[0]["rawcode"], "A02H")
        self.assertEqual(orb_abilities[0]["levels"][0]["cooldown"], "60.0")
        self.assertEqual(orb_abilities[0]["levels"][0]["static_cooldown"], "99")

        with (self.resolved / "item-mechanics.tsv").open(encoding="utf-8") as handle:
            mechanics = list(csv.DictReader(handle, delimiter="\t"))
        by_kind = {(row["item_rawcode"], row["mechanic_kind"]): row for row in mechanics}

        cheese = json.loads(by_kind[("I004", "legendary-slot-or-refund-on-pickup")]["parameters_json"])
        self.assertEqual(cheese["food_cap_delta_when_active"], 1)
        self.assertEqual(cheese["refund_gold_when_inactive"], 1750)

        recipe = by_kind[("I001", "four-copy-inventory-upgrade")]
        self.assertEqual(recipe["related_item_rawcodes"], "I002,I001")
        recipe_parameters = json.loads(recipe["parameters_json"])
        self.assertEqual(recipe_parameters["required_count"], 4)
        self.assertEqual(recipe_parameters["add_item_rawcode"], "I002")
        result_item = json.loads(recipe["related_item_objects_json"])[0]
        self.assertEqual(result_item["name"], "Multi Blast Staff")
        self.assertEqual(result_item["abilities"], ["A02D", "A02D", "A02D", "A02D"])

        orb = by_kind[("I006", "target-triggered-scaling-dummy-effect")]
        self.assertEqual(orb["trigger_ability_rawcodes"], "A02H")
        orb_effect = json.loads(orb["effect_ability_objects_json"])[0]
        self.assertEqual(orb_effect["rawcode"], "cfOL")
        self.assertEqual([level["data_fields_labeled"]["Number of Targets Hit"] for level in orb_effect["levels"]], [4, 6, 8, 10, 12])
        self.assertTrue(all(level["data_fields_labeled"]["Damage per Target"] == 1000 for level in orb_effect["levels"]))

        stone = json.loads(by_kind[("I005", "point-triggered-dummy-effect")]["effect_ability_objects_json"])[0]["levels"][0]
        self.assertEqual(stone["duration_normal"], "30")
        self.assertEqual(stone["data_fields_labeled"], {"Defense Bonus": 8, "Hit Points Gained": 400, "Mana Points Gained": 200})
        speed = json.loads(by_kind[("I00F", "point-triggered-dummy-effect")]["effect_ability_objects_json"])[0]["levels"][0]
        self.assertEqual(speed["duration_normal"], "15")
        self.assertEqual(speed["data_fields_labeled"]["Movement Speed Increase"], 2)

    def test_protection_conflicts_are_independently_confirmed(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["protection_conflicts"], 14)
        self.assertEqual(summary["protection_conflicts_runtime_unitstat_confirmed"], 13)
        self.assertEqual(summary["protection_conflicts_stable_final_write"], 1)
        self.assertEqual(summary["protection_conflict_selection_counts"], {
            "map-stable-final-write": 1,
            "w3p-runtime-unitstat-confirmed": 13,
        })

        with (self.resolved / "protection-conflicts.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        numeric = [row for row in rows if not (row["rawcode"] == "h07W" and row["field_id"] == "uabi")]
        self.assertEqual(len(numeric), 13)
        self.assertTrue(all(row["selection"] == "w3p-runtime-unitstat-confirmed" for row in numeric))

        snow_abilities = next(row for row in rows if row["rawcode"] == "h07W" and row["field_id"] == "uabi")
        self.assertEqual(snow_abilities["selection"], "map-stable-final-write")
        self.assertEqual(json.loads(snow_abilities["recovered_value_json"]), "A0HO,AM0{")

    def test_production_unit_special_mechanics_are_importer_ready(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["production_unit_special_mechanic_rows"], 35)
        self.assertEqual(summary["production_unit_special_mechanic_kinds"], {
            "auto-spawn-tree-and-grab-war-club": 1,
            "attack-damage-flying-target-multiplier": 1,
            "attack-proc-dispel-positive-buffs": 3,
            "attack-proc-ground-whirlwind-aoe": 1,
            "attack-proc-native-mirror-image": 1,
            "attack-stacking-corrosion": 1,
            "automatic-defend-state-maintenance": 1,
            "damage-to-mana-kaboom-charge": 1,
            "damage-triggered-auto-fan-of-knives": 1,
            "damage-triggered-echo-step-and-remnant": 1,
            "damage-triggered-feral-rage-and-hibernation": 2,
            "damage-triggered-persistent-low-hp-attack-bonus": 1,
            "death-retaliation-damage-to-killer": 1,
            "kill-heal-percent-max-hp": 2,
            "kill-triggered-native-berserk": 3,
            "native-lava-spawn-split-with-child-conversion": 1,
            "organic-kill-eternal-servitude": 2,
            "source-damage-health-scaled-aftershock": 2,
            "source-damage-mastery-over-death": 1,
            "source-damage-stacking-blood-corrosion": 1,
            "summon-event-mana-reset": 1,
            "target-damage-melee-thunderbolt-retaliation": 2,
            "train-event-random-initial-mana": 1,
            "train-event-set-exploded-flag": 1,
            "retarget-flying-damage-source": 2,
        })
        with (self.resolved / "production-unit-special-mechanics.tsv").open(encoding="utf-8") as handle:
            row_list = list(csv.DictReader(handle, delimiter="\t"))
        rows = {(row["unit_rawcode"], row["mechanic_kind"]): row for row in row_list}
        self.assertEqual(len(rows), 35)

        giant_row = rows[("e00F", "auto-spawn-tree-and-grab-war-club")]
        giant = json.loads(giant_row["parameters_json"])
        self.assertEqual(giant_row["building_rawcode"], "h028")
        self.assertEqual(giant["tree_forward_offset"], 32)
        self.assertEqual(giant["grab_tree_order_id"], 852511)
        self.assertEqual(giant["resume_attack_delay_seconds"], 1.6)
        self.assertEqual(giant["tree_total_lifetime_seconds"], 3.6)
        self.assertEqual(giant["war_club_object_data"]["Maximum Attacks"], 10)
        self.assertEqual(giant["war_club_object_data"]["Enabled Attack Index"], 1)

        echo_row = rows[("n03I", "damage-triggered-echo-step-and-remnant")]
        echo = json.loads(echo_row["parameters_json"])
        self.assertEqual(echo_row["building_rawcode"], "n03H")
        self.assertEqual(echo["trigger_range"], 250)
        self.assertEqual(echo["runtime_cooldown_seconds"], 15)
        self.assertEqual(echo["protected_ability_cooldown_seconds"], 20.0)
        self.assertTrue(echo["script_proc_cooldown_overrides_protected_ability_cooldown"])
        self.assertEqual(echo["blink_distance_toward_own_castle"], 1200)
        self.assertEqual(echo["remnant_unit_id"], 1848652626)
        self.assertEqual(echo["remnant_timed_life_seconds"], 60)
        self.assertEqual(echo["remnant_explosion_radius"], 300)
        self.assertEqual(echo["remnant_explosion_damage"], 150)
        self.assertEqual(echo["remnant_explosion_damage_type"], "magic")
        self.assertEqual(echo["remnant_target_filter_function"], "UC")
        self.assertIn("enemy-of-mIb", echo["remnant_target_predicate"])
        self.assertEqual(echo["remnant_activation_object_data"]["Activation Delay"], 0.75)

        for rawcode in ("n02S", "n02T"):
            retaliation = json.loads(rows[(rawcode, "retarget-flying-damage-source")]["parameters_json"])
            self.assertTrue(retaliation["trigger_source_requires_flying"])
            self.assertEqual(retaliation["retaliation_target"], "damage-source")
            self.assertEqual(retaliation["issued_order"], "attack")
            self.assertEqual(retaliation["per_unit_retarget_throttle_seconds"], 2.5)

        defender = json.loads(rows[("h03A", "automatic-defend-state-maintenance")]["parameters_json"])
        self.assertEqual(defender["defend_order_id"], 852055)
        self.assertEqual(defender["undefend_order_id"], 852056)
        self.assertEqual(defender["initial_defend_delay_seconds"], 0.7)
        self.assertEqual(defender["undefend_reactivation_delay_seconds"], 5.5)
        self.assertEqual(defender["defend_object_data"]["Damage Taken (%)"], 0.4)
        self.assertEqual(defender["defend_object_data"]["Chance to Deflect"], 50)

        fire_row = rows[("u00F", "native-lava-spawn-split-with-child-conversion")]
        fire = json.loads(fire_row["parameters_json"])
        self.assertEqual(fire_row["building_rawcode"], "h046")
        self.assertEqual(fire["trained_proxy_unit_id"], 1966092358)
        self.assertEqual(fire["trained_proxy_marker_ability_id"], 1093682266)
        self.assertEqual(fire["trained_proxy_lava_spawn_ability_id"], 1093682265)
        self.assertTrue(fire["trained_proxy_hidden_immediately"])
        self.assertEqual(fire["trained_proxy_immediate_order_id"], 852667)
        self.assertEqual(fire["trained_proxy_timed_life_seconds"], 1.0)
        self.assertFalse(fire["trained_proxy_is_gameplay_combat_body"])
        self.assertEqual(fire["trained_proxy_rawcode"], "u00F")
        self.assertEqual(fire["trained_proxy_static_hp"], 1.0)
        self.assertEqual(fire["trained_proxy_static_hp_regen_per_second"], -2.0)
        self.assertEqual(fire["trained_proxy_static_model"], "no_model.mdl")
        self.assertFalse(fire["trained_proxy_static_stats_are_gameplay_authoritative"])
        self.assertEqual(fire["runtime_combat_unit_id"], 1747989296)
        self.assertEqual(fire["runtime_combat_unit_rawcode"], "h030")
        self.assertEqual(fire["runtime_combat_unit_hp"], 900.0)
        self.assertEqual(fire["runtime_combat_unit_hp_regen_per_second"], 1.0)
        self.assertEqual(fire["runtime_combat_unit_hp_regen_type"], "always")
        self.assertEqual(fire["runtime_combat_unit_armor"], 7.0)
        self.assertEqual(fire["runtime_combat_unit_dps"], 48.4375)
        self.assertEqual(fire["runtime_combat_unit_attack_range"], 450.0)
        self.assertEqual(fire["runtime_combat_unit_move_speed"], 260.0)
        self.assertEqual(fire["runtime_combat_unit_model"], "Units\\Creeps\\HeroFlameLord\\HeroFlameLord.mdl")
        self.assertEqual(fire["native_split_base_ability_rawcode"], "ANlm")
        self.assertEqual(fire["native_split_summoned_unit_rawcode"], "h030")
        self.assertEqual(fire["native_split_object_data"]["Split Attack Count"], 15)
        self.assertEqual(fire["native_split_object_data"]["Split Delay"], 2)
        self.assertEqual(fire["native_split_object_data"]["Max Hitpoint Factor"], 0.5)
        self.assertEqual(fire["native_split_object_data"]["Generation Count"], 3)
        self.assertEqual(fire["native_split_object_data"]["Summoned Unit Count"], 1)
        self.assertEqual(fire["native_split_effective_cooldown"], 0.1)
        self.assertEqual(fire["nested_h030_child_replacement_unit_id"], 1747989082)
        self.assertTrue(fire["remove_summoned_type_from_h030_child"])
        self.assertEqual(fire["post_child_attack_order_delay_seconds"], 0.2)
        self.assertTrue(fire["split_child_inherits_spawn_building_provenance"])
        self.assertEqual(fire["split_child_provenance_parent_unit_id"], 1966092358)
        self.assertEqual(fire["split_child_provenance_child_unit_id"], 1747989296)
        self.assertIn("onUnitTrained", fire_row["source_functions"])
        self.assertIn("EventListener_add_PerkUtils_onEvent_add_PerkUtils", fire_row["source_functions"])
        self.assertIn("setSpawnBuilding", fire_row["source_functions"])

        avatar_death = json.loads(rows[("e00A", "death-retaliation-damage-to-killer")]["parameters_json"])
        self.assertEqual(avatar_death["damage"], 350)
        self.assertEqual(avatar_death["attack_type"], "chaos")
        self.assertEqual(avatar_death["damage_target"], "killer")

        avenging_heal = json.loads(rows[("e00B", "kill-heal-percent-max-hp")]["parameters_json"])
        avatar_heal = json.loads(rows[("e00A", "kill-heal-percent-max-hp")]["parameters_json"])
        self.assertEqual(avenging_heal["heal_percent_of_max_hp"], 14)
        self.assertEqual(avatar_heal["heal_percent_of_max_hp"], 20)

        vampire = json.loads(rows[("h01U", "organic-kill-eternal-servitude")]["parameters_json"])
        lord = json.loads(rows[("h05H", "organic-kill-eternal-servitude")]["parameters_json"])
        self.assertEqual(vampire["spawned_unit_id"], 1747988822)
        self.assertEqual(lord["spawned_unit_id"], 1747988821)
        self.assertTrue(vampire["remove_original_dead_unit"])
        self.assertTrue(lord["victim_requires_not_undead"])

        riptide_row = rows[("n03U", "attack-damage-flying-target-multiplier")]
        riptide = json.loads(riptide_row["parameters_json"])
        self.assertIn("DamageListener_addListener_RiptideAttack", riptide_row["source_functions"])
        self.assertEqual(riptide["bonus_fraction"], 0.45)
        self.assertEqual(riptide["damage_multiplier"], 1.45)
        self.assertTrue(riptide["target_requires_flying"])
        self.assertEqual(riptide["required_damage_event_type"], 0)
        self.assertTrue(riptide["modifies_current_damage_instance"])

        troll_blood = json.loads(rows[("n02G", "damage-triggered-persistent-low-hp-attack-bonus")]["parameters_json"])
        self.assertEqual(
            [(entry["hp_ratio_below"], entry["attack_bonus"]) for entry in troll_blood["thresholds"]],
            [(0.75, 30), (0.5, 60), (0.25, 90)],
        )
        self.assertTrue(troll_blood["levels_only_increase"])
        self.assertTrue(troll_blood["healing_does_not_downgrade_reached_level"])
        self.assertTrue(troll_blood["mechanic_absent_from_unit_tooltip"])

        whirlwind = json.loads(rows[("n03J", "attack-proc-ground-whirlwind-aoe")]["parameters_json"])
        self.assertEqual(whirlwind["proc_probability"], 0.2)
        self.assertEqual(whirlwind["radius"], 160)
        self.assertEqual(whirlwind["damage"], 175)
        self.assertEqual(whirlwind["tooltip_damage"], 150)
        self.assertTrue(whirlwind["tooltip_damage_disagrees_with_runtime"])
        self.assertEqual(whirlwind["target_predicate"], "enemy-of-source;alive;combat-sapper;not-flying")
        self.assertEqual(whirlwind["damage_type"], "universal")

        for rawcode in ("n00V", "n01O", "n02G"):
            berserk = json.loads(rows[(rawcode, "kill-triggered-native-berserk")]["parameters_json"])
            self.assertEqual(berserk["berserk_order_id"], 852100)
            self.assertEqual(berserk["berserk_base_order"], "berserk")
            self.assertEqual(berserk["berserk_duration_seconds"], 6)
            self.assertEqual(berserk["berserk_effective_cooldown_seconds"], 0.0)
            self.assertEqual(berserk["berserk_object_data"]["Attack Speed Increase"], 2)
            self.assertEqual(berserk["berserk_object_data"]["Movement Speed Increase"], 0.25)
            self.assertEqual(berserk["berserk_object_data"]["Damage Taken Increase"], 0.1)

        for rawcode, chance, radius in (("e006", 15, 0), ("e009", 20, 0), ("e00C", 25, 100)):
            dispel = json.loads(rows[(rawcode, "attack-proc-dispel-positive-buffs")]["parameters_json"])
            self.assertEqual(dispel["proc_chance_percent"], chance)
            self.assertEqual(dispel["effect_radius"], radius)
            self.assertEqual(dispel["unit_remove_buffs_ex_args"], [True, False, True, False, False, False, False])
        ancient_dispel = json.loads(rows[("e00C", "attack-proc-dispel-positive-buffs")]["parameters_json"])
        self.assertEqual(ancient_dispel["aoe_target_predicate"], "isDispellableEnemyOf")

        bear = json.loads(rows[("n029", "damage-triggered-feral-rage-and-hibernation")]["parameters_json"])
        ancient_bear = json.loads(rows[("n02B", "damage-triggered-feral-rage-and-hibernation")]["parameters_json"])
        self.assertEqual(bear["feral_rage_proc_chance_percent"], 15)
        self.assertEqual(bear["feral_rage_damage_object_data"]["Damage Increase (%)"], 0.2)
        self.assertEqual(bear["feral_rage_attack_speed_object_data"]["Attack Speed Increase (%)"], 0.2)
        self.assertEqual(bear["feral_rage_buff_duration_seconds"], 10)
        self.assertEqual(bear["hibernate_trigger_life_below"], 255)
        self.assertEqual(bear["hibernate_retreat_seconds"], 5)
        self.assertEqual(bear["hibernate_sleep_seconds"], 10)
        self.assertEqual(bear["hibernate_regen_object_data"]["Hit Points Regenerated Per Second"], 20)
        self.assertEqual(ancient_bear["feral_rage_proc_chance_percent"], 20)
        self.assertEqual(ancient_bear["feral_rage_damage_object_data"]["Damage Increase (%)"], 0.3)
        self.assertEqual(ancient_bear["hibernate_trigger_life_below"], 326)
        self.assertEqual(ancient_bear["hibernate_regen_object_data"]["Hit Points Regenerated Per Second"], 40)
        self.assertEqual(ancient_bear["hibernate_extra_sleep_object_data"]["Attack Bonus"], 115)

        razormane = json.loads(rows[("n02U", "damage-triggered-auto-fan-of-knives")]["parameters_json"])
        self.assertEqual(razormane["effective_mana_cost"], 40)
        self.assertEqual(razormane["effective_cooldown_seconds"], 1.0)
        self.assertEqual(razormane["area"], 400)
        self.assertEqual(razormane["ability_object_data"]["Damage Per Target"], 45)

        wind = json.loads(rows[("o00E", "damage-to-mana-kaboom-charge")]["parameters_json"])
        self.assertEqual(wind["detonation_threshold"], 680)
        self.assertEqual(wind["charge_delta"], "event-damage-amount")
        self.assertEqual(wind["kaboom_object_data"]["Full Damage Amount"], 375)
        self.assertEqual(wind["kaboom_object_data"]["Full Damage Radius"], 250)
        self.assertEqual(wind["kaboom_object_data"]["Partial Damage Amount"], 225)
        self.assertEqual(wind["kaboom_object_data"]["Partial Damage Radius"], 350)

        emerald = json.loads(rows[("n02A", "attack-stacking-corrosion")]["parameters_json"])
        self.assertEqual(emerald["maximum_stack_level"], 3)
        self.assertEqual([entry["defense_bonus"] for entry in emerald["stack_levels"]], [-2, -4, -6])

        water = json.loads(rows[("h02Y", "attack-proc-native-mirror-image")]["parameters_json"])
        self.assertEqual(water["proc_chance_percent"], 20)
        self.assertEqual(water["mirror_image_order_id"], 852123)
        self.assertEqual(water["mirror_image_object_data"]["Number of Images"], 1)
        self.assertEqual(water["mirror_image_object_data"]["Damage Dealt (%)"], 0.6)
        self.assertEqual(water["mirror_image_object_data"]["Damage Taken (%)"], 2)
        self.assertEqual(water["mirror_image_duration_seconds"], 60)
        self.assertEqual(water["mirror_image_effective_mana_cost"], 0)

        lich = json.loads(rows[("u00D", "source-damage-mastery-over-death")]["parameters_json"])
        self.assertEqual(lich["devour_roll_threshold_exclusive"], 15)
        self.assertEqual(lich["devour_heal_fraction_of_target_current_hp"], 0.5)
        self.assertEqual(lich["devour_damage"], 10000)
        self.assertEqual(lich["death_and_decay_probability_if_devour_eligible_percent"], 10)
        self.assertEqual(lich["death_and_decay_probability_if_devour_ineligible_percent"], 25)
        self.assertEqual(lich["death_and_decay_object_data"]["Max Life Drained per Second (%)"], 0.08)
        self.assertEqual(lich["death_and_decay_object_data"]["Building Reduction"], 0.1)
        self.assertEqual(lich["death_and_decay_duration_seconds"], 5)
        self.assertEqual(lich["death_and_decay_area"], 350)
        self.assertEqual(lich["death_and_decay_effective_mana_cost"], 0)
        self.assertEqual(lich["death_and_decay_effective_cooldown_seconds"], 0.0)

        lord_corrosion = json.loads(rows[("h05H", "source-damage-stacking-blood-corrosion")]["parameters_json"])
        self.assertEqual(lord_corrosion["maximum_stack_level"], 5)
        self.assertEqual(
            [entry["defense_bonus"] for entry in lord_corrosion["stack_levels"]],
            [-2, -4, -6, -8, -10],
        )

        earth = json.loads(rows[("h034", "source-damage-health-scaled-aftershock")]["parameters_json"])
        greater_earth = json.loads(rows[("h03N", "source-damage-health-scaled-aftershock")]["parameters_json"])
        self.assertEqual(earth["object_unit_level"], 1)
        self.assertEqual(earth["maximum_bonus_damage_at_full_hp"], 50)
        self.assertEqual(greater_earth["object_unit_level"], 2)
        self.assertEqual(greater_earth["maximum_bonus_damage_at_full_hp"], 100)
        self.assertEqual(earth["damage_type"], "demolition")

        lightning = json.loads(rows[("h03P", "target-damage-melee-thunderbolt-retaliation")]["parameters_json"])
        greater_lightning = json.loads(rows[("h03R", "target-damage-melee-thunderbolt-retaliation")]["parameters_json"])
        self.assertEqual(lightning["effective_proc_probability_percent"], 16)
        self.assertEqual(lightning["thunderbolt_object_data"]["Damage"], 25)
        self.assertEqual(lightning["thunderbolt_duration_seconds"], 2)
        self.assertEqual(greater_lightning["effective_proc_probability_percent"], 31)
        self.assertEqual(greater_lightning["thunderbolt_object_data"]["Damage"], 50)
        self.assertEqual(greater_lightning["thunderbolt_duration_seconds"], 4)
        self.assertEqual(greater_lightning["thunderbolt_order_id"], 852095)

        paladin_summon = json.loads(rows[("h03C", "summon-event-mana-reset")]["parameters_json"])
        self.assertEqual(paladin_summon["set_mana_to"], 30)
        self.assertEqual(paladin_summon["static_object_mana_start"], 200)
        self.assertEqual(paladin_summon["mana_max"], 300)

        mine_init = json.loads(rows[("h06U", "train-event-random-initial-mana")]["parameters_json"])
        self.assertEqual(mine_init["mana_random_min"], 0.2)
        self.assertEqual(mine_init["mana_random_max"], 30)
        self.assertEqual(mine_init["static_object_mana_start"], 12)
        self.assertEqual(mine_init["mana_max"], 50)
        self.assertEqual(mine_init["mine_spell_effective_mana_cost"], 25)

        rocketeer = json.loads(rows[("n02M", "train-event-set-exploded-flag")]["parameters_json"])
        self.assertTrue(rocketeer["set_unit_exploded"])
        self.assertEqual(rocketeer["death_explosion_ability_rawcode"], "A0FZ")
        self.assertEqual(rocketeer["death_explosion_object_data"]["Full Damage Amount"], 325)
        self.assertEqual(rocketeer["death_explosion_object_data"]["Full Damage Radius"], 160)
        self.assertEqual(rocketeer["death_explosion_object_data"]["Partial Damage Amount"], 175)
        self.assertEqual(rocketeer["death_explosion_object_data"]["Partial Damage Radius"], 320)

    def test_production_unit_runtime_coverage_is_closed(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["production_unit_runtime_coverage_rows"], 34)
        self.assertEqual(summary["production_unit_runtime_coverage_status_counts"], {
            "parent-special-endpoint": 1,
            "special-mechanic": 24,
            "special-mechanic+unit-spell": 8,
            "verified-visual-only": 1,
        })
        with (self.resolved / "production-unit-runtime-coverage.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 34)
        self.assertEqual(rows["n01B"]["coverage_status"], "verified-visual-only")
        self.assertIn("vertex color 82,0,135,102", rows["n01B"]["coverage_note"])
        self.assertEqual(rows["h02Z"]["coverage_status"], "parent-special-endpoint")
        self.assertIn("u00F split row", rows["h02Z"]["coverage_note"])
        for rawcode in ("h034", "h03N"):
            self.assertEqual(rows[rawcode]["marker_runtime_hooks"], "ability-marker:A0DW")
            self.assertIn("source-damage-health-scaled-aftershock", rows[rawcode]["special_mechanic_kinds"])
        for rawcode in ("h03P", "h03R"):
            self.assertEqual(rows[rawcode]["marker_runtime_hooks"], "ability-marker:A0DX")
            self.assertIn("target-damage-melee-thunderbolt-retaliation", rows[rawcode]["special_mechanic_kinds"])
        self.assertEqual(rows["n03U"]["explicit_runtime_functions"], "hL")
        self.assertIn("attack-damage-flying-target-multiplier", rows["n03U"]["special_mechanic_kinds"])
        self.assertEqual(rows["n03J"]["marker_runtime_hooks"], "ability-marker:A0HG")
        self.assertIn("attack-proc-ground-whirlwind-aoe", rows["n03J"]["special_mechanic_kinds"])

    def test_ability_and_buff_runtime_coverage_is_closed(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["ability_runtime_coverage_rows"], 723)
        self.assertEqual(summary["ability_runtime_coverage_status_counts"], {
            "control-ui-helper": 32,
            "e2e-only": 1,
            "marker-data-only": 21,
            "native-engine-implicit": 2,
            "native-object-indirect": 27,
            "native-object-owned": 244,
            "native-object-owned+normalized-script": 169,
            "protected-runtime-overlay": 46,
            "runtime-granted-normalized": 53,
            "runtime-preload-only": 1,
            "scripted-normalized": 83,
            "unresolved-reachability": 1,
            "unused-orphaned": 43,
        })
        self.assertEqual(summary["buff_runtime_coverage_rows"], 100)
        self.assertEqual(summary["buff_runtime_coverage_status_counts"], {
            "native-ability-linked": 81,
            "native-ability-linked+normalized-script": 6,
            "scripted-normalized": 1,
            "unresolved-reachability": 2,
            "unused-orphaned": 10,
        })
        self.assertEqual(summary["ability_buff_unresolved_script_semantics"], [])

        with (self.resolved / "ability-runtime-coverage.tsv").open(encoding="utf-8") as handle:
            abilities = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        with (self.resolved / "buff-runtime-coverage.tsv").open(encoding="utf-8") as handle:
            buffs = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(abilities), 723)
        self.assertEqual(len(buffs), 100)

        self.assertEqual(abilities["AUbu"]["coverage_status"], "native-engine-implicit")
        self.assertEqual(abilities["ARal"]["coverage_status"], "native-engine-implicit")
        self.assertEqual(abilities["ACrj"]["coverage_status"], "unused-orphaned")
        self.assertEqual(abilities["Ahsb"]["coverage_status"], "unused-orphaned")
        self.assertEqual(abilities["A0DA"]["coverage_status"], "runtime-preload-only")
        self.assertEqual(abilities["A0ES"]["coverage_status"], "unresolved-reachability")
        self.assertEqual(
            {rawcode for rawcode, row in abilities.items() if "unresolved" in row["coverage_status"]},
            {"A0ES"},
        )

        for rawcode in ("B006", "B00B", "B00O", "B013", "B01A", "B01G", "B01Z", "B027"):
            self.assertEqual(buffs[rawcode]["coverage_status"], "unused-orphaned")
        self.assertEqual(buffs["B000"]["script_reference_class"], "none")
        self.assertEqual(buffs["B001"]["script_reference_class"], "none")
        self.assertEqual(buffs["Xbdt"]["coverage_status"], "native-ability-linked")
        self.assertEqual(buffs["Xbdt"]["parent_ability_links"], "A9FS:aeff")
        self.assertEqual(buffs["B00D"]["coverage_status"], "unresolved-reachability")
        self.assertEqual(buffs["B029"]["coverage_status"], "unresolved-reachability")
        self.assertEqual(
            {rawcode for rawcode, row in buffs.items() if "unresolved" in row["coverage_status"]},
            {"B00D", "B029"},
        )

    def test_runtime_system_mechanics_are_importer_ready(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["runtime_system_mechanic_rows"], 31)
        self.assertEqual(summary["runtime_system_mechanic_kinds"], {
            "allied-production-queue-synchronization-and-order-reset": 1,
            "area-building-buffs-cleanse-and-spawn-augmentation": 1,
            "catalog-authoritative-building-income-cache-correction": 1,
            "buff-marker-non-attack-current-damage-multiplier": 1,
            "body-replacement-plus-independent-random-trait-groups": 1,
            "consumable-building-spell-shields-and-goblin-shredder-overheat": 1,
            "builder-point-teleport-clamped-to-own-castle": 1,
            "builder-point-cast-team-coordinated-area-execution": 1,
            "tower-damage-impact-batched-temporary-vision": 1,
            "building-random-enemy-base-attack-ground-controller": 1,
            "per-player-building-count-income-multiplier": 1,
            "periodic-assassin-ambush-and-gobbo-repair-order-controller": 1,
            "player-order-rejection-autonomous-order-restoration-and-escalating-control-penalty": 1,
            "periodic-idle-combat-unit-attack-order-recovery": 1,
            "enchantment-marker-driven-half-damage-cleave": 1,
            "first-fifteen-seconds-castle-damage-immunity": 1,
            "persistent-carrier-auto-attack-damage-triggered-full-cleanse": 1,
            "unit-type-structure-target-current-damage-halving": 1,
            "summoned-carrier-random-unit-replacement": 1,
            "team-constructed-building-count-to-spell-level": 1,
            "team-presence-gated-owner-scaled-elemental-death-heal": 1,
            "team-shrine-independent-clone-rolls-and-train-transformations": 1,
            "team-stacked-one-time-delayed-unit-revival": 1,
            "spawn-area-marker-and-conditional-pathing-release-pulse": 1,
            "delayed-stock-trap-hiding-and-per-player-single-totem-tech-caps": 1,
            "enter-rect-combat-sapper-attack-order-recovery": 1,
            "exact-resource-or-item-mutation-body-with-unresolved-scheduler": 1,
            "construction-finish-hidden-native-ability-grants": 1,
            "damage-event-15-percent-chain-lightning-dummy-proc": 1,
            "damage-event-40-percent-hurl-boulder-dummy-proc": 1,
            "exact-death-spawned-slow-aura-body-with-unresolved-creation-scheduler": 1,
        })
        with (self.resolved / "runtime-system-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["system_id"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 31)
        hidden_grants = json.loads(rows["construction-granted-hidden-building-abilities"]["parameters_json"])
        grants = {row["building_rawcode"]: row for row in hidden_grants["resolved_grants"]}
        self.assertEqual([row["rawcode"] for row in grants["h00G"]["runtime_granted_abilities"]], ["A0HY", "A007"])
        self.assertEqual(grants["h00G"]["runtime_granted_abilities"][0]["object_data"]["Armor Bonus"], 8)
        self.assertEqual(grants["h00G"]["runtime_granted_abilities"][1]["object_data"]["Amount of Hit Points Regenerated"], 7.5)
        self.assertEqual(grants["h01O"]["runtime_granted_abilities"][0]["object_data"]["Armor Bonus"], -3)
        self.assertEqual([row["rawcode"] for row in grants["h061"]["runtime_granted_abilities"]], ["A03Q", "A0EV"])

        shields = json.loads(rows["targeted-negative-effect-shields"]["parameters_json"])
        self.assertEqual(shields["active_shield_ability_rawcode"], "A09L")
        self.assertEqual(shields["active_shield_buff_rawcode"], "B01H")
        self.assertEqual(shields["active_shield_armor_bonus_by_level"], [2, 4])
        self.assertTrue(shields["active_shield_level_2_first_block_heals_to_max_life"])
        self.assertEqual(shields["anti_hex_marker_ability_rawcode"], "A070")
        self.assertEqual(shields["goblin_shredder_rawcode"], "n01T")
        self.assertEqual(shields["goblin_shredder_block_chance_percent"], 60)
        self.assertEqual(shields["goblin_shredder_overheat_attack_speed_fraction_by_level"], [0, 0.5, 1, 1.5, 2])
        self.assertEqual(shields["goblin_shredder_overheat_full_explosion_damage_by_level"], [50, 100, 200, 400, 800])
        self.assertEqual(
            shields["goblin_shredder_overheat_visual_spell_list_by_level"],
            ["", "A027", "A027,A0A1", "A027,A0A1,A0A2", "A027,A0A1,A0A2,A0A4"],
        )
        self.assertTrue(shields["successful_shredder_blocks_increment_all_three_levels_together"])

        energy = json.loads(rows["energy-tower-energy-burst"]["parameters_json"])
        self.assertEqual(energy["proc_probability_percent"], 15)
        self.assertEqual(energy["effect_damage"], 50)
        self.assertEqual(energy["effect_target_count"], 8)
        self.assertEqual(energy["effect_damage_reduction_per_bounce"], 0.15)
        self.assertEqual(energy["tooltip_advertised_damage_reduction_per_bounce"], 0.2)
        self.assertTrue(energy["tooltip_object_discrepancy_preserved"])
        self.assertEqual(energy["protected_effect_cooldown_seconds"], 0.0)
        self.assertEqual(energy["protected_effect_mana_cost"], 0.0)

        avalanche = json.loads(rows["elemental-guard-tower-avalanche"]["parameters_json"])
        self.assertEqual(avalanche["proc_probability_percent"], 40)
        self.assertEqual(avalanche["A0EV_tooltip_advertised_proc_probability_percent"], 70)
        self.assertTrue(avalanche["runtime_probability_overrides_helper_tooltip"])
        self.assertEqual(avalanche["effect_damage"], 100)
        self.assertEqual(avalanche["effect_duration_normal_seconds"], 10.0)
        self.assertEqual(avalanche["effect_duration_hero_seconds"], 5.0)
        self.assertEqual(avalanche["protected_effect_cooldown_seconds"], 0.1)
        self.assertEqual(avalanche["protected_effect_mana_cost"], 0.0)

        wisp = json.loads(rows["wisp-death-slow-unresolved-reachability"]["parameters_json"])
        self.assertFalse(wisp["production_reachability_proven"])
        self.assertEqual(wisp["readable_creation_function_named_callers"], [])
        self.assertEqual(wisp["slow_aura_radius"], 350)
        self.assertEqual(wisp["movement_speed_factor"], -0.8)
        self.assertEqual(wisp["slow_aura_timed_life_seconds"], 8)
        self.assertTrue(wisp["must_not_be_treated_as_live_gameplay_without_creation_reachability_proof"])

        ledger = json.loads(rows["protected-runtime-ledger-unresolved-reachability"]["parameters_json"])
        self.assertFalse(ledger["production_reachability_proven"])
        self.assertTrue(ledger["must_not_be_treated_as_live_gameplay_without_scheduler_proof"])
        self.assertEqual(ledger["branch_formula"], "(counter + lane) mod 3")
        self.assertEqual(ledger["branch_0_item_rawcode"], "ratf")
        self.assertEqual(ledger["branch_1_effect"], "selected-lane-player gold +1")
        self.assertEqual(ledger["branch_2_effect"], "selected-lane-player lumber +1")
        self.assertEqual(ledger["callback_constructor_readable_named_callers"], [])
        order_rects = json.loads(rows["lane-order-rect-attack-reengage"]["parameters_json"])
        self.assertEqual(order_rects["setup_delay_seconds"], 0.1)
        self.assertEqual(order_rects["target_predicate"], "combat-sapper;vulnerable;not-cloaked")
        self.assertEqual(order_rects["action"], "orderCodeAttack")
        self.assertEqual(order_rects["rectangles"][0], {"max_x": -1024.0, "max_y": 1024.0, "min_x": -2047.9, "min_y": -1536.0})
        self.assertEqual(order_rects["rectangles"][1], {"max_x": 2047.9, "max_y": 1024.0, "min_x": 1536.0, "min_y": -1024.0})
        stasis = json.loads(rows["stasis-totem-global-setup"]["parameters_json"])
        self.assertEqual(stasis["setup_delay_seconds"], 0.1)
        self.assertEqual(stasis["stock_stasis_trap_rawcode"], "otot")
        self.assertTrue(stasis["stock_stasis_trap_hidden_on_enter"])
        self.assertEqual(stasis["totem_rawcodes"], ["h07X", "h07Y", "h07Z"])
        self.assertEqual(stasis["maximum_allowed_per_player_each"], 1)
        self.assertEqual([totem["name"] for totem in stasis["totems"]], ["Stasis Totem", "Healing Totem", "Endurance Totem"])

        power = json.loads(rows["power-plant-power-surge"]["parameters_json"])
        self.assertEqual(power["building_armor_bonus"], 2)
        self.assertEqual(power["mana_regen_bonus_per_second"], 0.2)
        self.assertEqual(power["tower_attack_damage_bonus_fraction"], 0.25)
        self.assertEqual(power["disable_cleanse_sweep_interval_seconds"], 4)
        self.assertEqual(power["disable_cleanse_radius"], 252)
        self.assertEqual(power["cleaned_buff_ids"], [1114010234, 1110454349])
        self.assertEqual(power["spawn_permanent_max_hp_bonus"], 150)
        self.assertEqual(power["spawn_armor_bonus"], 4)
        self.assertEqual(power["spawn_attack_damage_bonus_fraction"], 0.2)
        self.assertEqual(power["spawn_spell_damage_reduction"], 0.15)
        self.assertEqual(power["spawn_hp_stack_object_data_at_level4"]["Max Life Gained"], -150)
        self.assertEqual(len(power["spell_resist_exclusion_ability_ids"]), 17)

        heroic = json.loads(rows["heroic-shrine-companion-spawning"]["parameters_json"])
        self.assertEqual(heroic["tooltip_probability_percent_per_shrine"], 16)
        self.assertEqual(heroic["per_shrine_actual_probability_percent"], 17)
        self.assertTrue(heroic["tooltip_disagrees_with_runtime"])
        self.assertEqual(heroic["maximum_shrines_checked"], 2)
        self.assertEqual(heroic["type_murloc_unconditional_local_extra_copy"], 1)
        self.assertEqual(
            [unit["rawcode"] for unit in heroic["twins_replacement_units"]],
            ["n02K", "n02L"],
        )

        golden = json.loads(rows["golden-shrine-revival"]["parameters_json"])
        self.assertEqual(golden["chance_percent_per_shrine"], 20)
        self.assertEqual(golden["maximum_effective_chance_percent"], 40)
        self.assertEqual(golden["revive_delay_seconds"], 2)
        self.assertTrue(golden["exclude_wc3_summoned_unit_type"])
        self.assertTrue(golden["revived_unit_cannot_trigger_golden_shrine_again"])
        self.assertTrue(golden["revive_requires_same_death_generation"])

        fiend = json.loads(rows["blood-fiend-randomization"]["parameters_json"])
        bodies = fiend["resolved_body_distribution"]
        self.assertEqual(sum(body["probability_percent"] for body in bodies), 100)
        self.assertEqual(
            [(body["rawcode"], body["probability_percent"], body["armor_type"]) for body in bodies],
            [
                ("n00M", 5.0, "divine"),
                ("n00R", 9.0, "hero"),
                ("n00Q", 21.5, "none"),
                ("n00N", 21.5, "small"),
                ("n00O", 21.5, "medium"),
                ("n00P", 21.5, "large"),
            ],
        )
        self.assertTrue(all(body["hp"] == 640 for body in bodies))
        self.assertTrue(all(body["attack1_min"] == 45 for body in bodies))
        self.assertTrue(all(body["attack1_max"] == 65 for body in bodies))
        self.assertEqual(len(fiend["resolved_trait_groups"]), 6)
        for group in fiend["resolved_trait_groups"]:
            self.assertEqual(sum(outcome["probability_percent"] for outcome in group["outcomes"]), 100)

        linker = json.loads(rows["elemental-linker-death-heal"]["parameters_json"])
        self.assertEqual(linker["tooltip_heal_per_building"], 17.5)
        self.assertEqual(linker["heal_per_owned_elemental_production_building"], 17)
        self.assertTrue(linker["heal_per_building_tooltip_disagrees_with_runtime"])
        self.assertEqual(linker["maximum_heal"], 500)
        self.assertEqual(linker["heal_radius"], 350)
        self.assertEqual(linker["same_unit_type_heal_factor"], 1.0)
        self.assertEqual(linker["other_unit_type_heal_factor"], 0.2)
        self.assertEqual(linker["tooltip_other_race_reduction_percent"], 70)
        self.assertEqual(linker["runtime_other_type_reduction_percent"], 80)
        self.assertTrue(linker["other_type_tooltip_disagrees_with_runtime"])
        self.assertEqual(linker["resolved_elemental_building_count"], 12)
        self.assertEqual(linker["resolved_elemental_building_buckets"], [1, 2, 3, 4, 5])

        treasure = json.loads(rows["treasure-box-income-multiplier"]["parameters_json"])
        self.assertEqual(treasure["tooltip_first_box_bonus_percent"], 25)
        self.assertEqual(treasure["tooltip_later_box_reduction_percent"], 15)
        self.assertEqual(
            treasure["multiplier_table_indexes_0_through_9"],
            [0.0, 1.0, 1.85, 2.57, 3.18, 3.7, 4.14, 4.52, 4.84, 5.11],
        )
        self.assertEqual(
            treasure["multipliers_0_through_9"],
            [1.0, 1.25, 1.4625, 1.6425, 1.795, 1.925, 2.035, 2.13, 2.21, 2.2775],
        )
        self.assertEqual(treasure["count_10_plus_marginal_multiplier_per_box"], 0.0625)
        self.assertTrue(treasure["applied_before_progressive_income_tax"])

        artillery = json.loads(rows["human-artillery-auto-bombardment"]["parameters_json"])
        self.assertEqual(artillery["runtime_hp"], 700)
        self.assertEqual((artillery["attack_min"], artillery["attack_max"]), (300, 400))
        self.assertEqual(artillery["native_attack_cooldown_seconds"], 15)
        self.assertEqual(artillery["maintenance_interval_seconds"], 9)
        self.assertEqual(artillery["intercepted_order_id"], 851971)
        self.assertEqual(artillery["forced_order_id"], 851984)
        self.assertEqual(artillery["full_aoe_radius"], 60)
        self.assertEqual(artillery["half_aoe_radius"], 150)
        self.assertEqual(artillery["quarter_aoe_radius"], 320)

        skeletons = json.loads(rows["raise-dead-skeleton-randomization"]["parameters_json"])
        self.assertEqual(
            [unit["rawcode"] for unit in skeletons["resolved_skeleton_table"]],
            ["u002", "n00D", "u00A", "n00C", "u00B", "n01L", "u003", "n01M"],
        )
        self.assertEqual(skeletons["necromancer_table_indexes"], [0, 1, 2])
        self.assertEqual(skeletons["mighty_necromancer_table_indexes"], [0, 1, 2, 3, 4, 5, 6])
        self.assertEqual(skeletons["lich_king_general_probability_percent"], 12.5)
        self.assertEqual(skeletons["lich_king_each_other_probability_percent"], 21.875)
        self.assertEqual(skeletons["lich_king_bonus_damage_object_data"]["Attack Bonus"], 12)
        self.assertEqual(skeletons["lich_king_bonus_armor_object_data"]["Defense Bonus"], 3)

        gjallar = json.loads(rows["gjallarhorn-team-count-scaling"]["parameters_json"])
        self.assertEqual(gjallar["counter_scope"], "team")
        self.assertEqual(gjallar["effect_level_formula"], "min(4, team_constructed_gjallarhorn_count)")
        self.assertTrue(gjallar["counter_resets_on_round_signal"])
        self.assertFalse(gjallar["counter_decrement_on_building_death"])
        self.assertEqual(
            [level["attack_speed_increase"] for level in gjallar["resolved_effect_levels"]],
            [0.4, 0.45, 0.5, 0.55],
        )
        self.assertTrue(all(level["duration_seconds"] == 60 for level in gjallar["resolved_effect_levels"]))

        support = json.loads(rows["support-order-controller"]["parameters_json"])
        self.assertEqual(support["queue_interval_seconds"], 0.2)
        self.assertEqual(support["queue_tasks_per_tick"], 12)
        self.assertEqual(support["assassin_retarget_min_interval_seconds"], 3.5)
        self.assertEqual(support["target_group_refresh_min_interval_seconds"], 0.1)
        self.assertEqual(support["assassin_search_rect_player_id_lt_6"], [-6016, -4096, 1920, 4096])
        self.assertEqual(support["assassin_search_rect_player_id_gte_6"], [-1888, -4096, 6048, 4096])
        self.assertEqual(support["fallback_battlefield_rect"], [-6176, -3584, 6176, 3584])
        self.assertEqual(support["assassin_attack_order_id"], 851983)
        self.assertEqual(support["assassin_move_order_id"], 851986)
        self.assertEqual(support["assassin_windwalk_order_id"], 852129)
        actors = {actor["unit_rawcode"]: actor for actor in support["actors"]}
        self.assertEqual(actors["n01Z"]["low_hp_priority_threshold"], 150)
        self.assertEqual(actors["n01Z"]["windwalk_object_data"]["Backstab Damage"], 150)
        self.assertEqual(actors["n020"]["low_hp_priority_threshold"], 300)
        self.assertEqual(actors["n020"]["windwalk_object_data"]["Backstab Damage"], 300)
        self.assertEqual(actors["n01U"]["timed_life_seconds"], 45)
        self.assertEqual(actors["n01U"]["repair_object_data"]["Repair Time Ratio"], 0.45)
        self.assertFalse(support["gobbo_one_per_target_claim_proven_by_script"])

        order_suppression = json.loads(rows["external-unit-order-suppression"]["parameters_json"])
        self.assertEqual(order_suppression["listener_install_delay_seconds"], 0.1)
        self.assertEqual(order_suppression["rejected_order_ids"], {"attack": 851983, "move": 851972, "stop": 851971})
        self.assertEqual(order_suppression["control_lock_sequence_seconds"], [5, 15, 30, 60, 120, 240, 300])
        self.assertEqual(order_suppression["control_lock_cap_seconds"], 300)
        self.assertEqual(order_suppression["offense_count_reset_gap_seconds"], 600)
        self.assertTrue(order_suppression["already_locked_repeat_does_not_extend_lock"])
        self.assertTrue(order_suppression["new_lock_disables_user_control"])
        self.assertEqual(order_suppression["protected_release_timer_period_seconds"], 0.25)
        self.assertFalse(order_suppression["protected_release_timer_to_KL_binding_proven"])
        self.assertEqual(order_suppression["support_marker_ability"]["rawcode"], "A07M")
        roles = {unit["rawcode"]: unit["role"] for unit in order_suppression["resolved_exception_and_support_units"]}
        self.assertEqual(roles["e00F"], "attack-target exception target")
        self.assertEqual(roles["n01Z"], "support-order restoration actor")
        self.assertEqual(roles["n020"], "support-order restoration actor")
        self.assertEqual(roles["n01U"], "support-order restoration actor")

        idle = json.loads(rows["global-idle-attack-reengage"]["parameters_json"])
        self.assertEqual(idle["interval_seconds"], 4)
        self.assertTrue(idle["starts_each_round"])
        self.assertEqual(idle["excluded_ability_ids"], [1093678921, 1093678925])
        self.assertEqual(idle["target_predicate"], "alive;current-order-zero;combat-sapper;vulnerable;lacks-A07I;lacks-A07M")

        blink = json.loads(rows["builder-castle-blink"]["parameters_json"])
        self.assertEqual(blink["ability_rawcode"], "A0-1")
        self.assertEqual(blink["ability_range"], 10000)
        self.assertEqual(blink["rect_inset_world_units"], 64)
        self.assertEqual(blink["destination_rect"], "owner-own-castle-rect")
        self.assertEqual(blink["post_teleport_order_id"], 851972)

        rescue = json.loads(rows["rescue-strike"]["parameters_json"])
        self.assertEqual(rescue["ability_rawcode"], "A005")
        self.assertEqual(rescue["effective_protected_cooldown_seconds"], 60)
        self.assertEqual(rescue["static_object_cooldown_seconds"], 99)
        self.assertEqual(rescue["cast_delay_seconds"], 0.35)
        self.assertEqual(rescue["radius"], 700)
        self.assertEqual(rescue["finish_delay_seconds"], 1.5)
        self.assertEqual(rescue["team_coordination_cooldown_seconds"], 2)
        self.assertEqual(rescue["zero_kill_refund_cooldown_seconds"], 180)
        self.assertEqual([packet["amount"] for packet in rescue["damage_packets"]], [4444, 4444])
        self.assertTrue(rescue["zero_kill_refunds_ability_and_effect"])
        self.assertTrue(rescue["nonzero_kill_does_not_refund_in_finish_handler"])

        forge = json.loads(rows["elemental-forge-weapon-cleave"]["parameters_json"])
        self.assertEqual(forge["forge_weapon_marker_rawcode"], "A0F1")
        self.assertEqual(forge["source_damage_dispatch_marker_rawcode"], "A03Q")
        self.assertEqual(forge["damage_multiplier_of_triggering_damage"], 0.5)
        self.assertEqual(forge["radius"], 250)
        self.assertTrue(forge["source_attack_capability_controls_ground_and_flying_hits"])
        self.assertTrue(forge["later_compaction_can_move_forge_weapon_to_another_index"])
        self.assertTrue(forge["forge_weapon_can_therefore_exist_without_dispatcher_marker"])

        harpy = json.loads(rows["locust-harpy-structure-damage-penalty"]["parameters_json"])
        self.assertEqual(harpy["unit_rawcode"], "u00G")
        self.assertEqual(harpy["damage_multiplier"], 0.5)
        self.assertTrue(harpy["target_requires_structure"])
        self.assertEqual((harpy["runtime_attack_min"], harpy["runtime_attack_max"]), (142, 146))

        chi = json.loads(rows["celestial-chi-tower-impact-vision"]["parameters_json"])
        self.assertEqual(chi["tower_rawcode"], "h07V")
        self.assertEqual(chi["batch_window_seconds"], 0.03)
        self.assertEqual(chi["vision_radius"], 512)
        self.assertEqual(chi["vision_duration_seconds"], 8)
        self.assertEqual(chi["batch_position"], "arithmetic-mean-of-damaged-target-positions")

        castle_protection = json.loads(rows["round-start-castle-protection"]["parameters_json"])
        self.assertEqual(castle_protection["protection_duration_seconds"], 15)
        self.assertEqual(castle_protection["damage_rewrite"], 0)
        self.assertTrue(castle_protection["positive_damage_only"])
        self.assertEqual(castle_protection["round_time_formula"], "VGb * 60 + UGb")

        corruption = json.loads(rows["corrupted-eye-of-corruption-spell-vulnerability"]["parameters_json"])
        self.assertEqual(corruption["source_building_rawcode"], "h04T")
        self.assertEqual(corruption["aura_ability_rawcode"], "A02C")
        self.assertEqual(corruption["target_buff_rawcode"], "B00Q")
        self.assertEqual(corruption["aura_armor_bonus"], -6)
        self.assertEqual(corruption["aura_area"], 99999)
        self.assertEqual(corruption["damage_multiplier"], 1.12)
        self.assertEqual(corruption["extra_damage_fraction"], 0.12)
        self.assertTrue(corruption["positive_damage_only"])
        self.assertEqual(corruption["excluded_damage_event_type"], 0)
        self.assertTrue(corruption["multiple_source_buildings_do_not_stack_script_multiplier"])
        self.assertTrue(corruption["tooltip_multiple_buildings_no_additional_benefit"])

        obelisk = json.loads(rows["obelisk-of-light-cleansing-light"]["parameters_json"])
        self.assertEqual(obelisk["building_rawcode"], "h005")
        self.assertEqual(obelisk["effect_ability_rawcode"], "A000")
        self.assertEqual(obelisk["effect_ability_base_rawcode"], "Apxf")
        self.assertEqual(obelisk["effect_initial_damage"], 180)
        self.assertEqual(obelisk["effect_cooldown_seconds"], 5)
        self.assertTrue(obelisk["remove_native_positive_and_negative_buffs"])
        self.assertEqual(len(obelisk["removed_persistent_ability_ids"]), 25)
        self.assertIn(1093682737, obelisk["removed_persistent_ability_ids"])
        self.assertIn(1093683033, obelisk["removed_persistent_ability_ids"])

        income_sync = json.loads(rows["building-income-synchronization"]["parameters_json"])
        self.assertEqual(
            income_sync["delta_formula"],
            "buildingIncomeSyncDelta(cached,catalog,catalogBuildingIncomeDelta(unit))",
        )
        self.assertEqual(income_sync["authoritative_player_income_accumulator"], "iGb[player_id]")
        self.assertEqual(income_sync["per_unit_cached_income"], "Eab[unit_income_index]")
        self.assertTrue(income_sync["refreshes_current_income_cache_after_change"])
        self.assertEqual(
            [variant["construction_finished_flag"] for variant in income_sync["callback_variants"]],
            [True, False],
        )

        pathing = json.loads(rows["trained-unit-pathing-recovery"]["parameters_json"])
        self.assertEqual(pathing["marker_ability_rawcode"], "A0BG")
        self.assertEqual(pathing["tracker_delay_seconds"], 4.0)
        self.assertEqual(pathing["spawn_distance_threshold"], 768.0)
        self.assertEqual(pathing["pathing_disabled_pulse_seconds"], 0.7)
        self.assertTrue(pathing["pathing_restored_after_pulse"])
        self.assertTrue(pathing["tracker_restarts_after_pathing_restore"])

        building_sync = json.loads(rows["building-unit-synchronization"]["parameters_json"])
        self.assertEqual(building_sync["queue_cancel_order_id"], 851976)
        self.assertEqual(building_sync["queue_cancel_repetitions"], 7)
        self.assertEqual(building_sync["synchronized_train_order_repetitions"], 2)
        self.assertEqual(building_sync["sync_lightning_code"], "LEAS")
        self.assertEqual(building_sync["sync_lightning_duration_seconds"], 0.625)
        self.assertEqual(building_sync["post_sync_point_order_id"], 851971)
        self.assertEqual(building_sync["post_sync_point_offset_absolute"], [38.0, 68.0])

    def test_known_combat_values_use_recovered_protection_fields(self) -> None:
        with (self.resolved / "units.tsv").open(encoding="utf-8") as handle:
            units = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(units["hfoo"]["hp"], "250")
        self.assertEqual(units["hfoo"]["attack1_range"], "90")
        self.assertEqual(units["hmtm"]["hp"], "280")
        self.assertEqual(units["hmtm"]["attack1_range"], "1000")
        self.assertEqual(units["hrif"]["hp"], "270")
        self.assertEqual(units["hrif"]["attack1_range"], "500")

    def test_building_footprint_is_actual_tga_mask(self) -> None:
        with (self.resolved / "buildings.tsv").open(encoding="utf-8") as handle:
            buildings = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        barracks = buildings["h000"]
        self.assertEqual(barracks["footprint_width_cells"], "4")
        self.assertEqual(barracks["footprint_height_cells"], "4")
        self.assertEqual(barracks["footprint_width_world_units"], "128")
        self.assertEqual(barracks["footprint_hex_rows"], "5555/5555/5555/5555")

    def test_protected_unit_stat_overlay_recovers_attack_primitives(self) -> None:
        with (self.resolved / "protected-unit-stats.tsv").open(encoding="utf-8") as handle:
            units = {row["rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(units), 549)
        self.assertEqual(units["e000"]["static_hp"], "1")
        self.assertEqual(units["e000"]["override_hp"], "460")
        self.assertEqual(units["e000"]["unitstat_hp"], "460")
        self.assertEqual(units["e000"]["unitstat_attack1_base_damage"], "49")
        self.assertEqual(units["e000"]["unitstat_attack1_dice_number"], "1")
        self.assertEqual(units["e000"]["unitstat_attack1_dice_sides"], "1")
        self.assertEqual(units["e000"]["unitstat_attack1_cooldown"], "1.75")
        self.assertEqual(units["e000"]["unitstat_attack1_range"], "350")
        self.assertEqual(units["h00W"]["unitstat_attack1_cooldown"], "1.8")
        self.assertEqual(units["h00W"]["unitstat_attack1_range"], "650")
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["protected_unit_stat_rows"], 549)
        self.assertEqual(summary["protected_unit_stat_override_assignments"], 1887)
        self.assertEqual(summary["selected_game_data_set"]["label"], "Default (TFT)")
        self.assertEqual(summary["selected_game_data_set"]["profile_variant"], "custom,V1")
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["armor"], {"unitstat-match": 162})
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["hp"], {"unitstat-match": 162})
        self.assertEqual(
            summary["effective_unit_stat_vs_unitstat_comparisons"]["dps"],
            {"unitstat-match": 160, "unitstat-missing": 2},
        )
        self.assertEqual(summary["effective_unit_stat_vs_unitstat_comparisons"]["move_speed"], {"unitstat-match": 162})

    def test_production_attack_profiles_keep_conditional_and_dual_weapons(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["production_unit_attack_rows"], 324)
        self.assertEqual(summary["production_unit_available_attack_profiles"], 174)
        self.assertEqual(summary["production_unit_conditional_attack_profiles"], 1)
        self.assertEqual(summary["production_unit_two_profile_sum_patterns"], 2)

        with (self.resolved / "production-unit-attacks.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        indexed = {(row["unit_rawcode"], row["attack_index"]): row for row in rows}

        giant_default = indexed[("e00F", "1")]
        giant_club = indexed[("e00F", "2")]
        self.assertEqual(giant_default["activation"], "default")
        self.assertEqual(giant_default["disabled_by_ability"], "A0BC")
        self.assertEqual(giant_club["activation"], "conditional")
        self.assertEqual(giant_club["enabled_by_ability"], "A0BC")
        self.assertEqual(giant_club["conditional_max_attacks"], "10")
        self.assertEqual(giant_club["attack_type"], "siege")
        self.assertEqual(giant_club["range"], "150")
        self.assertIn("air", giant_club["targets"].split(","))
        self.assertEqual(giant_club["xo_range_relation"], "xo-two-profile-sum-minus-one")

        rotero_air = indexed[("h06Q", "1")]
        rotero_building = indexed[("h06Q", "2")]
        self.assertEqual(rotero_air["activation"], "default")
        self.assertEqual(rotero_air["targets"], "air")
        self.assertEqual(rotero_air["range"], "500")
        self.assertEqual(rotero_air["dps"], "25.0")
        self.assertEqual(rotero_building["activation"], "default")
        self.assertEqual(rotero_building["targets"], "structure")
        self.assertEqual(rotero_building["range"], "500")
        self.assertEqual(rotero_building["dps"], "20.0")
        self.assertEqual(rotero_building["xo_range_relation"], "xo-two-profile-sum-minus-one")

    def test_production_unit_abilities_keep_runtime_fields_and_inherited_base_links(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["production_unit_ability_links"], 481)
        self.assertEqual(summary["production_unit_unique_abilities"], 291)
        self.assertEqual(summary["production_unit_inherited_ability_links"], 6)
        self.assertEqual(summary["production_unit_ability_links_with_protected_runtime_fields"], 77)

        with (self.resolved / "production-unit-abilities.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        self.assertEqual(len(rows), 481)
        indexed = {(row["unit_rawcode"], row["ability_rawcode"]): row for row in rows}

        grab_tree = indexed[("e00F", "A0BC")]
        self.assertEqual(grab_tree["base_rawcode"], "Agra")
        self.assertEqual(grab_tree["static_cooldown"], "99")
        self.assertEqual(grab_tree["effective_cooldown"], "1.0")
        self.assertEqual(grab_tree["cooldown_source"], "protected-runtime")
        self.assertIn('"Maximum Attacks":10', grab_tree["data_fields_labeled_json"])
        self.assertEqual(grab_tree["script_reference_count"], "4")

        rotero_bash = indexed[("h06Q", "A09I")]
        self.assertEqual(rotero_bash["base_rawcode"], "ACbh")
        self.assertEqual(rotero_bash["targets"], "air")
        self.assertIn('"Chance to Bash":30', rotero_bash["data_fields_labeled_json"])
        self.assertIn('"Damage Bonus":40', rotero_bash["data_fields_labeled_json"])

        inherited = {row["ability_rawcode"]: row for row in rows if row["definition_source"] == "inherited-base"}
        self.assertEqual(set(inherited), {"Aeth", "Aloc", "Avul"})
        self.assertEqual(inherited["Aeth"]["name"], "Ghost")
        self.assertEqual(inherited["Aloc"]["name"], "Locust")
        self.assertEqual(inherited["Avul"]["name"], "Invulnerable")

    def test_production_building_catalog_uses_runtime_spawn_metadata_and_race_partition(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["building_catalog_rows"], 240)
        self.assertEqual(summary["building_catalog_production_rows"], 167)
        self.assertEqual(summary["building_catalog_campaign_production_rows"], 5)
        self.assertEqual(summary["building_catalog_normal_production_rows"], 162)
        self.assertEqual(summary["building_catalog_normal_production_rows_in_xo"], 162)
        self.assertEqual(summary["building_catalog_upgrade_edges"], 90)
        self.assertEqual(summary["building_catalog_semantic_rows"], 240)
        self.assertEqual(summary["building_catalog_two_second_production_build_rows"], 167)

        with (self.resolved / "production-buildings.tsv").open(encoding="utf-8") as handle:
            rows = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 240)

        barracks = rows["h000"]
        self.assertEqual(barracks["builder_names"], "Human Builder")
        self.assertEqual(barracks["building_kind"], "production")
        self.assertEqual(barracks["unit_rawcode"], "hfoo")
        self.assertEqual(barracks["gold_cost"], "100")
        self.assertEqual(barracks["spawn_time"], "20")
        self.assertEqual(barracks["static_object_build_time"], "2")
        self.assertEqual(barracks["income_factor_symbol"], "gvb")
        self.assertEqual(barracks["income_factor"], "0.2")
        self.assertEqual(barracks["own_income_contribution"], "0.2")
        self.assertEqual(barracks["catalog_income"], "0.2")
        self.assertEqual(barracks["has_tier_assignment"], "1")
        self.assertEqual(barracks["upgrade_to"], "h039")
        self.assertEqual(barracks["in_xo_runtime_catalog"], "1")

        stronghold = rows["h039"]
        self.assertEqual(stronghold["upgrade_from"], "h000")
        self.assertEqual(stronghold["own_income_contribution"], "0.35")
        self.assertEqual(stronghold["catalog_income"], "0.55")

        crab = rows["h0Z1"]
        self.assertEqual(crab["builder_names"], "Critter Builder")
        self.assertEqual(crab["campaign_only"], "1")
        self.assertEqual(crab["unit_rawcode"], "n0Z1")
        self.assertEqual(crab["in_xo_runtime_catalog"], "0")
        self.assertEqual(crab["upgrade_to"], "h0Z2")

        reef_guardian = rows["h0Z7"]
        self.assertEqual(reef_guardian["food_used"], "1")
        self.assertEqual(reef_guardian["is_legendary"], "1")
        self.assertEqual(reef_guardian["spawn_time"], "60")

        production_rows = [row for row in rows.values() if row["building_kind"] == "production"]
        self.assertEqual(len(production_rows), 167)
        self.assertEqual({row["static_object_build_time"] for row in production_rows}, {"2"})

    def test_scripted_unit_spell_registry_links_units_abilities_and_target_modes(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["scripted_unit_spell_rows"], 37)
        self.assertEqual(summary["scripted_unit_spell_production_rows"], 35)
        self.assertEqual(summary["scripted_unit_spell_resolved_order_ids"], 37)
        self.assertEqual(summary["scripted_unit_spell_order_id_sources"], {
            "script-integer": 1,
            "wc3-canonical-base-order": 36,
        })
        self.assertEqual(summary["scripted_unit_spell_target_modes"], {
            "ally-any": 2,
            "ally-ground": 10,
            "ally-structure": 1,
            "enemy-flying-combat-sapper": 3,
            "enemy-ground-combat-sapper": 20,
            "immediate-enemy-special-unit": 1,
        })

        with (self.resolved / "unit-spells.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 37)

        giant = rows["e00F"]
        self.assertEqual(giant["unit_names"], "Mountain Giant")
        self.assertEqual(giant["production_building_names"], "Monolith")
        self.assertEqual(giant["ability_rawcode"], "A0BJ")
        self.assertEqual(giant["target_mode_label"], "enemy-ground-combat-sapper")
        self.assertEqual(giant["base_order"], "parasite")
        self.assertEqual(giant["resolved_order_id"], "852601")
        self.assertEqual(giant["resolved_order_id_source"], "wc3-canonical-base-order")
        self.assertEqual(giant["effective_cooldown"], "15.0")
        self.assertEqual(giant["cooldown_source"], "protected-runtime")

        mine_layer = rows["h06U"]
        self.assertEqual(mine_layer["target_mode_label"], "immediate-enemy-special-unit")
        self.assertEqual(mine_layer["expected_immediate_unit_rawcode"], "h09M")
        self.assertEqual(mine_layer["expected_immediate_unit_name"], "MineDummy")
        self.assertEqual(mine_layer["evidence_kind"], "inlined-registration")

        monk = rows["n03E"]
        self.assertEqual(monk["target_mode_label"], "ally-any")
        self.assertEqual(monk["base_order"], "heal")
        self.assertEqual(monk["resolved_order_id"], "852063")
        self.assertEqual(monk["production_building_names"], "Bamboo Dojo")

        mana_generator = rows["h062"]
        self.assertEqual(mana_generator["target_mode_label"], "ally-structure")
        self.assertEqual(mana_generator["base_order"], "absorb")
        self.assertEqual(mana_generator["resolved_order_id"], "852529")
        self.assertEqual(mana_generator["production_building_rawcode"], "")

        twins = rows["n02L"]
        self.assertEqual(twins["unit_names"], "Twin Smiley")
        self.assertEqual(twins["production_building_rawcode"], "")

    def test_scripted_unit_spell_mechanics_profile_all_handlers_and_effect_links(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["scripted_unit_spell_mechanic_rows"], 37)
        self.assertEqual(summary["scripted_unit_spell_mechanics_with_delayed_callbacks"], 23)
        self.assertEqual(summary["scripted_unit_spell_mechanics_with_dynamic_callbacks"], 28)
        self.assertEqual(sum(summary["scripted_unit_spell_mechanic_kinds"].values()), 37)
        self.assertEqual(summary["scripted_unit_spell_mechanic_kinds"]["delegated-helper"], 12)

        with (self.resolved / "unit-spell-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 37)

        monk = rows["n03E"]
        self.assertEqual(monk["mechanic_kind"], "dummy-point-ability")
        self.assertEqual(json.loads(monk["scheduled_delays_json"]), ["0.1"])
        monk_effects = {row["rawcode"]: row for row in json.loads(monk["reachable_map_objects_json"])}
        self.assertEqual(json.loads(monk_effects["A0GP"]["ability_level1"]["data_fields_labeled_json"])["Damage Amount"], 40)

        twins = rows["n02L"]
        self.assertEqual(twins["mechanic_kind"], "spawn-unit")
        self.assertIn("CallbackSingle_doAfter_RaceChaosAbilities", twins["delayed_callback_functions"])
        self.assertIn("ForGroupCallback_forUnitsInRange_doAfter_RaceChaosAbilities", twins["dynamic_callback_functions"])
        twin_effects = {row["rawcode"]: row for row in json.loads(twins["reachable_map_objects_json"])}
        self.assertEqual(twin_effects["h06N"]["unit_object"]["name"], "ClapDummy")
        self.assertEqual(json.loads(twin_effects["A0FS"]["ability_level1"]["data_fields_labeled_json"])["AOE Damage"], 20)
        twin_literals = {row["function"]: row["literals"] for row in json.loads(twins["source_numeric_literals_json"])}
        self.assertIn("600.", twin_literals["CallbackSingle_doAfter_RaceChaosAbilities_call_doAfter_RaceChaosAbilities1"])
        self.assertIn("100.", twin_literals["CallbackSingle_doAfter_RaceChaosAbilities_call_doAfter_RaceChaosAbilities1"])

        flamegunner = rows["z003"]
        flamegunner_literals = {
            row["function"]: row["literals"] for row in json.loads(flamegunner["source_numeric_literals_json"])
        }
        self.assertIn("300.", flamegunner_literals["CallbackSingle_doAfter_RaceMechAbilities_call_doAfter_RaceMechAbilities2"])
        self.assertIn("100.", flamegunner_literals["CallbackSingle_doAfter_RaceMechAbilities_call_doAfter_RaceMechAbilities2"])

        witch = rows["n032"]
        witch_literals = {row["function"]: row["literals"] for row in json.loads(witch["source_numeric_literals_json"])}
        self.assertEqual(
            witch_literals["CallbackSingle_doAfter_RaceDesertAbilities_call_doAfter_RaceDesertAbilities2"].count("75."),
            2,
        )

        mine_layer = rows["h06U"]
        mine_literals = {row["function"]: row["literals"] for row in json.loads(mine_layer["source_numeric_literals_json"])}
        self.assertIn("50.", mine_literals["mineLayerSpell"])
        self.assertIn("220.", mine_literals["mineLayerSpell"])
        self.assertIn("700.", mine_literals["mineLayerSpell"])

        trapper = rows["n02G"]
        self.assertEqual(trapper["helper_functions"], "forestTrollTrapperSpell")
        trapper_effects = {row["rawcode"]: row for row in json.loads(trapper["reachable_map_objects_json"])}
        self.assertEqual(trapper_effects["A0FC"]["ability_level1"]["range"], "800")
        self.assertEqual(trapper_effects["A0FC"]["ability_level1"]["duration_normal"], "16")

        giant = rows["e00F"]
        self.assertEqual(giant["mechanic_kind"], "unit-immediate-order")
        self.assertEqual(json.loads(giant["scheduled_delays_json"]), ["0.6"])
        self.assertIn("CallbackSingle_doAfter_RaceNatureAbilities", giant["delayed_callback_functions"])

    def test_scripted_unit_spell_semantics_mark_ready_and_partial_rows_explicitly(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["scripted_unit_spell_semantic_rows"], 37)
        self.assertEqual(summary["protected_filter_binding_rows"], 22)
        self.assertEqual(summary["protected_filter_binding_resolved_rows"], 21)
        self.assertEqual(summary["protected_filter_binding_status_counts"], {
            "dynamic": 1, "resolved": 21,
        })
        with (self.resolved / "protected-filter-bindings.tsv").open(encoding="utf-8") as handle:
            filters = {row["symbol"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(filters["SX"]["resolved_function"], "vL")
        self.assertEqual(filters["SX"]["predicate"], "alive-combat-sapper;enemy-of-mIb")
        self.assertEqual(filters["QX"]["resolved_function"], "wL")
        self.assertIn("always-false", filters["QX"]["predicate"])
        self.assertEqual(filters["RX"]["resolved_function"], "AC")
        self.assertIn("missing-hp>1", filters["RX"]["predicate"])
        self.assertEqual(filters["vFb"]["resolved_function"], "IC")
        self.assertEqual(filters["vFb"]["predicate"], "always-true-marketplace-stock-filter")
        self.assertEqual(filters["cHb"]["resolved_function"], "UC")
        self.assertIn("unit-type-not-h06C", filters["cHb"]["predicate"])
        self.assertEqual(filters["dHb"]["resolved_function"], "TC")
        self.assertEqual(filters["dHb"]["predicate"], "life>0.405;sapper;vulnerable")
        self.assertEqual(filters["ZGb"]["resolved_function"], "VC")
        self.assertEqual(filters["ZGb"]["predicate"], "life>0.405;peon")
        self.assertEqual(filters["Y0"]["resolved_function"], "tK")
        self.assertEqual(filters["X0"]["resolved_function"], "sK")
        self.assertEqual(filters["W0"]["resolved_function"], "uK")
        self.assertEqual(filters["V0"]["resolved_function"], "vK")
        self.assertEqual(filters["G6"]["resolved_function"], "qJ")
        self.assertIn("mana>100", filters["G6"]["predicate"])
        self.assertEqual(filters["F6"]["resolved_function"], "rJ")
        self.assertIn("mana>0", filters["F6"]["predicate"])
        self.assertEqual(filters["Gib"]["resolved_function"], "kG")
        self.assertIn("closure-for-groups-dispatch", filters["Gib"]["predicate"])
        self.assertEqual(filters["NAb"]["resolved_function"], "mE")
        self.assertEqual(filters["MAb"]["resolved_function"], "nE")
        self.assertEqual(filters["LAb"]["resolved_function"], "oE")
        self.assertEqual(filters["SCr"]["resolution_status"], "dynamic")
        self.assertEqual(summary["scripted_unit_spell_semantic_status_counts"], {
            "object-effect-ready": 17,
            "script-native-ready": 20,
        })

        with (self.resolved / "unit-spell-semantics.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 37)
        self.assertEqual(
            {rawcode for rawcode, row in rows.items() if row["normalization_status"] == "partial"},
            set(),
        )

        faerie = rows["e000"]
        self.assertEqual(faerie["normalization_status"], "object-effect-ready")
        faerie_effects = {row["rawcode"]: row for row in json.loads(faerie["effect_objects_json"])}
        self.assertEqual(
            json.loads(faerie_effects["A0CG"]["ability_level1"]["data_fields_labeled_json"])["Hit Points Gained"],
            175,
        )

        dryad = rows["e006"]
        dryad_params = json.loads(dryad["parameters_json"])
        self.assertEqual(dryad_params["primary_effect_ability_rawcode"], "A004")
        self.assertEqual(dryad_params["permanent_armor_ability_rawcode"], "AId2")
        self.assertEqual(dryad_params["permanent_armor_bonus"], 2)

        keeper = rows["e009"]
        keeper_params = json.loads(keeper["parameters_json"])
        self.assertEqual(keeper_params["permanent_armor_bonus"], 3)
        self.assertEqual(keeper_params["summon_ability_rawcode"], "A0BU")
        self.assertEqual(keeper_params["summoned_unit_rawcode"], "e00D")
        self.assertEqual(keeper_params["summoned_unit_count"], 2)
        self.assertEqual(keeper_params["summoned_unit_duration_seconds"], 45)

        crusader = json.loads(rows["h03B"]["parameters_json"])
        self.assertEqual(crusader["primary_heal"], 25)
        self.assertEqual(crusader["armor_bonus"], 6)
        self.assertEqual(crusader["life_regen_per_second"], 16)
        self.assertEqual(crusader["buff_duration_seconds"], 10)

        paladin = json.loads(rows["h03C"]["parameters_json"])
        self.assertEqual(paladin["armor_bonus"], 9)
        self.assertEqual(paladin["life_regen_per_second"], 24)
        self.assertEqual(paladin["permanent_max_hp_bonus"], 100)

        master = json.loads(rows["h03V"]["parameters_json"])
        self.assertEqual(rows["h03V"]["normalization_status"], "script-native-ready")
        self.assertEqual(master["frost"]["target_filter_status"], "resolved")
        self.assertEqual(master["frost"]["target_filter_function"], "vL")
        self.assertEqual(master["frost"]["target_predicate"], "alive-combat-sapper;enemy-of-mIb")
        self.assertEqual(paladin["resurrection_precheck_radius"], 900)
        self.assertFalse(paladin["resurrection_precheck_checks_wc3_can_raise"])
        self.assertTrue(paladin["resurrection_effect_uses_wc3_corpse_eligibility"])

        twins = json.loads(rows["n02L"]["parameters_json"])
        self.assertEqual(twins["impact_radius"], 600)
        self.assertEqual(twins["max_mana_burn_per_target"], 100)
        self.assertEqual(twins["projectile_speed"], 700)

        ancient = json.loads(rows["e00C"]["parameters_json"])
        self.assertEqual(ancient["tree_count"], 5)
        self.assertEqual(ancient["tree_angles_degrees"], [0, 72, 144, 216, 288])
        self.assertEqual(ancient["transform_delay_seconds"], 3)
        self.assertEqual(ancient["treant_timed_life_seconds"], 20)

        sandkin = json.loads(rows["n030"]["parameters_json"])
        self.assertEqual(sandkin["effect_ability_rawcode"], "A0GA")
        self.assertEqual(sandkin["period_seconds"], 1)
        self.assertEqual(sandkin["cast_iterations"], 5)

        self.assertEqual(
            rows["h062"]["semantic_kind"],
            "permanent-special-building-mana-regen-improvement",
        )
        mana_generator = json.loads(rows["h062"]["parameters_json"])
        self.assertEqual(mana_generator["normal_mana_regen_bonus"], 0.15)
        self.assertEqual(mana_generator["elemental_mana_regen_bonus"], 0.30)
        self.assertTrue(mana_generator["target_rejects_production_buildings"])
        self.assertNotIn("spawn_trait", mana_generator)
        reachability = mana_generator["unreachable_spawn_code_audit"]
        self.assertFalse(reachability["gameplay_reachable"])
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["building_improvement_spawn_mechanic_rows"], 1)
        cross_path = self.root / "docs" / "original_map" / "extracted" / "script" / "building-improvement-spawn-mechanics.tsv"
        with cross_path.open(encoding="utf-8") as handle:
            cross_rows = list(csv.DictReader(handle, delimiter="\t"))
        self.assertEqual(len(cross_rows), 1)
        self.assertEqual(cross_rows[0]["source_unit_rawcode"], "h062")
        self.assertEqual(cross_rows[0]["mechanic_kind"], "unreachable-production-spawn-branch-keyed-by-A0EL")
        cross_params = json.loads(cross_rows[0]["parameters_json"])
        self.assertFalse(cross_params["gameplay_reachable_from_legal_mana_generator_target"])
        self.assertTrue(cross_params["mana_generator_rejects_production_buildings"])
        self.assertFalse(cross_params["normal_unit_train_finish_uses_setup_unit"])
        self.assertEqual(
            cross_params["setup_unit_callers"],
            ["spawnSyncedCompanions", "EventListener_add_CompanionSpawning_onEvent_add_CompanionSpawning"],
        )

        ogre = json.loads(rows["n02Y"]["parameters_json"])
        self.assertEqual(ogre["berserk_ability_rawcode"], "A0GR")
        self.assertEqual(ogre["random_activation_delay_seconds"], [0.1, 0.3])
        self.assertEqual(ogre["duration_seconds"], 6)

        giant = rows["e00F"]
        self.assertEqual(giant["normalization_status"], "script-native-ready")
        giant_params = json.loads(giant["parameters_json"])
        self.assertEqual(giant_params["immediate_order_id"], 852520)
        self.assertEqual(giant_params["immediate_order_name"], "taunt")
        self.assertEqual(giant_params["taunt_ability_rawcode"], "A0BI")
        self.assertEqual(giant_params["taunt_area"], 350)

        brood_row = rows["n01W"]
        self.assertEqual(brood_row["normalization_status"], "script-native-ready")
        brood = json.loads(brood_row["parameters_json"])
        self.assertNotIn("primary_effect_initial_damage", brood)
        self.assertEqual(brood["primary_effect_spawn_unit_rawcode"], "n00T")
        self.assertEqual(brood["first_followup_order_id"], 852212)
        self.assertEqual(brood["first_followup_order_name"], "webon")
        self.assertEqual(brood["first_followup_ability_rawcode"], "A0AS")
        self.assertEqual(brood["second_followup_order_id"], 852602)
        self.assertEqual(brood["second_followup_order_name"], "parasiteon")
        self.assertEqual(brood["second_followup_ability_rawcode"], "A0AV")

        master = rows["h03V"]
        self.assertEqual(master["semantic_kind"], "element-scaled-dual-projectile-system")
        self.assertEqual(master["normalization_status"], "script-native-ready")
        master_params = json.loads(master["parameters_json"])
        self.assertEqual(master_params["branch_roll"]["lightning_if_less_than"], 50)
        self.assertEqual(master_params["lightning"]["projectile_count_formula"], "2 + floor(lightning_building_count / 3)")
        self.assertEqual(master_params["lightning"]["damage_formula"], "75 * min(1 + fire_building_count, 4)")
        self.assertEqual(master_params["lightning"]["hit_radius"], 34)
        self.assertEqual(master_params["frost"]["projectile_count_formula"], "5 + 2 * water_building_count")
        self.assertEqual(master_params["frost"]["damage_formula"], "75 * min(1 + earth_building_count, 4)")
        self.assertEqual(master_params["frost"]["frost_nova_level_formula"], "clamp(floor(wind_building_count / 4), 1, 3)")
        self.assertEqual(master_params["frost"]["hit_radius"], 38)
        self.assertEqual(master_params["frost"]["target_filter_symbol"], "SX")
        self.assertEqual(master_params["frost"]["target_filter_status"], "resolved")
        self.assertEqual(master_params["frost"]["target_filter_function"], "vL")
        self.assertEqual(master_params["frost"]["target_predicate"], "alive-combat-sapper;enemy-of-mIb")

    def test_element_building_buckets_resolve_master_scaling_inputs(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["element_building_bucket_rows"], 12)

        with (self.resolved / "element-building-buckets.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        by_element: dict[str, set[str]] = {}
        for row in rows:
            by_element.setdefault(row["element"], set()).add(row["building_rawcode"])
        self.assertEqual(by_element, {
            "fire": {"h045", "h046"},
            "earth": {"h040", "h041"},
            "lightning": {"h042", "h044"},
            "water": {"h03X", "h03Y", "h03Z"},
            "wind": {"h04A", "h04C", "h04D"},
        })

    def test_scripted_building_spells_recover_handlers_and_mana_timed_cadence(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["scripted_building_spell_rows"], 43)
        self.assertEqual(summary["scripted_building_spell_registration_evidence_kinds"], {
            "direct-spell-effect-event-listener": 28,
            "protected-registry-call": 15,
        })
        self.assertEqual(summary["scripted_building_spell_mana_timed_rows"], 42)
        self.assertEqual(summary["scripted_building_spell_wc3_cooldown_timed_rows"], 1)
        self.assertEqual(summary["scripted_building_spell_evidence_rows"], 43)

        with (self.resolved / "building-spells.tsv").open(encoding="utf-8") as handle:
            rows = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 43)

        skull_pile = rows["h01P"]
        self.assertEqual(skull_pile["ability_rawcode"], "A01Q")
        self.assertEqual(skull_pile["cadence_seconds"], "12")
        self.assertEqual(skull_pile["effective_mana_cost"], "12")
        self.assertEqual(skull_pile["building_mana_regen"], "1")
        self.assertEqual(skull_pile["effective_wc3_cooldown"], "1.0")
        self.assertIn("RaceUndeadAbilities", skull_pile["handler_function"])

        death_pit = rows["h00A"]
        self.assertEqual(death_pit["cadence_seconds"], "15")
        self.assertEqual(death_pit["effective_mana_cost"], "15")
        self.assertEqual(death_pit["effective_wc3_cooldown"], "0.0")

        vessel = rows["h07U"]
        self.assertEqual(vessel["ability_rawcode"], "A0HN")
        self.assertEqual(vessel["cadence_seconds"], "15")
        self.assertIn("VesselOfPurity", vessel["handler_function"])

        tidal = rows["h00N"]
        self.assertEqual(tidal["ability_rawcode"], "A09X")
        self.assertEqual(tidal["effective_mana_cost"], "0")
        self.assertEqual(tidal["cadence_seconds"], "15")
        self.assertEqual(tidal["cadence_source"], "effective-wc3-ability-cooldown")
        self.assertEqual(tidal["registration_evidence_kind"], "direct-spell-effect-event-listener")

        with (self.resolved / "building-spell-evidence.tsv").open(encoding="utf-8") as handle:
            evidence = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(evidence), 43)
        pyramid = evidence["h00I"]
        self.assertEqual(pyramid["helper_functions"], "pyramidSpell")
        reachable = {row["rawcode"]: row for row in json.loads(pyramid["reachable_map_objects_json"])}
        self.assertIn("A00B", reachable)
        self.assertEqual(reachable["A00B"]["ability_level1"]["area"], "375")

    def test_scripted_building_spell_mechanics_normalize_all_registered_handlers(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["scripted_building_spell_mechanic_rows"], 43)
        self.assertEqual(summary["scripted_building_spell_mechanic_rows_with_evidence_disagreement"], 1)

        with (self.resolved / "building-spell-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(len(rows), 43)

        ruin = rows["h05I"]
        self.assertEqual(ruin["mechanic_kind"], "uniform-random-effect-table")
        ruin_params = json.loads(ruin["parameters_json"])
        self.assertEqual(ruin_params["selection"], "uniform-GetRandomInt(0,9)")
        self.assertEqual(ruin_params["branch_probability_percent"], 10)
        self.assertEqual([branch["roll"] for branch in ruin_params["branches"]], list(range(10)))
        self.assertEqual(ruin_params["branches"][1]["damage"], 50000)
        self.assertEqual(ruin_params["branches"][7]["unit_id"], int.from_bytes(b"n01S", "big"))
        ruin_reengage = ruin_params["control_reengage"]
        self.assertEqual(ruin_reengage["initial_reengage_delay_seconds"], "1")
        self.assertEqual(ruin_reengage["defender_defend_restore_delay_seconds_after_initial_reengage"], "44.5")
        self.assertEqual(ruin_reengage["defender_defend_order_id"], 852055)
        self.assertEqual(ruin_reengage["defender_attack_resume_delay_seconds_after_defend"], "0.5")

        city = json.loads(rows["h00Z"]["parameters_json"])
        self.assertEqual(city["effect_ability_id"], int.from_bytes(b"A018", "big"))
        self.assertEqual(city["control_reengage"], ruin_reengage)

        replenish = rows["h079"]
        replenish_params = json.loads(replenish["parameters_json"])
        self.assertEqual(replenish_params["selection_order"], ["Y0", "X0", "W0"])
        self.assertEqual(replenish_params["life_gain"], 175)
        self.assertEqual(replenish_params["mana_gain"], 40)
        self.assertEqual(replenish["evidence_kind"], "script-direct-with-protected-target-filters")

        peyote = rows["h07A"]
        peyote_effects = {row["rawcode"]: row for row in json.loads(peyote["effect_objects_json"])}
        illusion = peyote_effects["AM0y"]["ability_level1"]
        illusion_data = json.loads(illusion["data_fields_labeled_json"])
        self.assertEqual(illusion_data["Damage Dealt (% of normal)"], 1.25)
        self.assertEqual(illusion_data["Damage Received Multiplier"], 0.75)
        self.assertEqual(illusion["duration_normal"], "60")
        self.assertEqual(json.loads(peyote["parameters_json"])["order_id"], 852274)

        storm = rows["n03A"]
        storm_params = json.loads(storm["parameters_json"])
        self.assertEqual(storm_params["duration_seconds"], "6")
        storm_effects = {row["rawcode"]: row for row in json.loads(storm["effect_objects_json"])}
        sandstorm = json.loads(storm_effects["A0G4"]["ability_level1"]["data_fields_labeled_json"])
        self.assertEqual(sandstorm["Chance To Miss (%)"], 0.65)
        self.assertEqual(sandstorm["Attack Speed Modifier"], -0.3)
        self.assertEqual(sandstorm["Movement Speed Modifier"], -0.3)

        withering = json.loads(rows["h07T"]["parameters_json"])
        self.assertEqual(withering["mana_burn_cap"], 200)
        self.assertEqual(withering["damage_formula"], "2 * min(target_mana, 200)")
        self.assertEqual(withering["damage_type"], "sonic")

        silver = json.loads(rows["h08P"]["parameters_json"])
        self.assertEqual(silver["charges_granted_per_cast"], 1)
        self.assertEqual(silver["trigger_event"], "EVENT_PLAYER_UNIT_ATTACKED")
        self.assertEqual(silver["effect_ability_id"], int.from_bytes(b"A08A", "big"))

        wonders = json.loads(rows["h02D"]["parameters_json"])
        self.assertEqual(wonders["period_seconds"], "0.3")
        self.assertEqual(wonders["iterations"], 6)
        self.assertEqual(wonders["timed_life_seconds"], "42")

        mushroom = rows["h047"]
        self.assertEqual(mushroom["mechanic_kind"], "dummy-target-ability")
        self.assertEqual(mushroom["cadence_seconds"], "6")
        self.assertEqual(mushroom["evidence_disagreements"], "tooltip-spell-damage=150;linked-A0AK-Damage=200")
        mushroom_effects = {row["rawcode"]: row for row in json.loads(mushroom["effect_objects_json"])}
        mushroom_ability = mushroom_effects["A0AK"]["ability_level1"]
        self.assertEqual(json.loads(mushroom_ability["data_fields_labeled_json"])["Damage"], 200)
        self.assertEqual(mushroom_ability["duration_normal"], "2.5")

        frost = rows["h048"]
        frost_effects = {row["rawcode"]: row for row in json.loads(frost["effect_objects_json"])}
        self.assertEqual(frost_effects["h04G"]["protected_unitstat"]["attack1_min"], "50.0")
        self.assertEqual(frost_effects["h04G"]["protected_unitstat"]["attack1_max"], "50.0")
        self.assertEqual(frost_effects["A04F"]["ability_level1"]["duration_normal"], "10")

        greater_frost = rows["h03L"]
        greater_effects = {row["rawcode"]: row for row in json.loads(greater_frost["effect_objects_json"])}
        self.assertEqual(greater_effects["h04H"]["protected_unitstat"]["attack1_min"], "100.0")
        self.assertEqual(greater_effects["A04K"]["ability_level1"]["duration_normal"], "12")
        self.assertEqual(greater_effects["h04H"]["unit_object"]["attack1_full_aoe"], "200")

        world = rows["h03O"]
        world_params = json.loads(world["parameters_json"])
        self.assertEqual(world_params["orb_count"], 3)
        self.assertEqual(world_params["movement_speed_world_units_per_second"], "300")
        self.assertEqual(world_params["target_check_nominal_seconds"], "2.04")
        world_effects = {row["rawcode"]: row for row in json.loads(world["effect_objects_json"])}
        self.assertEqual(json.loads(world_effects["A081"]["ability_level1"]["data_fields_labeled_json"])["Movement Speed Increase (%)"], -0.3)
        self.assertEqual(json.loads(world_effects["A082"]["ability_level1"]["data_fields_labeled_json"])["Damage"], 125)
        self.assertEqual(world_effects["A082"]["ability_level1"]["duration_normal"], "8")
        self.assertEqual(json.loads(world_effects["A04H"]["ability_level1"]["data_fields_labeled_json"])["Damage per Second"], 15)
        self.assertEqual(world_effects["A04H"]["ability_level1"]["duration_normal"], "15")

        ceremonial = rows["h02R"]
        ceremonial_params = json.loads(ceremonial["parameters_json"])
        self.assertEqual(ceremonial_params["option_selection"], "uniform-GetRandomInt(1,3)")
        self.assertEqual(ceremonial_params["option_weights"], [1, 1, 1])
        ceremonial_effects = {row["rawcode"]: row for row in json.loads(ceremonial["effect_objects_json"])}
        self.assertEqual(json.loads(ceremonial_effects["A03D"]["ability_level1"]["data_fields_labeled_json"])["Max Life Gained"], 200)
        self.assertEqual(json.loads(ceremonial_effects["A08L"]["ability_level1"]["data_fields_labeled_json"])["Defense Bonus"], 3)

        stasis_effects = {row["rawcode"]: row for row in json.loads(rows["h07X"]["effect_objects_json"])}
        self.assertEqual(json.loads(stasis_effects["Ast9"]["ability_level1"]["data_fields_labeled_json"])["Stun Duration"], 4)
        healing_effects = {row["rawcode"]: row for row in json.loads(rows["h07Y"]["effect_objects_json"])}
        self.assertEqual(json.loads(healing_effects["AstB"]["ability_level1"]["data_fields_labeled_json"])["Amount of Hit Points Regenerated"], 20)
        self.assertEqual(json.loads(rows["h07Y"]["parameters_json"])["carrier_or_dummy_lifetime_seconds"], "8")
        endurance_effects = {row["rawcode"]: row for row in json.loads(rows["h07Z"]["effect_objects_json"])}
        self.assertEqual(json.loads(endurance_effects["AstC"]["ability_level1"]["data_fields_labeled_json"])["Attack Speed Increase (%)"], 0.33)
        self.assertEqual(json.loads(rows["h07Z"]["parameters_json"])["carrier_or_dummy_lifetime_seconds"], "6")

        serpent_effects = {row["rawcode"]: row for row in json.loads(rows["h02N"]["effect_objects_json"])}
        serpent_data = json.loads(serpent_effects["A030"]["ability_level1"]["data_fields_labeled_json"])
        self.assertEqual(serpent_data["Primary Damage"], 30)
        self.assertEqual(serpent_data["Armor Penalty"], 5)
        self.assertEqual(serpent_effects["A030"]["ability_level1"]["duration_normal"], "5")

        death = json.loads(rows["h00A"]["parameters_json"])
        self.assertEqual(death["damage"], "50000")
        self.assertEqual(death["damage_type"], "death")

        snow = json.loads(rows["h07W"]["parameters_json"])
        self.assertEqual(snow["incoming_damage_reduction_percent"], 20)
        self.assertEqual(snow["manual_explosion_radius"], "384")
        self.assertEqual(snow["manual_explosion_damage"], "350")
        self.assertTrue(snow["manual_explosion_issues_stop_to_casting_fountain"])
        self.assertTrue(snow["manual_explosion_cooldown_applies_to_all_owner_snowveil_fountains"])
        self.assertEqual(snow["manual_explosion_target_point_source"], "GetSpellTargetX/GetSpellTargetY")
        self.assertEqual(rows["h07W"]["evidence_kind"], "script-direct-with-resolved-generated-target-filter")

        thunder = json.loads(rows["h07R"]["parameters_json"])
        self.assertEqual(thunder["damage_multiplier"], 2)
        self.assertEqual(thunder["minimum_damage"], 75)
        self.assertEqual(thunder["maximum_damage"], 300)
        self.assertEqual(thunder["effect_radius"], 300)
        self.assertEqual(thunder["flying_recipient_multiplier"], "0.5")
        self.assertEqual(thunder["special_source_multiplier"], "0.35")
        self.assertEqual(thunder["proc_source_reentrancy_guard"], "QS")
        self.assertTrue(thunder["proc_source_guard_cleared_after_zero_delay"])

    def test_scripted_corpse_building_mechanics_keep_exact_predicates_and_effects(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["corpse_building_mechanic_rows"], 3)
        self.assertEqual(summary["corpse_building_raise_rows"], 2)

        with (self.resolved / "corpse-building-mechanics.tsv").open(encoding="utf-8") as handle:
            rows = {row["building_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(set(rows), {"h01P", "h056", "h07U"})

        skull_pile = rows["h01P"]
        self.assertEqual(skull_pile["cadence_seconds"], "12")
        self.assertEqual(skull_pile["corpse_phase"], "dying")
        self.assertEqual(skull_pile["requires_wc3_can_raise"], "0")
        self.assertEqual(skull_pile["invulnerable_ability_rawcode"], "Avul")
        self.assertEqual(skull_pile["invulnerable_ability_name"], "Invulnerable")
        outcomes = json.loads(skull_pile["summon_outcomes_json"])
        self.assertEqual(
            [(row["rawcode"], row["probability_percent"]) for row in outcomes],
            [("n00C", 10), ("u002", 30), ("n00D", 30), ("u00A", 30)],
        )

        shrine = rows["h056"]
        shrine_outcomes = json.loads(shrine["summon_outcomes_json"])
        self.assertEqual(
            [(row["rawcode"], row["probability_percent"]) for row in shrine_outcomes],
            [("n01M", 10), ("u00B", 30), ("n01L", 30), ("u003", 30)],
        )

        vessel = rows["h07U"]
        self.assertEqual(vessel["cadence_seconds"], "15")
        self.assertEqual(vessel["corpse_phase"], "dead")
        self.assertEqual(vessel["consumption_mode"], "all-qualifying-within-radius")
        self.assertEqual(vessel["consume_radius"], "220")
        self.assertEqual(vessel["effect_radius"], "300")
        self.assertEqual(vessel["damage"], "150")
        self.assertEqual(vessel["damage_type"], "universal")
        self.assertEqual(vessel["auxiliary_ability_rawcode"], "A9FS")
        self.assertEqual(vessel["auxiliary_ability_name"], "Far Sight")
        self.assertEqual(vessel["requires_wc3_can_raise"], "0")

    def test_production_corpse_profiles_keep_death_type_capabilities_and_decay_constants(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["production_unit_corpse_rows"], 167)
        self.assertEqual(summary["production_unit_death_type_counts"], {"0": 38, "1": 2, "2": 24, "3": 103})
        self.assertEqual(summary["normal_production_unit_death_type_counts"], {"0": 38, "1": 2, "2": 24, "3": 98})
        self.assertEqual(summary["death_decay_constants"], {
            "bone_decay": "25", "flesh_decay": "2", "structure_decay": "0.1",
        })

        with (self.resolved / "death-decay-constants.tsv").open(encoding="utf-8") as handle:
            constants = {row["constant"]: row for row in csv.DictReader(handle, delimiter="\t")}
        self.assertEqual(constants["flesh_decay"]["base_value"], "2")
        self.assertEqual(constants["flesh_decay"]["map_override"], "")
        self.assertEqual(constants["flesh_decay"]["effective_source"], "base-default")
        self.assertEqual(constants["bone_decay"]["base_value"], "88")
        self.assertEqual(constants["bone_decay"]["map_override"], "25")
        self.assertEqual(constants["structure_decay"]["base_value"], "30")
        self.assertEqual(constants["structure_decay"]["map_override"], "0.1")

        with (self.resolved / "production-unit-corpses.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        footman = rows["hfoo"]
        self.assertEqual(footman["death_type"], "3")
        self.assertEqual(footman["death_type_label"], "can-raise-decays")
        self.assertEqual(footman["can_raise"], "1")
        self.assertEqual(footman["does_decay"], "1")
        self.assertEqual(footman["death_time"], "3.04")
        self.assertEqual(footman["death_plus_flesh_plus_bones"], "30.04")

        bear = rows["n029"]
        self.assertEqual(bear["death_type"], "1")
        self.assertEqual(bear["can_raise"], "1")
        self.assertEqual(bear["does_decay"], "0")
        self.assertEqual(bear["death_plus_flesh_plus_bones"], "")

        light_tank = rows["h06T"]
        self.assertEqual(light_tank["is_mechanical"], "1")
        self.assertEqual(light_tank["death_type"], "2")
        self.assertEqual(light_tank["can_raise"], "0")
        self.assertEqual(light_tank["does_decay"], "1")

    def test_resolution_has_no_inheritance_or_pathing_gaps(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["unresolved_base_objects"], [])
        self.assertEqual(summary["missing_pathing_textures"], [])
        self.assertEqual(summary["unresolved_placed_object_types"], [])
        self.assertEqual(summary["missing_placed_pathing_textures"], [])

    def test_protected_ability_runtime_table_overrides_static_sentinels(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["protected_ability_runtime_fields"], 452)
        self.assertEqual(summary["protected_ability_runtime_field_comparisons"], {"static-differs": 452})

        with (self.resolved / "protected-ability-fields.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        indexed = {(row["rawcode"], row["level"], row["field"]): row for row in rows}

        rescue = indexed[("A005", "1", "cooldown")]
        self.assertEqual(rescue["runtime_value"], "60.0")
        self.assertEqual(rescue["static_resolved_value"], "99")
        self.assertEqual(rescue["comparison"], "static-differs")

        snowfall = indexed[("A0HO", "1", "mana_cost")]
        self.assertEqual(snowfall["runtime_value"], "15")
        self.assertEqual(snowfall["static_resolved_value"], "9999")

    def test_jass_add_restore_cross_check_keeps_static_match(self) -> None:
        with (self.resolved / "protected-ability-jass-add-restores.tsv").open(encoding="utf-8") as handle:
            rows = list(csv.DictReader(handle, delimiter="\t"))
        jass_only = [row for row in rows if row["canonical_relation"] == "jass-only"]
        self.assertEqual(len(jass_only), 1)
        self.assertEqual(jass_only[0]["rawcode"], "A010")
        self.assertEqual(jass_only[0]["runtime_value"], "3.")
        self.assertEqual(jass_only[0]["static_resolved_value"], "3")
        self.assertEqual(jass_only[0]["comparison"], "static-match")

    def test_effective_unit_catalog_exposes_poisoned_static_stats(self) -> None:
        summary = json.loads((self.resolved / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(summary["effective_unit_stat_rows"], 162)
        self.assertEqual(summary["effective_unit_stat_comparisons"]["hp"], {
            "static-differs": 152,
            "static-match": 10,
        })
        self.assertEqual(summary["effective_unit_stat_comparisons"]["move_speed"], {
            "static-match": 162,
        })

        with (self.resolved / "effective-unit-stats.tsv").open(encoding="utf-8") as handle:
            rows = {row["unit_rawcode"]: row for row in csv.DictReader(handle, delimiter="\t")}
        faerie = rows["e000"]
        self.assertEqual(faerie["building_rawcode"], "h00B")
        self.assertEqual(faerie["effective_hp"], "460")
        self.assertEqual(faerie["static_hp"], "1")
        self.assertEqual(faerie["effective_attack_range"], "350")
        self.assertEqual(faerie["static_attack1_range"], "1")
        self.assertEqual(faerie["hp_comparison"], "static-differs")

        footman = rows["hfoo"]
        self.assertEqual(footman["effective_hp"], "250")
        self.assertEqual(footman["static_hp"], "250")
        self.assertEqual(footman["dps_comparison"], "static-match")

        mortar = rows["hmtm"]
        self.assertEqual(mortar["effective_move_speed"], "270")
        self.assertEqual(mortar["static_move_speed"], "270")
        self.assertEqual(mortar["move_speed_comparison"], "static-match")


if __name__ == "__main__":
    unittest.main()
