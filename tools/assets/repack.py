"""Viewer-only slimming: drop animation channels that hold a node at its default value for the whole clip,
then repack the binary so orphaned data is removed. (three.js restores defaults when a track is absent.)"""
import json, struct, sys, numpy as np
def load(p):
    b = open(p, 'rb').read(); jl = struct.unpack('<I', b[12:16])[0]; J = json.loads(b[20:20 + jl]); B = b[20 + jl + 8:]; return J, B
def acc_arr(J, B, i):
    a = J['accessors'][i]; v = J['bufferViews'][a['bufferView']]; n = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4}[a['type']]
    dt = np.float32 if a['componentType'] == 5126 else np.uint32
    return np.frombuffer(B[v['byteOffset']:v['byteOffset'] + v['byteLength']], dt).reshape(-1, n)
def repack(src, dst):
    J, B = load(src); dropped = 0
    for an in J.get('animations', []):
        keep_ch = []
        for ch in an['channels']:
            s = an['samplers'][ch['sampler']]; out = acc_arr(J, B, s['output']); node = J['nodes'][ch['target']['node']]; path = ch['target']['path']
            dflt = np.array(node.get(path, {'scale': [1, 1, 1], 'rotation': [0, 0, 0, 1], 'translation': [0, 0, 0]}[path]), np.float32)
            if np.allclose(out, dflt[None, :], atol=1e-6): dropped += 1; continue
            keep_ch.append(ch)
        used = sorted({c['sampler'] for c in keep_ch}); remap = {o: n for n, o in enumerate(used)}
        an['samplers'] = [an['samplers'][o] for o in used]
        for c in keep_ch: c['sampler'] = remap[c['sampler']]
        an['channels'] = keep_ch
    J['animations'] = [a for a in J.get('animations', []) if a['channels']]
    # collect referenced accessors / bufferViews
    used_acc = set()
    for m in J['meshes']:
        for p in m['primitives']:
            used_acc |= set(p['attributes'].values()); used_acc.add(p['indices'])
    for a in J['animations']:
        for s in a['samplers']: used_acc |= {s['input'], s['output']}
    acc_order = sorted(used_acc); amap = {o: n for n, o in enumerate(acc_order)}
    nb = bytearray(); views = []; vmap = {}
    def addview(vi):
        if vi in vmap: return vmap[vi]
        v = J['bufferViews'][vi]
        while len(nb) % 4: nb.append(0)
        nv = dict(v); nv['byteOffset'] = len(nb); nb.extend(B[v['byteOffset']:v['byteOffset'] + v['byteLength']]); views.append(nv); vmap[vi] = len(views) - 1; return vmap[vi]
    accs = []
    for o in acc_order:
        a = dict(J['accessors'][o]); a['bufferView'] = addview(a['bufferView']); accs.append(a)
    for im in J.get('images', []): im['bufferView'] = addview(im['bufferView'])
    J['accessors'] = accs; J['bufferViews'] = views
    for m in J['meshes']:
        for p in m['primitives']:
            p['attributes'] = {k: amap[v] for k, v in p['attributes'].items()}; p['indices'] = amap[p['indices']]
    for a in J['animations']:
        for s in a['samplers']: s['input'] = amap[s['input']]; s['output'] = amap[s['output']]
    while len(nb) % 4: nb.append(0)
    J['buffers'] = [{'byteLength': len(nb)}]
    js = json.dumps(J, separators=(',', ':')).encode()
    while len(js) % 4: js += b' '
    open(dst, 'wb').write(struct.pack('<4sII', b'glTF', 2, 28 + len(js) + len(nb)) + struct.pack('<I4s', len(js), b'JSON') + js + struct.pack('<I4s', len(nb), b'BIN\x00') + bytes(nb))
    return dropped
if __name__ == '__main__':
    import glob, os
    os.makedirs('cast/web', exist_ok=True); tot = 0
    for f in sorted(glob.glob('cast/slim/*.glb')):
        d = repack(f, 'cast/web/' + os.path.basename(f)); tot += os.path.getsize('cast/web/' + os.path.basename(f))
    print(tot)
