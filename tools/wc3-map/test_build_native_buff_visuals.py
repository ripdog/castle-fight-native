import hashlib
import json
import unittest
from pathlib import Path

import build_native_buff_visuals as buffs
from build_runtime_catalog import _load_release, _retained_file_bytes


def row(category, rawcode, base, field, value):
    return {"category": category, "rawcode": rawcode, "base_rawcode": base,
            "field_id": field, "recovered_value_json": json.dumps(value)}


class NativeBuffProjectionTests(unittest.TestCase):
    def test_buff_icons_keep_native_last_assignment_and_explicit_map_clear(self):
        skin = "[STOK]\nBuffart=old.blp\nBuffart=stock.blp\n"
        fields = [row("abilities", "CAST", "BASE", "abuf", "STOK,CUST,NONE"),
                  row("buffs", "CUST", "STOK", "fart", "custom.blp"),
                  row("buffs", "NONE", "STOK", "fart", "")]
        projection = {buff["rawcode"]: buff for buff in buffs.project(skin, fields)}
        self.assertEqual(projection["STOK"]["icon_art"], "stock.blp")
        self.assertEqual(projection["STOK"]["icon_art_source"], "native")
        self.assertEqual(projection["CUST"]["icon_art"], "custom.blp")
        self.assertEqual(projection["CUST"]["icon_art_source"], "resolved")
        self.assertEqual(projection["NONE"]["icon_art"], "")
        self.assertEqual(projection["NONE"]["icon_art_source"], "resolved")

    def test_empty_native_parasite_buff_uses_base_and_explicit_buff_overrides_it(self):
        inherited = row("abilities", "CAST", "ACpa", "abuf", "")
        inherited["base_value_json"] = json.dumps("STOK,NONE")
        skin = "[STOK]\nTargetart=stock.mdl\n[NONE]\n"
        self.assertEqual([p["rawcode"] for p in buffs.project(skin, [inherited])], ["NONE", "STOK"])
        inherited["recovered_value_json"] = json.dumps("NONE")
        self.assertEqual([p["rawcode"] for p in buffs.project(skin, [inherited])], ["NONE"])

    def test_ground_effect_references_preserve_skin_and_explicit_override(self):
        skin = "[FIRE]\nEffectart=embers.mdl\n"
        fields = [row("abilities", "CAST", "BASE", "aeff", "FIRE,CUST,NONE"),
                  row("buffs", "CUST", "FIRE", "feat", "other.mdl"),
                  row("buffs", "NONE", "FIRE", "feat", "")]
        projection = {buff["rawcode"]: buff for buff in buffs.project(skin, fields)}
        self.assertEqual(projection["FIRE"]["effect_art"], ["embers.mdl"])
        self.assertEqual(projection["FIRE"]["effect_art_source"], "native")
        self.assertEqual(projection["CUST"]["effect_art"], ["other.mdl"])
        self.assertEqual(projection["NONE"]["effect_art"], [])

    def test_stock_and_custom_art_preserve_explicit_empty_and_attachment_identity(self):
        skin = "[STOK]\nTargetArt=stock.mdl\nTargetattach=head\nTargetattachcount=1\n"
        fields = [row("abilities", "CAST", "BASE", "abuf", "STOK,CUST,NONE"),
                  row("buffs", "CUST", "STOK", "ftat", "custom.mdl"),
                  row("buffs", "CUST", "STOK", "fta0", "hand,left"),
                  row("buffs", "NONE", "STOK", "ftat", "")]
        projection = {buff["rawcode"]: buff for buff in buffs.project(skin, fields)}
        self.assertEqual(projection["STOK"]["target_art"], ["stock.mdl"])
        self.assertEqual(projection["STOK"]["target_attachments"],
                         [{"index": 0, "point": "head", "source": "native"}])
        self.assertEqual(projection["CUST"]["target_art"], ["custom.mdl"])
        self.assertEqual(projection["CUST"]["target_attachments"][0]["point"], "hand,left")
        self.assertEqual(projection["NONE"]["target_art"], [])
        self.assertEqual(projection["NONE"]["target_art_source"], "resolved")
        self.assertEqual(projection["NONE"]["target_attachment_count"], 1)
        self.assertEqual(buffs.project(skin, list(reversed(fields))), buffs.project(skin, fields))

    def test_multiple_models_and_points_keep_authored_count_and_order(self):
        skin = "[STOK]\nTargetArt=one.mdl,two.mdx\nTargetattach=hand,left\nTargetattach1=hand,right\nTargetattachcount=2\n"
        projection = buffs.project(skin, [row("abilities", "CAST", "BASE", "abuf", "STOK")])[0]
        self.assertEqual(projection["target_art"], ["one.mdl", "two.mdx"])
        self.assertEqual(projection["target_attachment_count"], 2)
        self.assertEqual([point["point"] for point in projection["target_attachments"]],
                         ["hand,left", "hand,right"])

    def test_missing_fields_are_distinct_from_explicit_empty_and_no_origin_is_invented(self):
        projection = buffs.project("[STOK]\nBufftip=Marker\n", [row("abilities", "CAST", "BASE", "abuf", "STOK")])[0]
        self.assertEqual(projection["target_art_source"], "absent")
        self.assertIsNone(projection["target_attachment_count"])
        self.assertEqual(projection["target_attachments"], [])

    def test_invalid_and_conflicting_source_values_are_rejected(self):
        for fields in ([row("abilities", "CAST", "BASE", "abuf", "bad")],
                       [row("abilities", "CAST", "BASE", "abuf", 5)],
                       [row("abilities", "CAST", "BASE", "abuf", "CUST"), row("buffs", "CUST", "STOK", "ftac", 7)],
                       [row("buffs", "CUST", "STOK", "ftat", "one"), row("buffs", "CUST", "STOK", "ftat", "two")],
                       [row("buffs", "CUST", "STOK", "ftat", "one"), row("buffs", "CUST", "DIFF", "fta0", "head")]):
            with self.subTest(fields=fields), self.assertRaises(ValueError):
                buffs.project("[STOK]\n", fields)
        with self.assertRaises(ValueError):
            buffs.sections("[STOK]\nTargetArt=one\nTargetArt=two\n")

    def test_native_cache_must_match_retained_manifest_not_just_filename(self):
        data = b"[STOK]\nTargetArt=stock.mdl\n"
        source = {"path": buffs.SKIN_PATH, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}
        self.assertEqual(buffs.verify_skin(data, [source]), source)
        for invalid_data, manifest in ((data + b" ", [source]), (data, []), (data, [source, source]),
                                       (data, [dict(source, sha256="0" * 64)])):
            with self.assertRaises(ValueError):
                buffs.verify_skin(invalid_data, manifest)

    def test_committed_projection_is_version_pinned_and_keeps_native_source_identity(self):
        artifact = json.loads(buffs.OUTPUT.read_text())
        release = _load_release(buffs.ROOT / "docs/original_map/releases.json",
                                artifact["map_version"], artifact["source_revision"])
        tree = release["extraction"]["git_tree"]
        self.assertEqual(artifact["extraction_git_tree"], tree)
        for path, digest in artifact["sources"].items():
            self.assertEqual(digest, hashlib.sha256(_retained_file_bytes(buffs.ROOT, tree, f"resolved/{path}")).hexdigest())
        manifest = json.loads(_retained_file_bytes(buffs.ROOT, tree, "resolved/base-data-manifest.json"))
        self.assertIn(artifact["native_skin"], manifest)
        self.assertEqual([buff["rawcode"] for buff in artifact["buffs"]],
                         sorted({buff["rawcode"] for buff in artifact["buffs"]}))


if __name__ == "__main__":
    unittest.main()
