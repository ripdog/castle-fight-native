import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


lightning = load("lightning", "extract-lightning-visuals.py")
audit = load("audit", "audit-race-presentation.py")


class LightningProjectionTests(unittest.TestCase):
    def test_child_identity_case_variant_and_map_art_overrides(self):
        skin = "[aBcD]\nLightningEffect=ONE,TWO\nTargetArt=stock.mdl\n"
        slk = '\n'.join([
            'C;X1;Y1;K"Name"', 'C;X2;K"Dir"', 'C;X3;K"file"', 'C;X4;K"Width"',
            'C;X5;K"AvgSegLen"', 'C;X6;K"NoiseScale"', 'C;X7;K"TexCoordScale"',
            'C;X8;K"R"', 'C;X9;K"G"', 'C;X10;K"B"', 'C;X11;K"A"',
            'C;X1;Y2;K"ONE"', 'C;X2;K"Textures"', 'C;X3;K"one.blp"',
            'C;X4;K12', 'C;X5;K24', 'C;X6;K0.1', 'C;X7;K0.5',
            'C;X8;K1', 'C;X9;K2', 'C;X10;K3', 'C;X11;K4'])
        fields = [{"category": "abilities", "rawcode": "CHLD", "base_rawcode": "abcd",
                   "field_id": "alig", "recovered_value_json": '"ONE"'},
                  {"category": "abilities", "rawcode": "CHLD", "base_rawcode": "abcd",
                   "field_id": "atat", "recovered_value_json": '"custom.mdx"'}]
        abilities, effects = lightning.project(skin, slk, fields)
        self.assertEqual(abilities, [{"rawcode": "CHLD", "base_rawcode": "abcd",
                                    "native_section": "aBcD", "effects": ["ONE"],
                                    "target_art": ["custom.mdx"]}])
        self.assertEqual(effects[0]["texture"], "Textures\\one.blp")
        self.assertEqual(effects[0]["width"], 12)
        self.assertEqual(effects[0]["color"], [1, 2, 3, 4])

    def test_retained_projection_preserves_source_identity(self):
        import hashlib
        evidence = HERE.parents[1] / "docs/original_map/extracted/resolved"
        projection = json.loads((evidence / "native-lightning-visuals.json").read_text())
        self.assertEqual(projection["objects_sha256"], hashlib.sha256(
            (evidence / "object-fields.tsv").read_bytes()).hexdigest())
        self.assertTrue(all(len(source["sha256"]) == 64 for source in projection["sources"]))
        definitions = {effect["id"] for effect in projection["effects"]}
        self.assertTrue(all(set(ability["effects"]) <= definitions for ability in projection["abilities"]))


class ModelBindingAuditTests(unittest.TestCase):
    def test_safe_paths_allow_pack_local_gltf_dependencies_but_reject_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "models").mkdir()
            (root / "textures").mkdir()
            (root / "textures/test.png").touch()
            self.assertEqual(audit.safe_file(root / "models", "../textures/test.png", root),
                             root / "textures/test.png")
            with self.assertRaisesRegex(ValueError, "unsafe"):
                audit.safe_file(root / "models", "../../escape.png", root)
            with self.assertRaisesRegex(ValueError, "missing"):
                audit.safe_file(root, "absent.bin")

    def test_rejects_missing_joints_and_animation_bindings(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "model.gltf"
            gltf = {"nodes": [{"skin": 0, "mesh": 0}],
                    "skins": [{"joints": [8]}], "meshes": []}
            path.write_text(json.dumps(gltf))
            with self.assertRaisesRegex(ValueError, "skeletal"):
                audit.validate_gltf(path)
            path.write_text(json.dumps({"nodes": [{}], "animations": [
                {"channels": [{"target": {"node": 7}}]}]}))
            with self.assertRaisesRegex(ValueError, "animation"):
                audit.validate_gltf(path)

    def test_attachment_and_model_aliases_match_exporter_normalization(self):
        self.assertEqual(audit.attachment_name("Weapon - Ref "), "weapon")
        self.assertEqual(audit.model_identity("Art\\Model.MDL"),
                         audit.model_identity("art/model.mdx"))


if __name__ == "__main__":
    unittest.main()
