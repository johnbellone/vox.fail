#!/usr/bin/env python3
"""Bare heads for characters whose cap or hat is part of the head, so a dress-up hat replaces it
instead of sitting on top of it.

For each character listed in HEADWEAR this rebuilds the Head mesh's voxels (5 cm), removes the
headwear (its colours, from the band up, plus anything sticking out past the skull such as a
brim), fills the skull up to a plain top with the character's hair (or peanut shell), and adds
the result to the model as a `Head_bare` node under `Head`. The app hides it until a hat goes on,
then swaps it in (src/wardrobe.rs). Hat heights go to data/bare_heads.json.

    python3 tools/assets/build_bare_heads.py           # updates assets/models/*.glb and data/bare_heads.json
    python3 tools/assets/build_bare_heads.py --preview out.png   # front/side views, before and after

Helmeted characters (riot_a, riot_b, roasted_riot) keep their helmets: under those there is no
head to show.
"""
import io
import json
import struct
import sys
from pathlib import Path

import numpy as np
from PIL import Image

sys.path.insert(0, str(Path(__file__).parent))
import build_animals as B  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
V = 0.05
SHELL = ["cc9654", "a06c36", "cc9654", "805026", "cc9654"]
PEANUT = ["deba80", "be9660", "deba80", "c8a068"]
# model: (headwear colours, first row of the band, fill colours)
HEADWEAR = {
    "scene_all": (["ce282c", "f8f8f6", "961a20"], 9, ["704626"]),
    "executive": (["121216", "1a1a1e", "96969c"], 9, ["261c18"]),
    "officer": (["1e2848", "e8b923", "1a1a1e"], 9, ["1a1a1e"]),
    "rally_hat": (["beaa78", "96845a"], 8, ["e8e8e4"]),
    "rally_sign": (["283878", "f8f8f6"], 9, ["e2be6e"]),
    "roasted_army": (["566234", "3e4826"], 9, SHELL),
    "roasted_cook": (["f8f8f4", "c82c2c"], 10, SHELL),
    "roasted_garbage": (["e86e1e"], 10, SHELL),
    "roasted_medic": (["f4f4f0", "289646"], 9, SHELL),
    "peanut": (["ec8024", "c46018"], 10, PEANUT),
}
TOP_ROW = 10  # bare heads end at 0.55 m above the Head joint, like most bareheaded characters
DIRS = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]


def hexc(c):
    return "%02x%02x%02x" % tuple(c)


def head_voxels(j, b):
    """Head mesh -> {(x,y,z): colour hex} for every visible voxel, and colour -> palette uv."""
    img = j["images"][0]
    v = j["bufferViews"][img["bufferView"]]
    o = v.get("byteOffset", 0)
    pal = np.array(Image.open(io.BytesIO(b[o:o + v["byteLength"]])).convert("RGB"))
    ph, pw = pal.shape[:2]
    head = next(n for n in j["nodes"] if n.get("name") == "Head")
    vox, uvs = {}, {}
    for pr in j["meshes"][head["mesh"]]["primitives"]:
        if pr.get("material", 0) != 0:
            continue
        pos = B.read_acc(j, b, pr["attributes"]["POSITION"])
        nrm = B.read_acc(j, b, pr["attributes"]["NORMAL"])
        uv = B.read_acc(j, b, pr["attributes"]["TEXCOORD_0"])
        idx = B.read_acc(j, b, pr["indices"]).ravel()
        for t in idx.reshape(-1, 6):
            u = sorted(set(t.tolist()))
            p, n, c = pos[u], nrm[u[0]], uv[u[0]]
            col = hexc(pal[min(ph - 1, int(c[1] * ph)), min(pw - 1, int(c[0] * pw))])
            uvs.setdefault(col, (float(c[0]), float(c[1])))
            lo = np.round(p.min(0) / V).astype(int)
            hi = np.round(p.max(0) / V).astype(int)
            ax = int(np.argmax(np.abs(n)))
            rng = [range(lo[k], hi[k]) if k != ax else [lo[k] - (1 if n[ax] > 0 else 0)] for k in range(3)]
            for x in rng[0]:
                for y in rng[1]:
                    for z in rng[2]:
                        vox[(x, y, z)] = col
    return vox, uvs


