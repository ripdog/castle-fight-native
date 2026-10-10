#!/usr/bin/env python3
"""Dedicated reproducible 9.27 r1 building-mechanic projection (no native recipe edits)."""
import csv
import hashlib
import json
import io
from pathlib import Path

from build_runtime_catalog import _load_release, _retained_file_bytes

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / 'crates/sim/data/castle-fight/9.27/building-mechanics-r1.json'


def build(map_version='9.27', revision='r1'):
    release = _load_release(ROOT / 'docs/original_map/releases.json', map_version, revision)
    if map_version != '9.27':
        raise ValueError(f'building-mechanic recipes do not support {map_version}')
    tree = release['extraction']['git_tree']
    if not tree:
        raise ValueError('building mechanics require a retained extraction tree')
    sources = {}

    def rows(name):
        data = _retained_file_bytes(ROOT, tree, f'resolved/{name}')
        sources[name] = hashlib.sha256(data).hexdigest()
        return list(csv.DictReader(io.StringIO(data.decode()), delimiter='\t'))

    building_rows = rows('building-spell-mechanics.tsv')
    mechanics = next(r for r in building_rows if r['building_rawcode'] == 'h00Z')
    snow = next(r for r in building_rows if r['building_rawcode'] == 'h07W')
    launchers = [r for r in building_rows if r['building_rawcode'] in ('h048', 'h03L')]
    launcher_codes = ['h048', 'h04G', 'A04J', 'A04F', 'h03L', 'h04H', 'A04L', 'A04K']
    mushroom = next(r for r in building_rows if r['building_rawcode'] == 'h047')
    serpent = next(r for r in building_rows if r['building_rawcode'] == 'h02N')
    ceremonial = next(r for r in building_rows if r['building_rawcode'] == 'h02R')
    stasis = next(r for r in building_rows if r['building_rawcode'] == 'h07X')
    healing = next(r for r in building_rows if r['building_rawcode'] == 'h07Y')
    endurance = next(r for r in building_rows if r['building_rawcode'] == 'h07Z')
    enchantment = next(r for r in building_rows if r['building_rawcode'] == 'h060')
    grant_params = json.loads(ceremonial['parameters_json'])
    grant_codes = [value.to_bytes(4, 'big').decode() for value in grant_params['always_ability_ids']
                   + [value for option in grant_params['option_ability_ids'] for value in option]]
    launcher_codes += ['h02R', 'A039', 'A0F1', *grant_codes]
    launcher_codes += ['h02N', 'A02O', 'A02Z', 'A030']
    launcher_codes += ['h07X', 'A032', 'Ast9']
    launcher_codes += ['h07Y', 'A0HT', 'AstB', 'e008', 'h07Z', 'A0HS', 'AstC']
    launcher_codes += ['h047', 'A06P', 'A0AK']
    freezer = next(r for r in building_rows if r['building_rawcode'] == 'h03O')
    launcher_codes += ['h03O', 'h08U', 'A04I', 'A081', 'A080', 'A084', 'A082', 'A083', 'A04H']
    guard = next(r for r in rows('runtime-system-mechanics.tsv') if r['system_id'] == 'elemental-guard-tower-avalanche')
    launcher_codes += ['h061', 'A03Q', 'A0EV', 'A0DC', 'e008']
    shield = next(r for r in rows('runtime-system-mechanics.tsv') if r['system_id'] == 'targeted-negative-effect-shields')
    units = {r['rawcode']: r for r in rows('units.tsv') if r['rawcode'] in ('n00F', 'n00G')}
    protected = [r for r in rows('protected-ability-fields.tsv') if r['rawcode'] in ('A017', 'A018', 'A0HO', 'AM0{', *launcher_codes)]
    overheat_explosions = [r for r in rows('abilities.tsv') if r['rawcode'] == 'A09B']
    fields = rows('object-fields.tsv')
    selected = {code: {r['field_id'] + ':' + r['level']: json.loads(r['recovered_value_json'])
                       for r in fields if r['rawcode'] == code}
                for code in ('h00Z', 'A017', 'A018', 'A09L', 'A070', 'BOhx', 'B01H', 'n00F', 'n00G', 'h07W', 'A0HO', 'AM0{', *launcher_codes)}
    markers = {r['rawcode']: r['abilities'].split(',') for r in rows('units.tsv')
               if any(a in r['abilities'].split(',') for a in ('A070', 'A08H', 'Avul')) or r['rawcode'] == 'h06B'}
    script_bytes = _retained_file_bytes(ROOT, tree, 'script/war3map.lua')
    script = script_bytes.decode()
    sources['script/war3map.lua'] = hashlib.sha256(script_bytes).hexdigest()
    import re
    traces = {}
    for function in ('hexSpell', 'hasShield', 'checkForShield', 'Vd', 'Ed',
                     'randomAliveEnemy', 'isAliveCombatSapper', 'isCombatSapper',
                     'ForGroupCallback_forUnitsInRect_SpellHelpers_callback_forUnitsInRect_SpellHelpers',
                     'orderCodeAttack', 'issueCodeAttack', 'dummyCastTargetWithVision1',
                     'dummyCastTargetFrom2', 'recycleSpellDummy',
                     'CallbackSingle_doAfter_ReengageRuntime_call_doAfter_ReengageRuntime',
                     'CallbackSingle_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_ReengageRuntime',
                     'CallbackSingle_doAfter_doAfter_doAfter_ReengageRuntime_call_doAfter_doAfter_doAfter_ReengageRuntime',
                     *guard['source_functions'].split(','), *snow['source_functions'].split(','), *serpent['source_functions'].split(','), *ceremonial['source_functions'].split(','),
                     *stasis['source_functions'].split(','), *enchantment['source_functions'].split(','), 'dummyCastPointFrom',
                     *healing['source_functions'].split(','), *endurance['source_functions'].split(','), 'dummyCarrierWithAbility', 'createSpellCarrier', 'QL', 'iD', 'hD',
                     'CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities1',
                     'CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities2',
                     'CallbackSingle_doAfter_SpellHelpers_call_doAfter_SpellHelpers4',
                     'CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities',
                     'CallbackSingle_doAfter_StasisTrap_call_doAfter_StasisTrap',
                     'code__onEnter_doAfter_StasisTrap', 'BO', 'isSingleLimitBuilding',
                     'getBuildingTechMaxAllowedForCurrentModes', 'applyRaceTechAvailabilityForPlayer',
                     'setBuildingLineTechAvailabilityForCurrentModes',
                     'CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities3',
                     'CallbackSingle_doAfter_doAfter_RaceOrcAbilities_call_doAfter_doAfter_RaceOrcAbilities',
                     *(f for row in launchers for f in row['source_functions'].split(',')), *mushroom['source_functions'].split(','), *freezer['source_functions'].split(','), 'randomAliveEnemyNear', 'OC', 'applyArtilleryModeEnabled', 'CallbackSingle_doAfter_doAfter_RaceNorthernAbilities_call_doAfter_doAfter_RaceNorthernAbilities', 'CallbackSingle_doAfter_doAfter_doAfter_RaceNorthernAbilities_call_doAfter_doAfter_doAfter_RaceNorthernAbilities', 'ForGroupCallback_forUnitsInRange_RaceNorthernAbilities_callback_forUnitsInRange_RaceNorthernAbilities', 'dummyCastTargetWithVision', 'tileR', 'tile_toVec2', 'vL', 'AI', 'unit_resetSpawnQueue', 'vM', 'ZE', 'EventListener_add_BuildingSpells_onEvent_add_BuildingSpells'):
        start = script.index('function ' + function + '(')
        end = script.index('function ', start + 10)
        traces[function] = {'byte_offset': len(script[:start].encode()), 'source': script[start:end]}
    script_art = {function: [json.loads('"' + literal + '"')
                            for literal in re.findall(r'"(.*?)"', traces[function]['source'])
                            if literal.endswith('.mdl')]
                  for function in ('checkForShield', 'Vd', 'Ed', 'frostLauncherSpell', 'ceremonialTotemSpell')}
    grant_source = traces['ceremonialTotemSpell']['source']
    native_calls = re.findall(r'addProtectedAbility\(\w+,(\d+)\)', grant_source)
    projected_ids = grant_params['always_ability_ids'] + [value for option in grant_params['option_ability_ids'] for value in option]
    if list(map(int, native_calls)) != projected_ids:
        raise ValueError('ceremonial grant identities differ from literal source control flow')
    target_art = re.search(r'addEffect1\("(.*?)",\w+,"(.*?)"\)', grant_source)
    source_art = re.search(r'addEffect\("(.*?)",unit_getPos\(\w+\)\)', grant_source)
    if not target_art or not source_art:
        raise ValueError('ceremonial source/recipient art no longer matches literal calls')
    retry_marker = int(re.findall(r'unit_getAbilityLevel\(\w+,(\d+)\)', traces['isCeremonialTotemTarget']['source'])[-1])
    ceremonial_visuals = [
        {'role': 'target', 'path': json.loads('"' + target_art[1] + '"'), 'attachment': target_art[2]},
        {'role': 'caster', 'path': json.loads('"' + source_art[1] + '"')},
    ]
    enchantment_source = traces['holyShrineSpell']['source']
    enchantment_target_art = re.search(r'addEffect1\("(.*?)",\w+,"(.*?)"\)', enchantment_source)
    enchantment_source_art = re.search(r'addEffect\("(.*?)",unit_getPos\(\w+\)\)', enchantment_source)
    if not enchantment_target_art or not enchantment_source_art:
        raise ValueError('enchantment source/recipient art must retain literal calls')
    enchantment_visuals = [
        {'role': 'target', 'path': json.loads('"' + enchantment_target_art[1] + '"'), 'attachment': enchantment_target_art[2]},
        {'role': 'caster', 'path': json.loads('"' + enchantment_source_art[1] + '"')},
    ]
    limit_source = traces['CallbackSingle_doAfter_StasisTrap_call_doAfter_StasisTrap']['source']
    limits = {int(code).to_bytes(4, 'big').decode(): int(limit)
              for code, limit in re.findall(r'SetPlayerTechMaxAllowed\(\w+\[\w+\],(\d+),(\d+)\)', limit_source)}
    if not limits:
        raise ValueError('native type limit source no longer matches audited callback')
    predicate_ids = set(map(int, re.findall(r'==([0-9]+)', traces['isSingleLimitBuilding']['source'])))
    if predicate_ids != {int.from_bytes(code.encode(), 'big') for code in limits}:
        raise ValueError('type limit initializer and current-mode predicate differ')
    stasis_source = traces[stasis['source_functions']]['source']
    point_call = re.search(r'dummyCastPointFrom\(\w+,(\d+),OrderId\("([^"]+)"\),\w+,\w+,([\d.]+)\)', stasis_source)
    params = json.loads(stasis['parameters_json'])
    if not point_call or int(point_call[1]) != params['effect_ability_id']:
        raise ValueError('native Stasis child identity differs from literal callback')
    if float(point_call[3]) != float(params['carrier_or_dummy_lifetime_seconds']):
        raise ValueError('native Stasis dummy recycle lifetime differs from source')
    hidden_ward = int(re.search(r'\bVR=(\d+)', traces['BO']['source'])[1]).to_bytes(4, 'big').decode()
    if selected['Ast9']['Stau:1'] != hidden_ward:
        raise ValueError('hidden trap listener identity differs from native summon')
    stasis_control = {'order': point_call[2], 'dummy_seconds': point_call[3],
                      'stand_seconds': re.search(r'doAfter\(([\d.]+),', stasis_source)[1],
                      'hidden_ward_rawcode': hidden_ward}
    carrier_rawcode = int(re.search(r'dV=(\d+)', traces['QL']['source'])[1]).to_bytes(4, 'big').decode()
    if carrier_rawcode not in selected:
        raise ValueError('aura carrier identity is missing from field closure')
    aura_controls = {}
    for name, aura in (('healing', healing), ('endurance', endurance)):
        callback = traces[aura['source_functions']]['source']
        params = json.loads(aura['parameters_json'])
        carrier_call = re.search(r'dummyCarrierWithAbility\(unit_getOwner\(\w+\),(\d+),unit_getPos\(\w+\),([\d.]+)\)', callback)
        if not carrier_call or int(carrier_call[1]) != params['effect_ability_id']:
            raise ValueError(f'{name} carrier child differs from literal callback')
        if float(carrier_call[2]) != float(params['carrier_or_dummy_lifetime_seconds']):
            raise ValueError(f'{name} carrier lifetime differs from literal callback')
        aura_controls[name] = {'carrier_rawcode': carrier_rawcode, 'lifetime_seconds': carrier_call[2],
                               'stand_seconds': re.search(r'doAfter\(([\d.]+),', callback)[1]}
    # The normalized summary reverses these names. Retain the actual callback assignment.
    initial = traces['FL']['source']
    callback = traces['ForGroupCallback_forUnitsOfPlayer_nullTimer_SnowveilFountain_callback_forUnitsOfPlayer_nullTimer_SnowveilFountain']['source']
    assignment = re.search(r'if Pcb then \w+=(\w+) else \w+=(\w+) end', callback)
    if not assignment:
        raise ValueError('Snowveil cooldown callback no longer matches the audited branch')
    cooldowns = [re.search(r'\b' + var + r'=(\d+(?:\.\d*)?)', initial).group(1)
                 for var in assignment.groups()]
    terrain_bytes = _retained_file_bytes(ROOT, tree, 'terrain.json')
    sources['terrain.json'] = hashlib.sha256(terrain_bytes).hexdigest()
    terrain = json.loads(terrain_bytes)['map']
    arena = re.search(r'\buIb=.{0,160}?\(\((-?\d+)\.\),\((-?\d+)\.\),(-?\d+)\.,(-?\d+)\.\)', script)
    if not arena:
        raise ValueError('battlefield rectangle initializer no longer matches audited source')
    terrain['battlefield_bounds'] = list(map(int, arena.groups()))
    snow_terrain = next(int(m.group(1)) for m in re.finditer(r'\bzS=(\d+)', script) if int(m.group(1)) != 0)
    terrain['snow_rawcode'] = snow_terrain.to_bytes(4, 'big').decode('ascii')
    from project_building_damage_proc import project as project_guard
    guard_profile = project_guard(guard, selected, protected, traces)
    return {'map_version': map_version, 'schema_version': 2, 'source_revision': revision,
            'extraction_git_tree': tree, 'source_sha256': sources,
            'guard': guard, 'guard_damage_proc': guard_profile,
            'city': mechanics, 'launchers': launchers, 'mushroom': mushroom, 'freezer': freezer,
            'freezer_animation_seconds': re.search(r'doAfter\(([\d.]+),', traces['CallbackSingle_doAfter_RaceNorthernAbilities_call_doAfter_RaceNorthernAbilities1']['source']).group(1),
            'freezer_weather_seconds': re.search(r'doAfter\(([\d.]+),', traces['CallbackSingle_doAfter_doAfter_RaceNorthernAbilities_call_doAfter_doAfter_RaceNorthernAbilities']['source']).group(1),
            'freezer_bounce_step': re.findall(r'getY\(\w+\)\+\((\d+\.)\*Sin', traces['processWorldFreezerMissiles']['source'])[-1], 'snow': snow, 'snow_manual_cooldowns': {'Pcb': cooldowns[0], 'normal': cooldowns[1]},
            'terrain_grid': terrain, 'shield': shield, 'forms': units,
            'launcher_units': {r['rawcode']: r for r in rows('units.tsv') if r['rawcode'] in launcher_codes},
            'target_vision': {'radius': re.search(r'getY\(\w+\),([\d.]+),true,false', traces['dummyCastTargetWithVision1']['source']).group(1) if 'getY' in traces['dummyCastTargetWithVision1']['source'] else re.search(r'\w+\[2\],([\d.]+),true,false', traces['dummyCastTargetWithVision1']['source']).group(1), 'duration': re.search(r'doAfter\(([\d.]+),', traces['dummyCastTargetWithVision1']['source']).group(1)},
            'launcher_vision': {'radius': re.search(r'getY\(\w+\),(\d+\.),true,false', traces['issueFrostLauncherOrder']['source']).group(1),
                                'duration': re.search(r'doAfter\((\d+\.\d+),', traces['issueFrostLauncherOrder']['source']).group(1)}, 'protected': protected,
            'overheat_explosions': overheat_explosions,
            'ceremonial': ceremonial, 'ceremonial_retry_marker': retry_marker, 'ceremonial_cast_visuals': ceremonial_visuals,
            'enchantment': enchantment, 'enchantment_cast_visuals': enchantment_visuals,
            'stasis': stasis, 'stasis_control': stasis_control, 'building_type_limits': limits,
            'healing': healing, 'healing_control': aura_controls['healing'],
            'endurance': endurance, 'endurance_control': aura_controls['endurance'],
            'serpent': serpent,
            'serpent_animation_delays': {
                'attack': re.search(r'doAfter\(([\d.]+),', traces['serpentRockShoot']['source']).group(1),
                'stand': re.search(r'doAfter\(([\d.]+),', traces['CallbackSingle_doAfter_RaceOrcAbilities_call_doAfter_RaceOrcAbilities3']['source']).group(1)},
            'fields': selected, 'target_markers': markers, 'script_traces': traces,
            'script_art': script_art}


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument('--map-version', default='9.27')
    parser.add_argument('--revision', default='r1')
    parser.add_argument('--output', type=Path, default=OUT)
    parser.add_argument('--check', nargs='?', const=OUT, type=Path)
    args = parser.parse_args()
    artifact = build(args.map_version, args.revision)
    if args.check:
        if json.loads(args.check.read_text()) != artifact:
            raise SystemExit(f'building projection is stale: {args.check}')
        print(f'verified {args.check}')
    else:
        args.output.write_text(json.dumps(artifact, indent=2, sort_keys=True) + '\n')
