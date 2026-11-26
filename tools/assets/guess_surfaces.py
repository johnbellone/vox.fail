"""Guesses which palette colours of each character are hair, beard and cloth, for the pixel-art
detail (src/bevel.rs), and adds them to data/surfaces.json. Entries already in the file win, so
hand-made lists (the lead's) and corrections are kept.

    python3 tools/assets/guess_surfaces.py           # update data/surfaces.json
    python3 tools/assets/guess_surfaces.py --dry     # just print the guesses

The rules, all on the model's rest pose:
- skin: the commonest colour on the front of the face, between eyes and chin
- hair: colours on the back of the head, in its upper half, that aren't skin
- beard: colours on the lower front of the face, darker than the skin and not skin
- cloth: the biggest colour on the torso and on the upper legs, when it isn't skin
- animals (cat, dog, donkey, rhino): the back of the head is fur ("hair"), the torso cloth
- helmeted and peanut characters are skipped
"""
import io
import json
import pathlib
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import build_animals as B  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "surfaces.json"
ANIMALS = {"cat", "dog", "donkey", "rhino"}
# helmets and visors would read as hair; the peanuts' shells aren't hair or cloth
SKIP = {"peanut", "rally_sign", "officer", "riot_a", "riot_b", "roasted_army", "roasted_cook", "roasted_garbage", "roasted_medic", "roasted_miami", "roasted_riot"}


def faces(path):
    """(mesh name, colour hex, centre xyz, normal xyz, area) per quad."""
    j, b = B.load_glb(path)
    img = j["images"][0]
    v = j["bufferViews"][img["bufferView"]]
    o = v.get("byteOffset", 0)
    pal = np.array(Image.open(io.BytesIO(b[o:o + v["byteLength"]])).convert("RGB"))
    ph, pw = pal.shape[:2]
    out = []
    for m in j["meshes"]:
        for pr in m["primitives"]:
            if pr.get("material", 0) != 0:
                continue
            pos = B.read_acc(j, b, pr["attributes"]["POSITION"])
            nrm = B.read_acc(j, b, pr["attributes"]["NORMAL"])
            uv = B.read_acc(j, b, pr["attributes"]["TEXCOORD_0"])
            idx = B.read_acc(j, b, pr["indices"])
            if len(idx) % 6:
                continue
            for t in idx.reshape(-1, 6):
                u = sorted(set(t.tolist()))
                c = uv[u[0]]
                col = "%02x%02x%02x" % tuple(pal[min(ph - 1, int(c[1] * ph)), min(pw - 1, int(c[0] * pw))])
                p = pos[u]
                ext = sorted(p.max(0) - p.min(0))
                out.append((m.get("name", ""), col, p.mean(0), nrm[u[0]], ext[1] * ext[2]))
    return out


def lum(hexc):
    r, g, b = (int(hexc[i:i + 2], 16) for i in (0, 2, 4))
    return 0.299 * r + 0.587 * g + 0.114 * b


def guess(path):
    fs = faces(path)
    name = path.stem
    head = [f for f in fs if f[0] == "Head_mesh"]
    if not head:
        return {}
    ys = np.array([f[2][1] for f in head])
    y0, y1 = ys.min(), ys.max()
    mid = (y0 + y1) / 2

    def biggest(sel):
        acc = {}
        for f in sel:
            acc[f[1]] = acc.get(f[1], 0) + f[4]
        return sorted(acc.items(), key=lambda kv: -kv[1])

    out = {}
    if name in ANIMALS:
        # the fur: the biggest colour on the back of the head (their clothes are cloth)
        back = [f for f in head if f[3][2] < -0.9]
        if back:
            out[biggest(back)[0][0]] = "hair"
        torso = [f for f in fs if f[0] == "Hips_mesh"]
        big = biggest(torso)
        if big and big[0][0] not in out:
            out[big[0][0]] = "cloth"
        return out
    front = [f for f in head if f[3][2] > 0.9]
    face = [f for f in front if y0 + (y1 - y0) * 0.15 < f[2][1] < mid + (y1 - y0) * 0.1]
    if not face:
        return {}
    skin = biggest(face)[0][0]
    back = [f for f in head if f[3][2] < -0.9 and f[2][1] > mid]
    for c, a in biggest(back)[:2]:
        if c != skin and a > 0.004:
            out[c] = "hair"
    low = [f for f in front if f[2][1] < y0 + (y1 - y0) * 0.35]
    for c, a in biggest(low)[:2]:
        if c != skin and a > 0.008 and lum(c) < lum(skin) - 25 and c not in out:
            out[c] = "stubble"
    torso = [f for f in fs if f[0] == "Hips_mesh"]
    legs = [f for f in fs if f[0].startswith("UpperLeg")]
    for sel in (torso, legs):
        big = biggest(sel)
        if big and big[0][0] != skin and big[0][0] not in out:
            out[big[0][0]] = "cloth"
    return out


def main():
    dry = "--dry" in sys.argv
    have = json.loads(OUT.read_text()) if OUT.exists() else {}
    for p in sorted((ROOT / "assets" / "models").glob("*.glb")):
        if p.stem in SKIP or p.stem == "scene_all":
            continue
        key = f"models/{p.name}"
        g = guess(p)
        merged = {**g, **have.get(key, {})}
        print(key, merged)
        if merged:
            have[key] = merged
    if not dry:
        OUT.write_text(json.dumps(have, indent=2) + "\n")


if __name__ == "__main__":
    main()
