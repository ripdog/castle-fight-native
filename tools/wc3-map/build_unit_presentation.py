#!/usr/bin/env python3
"""Project native Sphere attachments and entry-script vertex colors from retained evidence."""
import argparse
import csv
import io
import json
import re
import build_runtime_catalog as catalog


def entry_type_colors(script):
    """Only the native entry handler's exact unit-type branches can override object tint."""
    body = re.search(r'function onUnitEnteredMap\(\)(.*?)function ', script, re.S)
    if body is None:
        raise ValueError('missing native unit entry handler')
    body = body[1]
    actor = re.search(r'(\w+)=GetTriggerUnit\(\)', body)
    unit_type = re.search(r'(\w+)=unit_getTypeId\((\w+)\)', body)
    if not actor or not unit_type or unit_type[2] != actor[1]:
        raise ValueError('entry color requires the trigger unit type identity')
    colors = {}
    for branch in re.finditer(r'(?:if|elseif)\((\w+)==(\d+)\)then __wurst_safe_SetUnitVertexColor\((\w+),(\d+),(\d+),(\d+),(\d+)\)', body):
        predicate, rawcode, target, *rgba = branch.groups()
        if predicate != unit_type[1] or target != actor[1] or any(not 0 <= int(c) <= 255 for c in rgba):
            raise ValueError('entry color branch must mutate its own trigger unit')
        colors[int(rawcode).to_bytes(4, 'big').decode()] = list(map(int, rgba))
    return colors


def build(release):
    tree = release['extraction']['git_tree']
    retained = lambda path: catalog._retained_file_bytes(catalog.REPO_ROOT, tree, path)
    script = retained('script/war3map.lua').decode()
    # Audit the exact predicate, native operation and formula; reject changed control flow.
    match = re.search(r'elseif\(unit_getAbilityLevel\((\w+),(\d+)\)>0\)then __wurst_safe_SetUnitVertexColor\(\1,(\d+),(\d+),(\d+),\((\d+)-\((\d+)\*unit_getLevel\(\1\)\)\)\)end orderCodeAttackIfAllowed\(\1\)', script)
    if match is None:
        raise ValueError('entry vertex color needs its audited native ability predicate and level formula')
    _, ability, red, green, blue, maximum, per_level = match.groups()
    marker = int(ability).to_bytes(4, 'big').decode()
    type_colors = entry_type_colors(script)
    units, abilities = {}, {}
    for row in csv.DictReader(io.StringIO(retained('resolved/object-fields.tsv').decode()), delimiter='\t'):
        if row['level'] not in ('0', '1'):
            continue
        group = units if row['category'] == 'units' else abilities if row['category'] == 'abilities' else None
        if group is not None:
            fields = group.setdefault(row['rawcode'], {'base': row['base_rawcode']})
            fields[row['field_id']] = json.loads(row['recovered_value_json'])
    entries = []
    for rawcode, fields in sorted(units.items()):
        inventory = fields.get('uabi', '').split(',')
        spheres = []
        for source in inventory:
            native = abilities.get(source, {})
            if native.get('base') not in {'Asph', 'Asp1', 'Asp2', 'Asp3'} or not native.get('atat'):
                continue
            count = int(native['atac'])
            points = [native.get(f'ata{index}', '') for index in range(count)]
            if count == 0:
                continue
            if not 1 <= count <= 6 or not all(points):
                raise ValueError('Sphere must retain every authored native attachment point')
            spheres.append({'ability_rawcode': source, 'attachment_points': points})
        entry = {'unit_rawcode': rawcode, 'spheres': spheres}
        if rawcode in type_colors and 'sapper' in fields.get('utyp', '').split(','):
            entry['vertex_rgba'] = type_colors[rawcode]
        elif marker in inventory:
            alpha = int(maximum) - int(per_level) * int(fields['ulev'])
            entry['vertex_rgba'] = [int(red), int(green), int(blue), max(0, min(255, alpha))]
        if spheres or 'vertex_rgba' in entry:
            entries.append(entry)
    return {'schema_version': 1, 'map_version': release['map_version'],
        'source': {'git_tree': tree, 'path': 'resolved/object-fields.tsv', 'script': 'script/war3map.lua',
            'vertex_handler': 'onUnitEnteredMap', 'vertex_predicate_ability': marker,
            'vertex_type_overrides': sorted(type_colors)}, 'units': entries}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--map-version', required=True)
    parser.add_argument('--revision', required=True)
    parser.add_argument('--check')
    args = parser.parse_args()
    result = build(catalog._load_release(catalog.DEFAULT_RELEASES, args.map_version, args.revision))
    if args.check:
        from pathlib import Path
        if result != json.loads(Path(args.check).read_text()):
            raise SystemExit('native unit presentation projection is stale')
    else:
        print(json.dumps(result, indent=2))

if __name__ == '__main__':
    main()
