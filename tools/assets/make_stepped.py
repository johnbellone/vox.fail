"""Make a 'stepped' twin of a rigged .glb: every body clip snaps from sprite frame to sprite frame
(the 2D look) instead of interpolating. Usage: python make_stepped.py in.glb out.glb fps.json
fps.json maps clip name -> sprite fps (manifest.json clips/own_clips work; see build script)."""
import json, struct, sys, numpy as np
from repack import load, acc_arr
def stepped(src, dst, fps_by_name):
    J, B = load(src); B = bytearray(B)
    def add(arr, typ, mm=False):
        while len(B) % 4: B.append(0)
        off = len(B); B.extend(arr.astype(np.float32).tobytes())
        J['bufferViews'].append({'buffer': 0, 'byteOffset': off, 'byteLength': arr.nbytes})
        a = {'bufferView': len(J['bufferViews']) - 1, 'componentType': 5126, 'count': int(arr.shape[0]), 'type': typ}
        if mm: a['min'] = [float(arr.min())]; a['max'] = [float(arr.max())]
        J['accessors'].append(a); return len(J['accessors']) - 1
    for an in J.get('animations', []):
        fps = fps_by_name.get(an['name'])
        if not fps: continue                                  # face clips are already stepped
        cache = {}
        for s in an['samplers']:
            if s['interpolation'] != 'LINEAR': continue
            ts = acc_arr(J, B, s['input'])[:, 0]; vs = acc_arr(J, B, s['output'])
            keep = [i for i, t in enumerate(ts) if abs(t * fps - round(t * fps)) < 1e-3]
            if s['input'] not in cache: cache[s['input']] = add(ts[keep].reshape(-1), 'SCALAR', True)
            s['input'] = cache[s['input']]
            s['output'] = add(vs[keep], {1: 'SCALAR', 2: 'VEC2', 3: 'VEC3', 4: 'VEC4'}[vs.shape[1]]); s['interpolation'] = 'STEP'
    while len(B) % 4: B.append(0)
    J['buffers'] = [{'byteLength': len(B)}]
    js = json.dumps(J, separators=(',', ':')).encode()
    while len(js) % 4: js += b' '
    open(dst + '.tmp', 'wb').write(struct.pack('<4sII', b'glTF', 2, 28 + len(js) + len(B)) + struct.pack('<I4s', len(js), b'JSON') + js + struct.pack('<I4s', len(B), b'BIN\x00') + bytes(B))
    from repack import repack as rp
    import os
    # repack drops the now-unused linear key data; keep every channel (Bevy needs them)
    J2, B2 = load(dst + '.tmp'); _orig = rp.__globals__['np'].allclose
    rp.__globals__['np'].allclose = lambda *a, **k: False
    try: rp(dst + '.tmp', dst)
    finally: rp.__globals__['np'].allclose = _orig; os.remove(dst + '.tmp')
if __name__ == '__main__':
    src, dst, fj = sys.argv[1:4]; m = json.load(open(fj))
    fps = {c['name']: c['fps'] for c in m.get('clips', [])}
    for ch in m.get('characters', []):
        for o in ch.get('own_clips', []): fps[o['name']] = o['fps']
    stepped(src, dst, fps)
