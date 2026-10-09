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
