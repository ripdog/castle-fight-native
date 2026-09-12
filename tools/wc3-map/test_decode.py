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


if __name__ == "__main__":
    unittest.main()
