#!/usr/bin/env python3
"""Build the animal avatars (Blue the pit bull, peanut, dog, cat, rhino, donkey, capybara, and the Brain) on the shared block rig.

Starts from the lead's prop-free rig (assets/models/variants/character_rigged.glb) so every character
keeps the same node tree, the 100 body clips, the 24 face clips and the swappable face parts. Each
animal replaces the body meshes with its own voxels (5 cm), moves the mouth onto its snout, adds a
Tail joint (animated in every clip), and appends its own clips after index 123.

    python3 tools/assets/build_animals.py            # writes assets/models/<name>.glb and source/vox/characters/<name>.vox
"""
import io, json, math, struct, sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
TEMPLATE = ROOT / "assets/models/variants/character_rigged.glb"
V = 0.05  # voxel size

FACE_PARTS = ["eyes_open", "eyes_shut", "eyes_x", "eyes_wide", "eyes_squint", "eyes_left", "eyes_right", "eyes_up",
              "brows_normal", "brows_angry", "brows_raised", "brows_worried",
              "mouth_flat", "mouth_grin", "mouth_open", "mouth_grit", "mouth_pout", "mouth_sad", "mouth_o",
              "mouth_smirk", "mouth_tongue", "mouth_frown", "mouth_wavy"]
HIDDEN_PARTS = ["hand_palm", "hand_point", "hand_thumb", "paint", "horns", "vest"]
JOINTS = ["Hips", "Head", "UpperArm_L", "LowerArm_L", "UpperArm_R", "LowerArm_R", "UpperLeg_L", "LowerLeg_L", "UpperLeg_R", "LowerLeg_R"]

# ----------------------------------------------------------------------------------------------
# glTF in/out
# ----------------------------------------------------------------------------------------------
COMP = {5126: ("f4", 4), 5123: ("u2", 2), 5125: ("u4", 4), 5121: ("u1", 1)}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def load_glb(path):
    b = path.read_bytes()
    n = struct.unpack("<I", b[12:16])[0]
    j = json.loads(b[20:20 + n])
    binlen = struct.unpack("<I", b[20 + n:24 + n])[0]
    return j, b[28 + n:28 + n + binlen]


def read_acc(j, binb, i):
    a = j["accessors"][i]
    v = j["bufferViews"][a["bufferView"]]
    dt, sz = COMP[a["componentType"]]
    nc = NCOMP[a["type"]]
    off = v.get("byteOffset", 0) + a.get("byteOffset", 0)
    return np.frombuffer(binb[off:off + a["count"] * nc * sz], dtype=dt).reshape(a["count"], nc).copy()


class Out:
    def __init__(self):
        self.bin = bytearray()
        self.views = []
        self.accs = []

    def view(self, data, target=None):
        while len(self.bin) % 4:
            self.bin.append(0)
        v = {"buffer": 0, "byteOffset": len(self.bin), "byteLength": len(data)}
        if target:
            v["target"] = target
        self.bin += data
        self.views.append(v)
        return len(self.views) - 1

    def acc(self, arr, kind, comp=5126, target=None, minmax=False):
        arr = np.ascontiguousarray(arr, dtype=COMP[comp][0])
        if arr.ndim == 1:
            arr = arr.reshape(-1, 1)
        a = {"bufferView": self.view(arr.tobytes(), target), "componentType": comp, "count": int(arr.shape[0]), "type": kind}
        if minmax:
            a["min"] = [float(x) for x in arr.min(axis=0)]
            a["max"] = [float(x) for x in arr.max(axis=0)]
        self.accs.append(a)
        return len(self.accs) - 1


# ----------------------------------------------------------------------------------------------
# Voxels -> mesh
# ----------------------------------------------------------------------------------------------
class Vox:
    """Integer voxel grid for one body part, relative to the joint: 5 cm units, or 5 / `res` cm for
    a finer part (the Brain's head and shirt use res=4, 1.25 cm voxels). One part, one grid: the
    avatar app's bevel pass finds each mesh's grid from its quads."""

    def __init__(self, res=1):
        self.v = {}
        self.res = res
        # cells inside a see-through shell (a jar's cavity): its glass shows no faces towards them,
        # so the jar is one pane thick to look through, not two
        self.inner = set()

    def box(self, x0, x1, y0, y1, z0, z1, c):
        for x in range(x0, x1):
            for y in range(y0, y1):
                for z in range(z0, z1):
                    self.v[(x, y, z)] = c
        return self

    def cut(self, x0, x1, y0, y1, z0, z1):
        for x in range(x0, x1):
            for y in range(y0, y1):
                for z in range(z0, z1):
                    self.v.pop((x, y, z), None)
        return self

    def paint(self, pred, c):
        """recolour existing voxels where pred(x, y, z) is true"""
        for k in list(self.v):
            if pred(*k):
                self.v[k] = c
        return self

    def bevel(self, x0, x1, y0, y1, z0, z1, edges="all"):
        """knock out the corner line of each edge of a box, for a softer silhouette"""
        xs, ys, zs = (x0, x1 - 1), (y0, y1 - 1), (z0, z1 - 1)
        for k in list(self.v):
            x, y, z = k
            if not (x0 <= x < x1 and y0 <= y < y1 and z0 <= z < z1):
                continue
            on = (x in xs) + (y in ys) + (z in zs)
            if on >= 2 and (edges == "all" or (edges == "top" and y == y1 - 1)):
                del self.v[k]
        return self

    def surface(self, x, y, z):
        return any((x + dx, y + dy, z + dz) not in self.v for dx, dy, dz in DIRS)


DIRS = [(1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1)]
# quad corners (unit cube) per face, counter-clockwise seen from outside
FACES = {
    (1, 0, 0): [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)],
    (-1, 0, 0): [(0, 0, 0), (0, 0, 1), (0, 1, 1), (0, 1, 0)],
    (0, 1, 0): [(0, 1, 0), (0, 1, 1), (1, 1, 1), (1, 1, 0)],
    (0, -1, 0): [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)],
    (0, 0, 1): [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)],
    (0, 0, -1): [(0, 0, 0), (0, 1, 0), (1, 1, 0), (1, 0, 0)],
}


