"""Head-and-shoulders portraits of every character, for the viewer question page's character
picker and the shorts' title cards: assets/portraits/<model>.png (transparent, 160x160).

Renders each model's Head joint (with the neutral face: open eyes, normal brows, flat mouth) and
the top of the torso straight from the front, orthographic, one flat colour per voxel face.

    python3 tools/make_portraits.py
"""
import io
import pathlib
import sys

import numpy as np
from PIL import Image

sys.path.insert(0, str(pathlib.Path(__file__).parent))
import build_animals as B  # noqa: E402

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "portraits"
SIZE = 160
PX = 9  # pixels per voxel before the final resize
KEEP_FACE = {"eyes_open", "brows_normal", "mouth_flat"}


def palette(j, binb):
    img = j["images"][0]
    v = j["bufferViews"][img["bufferView"]]
    o = v.get("byteOffset", 0)
    return np.array(Image.open(io.BytesIO(binb[o:o + v["byteLength"]])).convert("RGBA"))


def quat_rot(q, v):
    x, y, z, w = q
    u = np.array([x, y, z])
    return v + 2 * np.cross(u, np.cross(u, v) + w * v)


def triangles(path):
    """(tris Nx3x3 world positions, colours Nx4, normals Nx3) of the head subtree and the torso"""
    j, binb = B.load_glb(path)
    pal = palette(j, binb)
    nodes = j["nodes"]
    parent = {c: i for i, n in enumerate(nodes) for c in n.get("children", [])}

    def world(i, p):
        while i is not None:
            n = nodes[i]
            if "rotation" in n:
                p = quat_rot(np.array(n["rotation"]), p)
            p = p + np.array(n.get("translation", [0, 0, 0]))
            i = parent.get(i)
        return p

    def wanted(i):
        # the body from the waist up with the neutral face; no props, no legs, nothing hidden
        name = nodes[i].get("name", "")
        if name.startswith("prop_") or "Leg" in name:
            return False
        if name in B.FACE_PARTS:
            return name in KEEP_FACE
        return 0.0 not in nodes[i].get("scale", [1, 1, 1])

    tris, cols, nors = [], [], []
    for i, n in enumerate(nodes):
        if "mesh" not in n or not wanted(i):
            continue
        for pr in j["meshes"][n["mesh"]]["primitives"]:
            if pr.get("material", 0) != 0:
                continue
            pos = B.read_acc(j, binb, pr["attributes"]["POSITION"])
            uv = B.read_acc(j, binb, pr["attributes"]["TEXCOORD_0"])
            idx = B.read_acc(j, binb, pr["indices"]).reshape(-1, 3)
            wp = np.array([world(i, p) for p in pos])
            for t in idx:
                tri = wp[t]
                nrm = np.cross(tri[1] - tri[0], tri[2] - tri[0])
                ln = np.linalg.norm(nrm)
                if ln == 0:
                    continue
                c = uv[t].mean(0)
                h, w = pal.shape[:2]
                col = pal[min(h - 1, int(c[1] * h)), min(w - 1, int(c[0] * w))]
                tris.append(tri)
                cols.append(col)
                nors.append(nrm / ln)
    return np.array(tris), np.array(cols), np.array(nors), j


def render(path):
    tris, cols, nors, j = triangles(path)
    # frame: the head and shoulders, from the top of the head down
    lo, hi = tris.reshape(-1, 3).min(0), tris.reshape(-1, 3).max(0)
    top = hi[1]
    span = min(max(hi[0] - lo[0], 0.9), 1.1)
    bottom = top - span * 1.05
    cx = 0.0
    x0 = cx - span * 0.55
    ppu = PX / B.V
    W = int(span * 1.1 * ppu)
    H = int((top - bottom) * ppu) + 2
    img = np.zeros((H, W, 4), np.uint8)
    zb = np.full((H, W), -1e9)
    light = np.array([0.35, 0.55, 1.0])
    light /= np.linalg.norm(light)
    for tri, col, nrm in zip(tris, cols, nors):
        if nrm[2] < -0.01:
            continue  # facing away
        shade = 0.62 + 0.38 * max(0.0, float(nrm @ light))
        c = np.array([*(np.clip(col[:3] * shade, 0, 255)), 255], np.uint8)
        px = (tri[:, 0] - x0) * ppu
        py = (top - tri[:, 1]) * ppu
        xmin, xmax = int(max(0, np.floor(px.min()))), int(min(W - 1, np.ceil(px.max())))
        ymin, ymax = int(max(0, np.floor(py.min()))), int(min(H - 1, np.ceil(py.max())))
        if xmin > xmax or ymin > ymax:
            continue
        (ax, ay), (bx, by), (qx, qy) = zip(px, py)
        den = (by - qy) * (ax - qx) + (qx - bx) * (ay - qy)
        if abs(den) < 1e-9:
            continue
        gx, gy = np.meshgrid(np.arange(xmin, xmax + 1) + 0.5, np.arange(ymin, ymax + 1) + 0.5)
        l1 = ((by - qy) * (gx - qx) + (qx - bx) * (gy - qy)) / den
        l2 = ((qy - ay) * (gx - qx) + (ax - qx) * (gy - qy)) / den
        l3 = 1 - l1 - l2
        inside = (l1 >= -1e-6) & (l2 >= -1e-6) & (l3 >= -1e-6)
        z = l1 * tri[0, 2] + l2 * tri[1, 2] + l3 * tri[2, 2]
        sub = zb[ymin:ymax + 1, xmin:xmax + 1]
        win = inside & (z > sub)
        sub[win] = z[win]
        img[ymin:ymax + 1, xmin:xmax + 1][win] = c
    im = Image.fromarray(img)
    # square, centred, transparent
    side = max(im.width, im.height)
    sq = Image.new("RGBA", (side, side))
    sq.paste(im, ((side - im.width) // 2, side - im.height))
    return sq.resize((SIZE, SIZE), Image.LANCZOS)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    models = sorted((ROOT / "assets" / "models").glob("*.glb"))
    for m in models:
        try:
            render(m).save(OUT / (m.stem + ".png"), optimize=True)
            print("ok", m.name)
        except Exception as e:  # a model without a Head joint, say
            print("skip", m.name, e)


if __name__ == "__main__":
    main()
