"""Builds the stage sets: voxel backdrops and floors that replace the green screen.

    python tools/assets/build_sets.py

Writes assets/sets/<name>.glb (+ source/vox/sets/<name>.vox). 10 cm voxels (the 5 cm grid
scaled 2x), origin on the floor under the character, x to the right, z toward the camera. Each
set is 14 m wide and 6 m tall so it fills the frame from every camera angle the app uses,
including the 16:9 virtual camera and the wide raid shot.
"""
import io, json, struct, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_animals as B
from build_animals import Vox, WHITE, BLACK

ROOT = Path(__file__).resolve().parents[2]
W0, W1 = -70, 70          # 14 m wide
FRONT, BACK = 60, -21     # floor depth
WALL = -20                # back wall face (2 m behind the character)
H = 60                    # 6 m tall


def room(floor, wall, floor2=None, check=0):
    v = Vox()
    for x in range(W0, W1):
        for z in range(BACK, FRONT):
            c = floor
            if floor2 is not None and check and ((x // check) + (z // check)) % 2:
                c = floor2
            v.v[(x, -1, z)] = c
    v.box(W0, W1, 0, H, BACK, WALL, wall)
    return v


def rally():
    navy, red, gold = (28, 40, 92), (190, 36, 44), (240, 196, 60)
    v = room((120, 84, 52), navy, (104, 72, 44), check=0)
    for z in range(BACK, FRONT, 3):                         # stage planks
        v.box(W0, W1, -1, 0, z, z + 1, (104, 72, 44))
    for i, y in enumerate(range(8, 44, 6)):                 # red and white banner stripes behind
        v.box(-40, 40, y, y + 3, WALL, WALL + 1, red if i % 2 == 0 else (236, 232, 224))
    for x in range(-60, 61, 8):                             # stars along the top
        for y in (49, 54):
            v.box(x, x + 2, y, y + 2, WALL, WALL + 1, WHITE)
    for s in (-1, 1):                                       # bleachers on the sides
        for k in range(6):
            x0 = 44 + k * 4 if s > 0 else -48 - k * 4
            v.box(x0, x0 + 4, 0, 3 + k * 3, WALL, WALL + 10, (150, 156, 164))
    for x in (-50, -25, 25, 50):                            # spotlights
        v.box(x, x + 3, 56, 59, WALL, WALL + 3, gold)
    return v


def pressroom():
    blue, blue2, gold = (36, 64, 140), (30, 54, 122), (226, 186, 70)
    v = room((40, 44, 92), blue)
    for x in range(W0, W1, 3):                              # curtain folds
        v.box(x, x + 1, 0, H, WALL, WALL + 1, blue2)
    v.box(W0, W1, 52, 55, WALL, WALL + 2, gold)             # pelmet
    for x0 in (-24, 30):                                    # flags on poles, generic stripes
        v.box(x0, x0 + 1, 0, 34, -14, -13, gold)
        for i in range(6):
            v.box(x0 + 1, x0 + 9, 26 - i * 2, 28 - i * 2, -14, -13, (190, 36, 44) if i % 2 == 0 else WHITE)
        v.box(x0 + 1, x0 + 4, 22, 28, -14, -13, (28, 40, 92))
    # a round gold emblem with a star, high on the wall
    for x in range(-6, 7):
        for y in range(-6, 7):
            if x * x + y * y <= 36:
                v.v[(10 + x, 26 + y, WALL + 1)] = gold if x * x + y * y > 18 else blue2
    for (x, y) in [(0, 3), (-1, 1), (0, 1), (1, 1), (-3, 0), (-2, 0), (-1, 0), (0, 0), (1, 0), (2, 0), (3, 0), (-1, -1), (0, -1), (1, -1), (-2, -2), (2, -2)]:
        v.v[(10 + x, 26 + y, WALL + 2)] = WHITE
    return v


def diner():
    red, cream, chrome = (196, 44, 52), (244, 232, 204), (196, 200, 208)
    v = room(BLACK, cream, WHITE, check=4)
    v.box(W0, W1, 0, 14, WALL, WALL + 1, red)               # lower wall
    v.box(W0, W1, 14, 15, WALL, WALL + 2, chrome)
    v.box(-50, 50, 34, 48, WALL, WALL + 1, (150, 200, 230))  # window strip
    for x in range(-50, 51, 20):
        v.box(x, x + 1, 34, 48, WALL, WALL + 2, chrome)
    letters = {"E": ["###", "#..", "##.", "#..", "###"], "A": [".#.", "#.#", "###", "#.#", "#.#"], "T": ["###", ".#.", ".#.", ".#.", ".#."]}
    for k, ch in enumerate("EAT"):                          # neon sign
        for r, row in enumerate(letters[ch]):
            for c, on in enumerate(row):
                if on == "#":
                    x = 5 + k * 4 + c
                    v.box(x, x + 1, 30 - r, 31 - r, WALL, WALL + 2, (255, 90, 150))
    v.box(36, W1, 0, 10, -14, -4, red).box(36, W1, 10, 11, -15, -3, chrome)   # counter
    for x in (40, 48, 56, 64):
        v.box(x, x + 1, 0, 7, 0, 1, chrome).box(x - 2, x + 3, 7, 8, -2, 3, red)  # stools
    return v


def hospital():
    wall, floor, green = (206, 230, 216), (236, 240, 238), (40, 150, 90)
    v = room(floor, wall, (220, 226, 224), check=6)
    v.box(W0, W1, 0, 8, WALL, WALL + 1, (150, 196, 170))    # wainscot
    # green first-aid cross sign
    v.box(5, 17, 19, 31, WALL, WALL + 1, WHITE).box(9, 13, 20, 30, WALL, WALL + 2, green).box(6, 16, 23, 27, WALL, WALL + 2, green)
    v.box(30, 62, 0, 7, -18, -2, (180, 186, 194)).box(30, 62, 7, 9, -18, -2, WHITE).box(30, 36, 9, 12, -18, -2, WHITE)  # bed
    v.box(-64, -40, 0, 50, -17, -16, (150, 196, 220))       # curtain
    for x in range(-64, -40, 3):
        v.box(x, x + 1, 0, 50, -16, -15, (130, 176, 200))
    return v


def newsroom():
    wall, screen, frame = (22, 28, 48), (60, 120, 190), (50, 56, 70)
    v = room((34, 38, 52), wall)
    for gx in range(-60, 60, 20):                           # wall of monitors
        for gy in (8, 24, 40):
            v.box(gx + 1, gx + 19, gy, gy + 14, WALL, WALL + 1, frame)
            v.box(gx + 2, gx + 18, gy + 1, gy + 13, WALL, WALL + 2, screen if (gx // 20 + gy // 16) % 3 else (90, 160, 220))
    letters = {"L": ["#..", "#..", "#..", "#..", "###"], "I": ["###", ".#.", ".#.", ".#.", "###"], "V": ["#.#", "#.#", "#.#", "#.#", ".#."], "E": ["###", "#..", "##.", "#..", "###"]}
    v.box(3, 20, 24, 31, WALL, WALL + 3, (200, 30, 40))    # LIVE sign
    for k, ch in enumerate("LIVE"):
        for r, row in enumerate(letters[ch]):
            for c, on in enumerate(row):
                if on == "#":
                    v.v[(4 + k * 4 + c, 30 - r, WALL + 3)] = WHITE
    return v


def night():
    sky, street = (16, 20, 44), (44, 44, 52)
    v = room(street, sky, (52, 52, 60), check=10)
    for i, (x0, w, h) in enumerate([(-70, 14, 30), (-55, 10, 44), (-44, 16, 24), (-27, 12, 38), (-14, 10, 20), (-3, 14, 48), (12, 10, 28), (23, 16, 40), (40, 12, 22), (53, 17, 34)]):
        c = (30 + i % 3 * 6, 32 + i % 2 * 6, 52)
        v.box(x0, x0 + w, 0, h, WALL + 1, WALL + 3, c)
        for wx in range(x0 + 2, x0 + w - 1, 3):             # windows, some lit
            for wy in range(4, h - 2, 4):
                if (wx * 7 + wy * 13 + i) % 5 < 2:
                    v.v[(wx, wy, WALL + 3)] = (255, 214, 110)
    for (x, y) in [(-62, 52), (-40, 56), (-18, 50), (5, 57), (28, 53), (50, 58), (64, 49), (-50, 46), (38, 47)]:
        v.v[(x, y, WALL + 1)] = WHITE
    v.box(36, 41, 42, 47, WALL + 1, WALL + 2, (240, 236, 210))  # moon
    return v


SETS = [rally, pressroom, diner, hospital, newsroom, night]


def boxes(vox):
    """merge same-coloured voxels into as few boxes as possible (greedy: x, then y, then z)"""
    left = dict(vox.v)
    out = []
    for key in sorted(left):
        if key not in left:
            continue
        c = left[key]
        x0, y0, z0 = key
        x1 = x0 + 1
        while left.get((x1, y0, z0)) == c:
            x1 += 1
        y1 = y0 + 1
        while all(left.get((x, y1, z0)) == c for x in range(x0, x1)):
            y1 += 1
        z1 = z0 + 1
        while all(left.get((x, y, z1)) == c for x in range(x0, x1) for y in range(y0, y1)):
            z1 += 1
        for x in range(x0, x1):
            for y in range(y0, y1):
                for z in range(z0, z1):
                    del left[(x, y, z)]
        out.append(((x0, y0, z0), (x1, y1, z1), c))
    return out


def write_set(name, vox, glb_path, pal_img):
    import numpy as np
    out = B.Out()
    pal = B.Palette(pal_img)
    P, N, U, I = [], [], [], []
    base = 0
    for b0, b1, c in boxes(vox):
        p, n, u, ix = B.mesh_box(np.array(b0, "f4") * B.V, np.array(b1, "f4") * B.V, c, pal)
        P.append(p); N.append(n); U.append(u); I.append(ix + base)
        base += len(p)
    p, n, u, ix = np.concatenate(P), np.concatenate(N), np.concatenate(U), np.concatenate(I)
    a = {"POSITION": out.acc(p, "VEC3", target=34962, minmax=True), "NORMAL": out.acc(n, "VEC3", target=34962), "TEXCOORD_0": out.acc(u, "VEC2", target=34962)}
    prims = [{"attributes": a, "indices": out.acc(ix, "SCALAR", 5125, target=34963), "material": 0}]
    buf = io.BytesIO()
    pal.img.save(buf, "PNG")
    iv = out.view(buf.getvalue())
    j = {"asset": {"version": "2.0", "generator": "block-avatar build_sets.py"}, "scene": 0, "scenes": [{"nodes": [0]}],
         "nodes": [{"name": name, "mesh": 0, "scale": [2.0, 2.0, 2.0]}], "meshes": [{"name": name, "primitives": prims}],
         "materials": [{"name": "Voxel", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.9}}],
         "textures": [{"sampler": 0, "source": 0}], "samplers": [{"magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071}],
         "images": [{"bufferView": iv, "mimeType": "image/png", "name": "palette"}]}
    j["accessors"], j["bufferViews"] = out.accs, out.views
    while len(out.bin) % 4:
        out.bin.append(0)
    j["buffers"] = [{"byteLength": len(out.bin)}]
    js = json.dumps(j, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    glb_path.write_bytes(struct.pack("<III", 0x46546C67, 2, 28 + len(js) + len(out.bin)) + struct.pack("<II", len(js), 0x4E4F534A) + js + struct.pack("<II", len(out.bin), 0x004E4942) + bytes(out.bin))
    return len(boxes(vox))


def main():
    j0, bin0 = B.load_glb(B.TEMPLATE)
    iv = j0["bufferViews"][j0["images"][0]["bufferView"]]
    from PIL import Image
    pal_img = Image.open(io.BytesIO(bin0[iv.get("byteOffset", 0):iv.get("byteOffset", 0) + iv["byteLength"]]))
    out, src = ROOT / "assets/sets", ROOT / "source/vox/sets"
    out.mkdir(parents=True, exist_ok=True)
    src.mkdir(parents=True, exist_ok=True)
    for fn in SETS:
        path = out / f"{fn.__name__}.glb"
        vox = fn()
        nb = write_set(fn.__name__, vox, path, pal_img)
        B.write_vox(dict(parts={"Hips": vox}, tail_at=None), src / f"{fn.__name__}.vox")
        print(f"{fn.__name__:10} {len(vox.v):6} voxels in {nb} boxes  {path.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