class Palette:
    """16x16 palette: rows 0-7 are the template's (so kept meshes stay valid), rows 8-15 are new."""

    def __init__(self, template_img):
        self.img = Image.new("RGBA", (16, 16), (0, 0, 0, 0))
        self.img.paste(template_img.convert("RGBA"), (0, 0))
        self.cells = {}
        self.next = 16 * 8

    def uv(self, c):
        c = tuple(c)
        if c not in self.cells:
            i = self.next
            self.next += 1
            assert i < 256, "palette full"
            self.img.putpixel((i % 16, i // 16), c + (255,) if len(c) == 3 else c)
            self.cells[c] = i
        i = self.cells[c]
        return ((i % 16) + 0.5) / 16, ((i // 16) + 0.5) / 16


def greedy(faces, s, inset=0.0):
    """Merges same-coloured faces in one plane into rectangles. `faces` maps (direction, voxel) to
    a palette uv; the result is (positions, normals, uvs, indices). For the see-through glass and
    fluid of a finer part, where per-voxel faces would only make the file bigger (the avatar app's
    bevel pass splits the glass back into voxel faces anyway)."""
    pos, nrm, uv, idx = [], [], [], []
    for d in DIRS:
        ax = [i for i in range(3) if d[i]][0]
        ua, va = (ax + 1) % 3, (ax + 2) % 3
        planes = {}
        for (fd, k), u in faces.items():
            if fd == d:
                planes.setdefault(k[ax], {})[(k[ua], k[va])] = u
        for w, cells in planes.items():
            left = dict(cells)
            for (a0, b0) in sorted(cells):
                if (a0, b0) not in left:
                    continue
                u = left[(a0, b0)]
                a1 = a0 + 1
                while left.get((a1, b0)) == u:
                    a1 += 1
                b1 = b0 + 1
                while all(left.get((a, b1)) == u for a in range(a0, a1)):
                    b1 += 1
                for a in range(a0, a1):
                    for b in range(b0, b1):
                        del left[(a, b)]
                base = len(pos)
                for cx, cy, cz in FACES[d]:
                    c = (cx, cy, cz)
                    q = [0.0, 0.0, 0.0]
                    q[ax] = (w + c[ax]) * s - d[ax] * inset
                    q[ua] = (a0 + (a1 - a0) * c[ua]) * s
                    q[va] = (b0 + (b1 - b0) * c[va]) * s
                    pos.append(tuple(q))
                    nrm.append(d)
                    uv.append(u)
                idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return np.array(pos, "f4").reshape(-1, 3), np.array(nrm, "f4").reshape(-1, 3), np.array(uv, "f4").reshape(-1, 2), np.array(idx, "u4")


def mesh_vox(vox, pal, translucent=False):
    """Colours with a 4th component are see-through (visors, shields) and go in their own primitive."""
    pos, nrm, uv, idx = [], [], [], []
    s = V / vox.res
    if vox.res > 1:
        # a finer part is merged into rectangles (the file stays small; the avatar app's bevel pass
        # splits them back into voxel faces). That pass finds a mesh's grid from its quads' sides,
        # and takes 5 cm when half of them are multiples of it: check a merged part can't pass
        faces = {}
        for (x, y, z), c in vox.v.items():
            if (len(c) == 4) != translucent:
                continue
            for d in DIRS:
                k = (x + d[0], y + d[1], z + d[2])
                nb = vox.v.get(k)
                if translucent:
                    show = (nb is None or len(nb) != 4) and k not in vox.inner
                else:
                    show = nb is None or len(nb) == 4
                if show:
                    faces[(d, (x, y, z))] = pal.uv(c[:3])
        out = greedy(faces, s)
        if not translucent and len(out[0]):
            q = out[0].reshape(-1, 4, 3)
            sides = np.concatenate([np.linalg.norm(q[:, 1] - q[:, 0], axis=1), np.linalg.norm(q[:, 3] - q[:, 0], axis=1)])
            coarse = np.mean(np.abs(sides / V - np.round(sides / V)) < 0.02)
            assert coarse < 0.4, f"a {s * 100:.2f} cm part merged into quads that look like a 5 cm grid ({coarse:.0%})"
        return out
    for (x, y, z), c in vox.v.items():
        if (len(c) == 4) != translucent:
            continue
        for d in DIRS:
            nb = vox.v.get((x + d[0], y + d[1], z + d[2]))
            if nb is not None and (len(nb) == 4) == translucent:
                continue
            base = len(pos)
            u = pal.uv(c[:3])
            for cx, cy, cz in FACES[d]:
                pos.append(((x + cx) * s, (y + cy) * s, (z + cz) * s))
                nrm.append(d)
                uv.append(u)
            idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return np.array(pos, "f4"), np.array(nrm, "f4"), np.array(uv, "f4"), np.array(idx, "u4")


# a fluid's faces sit this far inside its voxels, so they never fight the glass they touch (m)
FLUID_INSET = 0.0015


def mesh_fluid(fluid, solid, pal):
    """A jar's fluid (`Vox`, same grid as the part it fills): only the faces that show, i.e. not
    those against more fluid or against an opaque voxel of `solid` (the brain), each pulled a hair
    inward. It gets its own material ("VoxelFluid"), which the apps draw with the slosh shader
    (shaders/fluid.wesl)."""
    faces = {}
    for (x, y, z), c in fluid.v.items():
        for d in DIRS:
            k = (x + d[0], y + d[1], z + d[2])
            if k in fluid.v:
                continue
            nb = solid.v.get(k)
            if nb is not None and len(nb) == 3:
                continue
            faces[(d, (x, y, z))] = pal.uv(c[:3])
    return greedy(faces, V / fluid.res, FLUID_INSET)


def mesh_box(b0, b1, c, pal):
    """single float box (for hand shapes, which aren't on the 5 cm grid)"""
    pos, nrm, uv, idx = [], [], [], []
    u = pal.uv(c)
    for d, corners in FACES.items():
        base = len(pos)
        for cx, cy, cz in corners:
            pos.append(tuple(b0[i] + (b1[i] - b0[i]) * (cx, cy, cz)[i] for i in range(3)))
            nrm.append(d)
            uv.append(u)
        idx += [base, base + 1, base + 2, base, base + 2, base + 3]
    return np.array(pos, "f4"), np.array(nrm, "f4"), np.array(uv, "f4"), np.array(idx, "u4")


# ----------------------------------------------------------------------------------------------
# Rotations
# ----------------------------------------------------------------------------------------------
def quat_axis(axis, deg):
    h = math.radians(deg) / 2
    s = math.sin(h)
    return np.array([axis[0] * s, axis[1] * s, axis[2] * s, math.cos(h)])


def qmul(a, b):
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return np.array([aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx,
                     aw * bz + ax * by - ay * bx + az * bw, aw * bw - ax * bx - ay * by - az * bz])


def euler(x=0, y=0, z=0):
    """y (turn) * x (pitch) * z (roll), degrees. +x tips forward / bends a knee back; -x raises an arm or
    leg forward; +z on a left limb lifts it outward (-z for a right limb)."""
    return qmul(qmul(quat_axis((0, 1, 0), y), quat_axis((1, 0, 0), x)), quat_axis((0, 0, 1), z))


# rest pose for the animals' own clips (close to the lead's Idle, but symmetric)
REST = {"Hips": (0, 0, 0), "Head": (0, 0, 0), "UpperArm_L": (4, 0, 6), "LowerArm_L": (-14, 0, 0), "UpperArm_R": (4, 0, -6),
        "LowerArm_R": (-14, 0, 0), "UpperLeg_L": (0, 0, 0), "LowerLeg_L": (0, 0, 0), "UpperLeg_R": (0, 0, 0), "LowerLeg_R": (0, 0, 0)}
HIP_Y = 0.565


# ----------------------------------------------------------------------------------------------
# Character definitions
# ----------------------------------------------------------------------------------------------
BLACK = (28, 24, 28)
WHITE = (246, 244, 238)


def standard_body(skin, torso_fn, arm_fn, leg_fn, hand):
    """Body parts on the template's proportions so the shared clips (feet planting, hand reach) still fit."""
    parts = {}
    t = Vox().box(-5, 5, 0, 11, -3, 4, skin)
    torso_fn(t)
    parts["Hips"] = t
    for side in ("L", "R"):
        # arms are 3 voxels square centred on the joint (-0.075..0.075): built on [-2,1) and shifted half a voxel (OFFSETS)
        parts[f"UpperArm_{side}"] = Vox().box(-2, 1, -5, 1, -2, 1, skin)
        la = Vox().box(-2, 1, -5, 0, -2, 1, skin)
        parts[f"LowerArm_{side}"] = la
        ul = Vox().box(-2, 2, -5, 0, -2, 2, skin)
        parts[f"UpperLeg_{side}"] = ul
        ll = Vox().box(-2, 2, -6, 0, -2, 2, skin).box(-2, 2, -6, -4, 2, 4, skin)
        parts[f"LowerLeg_{side}"] = ll
        arm_fn(side, parts[f"UpperArm_{side}"], parts[f"LowerArm_{side}"])
        leg_fn(side, parts[f"UpperLeg_{side}"], parts[f"LowerLeg_{side}"])
    parts["_hand"] = hand
    return parts


# The arm meshes are 3 voxels wide centred on the joint (-0.075..0.075), so they sit half a voxel
# off the grid; OFFSETS shifts a part's voxels by that half voxel when meshing.
OFFSETS = {"UpperArm_L": (0.5, 0, 0.5), "LowerArm_L": (0.5, 0, 0.5), "UpperArm_R": (0.5, 0, 0.5), "LowerArm_R": (0.5, 0, 0.5)}


def head_base(c, top=11):
    h = Vox().box(-7, 7, -2, top, -6, 6, c)
    h.bevel(-7, 7, -2, top, -6, 6, "top")
    return h


def dog():
    fur, cream, dark, red, nose = (206, 150, 72), (244, 226, 188), (150, 96, 46), (196, 40, 44), BLACK

    def torso(t):
        t.paint(lambda x, y, z: z == 3 and -3 <= x <= 2 and y <= 8, cream)
        t.box(-5, 5, 9, 11, -3, 4, red)                               # bandana knot band
        for y, w in ((8, 3), (7, 2), (6, 1)):
            t.box(-w, w, y, y + 1, 3, 4, red)                         # bandana point on the chest
        t.box(-1, 1, 9, 10, 4, 5, (232, 185, 35))                     # tag

    def arm(side, ua, la):
        la.paint(lambda x, y, z: y <= -4, cream)                      # paws

    def leg(side, ul, ll):
        ll.paint(lambda x, y, z: y <= -5, cream)

    p = standard_body(fur, torso, arm, leg, cream)
    h = head_base(fur)
    h.paint(lambda x, y, z: z == 5 and -3 <= x <= 2 and y <= 3, cream)
    h.box(-3, 3, -2, 3, 6, 9, cream).bevel(-3, 3, -2, 3, 6, 9, "top")      # muzzle
    h.box(-1, 1, 3, 4, 7, 9, nose)                                          # nose on top of the muzzle
    for x0 in (-8, 7):                                                      # floppy ears
        h.box(x0, x0 + 1, 1, 10, -3, 2, dark).box(x0, x0 + 1, 0, 1, -2, 1, dark)
    h.box(-2, 2, 9, 11, 5, 6, (226, 176, 98))                               # forehead blaze
    p["Head"] = h
    tail = Vox().box(-1, 1, 0, 2, -2, 0, fur).box(-1, 1, 1, 4, -3, -2, fur).box(-1, 1, 3, 6, -4, -3, fur).box(-1, 1, 5, 7, -4, -3, cream)
    p["Tail"] = tail
    return dict(name="dog", label="Dog", parts=p, mouth=(0, -0.04, 0.15), tail_at=(0.0, 0.12, -0.15),
                tail_sway=dict(axis="y", amp=28, period=0.36), note="Golden-furred dog: cream muzzle and paws, floppy brown ears, red bandana with a gold tag.")


def blue():
    """Blue: an American pit bull terrier with a blue nose, a blue-grey coat and white markings. Vox's
    main character."""
    coat, shade, white, nose, collar, ring = (92, 104, 124), (70, 80, 98), (240, 240, 236), (52, 58, 70), (214, 112, 38), (200, 204, 210)

    def torso(t):
        t.paint(lambda x, y, z: z == 3 and -3 <= x <= 2 and y <= 9, white)            # white chest
        t.paint(lambda x, y, z: z == 3 and -1 <= x <= 0 and y <= 10, white)
        t.paint(lambda x, y, z: z == -3 and abs(x + 0.5) <= 2 and y >= 4, shade)      # darker along the back
        t.box(-5, 5, 9, 11, -3, 4, collar)                                            # orange collar
        t.box(-1, 1, 7, 9, 4, 5, ring)                                                # its tag

    def arm(side, ua, la):
        ua.paint(lambda x, y, z: y == -1, shade)
        la.paint(lambda x, y, z: y <= -4, white)                                      # white paws

    def leg(side, ul, ll):
        if side == "L":
            ll.paint(lambda x, y, z: y <= -5, white)                                  # one white back paw

    p = standard_body(coat, torso, arm, leg, white)
    h = head_base(coat)
    h.box(-8, -7, 0, 7, -4, 4, coat).box(7, 8, 0, 7, -4, 4, coat)                    # broad cheeks
    h.box(-4, 4, -1, 3, 6, 9, coat).bevel(-4, 4, -1, 3, 6, 9, "top")                  # wide, short muzzle
    h.paint(lambda x, y, z: z >= 6 and y <= 0, white)                                  # white jaw
    h.box(-2, 2, 2, 4, 8, 10, nose).box(-1, 1, 3, 4, 9, 10, (88, 96, 112))          # the blue nose, a highlight on top
    h.paint(lambda x, y, z: z == 5 and -1 <= x <= 0 and 3 <= y <= 10, white)          # blaze up the forehead
    h.paint(lambda x, y, z: z >= 6 and -1 <= x <= 0 and y >= 2 and (x, y, z) in h.v and h.v[(x, y, z)] == coat, white)
    h.paint(lambda x, y, z: z >= 6 and 1 <= y and abs(x + 0.5) >= 1.5, (124, 134, 152))  # a paler muzzle
    for x0, x1, tip in ((-9, -6, -9), (6, 9, 8)):                                        # rose ears: small, high, folded out and back
        h.box(x0, x1, 11, 12, -3, 0, shade).box(x0, x1, 10, 11, -3, -1, shade)
        h.box(tip, tip + 1, 9, 10, -3, -1, shade)
    p["Head"] = h
    tail = Vox().box(-1, 1, 0, 2, -2, 0, coat).box(-1, 1, 1, 3, -3, -2, coat).box(0, 1, 2, 4, -4, -3, coat).box(0, 1, 3, 5, -5, -4, shade)
    p["Tail"] = tail
    return dict(name="blue", label="Blue", parts=p, mouth=(0, -0.04, 0.15), tail_at=(0.0, 0.12, -0.15),
                tail_sway=dict(axis="y", amp=32, period=0.34),
                note="American pit bull terrier with a blue nose: blue-grey coat, white blaze, chest and paws, rose ears, orange collar. Vox's main character.")


def cat():
    fur, stripe, white, pink = (150, 150, 160), (92, 92, 104), (244, 242, 236), (238, 150, 160)

    def torso(t):
        t.paint(lambda x, y, z: (z < 3 or abs(x + 0.5) > 3) and y % 3 == 1, stripe)
        t.paint(lambda x, y, z: z == 3 and -3 <= x <= 2 and y >= 2, white)

    def arm(side, ua, la):
        ua.paint(lambda x, y, z: y == -3, stripe)
        la.paint(lambda x, y, z: y <= -4, white)

    def leg(side, ul, ll):
        ul.paint(lambda x, y, z: y == -3, stripe)
        ll.paint(lambda x, y, z: y <= -5, white)

    p = standard_body(fur, torso, arm, leg, white)
    h = head_base(fur, top=10)
    h.paint(lambda x, y, z: z < 5 and y >= 6 and x % 3 == 0, stripe)       # tabby stripes over the crown
    h.paint(lambda x, y, z: z == 5 and y >= 8 and x in (-1, 0), stripe)     # forehead "M"
    h.box(-3, 3, -2, 3, 6, 7, white)                                        # small muzzle
    h.box(-1, 1, 2, 3, 7, 8, pink)                                          # nose
    for x0, x1 in ((-6, -3), (3, 6)):                                       # pointed ears
        h.box(x0, x1, 10, 12, -2, 1, fur).box(x0 + 1, x1 - 1, 12, 13, -2, 1, fur)
        h.box(x0 + 1, x1 - 1, 10, 12, 1, 2, pink)
    for y in (0, 2):                                                        # whiskers
        h.box(-10, -7, y, y + 1, 5, 6, WHITE).box(7, 10, y, y + 1, 5, 6, WHITE)
    p["Head"] = h
    tail = Vox().box(-1, 1, 0, 2, -2, 0, fur).box(-1, 1, 1, 9, -3, -2, fur).box(-1, 1, 8, 10, -2, -1, fur)
    tail.paint(lambda x, y, z: y % 3 == 0, stripe)
    p["Tail"] = tail
    return dict(name="cat", label="Cat", parts=p, mouth=(0, -0.03, 0.05), tail_at=(0.0, 0.1, -0.15),
                tail_sway=dict(axis="z", amp=14, period=1.8), note="Grey tabby cat: striped back and crown, white chest and socks, pink nose, whiskers, tall tail.")


def rhino():
    skin, horn, navy, shirt, tie, shoe = (150, 150, 158), (226, 214, 186), (36, 46, 82), WHITE, (190, 36, 44), (110, 110, 118)

    def torso(t):
        t.box(-5, 5, 0, 11, -3, 4, navy)
        t.box(-6, 6, 0, 8, -3, 5, navy).cut(-6, -5, 0, 8, 4, 5).cut(5, 6, 0, 8, 4, 5)   # belly
        for y in range(6, 11):                                                          # shirt V and tie
            w = 1 + (y - 6) // 2
            t.box(-w, w, y, y + 1, 3, 4, shirt)
        t.box(-1, 1, 3, 10, 4, 5, tie).box(-1, 1, 6, 10, 3, 4, tie)
        t.box(2, 3, 8, 9, 4, 5, (206, 40, 44))                                         # flag pin
        t.box(-5, 5, 0, 1, -3, 5, (24, 26, 34))                                         # belt

    def arm(side, ua, la):
        ua.paint(lambda *_: True, navy)
        la.paint(lambda *_: True, navy)
        la.paint(lambda x, y, z: y == -4, shirt)
        la.paint(lambda x, y, z: y <= -5, skin)
        la.box(-2, 1, -6, -4, -2, 1, skin)

    def leg(side, ul, ll):
        ul.paint(lambda *_: True, navy)
        ll.paint(lambda *_: True, navy)
        ll.paint(lambda x, y, z: y <= -5, shoe)

    p = standard_body(skin, torso, arm, leg, skin)
    h = head_base(skin)
    h.paint(lambda x, y, z: y in (4, 5) and z < 5, (132, 132, 140))         # wrinkle band
    h.box(-4, 4, -2, 4, 6, 10, skin).bevel(-4, 4, -2, 4, 6, 10, "top")      # long snout
    h.box(-1, 1, 4, 6, 7, 10, horn).box(-1, 1, 6, 8, 8, 10, horn).box(-1, 1, 8, 9, 9, 11, horn)   # big horn on the snout tip, curving forward
    h.box(-1, 1, 4, 5, 6, 7, horn)                                          # small horn behind it
    for x0 in (-7, 5):                                                      # small ears
        h.box(x0, x0 + 2, 11, 13, -3, -1, skin).box(x0, x0 + 2, 11, 12, -1, 0, (200, 160, 170))
    p["Head"] = h
    tail = Vox().box(0, 1, -2, 1, -1, 0, skin).box(0, 1, -4, -1, -2, -1, skin).box(-1, 2, -6, -4, -2, -1, (60, 60, 66))
    p["Tail"] = tail
    return dict(name="rhino", label="Rhino", parts=p, mouth=(0, -0.05, 0.20), tail_at=(0.0, 0.15, -0.15),
                tail_sway=dict(axis="z", amp=12, period=0.9), note="Rhino in a navy suit, white shirt, red tie and flag pin; big ivory horn, wrinkled grey hide.")


def donkey():
    fur, light, mane, blazer, shirt, jeans, hoof = (142, 128, 118), (218, 208, 196), (66, 54, 50), (44, 86, 168), WHITE, (64, 92, 140), (54, 42, 36)

    def torso(t):
        t.box(-5, 5, 0, 11, -3, 4, blazer)
        t.box(-2, 2, 2, 11, 3, 4, shirt)                                       # open shirt front
        t.box(-1, 1, 9, 11, 3, 4, light)                                       # open collar shows fur
        t.box(-3, -2, 7, 8, 3, 4, (232, 232, 232))                             # pin
        t.box(-5, 5, 0, 1, -3, 4, jeans)

    def arm(side, ua, la):
        ua.paint(lambda *_: True, blazer)
        la.paint(lambda *_: True, blazer)
        la.paint(lambda x, y, z: y <= -4, fur)

    def leg(side, ul, ll):
        ul.paint(lambda *_: True, jeans)
        ll.paint(lambda *_: True, jeans)
        ll.paint(lambda x, y, z: y <= -5, hoof)

    p = standard_body(fur, torso, arm, leg, fur)
    h = head_base(fur)
    h.box(-4, 4, -2, 4, 6, 10, light).bevel(-4, 4, -2, 4, 6, 10, "top")    # long pale muzzle
    h.box(-1, 1, 10, 13, -6, 6, mane).box(-1, 1, 6, 11, -7, -6, mane)      # mane
    for x0 in (-6, 4):                                                      # tall ears with dark tips
        h.box(x0, x0 + 2, 11, 18, -1, 1, fur).box(x0, x0 + 2, 17, 19, -1, 1, mane)
        h.box(x0, x0 + 2, 12, 17, 1, 2, light)
    p["Head"] = h
    tail = Vox().box(0, 1, -1, 1, -1, 0, fur).box(0, 1, -5, -1, -2, -1, fur).box(-1, 2, -8, -5, -2, -1, mane)
    p["Tail"] = tail
    return dict(name="donkey", label="Donkey", parts=p, mouth=(0, -0.04, 0.20), tail_at=(0.0, 0.15, -0.15),
                tail_sway=dict(axis="z", amp=16, period=1.1), note="Donkey in a blue blazer over an open-collar white shirt and jeans; tall ears, dark mane, pale muzzle.")


def peanut():
    shell, dimple, limb, shoe, cap, capdark = (214, 168, 104), (180, 134, 76), (110, 72, 40), (60, 48, 40), (236, 128, 36), (196, 96, 24)
    tex = lambda x, y, z: (x * 3 + y * 5 + z * 7) % 6 == 0

    def torso(t):
        t.v.clear()
        t.box(-5, 5, 0, 11, -4, 5, shell).bevel(-5, 5, 0, 11, -4, 5)
        t.box(-4, 4, 0, 1, -3, 4, shell)
        t.paint(tex, dimple)
        t.cut(-5, 5, 10, 11, -4, 5).box(-4, 4, 10, 11, -3, 4, shell)       # waist pinch up into the head

    def arm(side, ua, la):
        ua.v.clear()
        la.v.clear()
        ua.box(-1, 1, -5, 1, -1, 1, limb)
        la.box(-1, 1, -4, 0, -1, 1, limb).box(-2, 1, -5, -4, -2, 1, (236, 226, 206))   # little cream hands

    def leg(side, ul, ll):
        ul.v.clear()
        ll.v.clear()
        ul.box(-1, 1, -5, 0, -1, 1, limb)
        ll.box(-1, 1, -5, 0, -1, 1, limb).box(-2, 2, -6, -5, -2, 4, shoe).box(-2, 2, -6, -5, 3, 4, WHITE)

    p = standard_body(shell, torso, arm, leg, (236, 226, 206))
    h = Vox().box(-6, 6, -2, 11, -6, 6, shell).bevel(-6, 6, -2, 11, -6, 6)
    h.box(-7, 7, 1, 8, -5, 5, shell)                                        # cheeks wider than the waist
    h.box(-6, 6, 0, 10, -5, 6, shell)                                       # keep the face plane flat
    h.paint(lambda x, y, z: tex(x, y, z) and not (z == 5 and -5 <= x <= 4 and -1 <= y <= 9), dimple)
    h.box(-6, 6, 10, 13, -5, 5, cap).box(-5, 5, 13, 14, -4, 4, cap)         # ball cap
    h.box(-5, 5, 10, 11, 5, 9, capdark).box(-1, 1, 14, 15, -1, 1, capdark)  # brim and button
    p["Head"] = h
    return dict(name="peanut", label="Peanut", parts=p, mouth=(0, 0.0, 0.0), tail_at=None, tail_sway=None,
                note="Original peanut character: two-lobed dimpled shell, stick limbs, cream hands, orange ball cap, sneakers.")


GRIP = (0.0, -0.25, 0.0)  # a held thing's place, in LowerArm space (as build_roasted.py)


def ball(r, c, cy=0.0):
    """voxels within `r` of a centre half a voxel off the grid (round from every side)"""
    v = Vox()
    R = int(r) + 1
    for x in range(-R, R):
        for y in range(-R, R + 1):
            for z in range(-R, R):
                if (x + 0.5) ** 2 + (y + 0.5 - cy) ** 2 + (z + 0.5) ** 2 <= r * r:
                    v.v[(x, y, z)] = c
    return v


def melon():
    """a wedge of watermelon, rind at the bottom (the grip), point up"""
    rind, pith, flesh = (52, 132, 60), (214, 236, 190), (230, 62, 74)
    v = Vox().box(-6, 6, -1, 0, -1, 1, rind).box(-6, 6, 0, 1, -1, 1, pith)
    for y, w in ((1, 5), (2, 5), (3, 4), (4, 3), (5, 2), (6, 1)):
        v.box(-w, w, y, y + 1, -1, 1, flesh)
    for x, y in ((-3, 2), (2, 2), (-1, 4), (0, 1)):
        v.box(x, x + 1, y, y + 1, 0, 1, BLACK)                               # seeds
    return v


def capybara():
    """An original capybara: a barrel of warm brown fur, a big blunt square muzzle, small round ears
    and eyes set high, no tail to speak of. Famously calm. Its props (a yuzu on its head, a towel for
    the hot spring, a slice of watermelon) appear only in the clips that use them."""
    fur, shade, belly, muzzle, nose, paw = (156, 108, 64), (124, 84, 48), (184, 140, 96), (136, 92, 54), (48, 34, 28), (88, 60, 38)

    def torso(t):
        t.box(-6, 6, 1, 9, -4, 5, fur).bevel(-6, 6, 1, 9, -4, 5)           # round barrel belly
        t.paint(lambda x, y, z: z >= 3 and abs(x + 0.5) <= 3 and y <= 8, belly)
        t.paint(lambda x, y, z: z <= -3 and y >= 7, shade)                  # darker along the back
        t.paint(lambda x, y, z: (x * 7 + y * 3 + z * 5) % 11 == 0 and z <= 2, shade)   # a little shag

    def arm(side, ua, la):
        la.paint(lambda x, y, z: y <= -4, paw)

    def leg(side, ul, ll):
        ll.paint(lambda x, y, z: y <= -5, paw)

    p = standard_body(fur, torso, arm, leg, paw)
    h = Vox().box(-7, 7, -2, 10, -6, 6, fur)
    h.bevel(-7, 7, -2, 10, -6, 6, "top")
    h.box(-5, 5, -2, 4, 6, 10, muzzle).bevel(-5, 5, -2, 4, 6, 10, "top")     # big blunt square muzzle, below the eyes
    h.box(-3, 3, 2, 4, 9, 10, nose)                                          # broad dark nose pad on its front
    for x0 in (-7, 5):                                                       # small round ears, set high and back
        h.box(x0, x0 + 2, 10, 12, -4, -1, shade).box(x0, x0 + 2, 12, 13, -3, -2, shade)
    h.paint(lambda x, y, z: z <= -5 and y >= 6, shade)
    p["Head"] = h
    props = [
        # a yuzu balanced on top of the head (the head is 10 voxels tall above its joint)
        dict(name="yuzu", parent="Head", at=(0.0, 0.5, -0.02), rot=(0, 0, 0), off=(0.5, 0, 0.5),
             vox=ball(3.2, (246, 186, 38), cy=3.2).box(-1, 1, 6, 7, -1, 1, (82, 150, 60)).box(0, 2, 7, 8, -1, 0, (82, 150, 60))),
        # a folded towel for the hot spring
        dict(name="towel", parent="Head", at=(0.0, 0.5, -0.02), rot=(0, 0, 6), off=(0.5, 0, 0.5),
             vox=Vox().box(-4, 4, 0, 2, -3, 3, WHITE).box(-4, 4, 0, 2, -1, 0, (90, 150, 210)).box(-4, -3, 0, 2, -3, 3, (236, 236, 230))),
        # a slice of watermelon, held by the rind
        dict(name="melon", parent="LowerArm_R", at=GRIP, rot=(90, 0, 0), off=(0.5, 0, 0.5), vox=melon()),
    ]
    return dict(name="capybara", label="Capybara", parts=p, props=props, mouth=(0, -0.04, 0.20), tail_at=None, tail_sway=None,
                note="Original capybara: warm brown barrel body, big blunt dark muzzle, small round ears; sometimes a yuzu on its head.")


def hash3(x, y, z, seed):
    h = (x * 374761393 + y * 668265263 + z * 2147483647 + seed * 1274126177) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 65535.0


def vnoise3(x, y, z, seed=0):
    """smooth value noise, 0-1 (for the brain's folds: deterministic, no dependencies)"""
    xi, yi, zi = math.floor(x), math.floor(y), math.floor(z)
    fx, fy, fz = x - xi, y - yi, z - zi
    fx, fy, fz = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy), fz * fz * (3 - 2 * fz)
    def lerp(a, b, t):
        return a + (b - a) * t
    c = [[[hash3(xi + i, yi + j, zi + k, seed) for k in (0, 1)] for j in (0, 1)] for i in (0, 1)]
    return lerp(lerp(lerp(c[0][0][0], c[0][0][1], fz), lerp(c[0][1][0], c[0][1][1], fz), fy),
                lerp(lerp(c[1][0][0], c[1][0][1], fz), lerp(c[1][1][0], c[1][1][1], fz), fy), fx)


# a red lumberjack flannel: red ground, a broad black band and a thin gold line, 20 cm repeat (16
# voxels of 1.25 cm); where the warp and weft threads differ the cell is their mix, as woven
PLAID_RED, PLAID_BLACK, PLAID_GOLD = (176, 34, 40), (34, 28, 32), (220, 184, 104)


def plaid_thread(k):
    k %= 16
    if 7 <= k <= 10:
        return PLAID_BLACK
    if k == 14:
        return PLAID_GOLD
    return PLAID_RED


def plaid(u, v):
    a, b = plaid_thread(u), plaid_thread(v)
    return a if a == b else tuple((p + q) // 2 for p, q in zip(a, b))


def paint_plaid(vox, x0, x1, z0, z1, keep=lambda x, y, z: False, shift=(0, 0)):
    """Plaid on a box-shaped part: the vertical threads run round it (x on the front and back,
    z on the sides), the horizontal ones down it."""
    for (x, y, z), c in list(vox.v.items()):
        if keep(x, y, z):
            continue
        side = x in (x0, x1 - 1) and z not in (z0, z1 - 1)
        u = (z if x == x1 - 1 else -z) if side else (x if z == z1 - 1 else -x)
        vox.v[(x, y, z)] = plaid(u + shift[0], y + shift[1])


def brain():
    """The Brain, from the comic From the Brain of Michael Collins (the requester's own): Michael's
    brain in a jar, pink with round glasses, fact-checking everything. Here the jar is the head of a
    professor in a red plaid flannel shirt: computer scientist, dungeon master and the foremost
    expert on tabletop wargame lore. Props: a pointer for lectures, a d20, a painted miniature.

    The head and the shirt are built from 1.25 cm voxels (res=4), four to the usual 5 cm one: the
    brain gets real folds, a fissure, a cerebellum and a stem; the glasses are round; the shirt is
    woven plaid with a collar, buttons and pockets. The fluid the brain floats in is its own layer
    (`fluid`), drawn by the apps with a slosh shader (shaders/fluid.wesl) that keeps its surface
    level as the head moves and sets it rocking."""
    R = 4
    glove, slacks, shoe, belt, buckle = (244, 238, 226), (66, 72, 86), (92, 60, 40), (70, 46, 30), (206, 178, 96)
    button, tee = (238, 232, 216), (240, 238, 232)
    pink, light, fold, deep = (244, 128, 164), (252, 168, 194), (206, 88, 128), (148, 48, 90)
    cereb, cereb_dk, stem = (226, 128, 154), (186, 88, 120), (230, 160, 160)
    frame, lens = (28, 28, 34), WHITE
    glass, glint, fluid_c = (214, 236, 244, 60), (240, 250, 255, 150), (150, 214, 236, 110)
    metal, metal_dk, bolt, copper = (150, 156, 164), (112, 118, 128), (70, 74, 82), (196, 116, 58)

    def torso(t):
        pass

    def arm(side, ua, la):
        pass

    def leg(side, ul, ll):
        ul.paint(lambda x, y, z: True, slacks)
        ll.paint(lambda x, y, z: True, slacks)
        ll.paint(lambda x, y, z: y <= -5, shoe)

    p = standard_body(slacks, torso, arm, leg, glove)

    # ---- the shirt (torso: the same box as the 5 cm one, -0.25..0.25 x 0..0.55 x -0.15..0.20)
    t = Vox(R).box(-20, 20, 0, 44, -12, 16, slacks)
    paint_plaid(t, -20, 20, -12, 16, keep=lambda x, y, z: y < 8)
    t.paint(lambda x, y, z: 6 <= y <= 7, belt)                                       # the belt
    t.box(-2, 2, 5, 9, 16, 17, buckle).cut(-1, 1, 6, 8, 16, 17)
    t.box(-1, 1, 6, 8, 16, 17, belt)
    t.paint(lambda x, y, z: z == 15 and -2 <= x <= 1 and y >= 38, tee)                # the tee in the open neck
    t.paint(lambda x, y, z: z == 15 and x in (-3, 2) and 9 <= y < 38, PLAID_BLACK)    # the placket's seams
    for y in range(12, 37, 7):
        t.box(-1, 1, y, y + 2, 16, 17, button)                                        # buttons
    for y in range(36, 44):                                                           # the collar points
        w = (y - 35) // 2 + 2
        for x in range(-2 - w, -2):
            t.v[(x, y, 16)] = plaid(x + 5, y)
        for x in range(2, 2 + w):
            t.v[(x, y, 16)] = plaid(x + 5, y)
    t.box(-20, 20, 42, 44, -12, -11, PLAID_RED)                                       # the collar at the back
    for x0 in (-15, 5):                                                               # chest pockets, cut on the bias
        for x in range(x0, x0 + 10):
            for y in range(24, 34):
                t.v[(x, y, 16)] = plaid(x + y, y - x + 3)
        t.box(x0, x0 + 10, 32, 35, 16, 17, PLAID_BLACK).box(x0 + 4, x0 + 6, 31, 33, 17, 18, button)
    p["Hips"] = t

    # ---- sleeves (6 voxels each side of the joint: the 5 cm arm's -0.075..0.075)
    for side in ("L", "R"):
        ua = Vox(R).box(-6, 6, -20, 4, -6, 6, PLAID_RED)
        paint_plaid(ua, -6, 6, -6, 6, shift=(3, 0))
        la = Vox(R).box(-6, 6, -20, 0, -6, 6, PLAID_RED)
        paint_plaid(la, -6, 6, -6, 6, shift=(3, 8))
        la.paint(lambda x, y, z: -16 <= y <= -13, PLAID_BLACK)                        # cuffs
        la.paint(lambda x, y, z: y < -16, glove)
        la.box(-1, 1, -15, -14, 6, 7, button)
        p[f"UpperArm_{side}"], p[f"LowerArm_{side}"] = ua, la

    # ---- the jar (head): glass -0.35..0.35 x -0.05..0.55 x -0.30..0.40, base ring, lid
    h = Vox(R).box(-28, 28, -4, 44, -24, 32, glass).cut(-27, 27, -4, 44, -23, 31)
    h.cut(-27, 27, -3, 43, 31, 32)                                                    # no pane over the face
    for y in range(4, 40):                                                            # glints down the front edge
        for x in (-26, -25):
            if (x + y) % 9 < 5:
                h.v[(x, y, 31)] = glint
    for x in range(14, 24):
        h.v[(x, 38 - (x - 14) // 3, 31)] = glint
    h.box(-28, 28, -8, -4, -24, 32, metal).box(-29, 29, -6, -5, -25, 33, metal_dk)    # the base ring and its rim
    h.box(-32, 32, 44, 52, -28, 36, metal).bevel(-32, 32, 44, 52, -28, 36, "top")     # the lid
    h.paint(lambda x, y, z: y in (45, 47) and (x, y, z) in h.v and h.v[(x, y, z)] == metal, metal_dk)  # its threads
    for x, z in ((-26, 34), (25, 34), (-26, -27), (25, -27)):
        h.box(x, x + 2, 48, 50, z, z + 2, bolt)
    h.box(-3, 3, 52, 55, -4, 2, metal_dk).box(-2, 2, 55, 57, -3, 1, bolt)             # a terminal on top

    # ---- the brain: a big flat front (the face kit sits on it, z = 0.30), rounded to the back
    b = Vox(R)
    for x in range(-24, 24):
        for y in range(-4, 41):
            for z in range(-22, 24):
                zc = z + 0.5
                s = 1.0 if zc >= -6 else math.sqrt(max(0.0, 1 - ((zc + 6) / 17.0) ** 2))
                if zc > 21:
                    s *= 0.96
                if s <= 0:
                    continue
                if (abs(x + 0.5) / (23.0 * s)) ** 6 + (abs(y + 0.5 - 18) / (22.0 * s)) ** 6 <= 1:
                    b.v[(x, y, z)] = pink
    # the two halves: a fissure from the back up over the top, stopping short of the face
    for k in list(b.v):
        x, y, z = k
        if x in (-1, 0) and y >= 22 + max(0, z - 10) // 1 and z < 20:
            del b.v[k]
    # the cerebellum: a smaller, finely ridged lobe under the back, and the stem below it
    for x in range(-18, 18):
        for y in range(-4, 14):
            for z in range(-24, -8):
                if (((abs(x + 0.5) - 8) / 10.0) ** 2 + ((y + 0.5 - 5) / 8.0) ** 2 + ((z + 0.5 + 15) / 8.0) ** 2) <= 1:
                    b.v[(x, y, z)] = cereb_dk if y % 3 == 0 else cereb
    b.box(-4, 4, -4, 6, -12, -4, stem)

    def depth(k):
        """0 on the surface, 1 just under it, 2 deeper"""
        x, y, z = k
        for d in range(3):
            if any((x + dx * (d + 1), y + dy * (d + 1), z + dz * (d + 1)) not in b.v for dx, dy, dz in DIRS):
                return d
        return 3

    face = lambda x, y, z: z >= 21 and -21 <= x <= 20 and -4 <= y <= 38
    shell = {k: depth(k) for k in b.v if b.v[k] == pink}
    for k, d in shell.items():
        x, y, z = k
        if d > 1 or face(x, y, z):
            continue
        # gyri and sulci: the folds wind where warped noise crosses its middle
        wx, wy, wz = x / 7.0, y / 7.0, z / 7.0
        n = vnoise3(wx + 1.6 * vnoise3(wx, wy, wz, 7), wy + 1.6 * vnoise3(wx, wy, wz, 11), wz, 3)
        n2 = vnoise3(wx * 2.1, wy * 2.1, wz * 2.1, 5)
        g = abs(n - 0.5) + 0.12 * (n2 - 0.5)
        if g < 0.07:
            if d == 0:
                del b.v[k]
            else:
                b.v[k] = deep
        elif g < 0.11 and d == 0:
            b.v[k] = fold
        elif g > 0.32 and d == 0:
            b.v[k] = light
    # the lateral groove down each side, between the temporal lobe and the rest
    for k in list(b.v):
        x, y, z = k
        if abs(x + 0.5) >= 21.5 and abs(y - (12 + (z + 6) * 0.35)) < 1.0 and -18 < z < 18 and k in shell and shell[k] == 0:
            del b.v[k]
    for k, c in list(b.v.items()):
        x, y, z = k
        if c == pink and shell.get(k, 3) <= 1 and abs(x + 0.5) >= 20.5 and abs(y - (12 + (z + 6) * 0.35)) < 2.0 and -18 < z < 18:
            b.v[k] = deep
    # the face plane: folds drawn on it (colour only: it stays flat for the face kit), kept clear
    # of the eyes, brows and mouth
    kit = lambda x, y: (abs(x + 0.5) < 18.5 and 14 <= y <= 26) or (abs(x + 0.5) < 21 and 27 <= y <= 38) or (abs(x + 0.5) < 11 and y <= 10)
    for k, c in list(b.v.items()):
        x, y, z = k
        if z < 22 or c != pink or kit(x, y):
            continue
        wx, wy = x / 6.0, y / 6.0
        n = vnoise3(wx + 1.6 * vnoise3(wx, wy, 0.5, 7), wy + 1.6 * vnoise3(wx, wy, 0.5, 11), 0.5, 3)
        g = abs(n - 0.5)
        if g < 0.04:
            b.v[k] = fold
        elif g > 0.42:
            b.v[k] = light
    h.v.update(b.v)

    # the fluid: the jar's inside, round the brain, up to just below the lid
    fl = Vox(R)
    for x in range(-27, 27):
        for y in range(-4, 41):
            for z in range(-23, 32):
                if (x, y, z) not in b.v:
                    fl.v[(x, y, z)] = fluid_c
    # electrodes: two copper leads from the lid down into the top of the brain
    for x0 in (-12, 10):
        for y in range(30, 44):
            if (x0, y, -4) not in b.v:
                h.v[(x0, y, -4)] = copper
                h.v[(x0 + 1, y, -4)] = copper
                fl.v.pop((x0, y, -4), None)
                fl.v.pop((x0 + 1, y, -4), None)
    for k in list(fl.v):
        if k in h.v and len(h.v[k]) == 3:
            del fl.v[k]
    h.inner = {(x, y, z) for x in range(-27, 27) for y in range(-4, 44) for z in range(-23, 31)}

    # round glasses: wire rims just proud of the face, white lenses on it, as drawn
    for cx in (-9.5, 9.5):
        for x in range(-24, 24):
            for y in range(8, 32):
                r = math.hypot(x + 0.5 - cx, y + 0.5 - 20)
                if 7.2 <= r < 8.6:
                    h.v[(x, y, 24)] = frame
                elif r < 7.2 and abs(y + 0.5 - 20) < 4.5:
                    h.v[(x, y, 23)] = lens
    for x in range(-2, 2):
        h.v[(x, 22, 24)] = frame                                                      # the bridge
    for x0, x1 in ((-24, -18), (18, 24)):                                             # the arms back to the "ears"
        for x in range(x0, x1):
            h.v[(x, 21, 24)] = frame
    p["Head"] = h

    die = ball(3.0, (204, 40, 52), cy=3.0)
    for k in [(0, 3, 2), (-3, 3, 0), (1, 5, 1), (0, 3, -3), (2, 1, 1), (-1, 5, -2)]:
        if k in die.v:
            die.v[k] = WHITE
    mini = Vox().box(-2, 2, 0, 1, -2, 2, (30, 30, 34)).bevel(-2, 2, 0, 1, -2, 2)     # round base
    mini.box(-1, 1, 1, 3, -1, 1, (66, 84, 128)).box(-2, 2, 3, 5, -1, 1, (66, 84, 128))
    mini.box(-2, -1, 4, 5, -1, 1, (212, 172, 64)).box(1, 2, 4, 5, -1, 1, (212, 172, 64))   # gold pauldrons
    mini.box(-1, 1, 5, 6, -1, 1, (176, 180, 188)).box(2, 3, 1, 7, 0, 1, (176, 180, 188))   # helm and a sword
    props = [
        dict(name="pointer", parent="LowerArm_R", at=GRIP, rot=(0, 0, 0), off=(0.5, 0, 0.5),
             vox=Vox().box(0, 1, -14, 1, 0, 1, (150, 100, 56)).box(0, 1, -15, -14, 0, 1, WHITE)),
        dict(name="d20", parent="LowerArm_R", at=GRIP, rot=(0, 0, 0), off=(0.5, 0, 0.5), vox=die),
        dict(name="mini", parent="LowerArm_L", at=GRIP, rot=(90, 0, 0), off=(0.5, 0, 0.5), vox=mini),
    ]
    return dict(name="brain", label="The Brain", parts=p, fluid={"Head": fl}, glass_alpha=0.22, props=props, mouth=(0, 0.0, 0.0), tail_at=None, tail_sway=None,
                note="The Brain from the comic From the Brain of Michael Collins: a pink brain with round glasses in a jar of sloshing fluid, on a professor in a red plaid flannel shirt.")


# ----------------------------------------------------------------------------------------------
# Own clips
# ----------------------------------------------------------------------------------------------
def osc(t, period, amp, phase=0.0):
    return amp * math.sin(2 * math.pi * (t / period + phase))


def keys_every(duration, fps=30):
    n = max(2, int(round(duration * fps)) + 1)
    return [duration * i / (n - 1) for i in range(n)]


def face(eyes="eyes_open", brows="brows_normal", mouth="mouth_flat"):
    return (eyes, brows, mouth)


def env(t, a, b, ramp=0.15):
    """0 outside [a,b], easing up/down over `ramp` seconds"""
    if t <= a or t >= b:
        return 0.0
    k = min(1.0, (t - a) / ramp, (b - t) / ramp)
    return k * k * (3 - 2 * k)


def clips_for(name):
    """Each clip: (name, duration, pose(t) -> {joint: (x,y,z) degrees, '_hip': (x,y,z) metres offset, 'Tail': deg}, face(t))"""
    C = []
    if name in ("dog", "blue"):
        C.append(("Tail wag", 1.2, lambda t: {"Hips": (0, 0, osc(t, 0.4, 4)), "Head": (0, 0, 10 + osc(t, 0.4, -4)), "Tail": osc(t, 0.2, 45),
                                              "_hip": (0, abs(osc(t, 0.4, 0.015)), 0)},
                  lambda t: face("eyes_open", "brows_raised", "mouth_tongue")))
        C.append(("Bark", 1.0, lambda t: {"Head": (-18 * (env(t, 0.05, 0.35, 0.08) + env(t, 0.45, 0.8, 0.08)), 0, 0),
                                          "Hips": (-5 * (env(t, 0.05, 0.35, 0.08) + env(t, 0.45, 0.8, 0.08)), 0, 0), "Tail": osc(t, 0.25, 20)},
                  lambda t: face("eyes_wide", "brows_raised", "mouth_open" if (0.1 < t < 0.3 or 0.5 < t < 0.72) else "mouth_flat")))
        def beg(t):
            k = env(t, 0.0, 1.6, 0.3)
            bob = osc(t, 0.5, 4) * k
            return {"UpperLeg_L": (-60 * k, 0, 0), "UpperLeg_R": (-60 * k, 0, 0), "LowerLeg_L": (90 * k, 0, 0), "LowerLeg_R": (90 * k, 0, 0),
                    "UpperArm_L": (-80 * k + bob, 0, 6), "UpperArm_R": (-80 * k + bob, 0, -6), "LowerArm_L": (-14 - 70 * k, 0, 0), "LowerArm_R": (-14 - 70 * k, 0, 0),
                    "Head": (-12 * k, 0, 8 * k), "_hip": (0, -0.165 * k, 0), "Tail": osc(t, 0.3, 30)}
        C.append(("Beg", 1.6, beg, lambda t: face("eyes_up", "brows_worried", "mouth_pout")))
        def scratch(t):
            k = env(t, 0.0, 1.4, 0.25)
            return {"UpperArm_R": (-30 * k, 0, -150 * k), "LowerArm_R": (-14 - (40 + osc(t, 0.16, 25)) * k, 0, 0), "Head": (0, 0, 18 * k),
                    "UpperLeg_L": (-20 * k + osc(t, 0.16, 10) * k, 0, 0), "Tail": osc(t, 0.5, 15)}
        C.append(("Scratch", 1.4, scratch, lambda t: face("eyes_squint", "brows_normal", "mouth_tongue")))
    elif name == "cat":
        def stretch(t):
            k = env(t, 0.0, 2.0, 0.6)
            return {"UpperArm_L": (-165 * k, 0, 6), "UpperArm_R": (-165 * k, 0, -6), "LowerArm_L": (-14 + 10 * k, 0, 0), "LowerArm_R": (-14 + 10 * k, 0, 0),
                    "Hips": (-8 * k, 0, 0), "Head": (-18 * k, 0, 0), "_hip": (0, 0.02 * k, 0), "Tail": 25 * k}
        C.append(("Stretch", 2.0, stretch, lambda t: face("eyes_shut", "brows_raised", "mouth_o" if 0.5 < t < 1.6 else "mouth_flat")))
        def hiss(t):
            k = env(t, 0.0, 1.2, 0.15)
            sh = osc(t, 0.07, 1.5) * k
            return {"Hips": (-10 * k, 0, sh), "UpperArm_L": (-75 * k, 0, 25 * k), "UpperArm_R": (-75 * k, 0, -25 * k), "LowerArm_L": (-14 - 50 * k, 0, 0),
                    "LowerArm_R": (-14 - 50 * k, 0, 0), "Head": (12 * k, 0, 0), "UpperLeg_L": (-15 * k, 0, 8 * k), "UpperLeg_R": (-15 * k, 0, -8 * k),
                    "LowerLeg_L": (30 * k, 0, 0), "LowerLeg_R": (30 * k, 0, 0), "_hip": (0, -0.04 * k, 0), "Tail": -35 * k}
        C.append(("Hiss", 1.2, hiss, lambda t: face("eyes_wide", "brows_angry", "mouth_open" if 0.15 < t < 1.0 else "mouth_grit")))
        def knock(t):
            reach = env(t, 0.0, 1.4, 0.4)
            swipe = env(t, 0.75, 1.05, 0.08)
            return {"UpperArm_R": (-80 * reach, 25 * swipe, -45 * swipe), "LowerArm_R": (-14 - 10 * reach, 0, 0), "Head": (10 * reach, -20 * reach + 35 * swipe, 0),
                    "Hips": (6 * reach, 0, 0), "Tail": osc(t, 0.7, 12)}
        C.append(("Knock it over", 1.4, knock, lambda t: face("eyes_squint", "brows_angry" if t < 0.75 else "brows_normal", "mouth_flat" if t < 1.05 else "mouth_smirk")))
        def groom(t):
            k = env(t, 0.0, 1.6, 0.35)
            lick = osc(t, 0.3, 8) * k
            return {"UpperArm_L": (-55 * k, 0, 6 - 25 * k), "LowerArm_L": (-14 - 105 * k + lick, -30 * k, 0), "Head": (15 * k, -15 * k, -8 * k), "Tail": 10 * k}
        C.append(("Groom", 1.6, groom, lambda t: face("eyes_shut", "brows_normal", "mouth_tongue" if (t % 0.3) < 0.15 and 0.3 < t < 1.3 else "mouth_flat")))
    elif name == "rhino":
        def charge(t):
            s = osc(t, 0.6, 1)
            return {"Hips": (24, 0, 0), "Head": (14, 0, 0), "UpperLeg_L": (-45 * s - 10, 0, 0), "UpperLeg_R": (45 * s - 10, 0, 0),
                    "LowerLeg_L": (35 + 30 * max(0, s), 0, 0), "LowerLeg_R": (35 + 30 * max(0, -s), 0, 0), "UpperArm_L": (40 * s, 0, 10), "UpperArm_R": (-40 * s, 0, -10),
                    "LowerArm_L": (-70, 0, 0), "LowerArm_R": (-70, 0, 0), "_hip": (0, -0.04 + abs(osc(t, 0.6, 0.03)), 0.05), "Tail": osc(t, 0.3, 20)}
        C.append(("Charge", 1.2, charge, lambda t: face("eyes_open", "brows_angry", "mouth_grit")))
        def toss(t):
            down = env(t, 0.05, 0.45, 0.15)
            up = env(t, 0.45, 0.95, 0.12)
            return {"Head": (32 * down - 40 * up, 0, 0), "Hips": (14 * down - 8 * up, 0, 0), "UpperArm_L": (-20 * up, 0, 6 + 30 * up), "UpperArm_R": (-20 * up, 0, -6 - 30 * up),
                    "_hip": (0, 0.03 * up, 0), "Tail": 30 * up}
        C.append(("Horn toss", 1.2, toss, lambda t: face("eyes_squint" if t < 0.45 else "eyes_wide", "brows_angry", "mouth_grit" if t < 0.45 else "mouth_open")))
        def stomp(t):
            r = env(t, 0.0, 0.55, 0.2)
            l = env(t, 0.5, 1.05, 0.2)
            slam = max(env(t, 0.42, 0.55, 0.04), env(t, 0.92, 1.05, 0.04))
            return {"UpperLeg_R": (-55 * r, 0, 0), "LowerLeg_R": (65 * r, 0, 0), "UpperLeg_L": (-55 * l, 0, 0), "LowerLeg_L": (65 * l, 0, 0),
                    "Hips": (5, 0, 6 * r - 6 * l), "UpperArm_L": (-30, 0, 20), "UpperArm_R": (-30, 0, -20), "LowerArm_L": (-60, 0, 0), "LowerArm_R": (-60, 0, 0),
                    "Head": (8 + 6 * slam, 0, 0), "_hip": (0, -0.025 * slam, 0)}
        C.append(("Stomp", 1.1, stomp, lambda t: face("eyes_open", "brows_angry", "mouth_grit")))
        def snort(t):
            j = env(t, 0.1, 0.3, 0.05) + env(t, 0.5, 0.7, 0.05)
            return {"Head": (-14 * j, 0, 0), "UpperArm_L": (4, 0, 6 + 8 * j), "UpperArm_R": (4, 0, -6 - 8 * j), "Hips": (-3 * j, 0, 0), "Tail": osc(t, 0.2, 15)}
        C.append(("Snort", 1.0, snort, lambda t: face("eyes_squint", "brows_angry", "mouth_pout" if (0.1 < t < 0.3 or 0.5 < t < 0.7) else "mouth_flat")))
    elif name == "donkey":
        def heehaw(t):
            a = env(t, 0.1, 0.55, 0.12)
            b = env(t, 0.65, 1.15, 0.12)
            return {"Head": (-38 * a - 38 * b + 10 * env(t, 0.55, 0.65, 0.04), 0, 0), "Hips": (-6 * (a + b), 0, 0), "UpperArm_L": (0, 0, 6 + 25 * (a + b)),
                    "UpperArm_R": (0, 0, -6 - 25 * (a + b)), "Tail": osc(t, 0.4, 20)}
        C.append(("Hee-haw", 1.4, heehaw, lambda t: face("eyes_shut", "brows_raised", "mouth_open" if (0.15 < t < 0.5 or 0.7 < t < 1.1) else "mouth_o")))
        def kick(t):
            lean = env(t, 0.05, 1.05, 0.25)
            k = env(t, 0.35, 0.75, 0.08)
            return {"Hips": (38 * lean, 0, 0), "Head": (-25 * lean, 0, 0), "UpperArm_L": (-50 * lean, 0, 6), "UpperArm_R": (-50 * lean, 0, -6),
                    "UpperLeg_L": (-35 * lean, 0, 0), "LowerLeg_L": (25 * lean, 0, 0), "UpperLeg_R": (-35 * lean + 95 * k, 0, 0), "LowerLeg_R": (25 * lean - 20 * k, 0, 0),
                    "_hip": (0, -0.03 * lean, -0.03 * lean), "Tail": 40 * k}
        C.append(("Back kick", 1.2, kick, lambda t: face("eyes_squint", "brows_angry", "mouth_grit")))
        def stubborn(t):
            k = env(t, 0.0, 2.0, 0.3)
            return {"UpperArm_L": (-25 * k, 0, 6 - 12 * k), "UpperArm_R": (-25 * k, 0, -6 + 12 * k), "LowerArm_L": (-14 - 80 * k, -55 * k, 0), "LowerArm_R": (-14 - 80 * k, 55 * k, 0),
                    "Head": (0, osc(t, 0.5, 28) * env(t, 0.4, 1.7, 0.2), 0), "Hips": (-4 * k, 0, 0), "Tail": osc(t, 0.6, 12)}
        C.append(("Stubborn", 2.0, stubborn, lambda t: face("eyes_squint", "brows_angry", "mouth_frown")))
        def munch(t):
            k = env(t, 0.0, 1.6, 0.3)
            return {"UpperArm_R": (-60 * k, 0, -6 + 18 * k), "LowerArm_R": (-14 - 85 * k, 25 * k, 0), "Head": (6 * k + osc(t, 0.25, 3) * k, 10 * k, 0), "Tail": osc(t, 0.8, 10)}
        C.append(("Munch", 1.6, munch, lambda t: face("eyes_shut", "brows_raised", "mouth_grit" if (t % 0.25) < 0.125 and 0.3 < t < 1.3 else "mouth_grin")))
    elif name == "capybara":
        def sit(k):
            """sitting back on its haunches, legs forward"""
            return {"UpperLeg_L": (-85 * k, 0, 6 * k), "UpperLeg_R": (-85 * k, 0, -6 * k), "LowerLeg_L": (80 * k, 0, 0), "LowerLeg_R": (80 * k, 0, 0),
                    "_hip": (0, -0.26 * k, -0.04 * k)}

        def chill(t):
            k = env(t, 0.0, 3.0, 0.5)
            breathe = osc(t, 1.5, 2) * k
            pz = sit(k)
            pz.update({"Hips": (-6 * k + breathe, 0, 0), "UpperArm_L": (-30 * k, 0, 6 + 4 * k), "UpperArm_R": (-30 * k, 0, -6 - 4 * k),
                       "LowerArm_L": (-14 - 30 * k, 0, 0), "LowerArm_R": (-14 - 30 * k, 0, 0), "Head": (4 * k - breathe, 0, 5 * k)})
            return pz
        C.append(("Chill", 3.0, chill, lambda t: face("eyes_squint" if 0.4 < t < 2.7 else "eyes_open", "brows_normal", "mouth_smirk" if 0.4 < t < 2.7 else "mouth_flat")))

        def spring(t):
            k = env(t, 0.0, 3.2, 0.6)
            bob = osc(t, 1.6, 0.012) * k
            return {"_hip": (0, -0.34 * k + bob, 0), "UpperLeg_L": (-70 * k, 0, 10 * k), "UpperLeg_R": (-70 * k, 0, -10 * k), "LowerLeg_L": (70 * k, 0, 0),
                    "LowerLeg_R": (70 * k, 0, 0), "UpperArm_L": (-10 * k, 0, 6 + 55 * k), "UpperArm_R": (-10 * k, 0, -6 - 55 * k), "LowerArm_L": (-14 - 10 * k, 0, 0),
                    "LowerArm_R": (-14 - 10 * k, 0, 0), "Hips": (-10 * k, 0, 0), "Head": (-8 * k, 0, osc(t, 3.2, 4) * k)}
        C.append(("Hot spring", 3.2, spring, lambda t: face("eyes_shut" if 0.5 < t < 2.9 else "eyes_squint", "brows_raised", "mouth_smirk"), lambda t: {"towel"}))

        def yuzu(t):
            k = env(t, 0.0, 2.4, 0.3)
            wob = osc(t, 0.6, 3) * env(t, 0.4, 1.6, 0.2)
            return {"Head": (-2 * k, 0, wob), "Hips": (0, 0, -wob * 0.6), "UpperArm_L": (-10 * k, 0, 6 + 35 * k + wob), "UpperArm_R": (-10 * k, 0, -6 - 35 * k + wob),
                    "LowerArm_L": (-14 - 10 * k, 0, 0), "LowerArm_R": (-14 - 10 * k, 0, 0)}
        C.append(("Yuzu balance", 2.4, yuzu, lambda t: face("eyes_up" if t < 1.6 else "eyes_squint", "brows_raised" if t < 1.6 else "brows_normal",
                                                             "mouth_o" if t < 1.6 else "mouth_smirk"), lambda t: {"yuzu"}))

        def snack(t):
            k = env(t, 0.0, 2.2, 0.35)
            chew = osc(t, 0.3, 4) * env(t, 0.6, 1.9, 0.1)
            return {"UpperArm_R": (-62 * k, 0, -6 + 26 * k), "LowerArm_R": (-14 - 58 * k, 0, 0), "Head": (6 * k + chew, -10 * k, 0), "Hips": (-3 * k, 0, 0)}
        C.append(("Melon snack", 2.2, snack, lambda t: face("eyes_open" if t < 0.6 else "eyes_shut", "brows_raised",
                                                              ("mouth_open" if (t % 0.3) < 0.15 else "mouth_grit") if 0.6 < t < 1.9 else "mouth_grin"), lambda t: {"melon"}))

        def nod(t):
            droop = min(1.0, max(0.0, (t - 0.2) / 1.4))
            droop = droop * droop * (3 - 2 * droop)
            jerk = env(t, 1.6, 1.85, 0.05)
            d = droop * (1.0 - min(1.0, max(0.0, (t - 1.6) / 0.15)))
            return {"Head": (32 * d - 10 * jerk, 0, 6 * d), "Hips": (6 * d, 0, 0), "UpperArm_L": (0, 0, 6 - 4 * d), "UpperArm_R": (0, 0, -6 + 4 * d),
                    "_hip": (0, 0.02 * jerk - 0.02 * d, 0)}
        C.append(("Nod off", 2.4, nod, lambda t: face("eyes_squint" if t < 0.9 else ("eyes_shut" if t < 1.6 else "eyes_wide"), "brows_normal" if t < 1.6 else "brows_raised",
                                                      "mouth_flat" if t < 1.6 else ("mouth_o" if t < 2.0 else "mouth_flat"))))
    elif name == "brain":
        def actually(t):
            k = env(t, 0.0, 2.0, 0.25)
            wag = osc(t, 0.4, 10) * env(t, 0.4, 1.7, 0.1)
            return {"UpperArm_R": (-140 * k, 0, -6 - 10 * k), "LowerArm_R": (-14 - 20 * k + wag, 0, 0), "Head": (-6 * k, 0, 8 * k), "Hips": (-3 * k, 0, 0)}
        C.append(("Well, actually", 2.0, actually, lambda t: face("eyes_open" if t < 1.6 else "eyes_squint", "brows_raised",
                                                                  ("mouth_open" if (t % 0.24) < 0.12 else "mouth_smirk") if 0.3 < t < 1.6 else "mouth_smirk"), lambda t: {"pointer"}))

        def roll(t):
            if t < 0.9:
                k = min(1.0, t / 0.25)
                s = osc(t, 0.15, 12) * k
                ua, la = -50 * k, -14 - 70 * k + s
            else:
                u = min(1.0, (t - 0.9) / 0.2)
                back = 1.0 - min(1.0, max(0.0, (t - 1.3) / 0.5))
                ua, la = (-50 - 40 * u) * back, (-84 + 70 * u) * back - 14 * (1 - back)
            look = env(t, 1.0, 1.8, 0.2)
            return {"UpperArm_R": (ua, 0, -6 + 18 * min(1.0, t / 0.25) * (1.0 if t < 1.3 else max(0.0, 1 - (t - 1.3) / 0.5))), "LowerArm_R": (la, 0, 0),
                    "Head": (16 * look, 0, 0), "Hips": (8 * look, 0, 0)}
        C.append(("Roll for initiative", 1.8, roll, lambda t: face("eyes_squint" if t < 0.9 else ("eyes_wide" if t < 1.2 else "eyes_open"),
                                                                     "brows_angry" if t < 0.9 else "brows_raised",
                                                                     "mouth_grit" if t < 0.9 else ("mouth_o" if t < 1.2 else "mouth_grin")), lambda t: {"d20"} if t < 1.0 else set()))

        def lore(t):
            k = env(t, 0.0, 2.4, 0.3)
            g = env(t, 0.4, 2.1, 0.15)
            return {"UpperArm_L": (-70 * k, 0, 6 - 20 * k), "LowerArm_L": (-14 - 60 * k, 0, 0), "Head": (-4 * k, 0, -6 * k),
                    "UpperArm_R": (-30 * k + osc(t, 0.5, 14) * g, 0, -6 - 20 * k), "LowerArm_R": (-14 - 50 * k, 0, 0)}
        C.append(("Lore dump", 2.4, lore, lambda t: face("eyes_open", "brows_raised",
                                                        ("mouth_open" if (t % 0.2) < 0.1 else "mouth_o") if 0.4 < t < 2.1 else "mouth_grin"), lambda t: {"mini"}))

        def think(t):
            k = env(t, 0.0, 1.5, 0.25)
            e = env(t, 1.35, 2.4, 0.15)
            return {"UpperArm_R": (-40 * k - 150 * e, 0, -6 + 30 * k - 4 * e), "LowerArm_R": (-14 - 120 * k, 0, 0),
                    "UpperArm_L": (-20 * k, 0, 6 - 10 * k), "LowerArm_L": (-14 - 80 * k, 0, 0),
                    "Head": (-10 * k - 6 * e, 0, 6 * k), "_hip": (0, 0.06 * env(t, 1.35, 1.75, 0.1), 0)}
        C.append(("Big think", 2.4, think, lambda t: face("eyes_up" if t < 1.4 else "eyes_wide", "brows_worried" if t < 1.4 else "brows_raised",
                                                         "mouth_pout" if t < 1.4 else "mouth_grin")))

        def check(t):
            k = env(t, 0.0, 2.0, 0.25)
            g = env(t, 0.3, 1.7, 0.1)
            return {"UpperArm_R": (-70 * k, 0, -6 + 10 * k), "LowerArm_R": (-14 - 60 * k, 0, osc(t, 0.3, 18) * g),
                    "Head": (4 * k, osc(t, 0.35, 14) * g, 0), "Hips": (2 * k, 0, 0)}
        C.append(("Fact check", 2.0, check, lambda t: face("eyes_squint", "brows_angry", "mouth_frown" if (t % 0.7) < 0.5 else "mouth_wavy")))
    elif name == "peanut":
        def tip(t):
            k = env(t, 0.05, 1.15, 0.25)
            bow = env(t, 0.4, 0.9, 0.15)
            return {"UpperArm_R": (-150 * k, 0, -20 * k), "LowerArm_R": (-14 - 40 * k, 0, 0), "Head": (18 * bow, 0, 0), "Hips": (10 * bow, 0, 0),
                    "UpperArm_L": (4, 0, 6 + 10 * bow)}
        C.append(("Tip cap", 1.2, tip, lambda t: face("eyes_open" if t < 0.4 else "eyes_shut", "brows_raised", "mouth_grin")))
        def flip(t):
            u = min(1.0, max(0.0, (t - 0.15) / 0.7))
            s = u * u * (3 - 2 * u)
            tuck = env(t, 0.1, 0.95, 0.12)
            return {"Hips": (-360 * s, 0, 0), "UpperLeg_L": (-90 * tuck, 0, 0), "UpperLeg_R": (-90 * tuck, 0, 0), "LowerLeg_L": (120 * tuck, 0, 0),
                    "LowerLeg_R": (120 * tuck, 0, 0), "UpperArm_L": (-60 * tuck, 0, 6), "UpperArm_R": (-60 * tuck, 0, -6), "_hip": (0, 0.55 * math.sin(math.pi * s), 0)}
        C.append(("Backflip", 1.1, flip, lambda t: face("eyes_wide" if 0.15 < t < 0.85 else "eyes_open", "brows_raised", "mouth_o" if 0.15 < t < 0.85 else "mouth_grin")))
        def wobble(t):
            w = osc(t, 0.5, 12)
            return {"Hips": (0, 0, w), "Head": (0, 0, -w * 1.3), "UpperArm_L": (0, 0, 45 - w), "UpperArm_R": (0, 0, -45 - w), "UpperLeg_L": (0, 0, -w * 0.6),
                    "UpperLeg_R": (0, 0, -w * 0.6), "_hip": (0, abs(osc(t, 0.5, 0.02)), 0)}
        C.append(("Wobble", 1.0, wobble, lambda t: face("eyes_open", "brows_raised", "mouth_grin")))
        def shock(t):
            j = env(t, 0.0, 0.35, 0.08)
            k = env(t, 0.05, 1.2, 0.12)
            return {"UpperArm_L": (0, 0, 6 + 150 * k), "UpperArm_R": (0, 0, -6 - 150 * k), "Hips": (-6 * k, 0, osc(t, 0.06, 2) * env(t, 0.35, 1.1, 0.1)),
                    "_hip": (0, 0.12 * j, 0), "Head": (-8 * k, 0, 0)}
        C.append(("Startled", 1.2, shock, lambda t: face("eyes_wide", "brows_raised", "mouth_o")))
    return C


# ----------------------------------------------------------------------------------------------
# Build one character
# ----------------------------------------------------------------------------------------------
def build(spec, j0, bin0, pal_img):
    j = json.loads(json.dumps(j0))
    out = Out()
    pal = Palette(pal_img)
    names = [n["name"] for n in j["nodes"]]
    nid = {n: i for i, n in enumerate(names)}
    parts = spec["parts"]
    hand = parts.pop("_hand")

    # ---- meshes: rebuild every mesh; body parts from voxels, kept parts re-emitted with remapped UVs
    meshes = []
    mesh_of_node = {}

    def prim(p, n, u, ix, mat=0):
        a = {"POSITION": out.acc(p, "VEC3", target=34962, minmax=True), "NORMAL": out.acc(n, "VEC3", target=34962), "TEXCOORD_0": out.acc(u, "VEC2", target=34962)}
        return {"attributes": a, "indices": out.acc(ix, "SCALAR", 5125, target=34963), "material": mat}

    def emit(name, p, n, u, ix):
        meshes.append({"name": name + "_mesh", "primitives": [prim(p, n, u, ix)]})
        return len(meshes) - 1

    def emit_vox(name, vox, off=(0, 0, 0)):
        prims = []
        for tr in (False, True):
            p, n, u, ix = mesh_vox(vox, pal, tr)
            if len(p):
                prims.append(prim(p + np.array(off, "f4") * V, n, u, ix, 1 if tr else 0))
        meshes.append({"name": name + "_mesh", "primitives": prims})
        return len(meshes) - 1

    fluids = spec.get("fluid", {})
    if fluids:
        # material 2: see-through like the glass, drawn by the apps' slosh shader (by this name)
        fl = json.loads(json.dumps(j["materials"][1]))
        fl["name"] = "VoxelFluid"
        j["materials"].append(fl)
    if "glass_alpha" in spec:
        # a big pane to look through (the Brain's jar) wants clearer glass than a visor
        j["materials"][1]["pbrMetallicRoughness"]["baseColorFactor"][3] = spec["glass_alpha"]
    for i, node in enumerate(j["nodes"]):
        if "mesh" not in node:
            continue
        nm = node["name"]
        if nm in parts:
            # the half-voxel shift of the arms is for 5 cm parts; a finer part is built centred
            mesh_of_node[i] = emit_vox(nm, parts[nm], OFFSETS.get(nm, (0, 0, 0)) if parts[nm].res == 1 else (0, 0, 0))
            if nm in fluids:
                p, n, u, ix = mesh_fluid(fluids[nm], parts[nm], pal)
                meshes[-1]["primitives"].append(prim(p, n, u, ix, 2))
        elif nm in ("hand_palm", "hand_point", "hand_thumb"):
            pr = j0["meshes"][node["mesh"]]["primitives"][0]
            a = j0["accessors"][pr["attributes"]["POSITION"]]
            p, n, u, ix = mesh_box(a["min"], a["max"], hand, pal)
            mesh_of_node[i] = emit(nm, p, n, u, ix)
        else:
            pr = j0["meshes"][node["mesh"]]["primitives"][0]
            p = read_acc(j0, bin0, pr["attributes"]["POSITION"])
            n = read_acc(j0, bin0, pr["attributes"]["NORMAL"])
            u = read_acc(j0, bin0, pr["attributes"]["TEXCOORD_0"])
            u[:, 1] *= 0.5                                       # palette grew from 8 to 16 rows
            ix = read_acc(j0, bin0, pr["indices"]).astype("u4").ravel()
            mesh_of_node[i] = emit(nm, p, n, u, ix)
            meshes[-1]["primitives"][0]["material"] = pr.get("material", 0)
    for i, m in mesh_of_node.items():
        j["nodes"][i]["mesh"] = m

    # ---- face: mouth onto the snout
    mx, my, mz = spec["mouth"]
    for nm in FACE_PARTS + ["paint"]:
        if nm.startswith("mouth_"):
            j["nodes"][nid[nm]]["translation"] = [mx, my, mz]

    # ---- tail joint
    tail_idx = None
    if spec["tail_at"] is not None:
        tm = emit_vox("Tail", parts["Tail"])
        j["nodes"].append({"name": "Tail", "translation": list(spec["tail_at"]), "mesh": tm})
        tail_idx = len(j["nodes"]) - 1
        j["nodes"][nid["Hips"]]["children"].append(tail_idx)

    # ---- props: child nodes of a joint, shown only by the clips that use them
    prop_idx = {}
    for pr in spec.get("props", []):
        m = emit_vox("prop_" + pr["name"], pr["vox"], pr.get("off", (0, 0, 0)))
        node = {"name": "prop_" + pr["name"], "translation": list(pr["at"]), "rotation": [float(x) for x in euler(*pr.get("rot", (0, 0, 0)))], "scale": [0.0, 0.0, 0.0], "mesh": m}
        j["nodes"].append(node)
        prop_idx[pr["name"]] = len(j["nodes"]) - 1
        j["nodes"][nid[pr["parent"]]].setdefault("children", []).append(prop_idx[pr["name"]])

    j["meshes"] = meshes

    # ---- animations: copy all, add the tail channel, then append own clips
    anims = []
    sway = spec["tail_sway"]

    def tail_q(deg):
        ax = sway["axis"] if sway else "y"
        base = euler(x=-10)                                   # tail cocked up a little at rest
        return qmul(base, euler(**{ax: deg}))

    for a in j0["animations"]:
        samplers, channels = [], []
        dur = 0.0
        for ch in a["channels"]:
            s = a["samplers"][ch["sampler"]]
            t = read_acc(j0, bin0, s["input"]).ravel()
            v = read_acc(j0, bin0, s["output"])
            dur = max(dur, float(t[-1]))
            kind = {3: "VEC3", 4: "VEC4"}[v.shape[1]]
            samplers.append({"input": out.acc(t, "SCALAR", minmax=True), "output": out.acc(v, kind), "interpolation": s["interpolation"]})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": ch["target"]["node"], "path": ch["target"]["path"]}})
        if tail_idx is not None:
            cycles = max(1, round(dur / sway["period"]))
            per = dur / cycles if dur > 0 else sway["period"]
            ts = [per * k / 8 for k in range(cycles * 8 + 1)]
            qs = [tail_q(osc(tt, per, sway["amp"])) for tt in ts]
            samplers.append({"input": out.acc(np.array(ts, "f4"), "SCALAR", minmax=True), "output": out.acc(np.array(qs, "f4"), "VEC4"), "interpolation": "LINEAR"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": tail_idx, "path": "rotation"}})
        for pi in prop_idx.values():
            samplers.append({"input": out.acc(np.array([0.0, max(dur, 0.01)], "f4"), "SCALAR", minmax=True), "output": out.acc(np.zeros((2, 3), "f4"), "VEC3"), "interpolation": "STEP"})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": pi, "path": "scale"}})
        anims.append({"name": a["name"], "samplers": samplers, "channels": channels})

    own = spec["clips"] if "clips" in spec else clips_for(spec["name"])
    for clip in own:
        cname, dur, pose, facefn = clip[:4]
        propfn = clip[4] if len(clip) > 4 else (lambda t: ())
        ts = keys_every(dur)
        samplers, channels = [], []

        def add(node, path, vals, interp="LINEAR", times=ts):
            kind = {3: "VEC3", 4: "VEC4"}[len(vals[0])]
            samplers.append({"input": out.acc(np.array(times, "f4"), "SCALAR", minmax=True), "output": out.acc(np.array(vals, "f4"), kind), "interpolation": interp})
            channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path}})

        poses = [pose(t) for t in ts]
        for jn in JOINTS:
            vals = []
            for pz in poses:
                r = REST[jn]
                d = pz.get(jn, (0, 0, 0))
                vals.append(euler(r[0] + d[0], r[1] + d[1], r[2] + d[2]) if jn in pz else euler(*r))
            add(nid[jn], "rotation", vals)
        add(nid["Hips"], "translation", [(pz.get("_hip", (0, 0, 0))[0], HIP_Y + pz.get("_hip", (0, 0, 0))[1], pz.get("_hip", (0, 0, 0))[2]) for pz in poses])
        if tail_idx is not None:
            add(tail_idx, "rotation", [tail_q(pz.get("Tail", 0.0)) for pz in poses])
        add(nid["Rig"], "scale", [(1, 1, 1), (1, 1, 1)], "STEP", [0.0, dur])
        # face parts and hidden extras: STEP keys wherever the face changes
        fts, fstates = [], []
        for t in ts:
            f = facefn(t)
            if not fstates or f != fstates[-1]:
                fts.append(t)
                fstates.append(f)
        if fts[-1] != ts[-1]:
            fts.append(ts[-1])
            fstates.append(fstates[-1])
        for part in FACE_PARTS:
            add(nid[part], "scale", [(1, 1, 1) if part in st else (0, 0, 0) for st in fstates], "STEP", fts)
        for part in HIDDEN_PARTS:
            if any(part in propfn(t) for t in ts):
                pts = [t for i, t in enumerate(ts) if i == 0 or (part in propfn(t)) != (part in propfn(ts[i - 1]))] + [ts[-1]]
                add(nid[part], "scale", [(1, 1, 1) if part in propfn(t) else (0, 0, 0) for t in pts], "STEP", pts)
            else:
                add(nid[part], "scale", [(0, 0, 0), (0, 0, 0)], "STEP", [0.0, dur])
        for pname, pi in prop_idx.items():
            pts, pvals = [], []
            for t in ts:
                on = pname in propfn(t)
                if not pvals or on != pvals[-1]:
                    pts.append(t)
                    pvals.append(on)
            if pts[-1] != ts[-1]:
                pts.append(ts[-1])
                pvals.append(pvals[-1])
            add(pi, "scale", [(1, 1, 1) if on else (0, 0, 0) for on in pvals], "STEP", pts)
        anims.append({"name": cname, "samplers": samplers, "channels": channels})
    j["animations"] = anims

    # ---- palette image
    buf = io.BytesIO()
    pal.img.save(buf, "PNG")
    img_view = out.view(buf.getvalue())
    j["images"] = [{"bufferView": img_view, "mimeType": "image/png", "name": "palette"}]

    j["accessors"] = out.accs
    j["bufferViews"] = out.views
    while len(out.bin) % 4:
        out.bin.append(0)
    j["buffers"] = [{"byteLength": len(out.bin)}]
    j["asset"] = {"version": "2.0", "generator": "block-avatar build_animals.py"}
    j["scenes"][0]["name"] = spec["label"]
    js = json.dumps(j, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    glb = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(js) + 8 + len(out.bin))
    glb += struct.pack("<II", len(js), 0x4E4F534A) + js + struct.pack("<II", len(out.bin), 0x004E4942) + bytes(out.bin)
    return glb, [c[0] for c in own], pal


# ----------------------------------------------------------------------------------------------
# .vox (MagicaVoxel) rest pose
# ----------------------------------------------------------------------------------------------
JOINT_POS = {"Hips": (0, 0.55, 0), "Head": (0, 1.10, 0), "UpperArm_L": (0.325, 1.05, 0.025), "LowerArm_L": (0.325, 0.80, 0.025),
             "UpperArm_R": (-0.325, 1.05, 0.025), "LowerArm_R": (-0.325, 0.80, 0.025), "UpperLeg_L": (0.15, 0.55, 0), "LowerLeg_L": (0.15, 0.30, 0),
             "UpperLeg_R": (-0.15, 0.55, 0), "LowerLeg_R": (-0.15, 0.30, 0)}


def write_vox(spec, path):
    grid = {}
    solid = set()
    for nm, vox in spec["parts"].items():
        if nm == "Tail":
            base = (spec["tail_at"][0], 0.55 + spec["tail_at"][1], spec["tail_at"][2])
        elif nm in JOINT_POS:
            base = JOINT_POS[nm]
        else:
            continue
        off = OFFSETS.get(nm, (0, 0, 0))
        r = vox.res
        if r != 1:
            # the MagicaVoxel source stays on the 5 cm grid: a finer part is sampled down to it
            off = (0, 0, 0)
        for (x, y, z), c in vox.v.items():
            x, y, z = x / r, y / r, z / r
            key = (int(math.floor(x + off[0] + base[0] / V + 1e-6)), int(round(y + base[1] / V)) if r == 1 else int(math.floor(y + base[1] / V + 1e-6)), int(math.floor(z + off[2] + base[2] / V + 1e-6)))
            if r != 1 and key in grid and (len(c) == 4 or key in solid):
                continue                                     # a cell's first opaque colour wins
            grid[key] = tuple(c[:3])
            if len(c) == 3:
                solid.add(key)
    xs = [k[0] for k in grid]; ys = [k[1] for k in grid]; zs = [k[2] for k in grid]
    x0, y0, z0 = min(xs), min(ys), min(zs)
    sx, sy, sz = max(xs) - x0 + 1, max(ys) - y0 + 1, max(zs) - z0 + 1
    colors = sorted(set(grid.values()))
    assert len(colors) <= 255
    cidx = {c: i + 1 for i, c in enumerate(colors)}
    # MagicaVoxel is z-up: (x, -z, y)
    xyzi = b"".join(struct.pack("<BBBB", x - x0, (sz - 1) - (z - z0), y - y0, cidx[c]) for (x, y, z), c in grid.items())
    size = struct.pack("<iii", sx, sz, sy)
    rgba = b"".join(struct.pack("<BBBB", *c, 255) for c in colors) + b"\0\0\0\0" * (256 - len(colors))

    def chunk(tag, content):
        return tag + struct.pack("<ii", len(content), 0) + content

    body = chunk(b"SIZE", size) + chunk(b"XYZI", struct.pack("<i", len(grid)) + xyzi) + chunk(b"RGBA", rgba)
    path.write_bytes(b"VOX " + struct.pack("<i", 150) + b"MAIN" + struct.pack("<ii", 0, len(body)) + body)


def main():
    j0, bin0 = load_glb(TEMPLATE)
    iv = j0["bufferViews"][j0["images"][0]["bufferView"]]
    pal_img = Image.open(io.BytesIO(bin0[iv.get("byteOffset", 0):iv.get("byteOffset", 0) + iv["byteLength"]]))
    want = sys.argv[1:] or ["blue", "dog", "cat", "rhino", "donkey", "capybara", "brain"]
    report = []
    for fn in (blue, dog, cat, rhino, donkey, capybara, brain):   # the peanut is built by build_roasted.py
        spec = fn()
        if spec["name"] not in want:
            continue
        glb, own, pal = build(spec, j0, bin0, pal_img)
        (ROOT / "assets/models" / f"{spec['name']}.glb").write_bytes(glb)
        spec = fn()  # parts were consumed; rebuild for the .vox
        write_vox(spec, ROOT / "source/vox/characters" / f"{spec['name']}.vox")
        report.append((spec["name"], len(glb), own))
        print(f"{spec['name']}.glb  {len(glb) // 1024} KB  own clips: {', '.join(own)}")
    return report


if __name__ == "__main__":
    main()
