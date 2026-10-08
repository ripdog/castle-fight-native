import json
import unittest
from pathlib import Path

import build_building_attachments as projection
import build_runtime_catalog as catalog


class BuildingAttachmentsTest(unittest.TestCase):
    def test_committed_projection_uses_retained_registration_and_script_geometry(self):
        release = catalog._load_release(catalog.DEFAULT_RELEASES, '9.27', 'r1')
        path = catalog.REPO_ROOT / 'crates/wc3-assets/data/castle-fight/9.27/native-building-attachments-r1.json'
        self.assertEqual(json.loads(path.read_text()), projection.build(release))

    def test_direct_native_call_preserves_world_offsets_facing_and_scale(self):
        body = 'function Handler(a,b)attachBuildingEffect(b,90.,12.,(-34.),"Model.mdl",0.5,(-20.))end'
        visual = projection.project_call(body)
        self.assertEqual(visual['offset_world'], [12, -34, -20])
        self.assertEqual(visual['yaw_degrees'], 90)
        self.assertEqual(visual['scale'], 0.5)
        with self.assertRaises(ValueError):
            projection.project_call(body.replace('attachBuildingEffect', 'attachBuildingEffectColor'))


if __name__ == '__main__':
    unittest.main()
