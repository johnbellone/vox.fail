"""Builds the dress-up wardrobe: hats and face pieces chat can put on the character.

    python tools/assets/build_wardrobe.py

Writes assets/props/wardrobe/<name>.glb (+ source/vox/props/wardrobe/<name>.vox) and
data/head_fit.json: where the top of the head, the eyes and the mouth are on every character,
so a hat sits on top (on the cap, if there is one) and glasses sit on the eyes.

Hats: origin at the bottom centre, made for a 12-voxel (60 cm) wide head; the app scales them.
Face pieces: origin at the centre of the eyes (glasses) or the mouth (moustache), on the face.
"""
import io, json, struct, sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_animals as B
from build_animals import Vox, WHITE, BLACK
from build_roasted import write_prop

ROOT = Path(__file__).resolve().parents[2]
GOLD, GOLD_D, RED, BLUE, GREEN = (240, 196, 60), (196, 150, 40), (214, 52, 52), (52, 96, 200), (60, 160, 80)
BROWN, BROWN_D, PINK, PURPLE, ORANGE = (150, 100, 56), (110, 70, 38), (236, 120, 170), (120, 70, 170), (240, 130, 40)
GREY, STEEL, CREAM = (120, 120, 128), (176, 180, 188), (240, 232, 210)


def ring(v, r_out, r_in, y0, y1, c, cx=0.0, cz=0.0):
    """filled disc / ring between radii, voxel centres"""
    R = int(r_out) + 1
    for x in range(-R, R):
        for z in range(-R, R):
            d = ((x + 0.5 - cx) ** 2 + (z + 0.5 - cz) ** 2) ** 0.5
            if r_in <= d < r_out:
                for y in range(y0, y1):
                    v.v[(x, y, z)] = c
    return v


def cowboy():
    v = Vox()
    ring(v, 10, 0, 0, 1, BROWN)                     # wide brim
    for x in (-10, 9):                              # curled sides
        v.box(x, x + 1, 1, 2, -6, 6, BROWN)
    v.box(-5, 5, 1, 6, -5, 5, BROWN).box(-5, 5, 1, 2, -5, 5, BROWN_D)   # crown + band
    v.cut(-1, 1, 5, 6, -5, 5)                       # pinch on top
    return v


def crown():
    v = Vox()
    v.box(-6, 6, 0, 3, -6, 6, GOLD).cut(-5, 5, 0, 3, -5, 5)
    for x in range(-6, 6, 3):
        v.box(x, x + 1, 3, 5, -6, -5, GOLD).box(x, x + 1, 3, 5, 5, 6, GOLD)
    for z in range(-6, 6, 3):
        v.box(-6, -5, 3, 5, z, z + 1, GOLD).box(5, 6, 3, 5, z, z + 1, GOLD)
    v.box(-1, 1, 1, 2, -7, -6, RED).box(-4, -3, 1, 2, -7, -6, BLUE).box(2, 3, 1, 2, -7, -6, GREEN)
    return v


