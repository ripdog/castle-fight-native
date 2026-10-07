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
    shield = next(r for r in rows('runtime-system-mechanics.tsv') if r['system_id'] == 'targeted-negative-effect-shields')
    units = {r['rawcode']: r for r in rows('units.tsv') if r['rawcode'] in ('n00F', 'n00G')}
    protected = [r for r in rows('protected-ability-fields.tsv') if r['rawcode'] in ('A017', 'A018', 'A0HO', 'AM0{')]
    overheat_explosions = [r for r in rows('abilities.tsv') if r['rawcode'] == 'A09B']
    fields = rows('object-fields.tsv')
    selected = {code: {r['field_id'] + ':' + r['level']: json.loads(r['recovered_value_json'])
                       for r in fields if r['rawcode'] == code}
                for code in ('h00Z', 'A017', 'A018', 'A09L', 'A070', 'BOhx', 'B01H', 'n00F', 'n00G', 'h07W', 'A0HO', 'AM0{')}
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
                     *snow['source_functions'].split(','), 'tileR', 'tile_toVec2', 'vL', 'AI'):
        start = script.index('function ' + function + '(')
        end = script.index('function ', start + 10)
        traces[function] = {'byte_offset': len(script[:start].encode()), 'source': script[start:end]}
    script_art = {function: [json.loads('"' + literal + '"')
                            for literal in re.findall(r'"(.*?)"', traces[function]['source'])
                            if literal.endswith('.mdl')]
                  for function in ('checkForShield', 'Vd', 'Ed')}
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
    return {'map_version': map_version, 'schema_version': 2, 'source_revision': revision,
            'extraction_git_tree': tree, 'source_sha256': sources,
            'city': mechanics, 'snow': snow, 'snow_manual_cooldowns': {'Pcb': cooldowns[0], 'normal': cooldowns[1]},
            'terrain_grid': terrain, 'shield': shield, 'forms': units, 'protected': protected,
            'overheat_explosions': overheat_explosions,
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
