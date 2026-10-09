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

    def test_colored_call_retains_rgb_without_accepting_missing_or_out_of_range_channels(self):
        body = 'function Handler(a,b)attachBuildingEffectColor(b,90.,0.,0.,"Model.mdl",1.2,100.,90,255,255)end'
        self.assertEqual(projection.project_call(body)['color_rgb'], [90,255,255])
        for invalid in (body.replace(',90,255,255)', ')'), body.replace(',90,255,255)', ',90,999,255)')):
            with self.assertRaises(ValueError):
                projection.project_call(invalid)

    def test_multiple_calls_preserve_order_zero_scale_and_reject_other_operations(self):
        first = 'attachBuildingEffect(b,270.,0.,28.,"One.mdl",0.,0.)'
        second = 'attachBuildingEffect(b,90.,(-12.),(-34.),"Two.mdl",0.2,(-20.))'
        body = f'function Handler(a,b){first}{second}end'
        visuals = projection.project_calls(body)
        self.assertEqual([row['model_path'] for row in visuals], ['One.mdl', 'Two.mdl'])
        self.assertEqual([row['scale'] for row in visuals], [0, 0.2])
        self.assertEqual(visuals[1]['offset_world'], [-12, -34, -20])
        for invalid in (
            body.replace(first, 'if flag then ' + first + 'end'),
            body.replace('270.', 'dynamicYaw'),
            body.replace(second, second.replace('(b,', '(a,')),
            body.replace(second, 'otherOperation(b)'),
        ):
            with self.assertRaises(ValueError):
                projection.project_calls(invalid)


if __name__ == '__main__':
    unittest.main()