def party():
    v = Vox()
    for y in range(0, 10):
        r = 4.6 * (1 - y / 10) + 0.4
        ring(v, r, 0, y, y + 1, PINK if (y // 2) % 2 == 0 else (90, 200, 230))
    v.box(-1, 1, 10, 12, -1, 1, GOLD)                # pom-pom
    return v


def top_hat():
    v = Vox()
    ring(v, 8.5, 0, 0, 1, BLACK)
    v.box(-5, 5, 1, 11, -5, 5, BLACK).box(-5, 5, 1, 3, -5, 5, RED)
    return v


def propeller():
    v = Vox()
    for y in range(0, 4):
        ring(v, 6.5 - y * 0.9, 0, y, y + 1, [RED, GOLD, BLUE, GREEN][y % 4] if y < 3 else RED)
    v.box(-6, 6, 0, 1, -7, -6, BLUE)                 # little visor
    v.box(0, 1, 4, 6, 0, 1, GREY)
    v.box(-6, 7, 6, 7, 0, 1, RED).box(0, 1, 6, 7, -5, 6, GOLD)
    return v


def chef():
    v = Vox()
    v.box(-6, 6, 0, 4, -6, 6, WHITE)
    ring(v, 8, 0, 4, 9, WHITE)
    v.bevel(-8, 8, 4, 9, -8, 8, edges="top")
    return v


def viking():
    v = Vox()
    v.box(-6, 6, 0, 5, -6, 6, STEEL).bevel(-6, 6, 0, 5, -6, 6, edges="top")
    v.box(-6, 6, 0, 1, -6, 6, BROWN_D).box(-1, 1, 0, 5, -7, -6, BROWN_D)
    for s in (-1, 1):                                # horns curving up and out
        for i, (dx, dy) in enumerate([(6, 2), (7, 3), (8, 4), (8, 5), (9, 6), (9, 7)]):
            x = dx if s > 0 else -dx - 1
            v.box(x, x + 1, dy, dy + 2, -1, 1, CREAM)
    return v


def pirate():
    v = Vox()
    v.box(-9, 9, 0, 4, -4, 4, BLACK)
    v.box(-6, 6, 4, 6, -3, 3, BLACK).box(-9, -6, 4, 5, -3, 3, BLACK).box(6, 9, 4, 5, -3, 3, BLACK)
    v.box(-1, 1, 1, 4, -5, -4, WHITE).box(-2, 2, 2, 3, -5, -4, WHITE)  # skull-ish badge
    v.box(-9, 9, 3, 4, -5, -4, GOLD)
    return v


def halo():
    v = Vox()
    ring(v, 6, 4.2, 3, 4, (255, 236, 120))
    return v


def cone():
    v = Vox()
    v.box(-6, 6, 0, 1, -6, 6, ORANGE)
    for y in range(1, 12):
        r = 4.4 * (1 - y / 13) + 0.6
        ring(v, r, 0, y, y + 1, WHITE if y in (4, 5, 8) else ORANGE)
    return v


def wizard():
    v = Vox()
    ring(v, 8.5, 0, 0, 1, PURPLE)
    for y in range(1, 14):
        r = 5.2 * (1 - y / 15) + 0.5
        ring(v, r, 0, y, y + 1, PURPLE, cx=0.15 * y, cz=0.0)
    for (x, y, z) in [(-3, 4, -5), (2, 7, -4), (0, 2, -6), (1, 10, -2)]:
        v.v[(x, y, z)] = GOLD
    return v


def hard_hat():
    v = Vox()
    ring(v, 6.6, 0, 0, 5, (250, 206, 40))
    v.bevel(-7, 7, 0, 5, -7, 7, edges="top")
    v.box(-6, 6, 0, 1, -9, -6, (250, 206, 40)).box(-1, 1, 1, 6, -7, 7, (230, 180, 30))
    return v


def santa():
    v = Vox()
    ring(v, 7, 0, 0, 2, WHITE)
    for y in range(2, 9):
        r = 5.6 * (1 - (y - 2) / 9) + 0.6
        ring(v, r, 0, y, y + 1, RED, cx=0.5 * (y - 2), cz=0)
    v.box(4, 7, 7, 10, -1, 2, WHITE)
    return v


def grad_cap():
    v = Vox()
    v.box(-5, 5, 0, 3, -5, 5, BLACK)
    v.box(-8, 8, 3, 4, -8, 8, BLACK)
    v.box(0, 1, 4, 5, 0, 1, GOLD).box(7, 8, 0, 4, -1, 0, GOLD)
    return v


def beanie():
    v = Vox()
    ring(v, 6.6, 0, 0, 2, (70, 120, 200))
    ring(v, 6.2, 0, 2, 5, (40, 80, 160))
    v.bevel(-7, 7, 2, 5, -7, 7, edges="top")
    v.box(-1, 1, 5, 7, -1, 1, WHITE)
    return v


# face pieces --------------------------------------------------------------------------------
def shades():
    """the pixel 'deal with it' sunglasses"""
    v = Vox()
    v.box(-6, 6, 0, 1, 0, 1, BLACK)
    v.box(-5, -1, -2, 0, 0, 1, BLACK).box(1, 5, -2, 0, 0, 1, BLACK)
    v.box(-4, -3, -1, 0, 0, 1, (90, 90, 100)).box(2, 3, -1, 0, 0, 1, (90, 90, 100))
    v.box(-6, -5, 0, 1, -6, 0, BLACK).box(5, 6, 0, 1, -6, 0, BLACK)
    return v


def nerd_glasses():
    v = Vox()
    for x0 in (-5, 1):
        v.box(x0, x0 + 4, -2, 1, 0, 1, (40, 30, 30)).cut(x0 + 1, x0 + 3, -1, 0, 0, 1)
        v.box(x0 + 1, x0 + 3, -1, 0, 0, 1, (200, 230, 255, 110))
    v.box(-1, 1, 0, 1, 0, 1, (40, 30, 30))
    v.box(-6, -5, 0, 1, -6, 0, (40, 30, 30)).box(5, 6, 0, 1, -6, 0, (40, 30, 30))
    return v


def moustache():
    v = Vox()
    v.box(-4, 4, 0, 1, 0, 1, BROWN_D).box(-5, -3, -1, 0, 0, 1, BROWN_D).box(3, 5, -1, 0, 0, 1, BROWN_D)
    v.box(-1, 1, 1, 2, 0, 1, BROWN_D)
    return v


def clown_nose():
    v = Vox()
    v.box(-1, 1, -1, 1, 0, 2, RED)
    return v


HATS = [cowboy, crown, party, top_hat, propeller, chef, viking, pirate, halo, cone, wizard, hard_hat, santa, grad_cap, beanie]
FACE = [shades, nerd_glasses, moustache, clown_nose]

FACE_PARTS = set(B.FACE_PARTS) | {"paint", "horns", "vest"}


def head_fit(path):
    """top of the head (incl. caps and ears), its width, the eye and mouth heights and the face
    front, all in the Head joint's space"""
    j, binb = B.load_glb(path)
    nodes = j["nodes"]
    head = next(i for i, n in enumerate(nodes) if n.get("name") == "Head")

    def bounds(mesh):
        lo, hi = np.full(3, 1e9), np.full(3, -1e9)
        for p in j["meshes"][mesh]["primitives"]:
            a = j["accessors"][p["attributes"]["POSITION"]]
            lo, hi = np.minimum(lo, a["min"]), np.maximum(hi, a["max"])
        return lo, hi

    top, front, xs = -1e9, -1e9, [1e9, -1e9]
    eye_y, mouth_y, eye_z = None, None, None
    cols = {}
    stack = [(head, np.zeros(3))]
    while stack:
        i, off = stack.pop()
        n = nodes[i]
        here = off + (np.array(n.get("translation", [0, 0, 0])) if i != head else 0)
        name = n.get("name", "")
        if "mesh" in n:
            lo, hi = bounds(n["mesh"])
            lo, hi = lo + here, hi + here
            if name == "eyes_open":
                eye_y = float((lo[1] + hi[1]) / 2)
                eye_z = float(hi[2])
            elif name == "mouth_flat":
                mouth_y = float((lo[1] + hi[1]) / 2)
            if name not in FACE_PARTS and not name.startswith("prop_"):
                front = max(front, hi[2])
                xs = [min(xs[0], lo[0]), max(xs[1], hi[0])]
                for pr in j["meshes"][n["mesh"]]["primitives"]:
                    for v in B.read_acc(j, binb, pr["attributes"]["POSITION"]):
                        k = (round((v[0] + here[0]) / B.V), round((v[2] + here[2]) / B.V))
                        cols[k] = max(cols.get(k, -1e9), v[1] + here[1])
        for c in n.get("children", []):
            stack.append((c, here))
    # where a hat rests: the height most of the head's top reaches (ignores a lone button,
    # antenna or ear tip; a cap's crown counts, its brim doesn't)
    # (column tops come from mesh corners: the highest level that a fifth of them reach)
    tops = sorted(cols.values(), reverse=True)
    need = max(6, int(len(tops) * 0.2))
    top = tops[min(need, len(tops)) - 1] if tops else 0.6
    return {"top": round(float(top), 3), "width": round(float(xs[1] - xs[0]), 3), "front": round(float(eye_z if eye_z is not None else front), 3),
            "eye_y": round(eye_y if eye_y is not None else top * 0.55, 3), "mouth_y": round(mouth_y if mouth_y is not None else top * 0.25, 3)}


def main():
    j0, bin0 = B.load_glb(B.TEMPLATE)
    iv = j0["bufferViews"][j0["images"][0]["bufferView"]]
    from PIL import Image
    pal_img = Image.open(io.BytesIO(bin0[iv.get("byteOffset", 0):iv.get("byteOffset", 0) + iv["byteLength"]]))
    out, src = ROOT / "assets/props/wardrobe", ROOT / "source/vox/props/wardrobe"
    out.mkdir(parents=True, exist_ok=True)
    src.mkdir(parents=True, exist_ok=True)
    for fn in HATS + FACE:
        write_prop({"name": fn.__name__, "vox": fn(), "off": (0, 0, 0)}, out / f"{fn.__name__}.glb", src / f"{fn.__name__}.vox", pal_img)
    fit = {}
    for p in sorted((ROOT / "assets/models").glob("*.glb")):
        try:
            fit[p.name] = head_fit(p)
        except StopIteration:
            pass
    (ROOT / "data/head_fit.json").write_text(json.dumps(fit, indent=1))
    print(f"{len(HATS)} hats, {len(FACE)} face pieces; head fit for {len(fit)} characters")
    for k in ["scene_all.glb", "dog.glb", "peanut.glb", "president.glb", "roasted_riot.glb"]:
        print(" ", k, fit.get(k))


if __name__ == "__main__":
    main()
