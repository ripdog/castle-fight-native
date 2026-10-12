import json
import unittest
import build_runtime_catalog as catalog
import build_unit_presentation as projection

class UnitPresentationTest(unittest.TestCase):
    def test_committed_projection_reproduces_retained_inventory_and_entry_script(self):
        release = catalog._load_release(catalog.DEFAULT_RELEASES, '9.27', 'r1')
        path = catalog.REPO_ROOT / 'crates/wc3-assets/data/castle-fight/9.27/native-unit-presentation-r1.json'
        self.assertEqual(json.loads(path.read_text()), projection.build(release))

if __name__ == '__main__':
    unittest.main()


class EntryTypeColorTests(unittest.TestCase):
    def script(self, target='actor', predicate='kind'):
        code=int.from_bytes(b'TEST','big')
        return f'function onUnitEnteredMap()actor=GetTriggerUnit()kind=unit_getTypeId(actor)if({predicate}=={code})then __wurst_safe_SetUnitVertexColor({target},11,22,33,44)end end function next()end'
    def test_direct_type_override_retains_native_rgba_without_copying_source_values(self):
        self.assertEqual(projection.entry_type_colors(self.script()), {'TEST':[11,22,33,44]})
    def test_foreign_actor_and_predicate_fail_closed(self):
        for target,predicate in [('other','kind'),('actor','other')]:
            with self.subTest(target=target,predicate=predicate),self.assertRaises(ValueError):
                projection.entry_type_colors(self.script(target,predicate))
