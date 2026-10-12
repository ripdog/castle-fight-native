import hashlib
import json
import tempfile
import unittest
from pathlib import Path

from project_native_reference import project


class NativeReferenceTests(unittest.TestCase):
    def fixture(self, root):
        entries = {
            'custom_v1/units/abilitydata.slk': b'ID;PWXL;N;E\nB;X2;Y2;D0\nC;X1;Y1;K"alias"\nC;X2;K"code"\nC;X1;Y2;K"ACsw"\nC;X2;K"Aslo"\nE;\n',
            'custom_v1/units/miscgame.txt': b'EtherealDamageBonus=0,0,0,2,0,2,0\nEtherealDamageBonusAlly=1\nEtherealHealBonus=3\n',
        }
        manifest = []
        for path, data in entries.items():
            file = root / '.wc3-base/source' / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_bytes(data)
            manifest.append({'path': path, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()})
        retained = {'resolved/base-data-manifest.json': json.dumps(manifest).encode(),
            'war3mapMisc.txt': b'[Misc]\nEtherealHealBonus=1.2\n'}
        return retained.__getitem__

    def test_reference_identity_and_map_override_are_retained(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            result = project(root, self.fixture(root))
            self.assertEqual(result['abilities']['ACsw']['code'], 'Aslo')
            self.assertEqual(result['ethereal']['EtherealHealBonus'], '1.2')
            self.assertEqual(len(result['source_sha256']), 2)

    def test_unregistered_reference_changes_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            retained = self.fixture(root)
            file = root / '.wc3-base/source/custom_v1/units/abilitydata.slk'
            file.write_bytes(file.read_bytes().replace(b'Aslo', b'OTHER'))
            with self.assertRaisesRegex(ValueError, 'registered extraction'):
                project(root, retained)


if __name__ == '__main__':
    unittest.main()
