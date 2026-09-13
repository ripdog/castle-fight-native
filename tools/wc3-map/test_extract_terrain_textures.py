from __future__ import annotations

import importlib.util
from pathlib import Path
import tempfile
import unittest

MODULE_PATH = Path(__file__).with_name("extract_terrain_textures.py")
SPEC = importlib.util.spec_from_file_location("wc3_extract_terrain_textures", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class TerrainTextureExtractionTests(unittest.TestCase):
    def test_parses_warcraft_sylk_rows_with_inherited_y(self) -> None:
        source = """ID;PWXL;N;E\r\nC;X1;Y2;K\"Adrt\"\r\nC;X3;K\"TerrainArt\\Ashenvale\"\r\nC;X4;K\"Ashen_Dirt\"\r\nC;X5;K\"Dirt\"\r\nC;X1;Y3;K\"CAgr\"\r\nC;X4;K\"TerrainArt\\Ashenvale\"\r\nC;X5;K\"Ashen_Cliff1\"\r\n"""
        with tempfile.TemporaryDirectory() as temp_name:
            path = Path(temp_name) / "fixture.slk"
            path.write_text(source, encoding="utf-8", newline="")
            rows = MODULE.parse_sylk(path)

        self.assertEqual(rows["Adrt"][3], r"TerrainArt\Ashenvale")
        self.assertEqual(rows["Adrt"][4], "Ashen_Dirt")
        self.assertEqual(rows["CAgr"][5], "Ashen_Cliff1")

    def test_resolves_ground_and_cliff_metadata_columns(self) -> None:
        ground = MODULE.texture_request(
            "Adrt",
            1,
            {3: r"TerrainArt\Ashenvale", 4: "Ashen_Dirt", 5: "Dirt"},
            "ground",
        )
        cliff = MODULE.texture_request(
            "CAgr",
            0,
            {4: r"TerrainArt\Ashenvale", 5: "Ashen_Cliff1", 6: "Ashenvale Cliff"},
            "cliff",
        )
        self.assertEqual(ground["logical_stem"], r"TerrainArt\Ashenvale\Ashen_Dirt")
        self.assertEqual(cliff["logical_stem"], r"TerrainArt\Ashenvale\Ashen_Cliff1")
        self.assertEqual(ground["palette_index"], 1)
        self.assertEqual(cliff["palette_index"], 0)

    def test_validates_square_and_extended_ground_atlases(self) -> None:
        square = MODULE.validate_ground_atlas(256, 256, "Adrg")
        extended = MODULE.validate_ground_atlas(512, 256, "Adrt")
        self.assertEqual(square["tile_pixels"], 64)
        self.assertFalse(square["extended"])
        self.assertTrue(extended["extended"])
        self.assertEqual(extended["full_variant_columns"], 4)

        with self.assertRaises(ValueError):
            MODULE.validate_ground_atlas(300, 256, "bad!")


if __name__ == "__main__":
    unittest.main()