def solid_of(vox):
    """Every cell inside the head: the visible voxels plus the hidden ones they enclose."""
    ks = np.array(list(vox))
    lo, hi = ks.min(0) - 1, ks.max(0) + 1
    outside = set()
    stack = [tuple(lo)]
    while stack:
        c = stack.pop()
        if c in outside or c in vox or any(c[k] < lo[k] or c[k] > hi[k] for k in range(3)):
            continue
        outside.add(c)
        stack.extend((c[0] + d[0], c[1] + d[1], c[2] + d[2]) for d in DIRS)
    solid = {}
    for x in range(lo[0], hi[0] + 1):
        for y in range(lo[1], hi[1] + 1):
            for z in range(lo[2], hi[2] + 1):
                c = (x, y, z)
                if c not in outside:
                    solid[c] = vox.get(c)  # None: hidden inside
    return solid


def bare(vox, hw, band, fill):
    solid = solid_of(vox)
    is_hw = lambda c, col: col is not None and col in hw and c[1] >= band - 3
    # the skull's footprint: the row under the band, without headwear
    foot = {(x, z) for (x, y, z), col in solid.items() if y == band - 1 and not is_hw((x, y, z), col)}
    out = {}
    for c, col in solid.items():
        x, y, z = c
        if y >= band and ((x, z) not in foot or y > TOP_ROW):
            continue  # brims and crowns
        if is_hw(c, col):
            continue
        if col is not None:
            out[c] = col
    # fill the skull up to the plain top (rounded by one cell on shells)
    shell = fill in (SHELL, PEANUT)
    for y in range(band - 3, TOP_ROW + 1):
        fp = foot
        if shell and y == TOP_ROW:
            fp = {(x, z) for (x, z) in foot if all((x + dx, z + dz) in foot for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)))}
        for x, z in fp:
            c = (x, y, z)
            if c in out:
                continue
            if y < band and not (c in solid and (solid[c] is None or is_hw(c, solid[c]))):
                continue  # below the band only replace what was headwear or hidden
            out[c] = fill[(x * 7 + y * 13 + z * 5) % len(fill)]
    # hidden cells below the band that are now exposed take the fill too; drop the rest
    return {c: col for c, col in out.items()}


def mesh(cells, uvs):
    pos, nrm, uv, idx = [], [], [], []
    for (x, y, z), col in cells.items():
        for d in DIRS:
            if (x + d[0], y + d[1], z + d[2]) in cells:
                continue
            ax = [k for k in range(3) if d[k]][0]
            s = d[ax]
            u_ax, v_ax = [k for k in range(3) if k != ax]
            base = [x, y, z]
            base[ax] += 1 if s > 0 else 0
            corners = []
            for du, dv in ((0, 0), (1, 0), (1, 1), (0, 1)):
                p = list(base)
                p[u_ax] += du
                p[v_ax] += dv
                corners.append([q * V for q in p])
            # counter-clockwise seen from outside
            e1 = np.subtract(corners[1], corners[0])
            e2 = np.subtract(corners[2], corners[0])
            if np.dot(np.cross(e1, e2), d) < 0:
                corners = corners[::-1]
            k = len(pos)
            pos += corners
            nrm += [list(d)] * 4
            uv += [uvs[col]] * 4
            idx += [k, k + 1, k + 2, k, k + 2, k + 3]
    return np.array(pos, np.float32), np.array(nrm, np.float32), np.array(uv, np.float32), np.array(idx, np.uint32)


