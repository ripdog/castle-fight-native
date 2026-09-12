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


if __name__ == "__main__":
    unittest.main()
