"""Validate the baked GPU buffers and usable rigged GLB animations (stdlib only)."""
import json
import math
from pathlib import Path
import struct

root = Path(__file__).resolve().parents[2] / 'assets' / 'cartoon'
manifest = json.loads((root / 'manifest.json').read_text())
for name, stats in manifest['meshes'].items():
    data = (root / 'meshes' / (name + '.fnmesh')).read_bytes()
    magic, nv, ni = struct.unpack_from('<4sII', data)
    assert magic == b'FNM1' and 0 < nv < 5000 and ni % 3 == 0, name
    assert len(data) == 12 + nv * 32 + ni * 4 == stats['bytes'], name
    assert nv == stats['vertices'] and ni == stats['triangles'] * 3, name
    for i in range(nv):
        vertex = struct.unpack_from('<6f8B', data, 12 + i * 32)
        assert all(math.isfinite(v) for v in vertex[:6]), name
        assert abs(sum(v*v for v in vertex[3:6]) - 1) < 0.002, name
    assert max(struct.unpack_from('<' + 'I' * ni, data, 12 + nv * 32)) < nv, name

for name in manifest['characters'] + ['items', 'foliage']:
    data = (root / (name + '.glb')).read_bytes()
    assert struct.unpack_from('<4sII', data) == (b'glTF', 2, len(data)), name
    size, kind = struct.unpack_from('<II', data, 12)
    assert kind == 0x4e4f534a, name
    gltf = json.loads(data[20:20 + size])
    assert gltf['asset']['version'] == '2.0' and gltf['meshes'], name
    if name in manifest['characters']:
        assert len(gltf['skins']) == 1 and len(gltf['skins'][0]['joints']) == 16, name
        assert {a['name'] for a in gltf['animations']} == set(manifest['animations']), name
        assert all('skin' in n for n in gltf['nodes'] if 'mesh' in n), name
        for animation in gltf['animations']:
            assert len(animation['channels']) >= 16, (name, animation['name'])
            times = [gltf['accessors'][s['input']] for s in animation['samplers']]
            # glTF optimizes stationary bones to a single sample; each clip must
            # still contain an animated channel with the full timeline.
            assert all(a['count'] >= 1 for a in times), name
            assert any(a['max'][0] - a['min'][0] >= 0.6 and a['count'] >= 12 for a in times), (name, animation['name'])
    print(name + '.glb:', len(gltf['meshes']), 'meshes,', len(gltf.get('animations', [])), 'clips')
print('Validated', len(manifest['meshes']), 'baked meshes and all GLB exports.')
