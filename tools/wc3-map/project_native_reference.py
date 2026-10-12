"""Resolve unchanged native abilities/rules from hash-verified extraction base inputs."""
import hashlib
import importlib.util
import json
import sys
from pathlib import Path


def project(root, retained):
    manifest = {entry['path']: entry for entry in json.loads(retained('resolved/base-data-manifest.json'))}
    sources = {}

    def verified(path):
        file = root / '.wc3-base/source' / path
        data = file.read_bytes()
        digest = hashlib.sha256(data).hexdigest()
        if digest != manifest[path]['sha256'] or len(data) != manifest[path]['bytes']:
            raise ValueError(f'native reference {path} differs from registered extraction inputs')
        sources[path] = digest
        return file

    spec = importlib.util.spec_from_file_location('resolve_base_data', Path(__file__).with_name('resolve-base-data.py'))
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    data = module.parse_slk(verified('custom_v1/units/abilitydata.slk'))
    fields = {key.lower(): value for key, value in data['ACsw'].items()}
    if fields['code'] != 'Aslo':
        raise ValueError('native creep Slow base changed')
    misc = {}
    for line in verified('custom_v1/units/miscgame.txt').read_text().splitlines():
        if '=' in line and not line.lstrip().startswith('//'):
            key, value = line.split('=', 1)
            misc[key] = value
    # Map overrides take precedence over the registered native dataset.
    for line in retained('war3mapMisc.txt').decode().splitlines():
        if '=' in line and not line.lstrip().startswith('//'):
            key, value = line.split('=', 1)
            misc[key] = value
    return {'abilities': {'ACsw': fields}, 'ethereal': {key: misc[key] for key in
        ['EtherealDamageBonus', 'EtherealDamageBonusAlly', 'EtherealHealBonus']},
        'source_sha256': sources, 'manifest_source': 'resolved/base-data-manifest.json'}
