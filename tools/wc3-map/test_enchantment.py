import copy
import unittest
from pathlib import Path
from unittest.mock import patch

import build_native_tuning as native
import build_runtime_catalog as catalog
from project_enchantment import grant


class EnchantmentProjectionTests(unittest.TestCase):
    def test_spell_book_keeps_both_children_and_rejects_a_hostile_badge(self):
        fields = {
            'BOOK': {'dataa1': 'ARMR,BADG', 'datab1': '1', 'datac1': '2', 'datad1': '2'},
            'ARMR': {'dataa1': '3'},
            'BADG': {'dataa1': '0', 'datab1': '0', 'targs1': 'self'},
        }
        bases = {'BOOK': 'Aspb', 'ARMR': 'AId4', 'BADG': 'Aasl'}
        payload, dependencies = grant('BOOK', fields, bases, {}, native.scaled)
        self.assertEqual(dependencies, ['ARMR', 'BADG'])
        self.assertEqual(payload['effect']['SpellBookArmor']['badge'], int.from_bytes(b'BADG'))
        for key, value in [('datac1', '1'), ('datad1', '1')]:
            changed = copy.deepcopy(fields)
            changed['BOOK'][key] = value
            with self.assertRaises(ValueError):
                grant('BOOK', changed, bases, {}, native.scaled)
        changed = copy.deepcopy(fields)
        changed['BADG']['targs1'] = 'enemies'
        with self.assertRaises(ValueError):
            grant('BOOK', changed, bases, {}, native.scaled)

    def test_self_haste_projects_native_payload_and_rejects_an_area_aura(self):
        fields = {'FAST': {'targs1': 'self', 'area1': '0', 'dataa1': '0.13', 'datab1': '0.27', 'buffid1': 'BUFF'}}
        payload, _ = grant('FAST', fields, {'FAST': 'AOae'}, {}, native.scaled)
        self.assertEqual(payload['effect'], {'Speed': {'attack': 27, 'movement': 13, 'buff_rawcode': int.from_bytes(b'BUFF')}})
        for key, value in [('targs1', 'friend'), ('area1', '10')]:
            changed = copy.deepcopy(fields)
            changed['FAST'][key] = value
            with self.assertRaises(ValueError):
                grant('FAST', changed, {'FAST': 'AOae'}, {}, native.scaled)

    def test_retained_enchantment_rejects_changed_index_and_dummy_damage_semantics(self):
        root = Path(__file__).resolve().parents[2]
        release = catalog._load_release(catalog.DEFAULT_RELEASES, '9.27', 'r1')
        recipes = {'schema_version': 1, 'effects': [
            {'kind': 'building-enchantment', 'source_kind': 'unit-ability', 'source_key': 'A0DV', 'unit_rawcode': 'h060'}
        ]}
        original = catalog._retained_file_bytes
        native.build_tuning(release, root, recipes)
        for before, after in [
            ('if(Spr==2)then', 'if(Spr==1)then'),
            ('ATTACK_TYPE_CHAOS,DAMAGE_TYPE_UNKNOWN', 'ATTACK_TYPE_MAGIC,DAMAGE_TYPE_UNKNOWN'),
            ('W9l.dummy,X9l,W9l.damage', 'W9l.source,X9l,W9l.damage'),
            ('Upq=unit_getX(Rpq)', 'Upq=unit_getX(Qpq)'),
        ]:
            def changed(repo, tree, path):
                data = original(repo, tree, path)
                return data.replace(before.encode(), after.encode()) if path == 'script/war3map.lua' else data
            with self.subTest(before=before), patch.object(catalog, '_retained_file_bytes', changed), self.assertRaises(ValueError):
                native.build_tuning(release, root, recipes)