def add_to_glb(path, cells, uvs):
    j, b = B.load_glb(path)
    b = bytearray(b)
    pos, nrm, uv, idx = mesh(cells, uvs)

    def put(arr, typ, target, minmax=False):
        while len(b) % 4:
            b.append(0)
        off = len(b)
        b.extend(arr.tobytes())
        j["bufferViews"].append({"buffer": 0, "byteOffset": off, "byteLength": arr.nbytes, "target": target})
        acc = {"bufferView": len(j["bufferViews"]) - 1, "componentType": 5126 if arr.dtype == np.float32 else 5125, "count": len(arr), "type": typ}
        if minmax:
            acc["min"] = arr.min(0).tolist()
            acc["max"] = arr.max(0).tolist()
        j["accessors"].append(acc)
        return len(j["accessors"]) - 1

    prim = {"attributes": {"POSITION": put(pos, "VEC3", 34962, True), "NORMAL": put(nrm, "VEC3", 34962), "TEXCOORD_0": put(uv, "VEC2", 34962)}, "indices": put(idx, "SCALAR", 34963), "material": 0}
    names = [n.get("name") for n in j["nodes"]]
    j["meshes"].append({"name": "Head_bare_mesh", "primitives": [prim]})
    j["nodes"].append({"name": "Head_bare", "mesh": len(j["meshes"]) - 1})
    head = j["nodes"][names.index("Head")]
    head.setdefault("children", []).append(len(j["nodes"]) - 1)
    j["buffers"][0]["byteLength"] = len(b)
    js = json.dumps(j, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    while len(b) % 4:
        b.append(0)
    out = b"glTF" + struct.pack("<II", 2, 28 + len(js) + len(b))
    out += struct.pack("<I", len(js)) + b"JSON" + js + struct.pack("<I", len(b)) + b"BIN\0" + bytes(b)
    path.write_bytes(out)
    return len(idx) // 6


def preview(rows, out):
    def view(vox):
        ks = np.array(list(vox))
        lo, hi = ks.min(0), ks.max(0)
        w, h, d = hi - lo + 1
        im = Image.new("RGB", (w + d + 2, h), "white")
        px = im.load()
        front, side = {}, {}
        for (x, y, z), c in vox.items():
            if (x, y) not in front or z > front[(x, y)][0]:
                front[(x, y)] = (z, c)
            if (z, y) not in side or x > side[(z, y)][0]:
                side[(z, y)] = (x, c)
        for (x, y), (_, c) in front.items():
            px[x - lo[0], hi[1] - y] = tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
        for (z, y), (_, c) in side.items():
            px[w + 2 + z - lo[2], hi[1] - y] = tuple(int(c[i:i + 2], 16) for i in (0, 2, 4))
        return im.resize((im.width * 5, im.height * 5), Image.NEAREST)

    ims = [(view(a), view(b)) for a, b in rows]
    W = max(a.width + b.width for a, b in ims) + 30
    H = max(max(a.height, b.height) for a, b in ims) + 10
    sheet = Image.new("RGB", (W * 2, H * ((len(ims) + 1) // 2)), "white")
    for i, (a, b) in enumerate(ims):
        x, y = (i % 2) * W, (i // 2) * H
        sheet.paste(a, (x, y))
        sheet.paste(b, (x + a.width + 10, y))
    sheet.save(out)


def main():
    prev = sys.argv[sys.argv.index("--preview") + 1] if "--preview" in sys.argv else None
    tops, rows = {}, []
    for name, (hw, band, fill) in HEADWEAR.items():
        path = ROOT / "assets/models" / f"{name}.glb"
        j, b = B.load_glb(path)
        if any(n.get("name") == "Head_bare" for n in j["nodes"]) and not prev:
            print(f"{name}: already has a bare head (rebuild the model first to redo it)")
            tops[f"{name}.glb"] = round((TOP_ROW + 1) * V, 3)
            continue
        vox, uvs = head_voxels(j, b)
        cells = bare(vox, set(hw), band, fill)
        missing = {c for c in cells.values() if c not in uvs}
        if missing:
            raise SystemExit(f"{name}: no palette entry for {missing}")
        rows.append((vox, cells))
        if prev:
            continue
        quads = add_to_glb(path, cells, uvs)
        tops[f"{name}.glb"] = round((TOP_ROW + 1) * V, 3)
        print(f"{name}: bare head, {quads} faces")
    if prev:
        preview(rows, prev)
        print("wrote", prev)
    else:
        (ROOT / "data/bare_heads.json").write_text(json.dumps(tops, indent=1) + "\n")


if __name__ == "__main__":
    main()
