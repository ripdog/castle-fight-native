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


class NativeStatusDeliveryAuditTests(unittest.TestCase):
    def setUp(self):
        self.buff = {"rawcode": "BUFF", "target_art": ["stock.mdl"],
                     "target_attachment_count": None,
                     "target_attachments": [{"index": 0, "point": "head", "source": "native"}]}
        self.projection = {"buffs": [self.buff]}
        self.fields = [{"category": "abilities", "rawcode": "CAST", "base_rawcode": "ACff",
                        "field_id": "abuf", "recovered_value_json": '"BUFF"'}]
        self.binding = {"ability_rawcode": "CAST", "buff_rawcode": "BUFF", "status_kind": "armor",
                        "source_model": "stock.mdx", "gltf": "models/stock.gltf",
                        "target_attachment_count": None,
                        "target_attachments": self.buff["target_attachments"]}

    def audit(self, entries):
        return audit.audit_native_status_visuals(self.projection, self.fields, {"CAST"},
                                                {"status_visuals": entries})

    def test_missing_duplicate_and_unresolved_native_stock_art_fail(self):
        self.assertEqual(self.audit([self.binding]), ([], [self.binding]))
        for entries in ([], [self.binding, self.binding], [dict(self.binding, gltf=None)]):
            self.assertEqual(len(self.audit(entries)[0]), 1)
        self.assertEqual(audit.audit_native_status_visuals(self.projection, self.fields, set(), {}), ([], []))

    def test_freeze_requires_its_native_buff_binding(self):
        self.fields[0]["base_rawcode"] = "Afrz"
        entry = dict(self.binding, status_kind="freeze")
        self.assertEqual(self.audit([entry]), ([], [entry]))
        self.assertTrue(self.audit([])[0])

    def test_authored_attachment_changes_are_not_delivery_success(self):
        for invalid in (dict(self.binding, target_attachments=[]),
                        dict(self.binding, target_attachment_count=2)):
            self.assertEqual(len(self.audit([invalid])[0]), 1)
        self.assertTrue(self.audit([dict(self.binding, buff_rawcode="WRNG")])[0])

    def test_explicit_empty_carrier_art_must_not_fall_back_to_stock(self):
        self.fields[0]["base_rawcode"] = "Apxf"
        self.buff["target_art"] = []
        self.assertEqual(self.audit([]), ([], []))
        unexpected = dict(self.binding, status_kind="damage_over_time")
        self.assertEqual(len(self.audit([unexpected])[0]), 1)


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


class NativeDeliveryAuditTests(unittest.TestCase):
    def test_native_ability_reference_closure_follows_orb_children_cycles_and_ignores_text(self):
        fields = [
            {"category": "abilities", "rawcode": "PARN", "value_type": "abilCode", "recovered_value_json": '"CHLD"'},
            {"category": "abilities", "rawcode": "CHLD", "value_type": "abilList", "recovered_value_json": '"LEAF,PARN,_,0"'},
            {"category": "abilities", "rawcode": "LEAF", "value_type": "string", "recovered_value_json": '"TEXT"'},
        ]
        self.assertEqual(audit.ability_dependency_closure(fields, {"PARN"}), {"PARN", "CHLD", "LEAF"})

    def projection(self):
        binding = {"rawcode": "CHLD", "base_rawcode": "BASE", "native_section": "BASE",
                   "effects": ["BEAM"], "target_art": ["impact.mdl"]}
        definition = {"id": "BEAM", "texture": "beam.blp", "width": 12}
        return {"abilities": [binding], "effects": [definition]}

    def test_missing_native_pack_cannot_pass_with_source_required_beams(self):
        findings = audit.audit_native_lightnings(self.projection(), {}, {"CHLD"}, HERE)
        self.assertEqual(len(findings), 3)
        self.assertTrue(any("binding CHLD" in finding for finding in findings))
        self.assertTrue(any("definition BEAM" in finding for finding in findings))
        self.assertEqual(audit.audit_native_lightnings(self.projection(), {}, {"OTHER"}, HERE), [])

    def test_native_binding_definition_and_delivered_texture_are_all_required(self):
        projection = self.projection()
        binding = {key: value for key, value in projection["abilities"][0].items() if key != "target_art"}
        manifest = {"native_lightnings": {
            "abilities": [binding],
            "effects": [dict(projection["effects"][0], png="beam.png")]},
            "assets": [{"owner_kind": "abilities", "owner_rawcode": "CHLD", "role": "target",
                        "source_model": "impact.mdx", "gltf": "impact.gltf"}]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.assertTrue(audit.audit_native_lightnings(projection, manifest, {"CHLD"}, root))
            (root / "beam.png").touch()
            self.assertEqual(audit.audit_native_lightnings(projection, manifest, {"CHLD"}, root), [])
            stale = json.loads(json.dumps(manifest))
            stale["native_lightnings"]["effects"][0]["width"] += 1
            self.assertIn("missing/stale native lightning definition BEAM",
                          audit.audit_native_lightnings(projection, stale, {"CHLD"}, root))
            stale = json.loads(json.dumps(manifest))
            stale["native_lightnings"]["abilities"][0]["effects"] = ["OTHER"]
            self.assertIn("missing/stale native lightning binding CHLD",
                          audit.audit_native_lightnings(projection, stale, {"CHLD"}, root))
            manifest["native_lightnings"]["abilities"] = [binding] * 2
            self.assertIn("duplicate native lightning binding CHLD",
                          audit.audit_native_lightnings(projection, manifest, {"CHLD"}, root))

    def test_native_inventory_ownership_requires_child_beam_and_missile_aliases(self):
        rows = [{"owner_kind": "abilities", "owner_rawcode": "CHLD", "role": role,
                 "source_unit_rawcode": None, "source_model": role + ".mdl", "gltf": role + ".gltf"}
                for role in ("missile", "target", "caster")]
        inventories = {"UNIT": {"PARN", "CHLD"}}
        self.assertEqual(len(audit.audit_native_inventory_ownership(inventories, rows, {"CHLD"})), 2)
        aliases = [dict(row, source_unit_rawcode="UNIT") for row in rows[:2]]
        self.assertEqual(audit.audit_native_inventory_ownership(inventories, rows + aliases, {"CHLD"}), [])
        self.assertEqual(audit.audit_native_inventory_ownership({"UNIT": {"OTHER"}}, rows, {"CHLD"}), [])

    def test_proxy_ownership_keeps_child_missile_and_beam_target_but_parent_cast(self):
        links = [{"unit": "UNIT", "parent": "PARN", "children": ["CHLD"]}]
        rows = [{"owner_kind": "abilities", "owner_rawcode": "CHLD",
                 "source_unit_rawcode": None, "role": role,
                 "source_model": role + ".mdl", "gltf": role + ".gltf"}
                for role in ("missile", "target", "caster")]
        self.assertEqual(len(audit.audit_proxy_ownership(links, rows, {"CHLD"})), 3)
        aliases = [dict(row, owner_rawcode="PARN" if row["role"] == "caster" else "CHLD",
                        source_unit_rawcode="UNIT", source_model=row["role"] + ".mdx")
                   for row in rows]
        self.assertEqual(audit.audit_proxy_ownership(links, rows + aliases, {"CHLD"}), [])
        aliases[0]["source_unit_rawcode"] = "ALLY"
        self.assertEqual(len(audit.audit_proxy_ownership(links, rows + aliases, {"CHLD"})), 1)


if __name__ == "__main__":
    unittest.main()
