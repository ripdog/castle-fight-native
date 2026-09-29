#!/usr/bin/env python3
"""Read-only audit of generated WC3 glTF channels; run from the repository root.

Count channels that can be considered for pruning only when EVERY clip's channel
for the same node/property equals that node's f32 rest value. Unsupported sampler
representations are conservatively excluded. This does not modify any assets.
"""
import base64, collections, json, pathlib, struct
stats = collections.defaultdict(collections.Counter)
for path in pathlib.Path('assets/wc3').rglob('*.gltf'):
    gltf = json.loads(path.read_text())
    if not gltf.get('animations'):
        continue
    buffers = {}
    def data(index):
        if index not in buffers:
            uri = gltf['buffers'][index]['uri']
            buffers[index] = base64.b64decode(uri.split(',', 1)[1]) if uri.startswith('data:') else (path.parent / uri).read_bytes()
        return buffers[index]
    groups = {}
    for animation in gltf['animations']:
        for channel in animation['channels']:
            target = channel['target']
            key = (target['node'], target['path'])
            count, invariant = groups.get(key, (0, True))
            sampler = animation['samplers'][channel['sampler']]
            accessor = gltf['accessors'][sampler['output']]
            defaults = {'translation': [0,0,0], 'rotation': [0,0,0,1], 'scale': [1,1,1]}
            valid = target['path'] in defaults and accessor['componentType'] == 5126 and 'sparse' not in accessor and sampler.get('interpolation','LINEAR') in ('LINEAR','STEP')
            if valid and invariant:
                rest = gltf['nodes'][target['node']].get(target['path'], defaults[target['path']])
                fmt = '<' + 'f' * len(rest)
                rest = struct.unpack(fmt, struct.pack(fmt, *rest))
                view = gltf['bufferViews'][accessor['bufferView']]
                offset = view.get('byteOffset',0) + accessor.get('byteOffset',0)
                stride = view.get('byteStride', struct.calcsize(fmt))
                raw = data(view['buffer'])
                invariant = all(struct.unpack_from(fmt, raw, offset + i*stride) == rest for i in range(accessor['count']))
            else:
                invariant = False
            groups[key] = (count + 1, invariant)
    pack = path.parts[2]
    stats[pack]['models'] += 1
    stats[pack]['target_properties'] += len(groups)
    stats[pack]['channels'] += sum(n for n, _ in groups.values())
    stats[pack]['constant_rest_properties'] += sum(fixed for _, fixed in groups.values())
    stats[pack]['removable_channels'] += sum(n for n, fixed in groups.values() if fixed)
    nodes = {node for node, prop in groups}
    stats[pack]['animated_nodes'] += len(nodes)
    stats[pack]['all_channels_at_rest_nodes'] += sum(all(fixed for (other, prop), (_, fixed) in groups.items() if other == node) for node in nodes)
print(json.dumps(stats, indent=2))
