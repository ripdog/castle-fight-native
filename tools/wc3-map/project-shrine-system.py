#!/usr/bin/env python3
"""Project retained Golden Shrine semantics and callback art without shared spell recipes."""
import argparse
import csv
from decimal import Decimal
import hashlib
import json
import pathlib
import re

from lua_index import _decode_w3p_keyed_hex_string

ROOT = pathlib.Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser()
parser.add_argument('--source', type=pathlib.Path, required=True)
parser.add_argument('--check', action='store_true')
args = parser.parse_args()
source = args.source.read_bytes()
text = source.decode()
table = ROOT / 'docs/original_map/extracted/resolved/runtime-system-mechanics.tsv'
row = next(r for r in csv.DictReader(table.open(), delimiter='\t')
           if r['system_id'] == 'golden-shrine-revival')
parameters = json.loads(row['parameters_json'])
def function_body(name):
    return text.split('function ' + name + '(', 1)[1].split('function ', 1)[0]
setup = function_body('ME')
death = function_body('fJ')
assert int(re.search(r'Ctb=(\d+)', setup)[1]) == parameters['chance_percent_per_shrine']
assert int(re.search(r'Btb=(\d+)', setup)[1]) == parameters['maximum_effective_chance_percent']
roll = re.search(r'_Ir\((\d+),(\d+)\)<elvenShrineEffectiveReviveChance', death)
assert [int(roll[1]), int(roll[2])] == [parameters['chance_roll_min'], parameters['chance_roll_max']]
assert float(re.search(r'doAfter\(([\d.]+),f3q\)', death)[1]) == parameters['revive_delay_seconds']
for key in ['exclude_legendary_marker_ability_id', 'exclude_summoned_unit_marker_ability_id']:
    assert f'unit_getAbilityLevel(M2q,{parameters[key]})<=0' in death
assert 'not unit_isType(M2q,UNIT_TYPE_SUMMONED)' in death
assert 'A7=(A7+1)' in function_body('Action_watch_OnUnitDeathHandler_run_watch_OnUnitDeathHandler')
assert 'Signal_Signal_get(ZW)' in function_body('Action_watch_OnUnitDeathHandler_run_watch_OnUnitDeathHandler')
assert 'UNIT_TYPE_SAPPER' in function_body('isCombatSapper')
assert 'not unit_isType(Vcs,UNIT_TYPE_STRUCTURE)' in function_body('isCombatSapper')
assert 'acquireElvenShrine(VDp)' in function_body('onBuildingFinished')
assert 'utb[gDp]=(__wurst_ensureInt(utb[gDp])+Ctb)' in function_body('acquireElvenShrine')
assert 'min(__wurst_ensureInt(utb[SBp]),Btb)' in function_body('elvenShrineEffectiveReviveChance')
assert 'ptb[iDp]=0' in function_body('QE')
assert 'utb[jDp]=max1(0,(__wurst_ensureInt(utb[jDp])-Ctb))' in function_body('QE')
assert 'utb[TDp]=max1(0,(__wurst_ensureInt(utb[TDp])-Ctb))' in function_body('migrateBuildingLifecycleOwner')
assert 'utb[UDp]=(__wurst_ensureInt(utb[UDp])+Ctb)' in function_body('migrateBuildingLifecycleOwner')
assert 'wGb[J2q]=false return true' in function_body('eJ')
assert 'z7[L2q]=true' in function_body('markShrineReviveBlocked')
assert 'widget_getLife(M2q)<.405' in death
assert 'if(not(N2q==nil))then' in death
# Decode the exact protected calls using the retained importer decoder. Illusions are an
# additional fJ exclusion not explicitly recorded in runtime-system-mechanics.tsv.
protected_calls = {}
for key, cipher in re.findall(r'_T\((\d+),"([0-9a-f]+)"\)', death):
    protected_calls[_decode_w3p_keyed_hex_string(int(key), cipher.encode(), 11351, 1106)] = {
        'key': int(key), 'cipher': cipher,
    }
for name in ['GetKillingUnit', '__wurst_safe_IsUnitIllusion', 'consumeDontReviveFlag', 'isShrineReviveBlocked']:
    assert name in protected_calls, f'missing death-handler protected call: {name}'
parameters['exclude_wc3_illusions'] = True
name = 'CallbackSingle_doAfter_OnUnitDeathHandler_call_doAfter_OnUnitDeathHandler'
callback = text.split('function ' + name + '(', 1)[1].split('function ', 1)[0]
assert 'if(E8m.deathIndex==A7)' in callback
assert '__wurst_safe_RemoveUnit(G8m)' in callback
assert 'markShrineReviveBlocked(F8m)' in callback
assert '__wurst_safe_CreateUnit(E8m.revivePlayer,E8m.reviveId,E8m.reviveX,E8m.reviveY,.0)' in callback
model = re.search(r'addEffect1\("([^"]+)",F8m,"origin"\)', callback)[1].replace('\\\\', '\\')
buildings = ROOT / 'docs/original_map/extracted/resolved/buildings.tsv'
building = next(r for r in csv.DictReader(buildings.open(), delimiter='\t') if r['rawcode'] == 'h059')
units = ROOT / 'docs/original_map/extracted/resolved/units.tsv'
unit = next(r for r in csv.DictReader(units.open(), delimiter='\t') if r['rawcode'] == 'h059')
assert unit['hp_regen_type'] == 'always'
assert unit['attack1_enabled'] == unit['attack2_enabled'] == 'False'
regen = Decimal(unit['hp_regen']) * 10_000
assert regen == int(regen)
coverage = ROOT / 'docs/original_map/extracted/resolved/ability-runtime-coverage.tsv'
ability_names = {r['rawcode']: r['name'] for r in csv.DictReader(coverage.open(), delimiter='\t')}
dormant = [int.from_bytes(rawcode.encode(), 'big') for rawcode in building['abilities'].split(',')
           if ability_names[rawcode] == 'Critical Strike']
artifact = {
    'map_version': '9.27', 'retained_revision': 'r1',
    'source': {'script_sha256': hashlib.sha256(source).hexdigest(),
               'mechanics_path': str(table.relative_to(ROOT)),
               'mechanics_sha256': hashlib.sha256(table.read_bytes()).hexdigest(),
               'buildings_sha256': hashlib.sha256(buildings.read_bytes()).hexdigest(),
               'units_sha256': hashlib.sha256(units.read_bytes()).hexdigest(),
               'ability_coverage_sha256': hashlib.sha256(coverage.read_bytes()).hexdigest(),
               'system_id': row['system_id'], 'functions': row['source_functions'] if 'source_functions' in row else row.get('functions', ''),
               'callback': name,
               'additional_audited_functions': ['QE', 'migrateBuildingLifecycleOwner', 'onBuildingFinished', 'isCombatSapper', 'eJ', 'dJ', 'Action_watch_OnUnitDeathHandler_run_watch_OnUnitDeathHandler'],
               'protected_death_calls': {key: protected_calls[key] for key in ['GetKillingUnit', '__wurst_safe_IsUnitIllusion', 'consumeDontReviveFlag', 'isShrineReviveBlocked']}},
    'parameters': parameters,
    'building_health_regen_per_second_per_10k': int(regen),
    'resurrection_model': model,
    'dormant_attack_abilities': dormant,
}
output = ROOT / 'crates/sim/data/castle-fight/9.27/shrine-system-r1.json'
encoded = json.dumps(artifact, indent=2, sort_keys=True) + '\n'
if args.check:
    assert output.read_text() == encoded, 'shrine projection is stale'
else:
    output.write_text(encoded)
