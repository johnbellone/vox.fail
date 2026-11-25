#!/usr/bin/env python3
"""Roasted peanut series: one roasted, salted peanut in six outfits, each with its own hand props and
five poses, on the shared block rig (so the 100 body clips and 24 face clips work too).

    python3 tools/build_roasted.py        # assets/models/roasted_*.glb, assets/props/roasted/*.glb, source/vox/...
"""
import io, math, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build_animals as B
from build_animals import Vox, standard_body, env, osc, face, euler, WHITE, BLACK

ROOT = B.ROOT

SHELL, DIMPLE, SCORCH, SALT = (204, 150, 84), (160, 108, 54), (128, 80, 38), (250, 250, 244)
LIMB, HAND = (96, 62, 34), (236, 226, 206)
GRIP = (0.0, -0.25, 0.0)  # fist, in LowerArm space


def h3(x, y, z):
    return (x * 73856093 ^ y * 19349663 ^ z * 83492791) & 0xFFFF


def roast(v, shell=None, ridge=None, toast=None, salt=True):
    """the peanut-shell net: long ridges with cross links, a few toasted spots, salt crystals"""
    shell, ridge, toast = shell or SHELL, ridge or DIMPLE, toast or SCORCH
    v.paint(lambda *_: True, shell)
    v.paint(lambda x, y, z: (y % 3 == 0 and (x + z) % 2 == 0) or ((x + z) % 3 == 0 and y % 2 == 1), ridge)
    v.paint(lambda x, y, z: h3(x, y, z) % 17 == 0, toast)
    if salt:
        v.paint(lambda x, y, z: h3(x + 7, y, z) % 21 == 0 and v.surface(x, y, z), SALT)
    return v


def lobe(v, c, r, n, zmax=None, col=SHELL):
    """superellipsoid lobe: rounded, but flat enough at the front to carry the face decals"""
    cx, cy, cz = c
    rx, ry, rz = r
    for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
        for y in range(int(cy - ry) - 1, int(cy + ry) + 2):
            for z in range(int(cz - rz) - 1, int(cz + rz) + 2):
                if zmax is not None and z > zmax:
                    continue
                if abs((x + 0.5 - cx) / rx) ** n + abs((y + 0.5 - cy) / ry) ** n + abs((z + 0.5 - cz) / rz) ** n <= 1.0:
                    v.v[(x, y, z)] = col
    return v


def base(sleeve=LIMB, pants=LIMB, shoe=(60, 48, 40), hand=HAND, sock=WHITE, shell=None, ridge=None, toast=None, salt=True):
    def torso(t):
        t.v.clear()
        lobe(t, (0, 5.0, 0), (5.6, 5.6, 5.0), 2.6)              # lower lobe, rounder
        roast(t, shell, ridge, toast, salt)

    def arm(side, ua, la):
        ua.v.clear(); la.v.clear()
        ua.box(-1, 1, -5, 1, -1, 1, sleeve)
        la.box(-1, 1, -4, 0, -1, 1, sleeve).box(-2, 1, -5, -4, -2, 1, hand)

    def leg(side, ul, ll):
        ul.v.clear(); ll.v.clear()
        ul.box(-1, 1, -5, 0, -1, 1, pants)
        ll.box(-1, 1, -5, 0, -1, 1, pants).box(-2, 2, -6, -5, -2, 4, shoe).box(-2, 2, -6, -5, 3, 4, sock)

    p = standard_body(SHELL, torso, arm, leg, hand)
    h = lobe(Vox(), (0, 4.5, -0.5), (7.0, 7.2, 6.5), 3.2, zmax=5)   # upper lobe; the waist pinch comes from both lobes rounding off
    roast(h, shell, ridge, toast, salt)
    p["Head"] = h
    return p


def prop(name, parent, vox, at=GRIP, rot=(0, 0, 0), off=(0.5, 0, 0.5)):
    return dict(name=name, parent=parent, vox=vox, at=at, rot=rot, off=off)


# ---------------------------------------------------------------------------- pose helpers
def walk(t, period, amp=30):
    s = osc(t, period, 1)
    return {"UpperLeg_L": (-amp * s, 0, 0), "UpperLeg_R": (amp * s, 0, 0), "LowerLeg_L": (20 * max(0, s), 0, 0), "LowerLeg_R": (20 * max(0, -s), 0, 0),
            "UpperArm_L": (amp * 0.8 * s, 0, 6), "UpperArm_R": (-amp * 0.8 * s, 0, -6), "_hip": (0, abs(osc(t, period, 0.025)), 0)}


def kneel(k):
    """left knee up, right knee down"""
    return {"UpperLeg_L": (-90 * k, 0, 0), "LowerLeg_L": (90 * k, 0, 0), "UpperLeg_R": (15 * k, 0, 0), "LowerLeg_R": (75 * k, 0, 0), "_hip": (0, -0.25 * k, 0)}


def both_knees(k):
    return {"UpperLeg_L": (0, 0, 0), "LowerLeg_L": (90 * k, 0, 0), "UpperLeg_R": (0, 0, 0), "LowerLeg_R": (90 * k, 0, 0), "_hip": (0, -0.29 * k, 0)}


def merge(*ds):
    out = {}
    for d in ds:
        for k, v in d.items():
            if k in out and k != "_hip" and isinstance(v, tuple):
                out[k] = tuple(a + b for a, b in zip(out[k], v))
            else:
                out[k] = v
    return out


# ---------------------------------------------------------------------------- outfits
def army():
    olive, olive_d, boot, metal = (86, 98, 52), (62, 72, 38), (34, 30, 28), (52, 52, 56)
    p = base(sleeve=olive, pants=olive, shoe=boot, sock=boot)
    t = p["Hips"]
    t.paint(lambda x, y, z: y == 3, olive_d)                                    # belt
    for x0 in (-3, 2):
        t.paint(lambda x, y, z, x0=x0: x == x0 and y >= 4 and (z >= 3 or z <= -4), olive_d)   # suspenders
    t.box(-4, -2, 2, 4, 5, 6, olive).box(2, 4, 2, 4, 5, 6, olive)              # pouches
    t.box(-4, 4, 2, 9, -6, -4, olive_d).box(2, 3, 9, 15, -6, -5, BLACK)        # radio pack and antenna
    for i in range(10):                                                         # rifle slung across the back
        t.box(-5 + i, -3 + i, 10 - i, 11 - i, -7, -6, (70, 54, 38) if i > 6 else metal)
    h = p["Head"]
    h.box(-7, 7, 9, 12, -7, 7, olive).box(-6, 6, 12, 13, -6, 6, olive).box(-7, 7, 9, 10, 7, 8, olive_d)   # helmet
    h.box(-7, -6, 3, 9, -1, 1, olive_d).box(6, 7, 3, 9, -1, 1, olive_d)        # chin strap
    props = [
        prop("binoculars", "LowerArm_R", Vox().box(-2, 2, -1, 1, 0, 3, BLACK).box(-2, -1, -1, 1, 3, 4, (120, 150, 170)).box(1, 2, -1, 1, 3, 4, (120, 150, 170))),
        prop("handset", "LowerArm_R", Vox().box(-1, 1, -2, 3, 0, 1, BLACK).box(-1, 1, 2, 3, 1, 2, BLACK)),
        prop("flag", "LowerArm_L", Vox().box(0, 1, -17, 2, 0, 1, (120, 90, 60))
             .box(1, 9, -17, -11, 0, 1, (200, 40, 44)).paint(lambda x, y, z: y % 2 == 0 and x >= 1 and y <= -11, WHITE).box(1, 4, -17, -14, 0, 1, (40, 56, 120))),
    ]

    def salute(t):
        k = env(t, 0.1, 1.5, 0.25)
        return {"UpperArm_R": (0, 25 * k, -6 - 104 * k), "LowerArm_R": (-14 + 14 * k, 0, -100 * k), "Head": (-4 * k, 0, 0), "Hips": (-3 * k, 0, 0)}

    def march(t):
        w = walk(t, 0.8, 35)
        w["UpperArm_R"] = (-35 * osc(t, 0.8, 1), 0, -6)
        w["Head"] = (-5, 0, 0)
        return w

    def scout(t):
        k = env(t, 0.0, 1.8, 0.3)
        return {"UpperArm_R": (-70 * k, 0, -6 + 25 * k), "LowerArm_R": (-14 - 105 * k, 0, 0), "UpperArm_L": (-70 * k, 0, 6 - 25 * k), "LowerArm_L": (-14 - 105 * k, 0, 0),
                "Head": (0, osc(t, 1.8, 25) * k, 0), "Hips": (6 * k, osc(t, 1.8, 10) * k, 0)}

    def radio(t):
        k = env(t, 0.0, 1.6, 0.25)
        return {"UpperArm_R": (-40 * k, 0, -6 + 30 * k), "LowerArm_R": (-14 - 125 * k, 0, 0), "Head": (6 * k, 0, -14 * k), "UpperArm_L": (-20 * k, 0, 15 * k), "LowerArm_L": (-14 - 60 * k, 0, 0)}

    def plant(t):
        k = env(t, 0.1, 1.7, 0.3)
        return merge(kneel(env(t, 0.1, 1.7, 0.3)), {"UpperArm_L": (-175 * k, 0, 6), "LowerArm_L": (-14 + 10 * k, 0, 0), "Head": (-20 * k, 0, 0), "UpperArm_R": (0, 0, -6 - 30 * k)})

    clips = [
        ("Salute", 1.6, salute, lambda t: face("eyes_open", "brows_angry", "mouth_flat")),
        ("March", 1.6, march, lambda t: face("eyes_open", "brows_angry", "mouth_grit")),
        ("Scout", 1.8, scout, lambda t: face("eyes_squint", "brows_angry", "mouth_flat"), lambda t: {"binoculars"} if 0.15 < t < 1.65 else set()),
        ("Radio call", 1.6, radio, lambda t: face("eyes_open", "brows_normal", "mouth_open" if (t % 0.3) < 0.15 and 0.3 < t < 1.3 else "mouth_flat"), lambda t: {"handset"}),
        ("Plant the flag", 1.8, plant, lambda t: face("eyes_open", "brows_raised", "mouth_grin"), lambda t: {"flag"}),
    ]
    return dict(name="roasted_army", label="Roasted Peanut (army)", parts=p, props=props, clips=clips,
                note="Army man: olive helmet with chin strap, webbing, belt pouches, radio pack, rifle slung on the back, combat boots.")


def medic():
    scrub, white, green, steth = (64, 160, 156), (244, 244, 240), (40, 150, 70), (90, 90, 96)
    p = base(sleeve=scrub, pants=scrub, shoe=WHITE, hand=(150, 200, 230))
    t = p["Hips"]
    t.paint(lambda x, y, z: y >= 2, white)                                      # vest
    t.box(-1, 1, 5, 8, 5, 6, green).box(-2, 2, 6, 7, 5, 6, green)              # green cross (first aid, not the Red Cross)
    t.box(-1, 1, 5, 8, -5, -4, green).box(-2, 2, 6, 7, -5, -4, green)
    for (x, y) in ((-3, 10), (-3, 9), (-3, 8), (-2, 7), (3, 10), (3, 9), (3, 8), (2, 7)):
        t.box(x, x + 1, y, y + 1, 5, 6, steth)                                  # stethoscope
    t.box(-1, 2, 5, 6, 6, 7, (180, 184, 190))
    h = p["Head"]
    h.box(-7, 7, 9, 11, -7, 7, white).box(-1, 1, 9, 11, 7, 8, green)          # headband with cross
    props = [
        prop("medkit", "LowerArm_R", Vox().box(-3, 3, -6, -1, -2, 2, white).box(-1, 1, -5, -2, 2, 3, green).box(-2, 2, -4, -3, 2, 3, green).box(-1, 1, -1, 0, -1, 1, (60, 60, 60))),
        prop("clipboard", "LowerArm_L", Vox().box(-3, 3, -1, 0, -1, 7, (150, 110, 70)).box(-2, 2, -2, -1, 0, 6, WHITE).box(-1, 1, -2, -1, 6, 7, (180, 184, 190)), rot=(0, 0, 0)),
        prop("bandage", "LowerArm_R", Vox().box(-1, 2, -2, 1, -1, 2, WHITE)),
    ]

    def rush(t):
        w = walk(t, 0.5, 45)
        w["Hips"] = (12, 0, 0)
        w["UpperArm_R"] = (-20 * osc(t, 0.5, 1), 0, -10)
        w["LowerArm_L"] = (-60, 0, 0)
        return w

    def cpr(t):
        k = env(t, 0.0, 2.0, 0.3)
        pump = max(0, osc(t, 0.4, 1)) * k
        return merge(both_knees(k), {"Hips": (40 * k + 6 * pump, 0, 0), "UpperArm_L": (-55 * k, 0, 6 - 12 * k), "UpperArm_R": (-55 * k, 0, -6 + 12 * k),
                                     "LowerArm_L": (-14 + 14 * k, 0, 0), "LowerArm_R": (-14 + 14 * k, 0, 0), "Head": (5 * k, 0, 0)})

    def listen(t):
        k = env(t, 0.0, 1.8, 0.3)
        return {"UpperArm_L": (-35 * k, 0, 6 - 25 * k), "LowerArm_L": (-14 - 75 * k, 0, 0), "UpperArm_R": (-25 * k, 0, -6 + 20 * k), "LowerArm_R": (-14 - 120 * k, 0, 0),
                "Head": (10 * k, 0, 18 * k), "Hips": (5 * k, 0, 0)}

    def bandage(t):
        k = env(t, 0.0, 1.8, 0.3)
        c = osc(t, 0.45, 1) * k
        s = osc(t, 0.45, 1, 0.25) * k
        return {"UpperArm_R": (-60 * k + 20 * c, 0, -6 + 20 * s), "LowerArm_R": (-14 - 60 * k, 0, 0), "UpperArm_L": (-55 * k, 0, 6 + 10 * k), "LowerArm_L": (-14 - 40 * k, 0, 0),
                "Hips": (14 * k, 0, 0), "Head": (14 * k, 0, 0)}

    def chart(t):
        k = env(t, 0.0, 1.8, 0.3)
        wr = osc(t, 0.25, 6) * k
        return {"UpperArm_L": (-30 * k, 0, 6 - 15 * k), "LowerArm_L": (-14 - 80 * k, -30 * k, 0), "UpperArm_R": (-30 * k + wr, 0, -6 + 15 * k), "LowerArm_R": (-14 - 80 * k, 35 * k, 0),
                "Head": (22 * k, 0, 0)}

    clips = [
        ("Rush in", 1.5, rush, lambda t: face("eyes_wide", "brows_worried", "mouth_open"), lambda t: {"medkit"}),
        ("CPR", 2.0, cpr, lambda t: face("eyes_open", "brows_worried", "mouth_grit")),
        ("Listen", 1.8, listen, lambda t: face("eyes_up", "brows_raised", "mouth_flat")),
        ("Bandage", 1.8, bandage, lambda t: face("eyes_open", "brows_normal", "mouth_pout"), lambda t: {"bandage"}),
        ("Chart", 1.8, chart, lambda t: face("eyes_squint", "brows_normal", "mouth_flat"), lambda t: {"clipboard"}),
    ]
    return dict(name="roasted_medic", label="Roasted Peanut (medic)", parts=p, props=props, clips=clips,
                note="Medic: teal scrubs, white vest with a green first-aid cross front and back, stethoscope, headband, blue gloves.")


def miami():
    jacket, shirt, pants, loafer, shades, gold = (110, 210, 200), (248, 160, 190), (246, 244, 236), (190, 160, 120), (24, 24, 30), (232, 190, 60)
    p = base(sleeve=jacket, pants=pants, shoe=loafer, sock=loafer)
    for side in ("L", "R"):
        p[f"LowerArm_{side}"].paint(lambda x, y, z: y >= -2 and y < 0, jacket).paint(lambda x, y, z: -4 <= y < -2, LIMB)  # sleeves pushed up
    t = p["Hips"]
    t.paint(lambda x, y, z: y >= 2, jacket)                                     # pastel jacket
    t.paint(lambda x, y, z: y < 2, pants)
    t.paint(lambda x, y, z: y >= 2 and -2 <= x <= 1 and z >= 2, shirt)           # pink tee showing
    t.paint(lambda x, y, z: y >= 5 and x in (-3, 2) and z >= 3, jacket)          # lapels
    h = p["Head"]
    h.box(-6, 6, 4, 7, 6, 7, shades).box(-6, -1, 3, 7, 6, 7, shades).box(1, 6, 3, 7, 6, 7, shades)    # aviators
    h.box(-7, -6, 5, 6, -3, 6, gold).box(6, 7, 5, 6, -3, 6, gold)            # arms of the shades
    props = [
        prop("badge", "LowerArm_R", Vox().box(-2, 2, -1, 0, -1, 3, (70, 44, 28)).box(-1, 1, -2, -1, 0, 2, gold)),
        prop("walkie", "LowerArm_R", Vox().box(-1, 1, -1, 3, -1, 1, BLACK).box(0, 1, 3, 6, -1, 0, BLACK).box(-1, 1, 1, 2, 1, 2, (200, 40, 40))),
        prop("donut", "LowerArm_L", Vox().box(-2, 2, -2, -1, -2, 2, (220, 150, 80)).box(-2, 2, -1, 0, -2, 2, (246, 150, 190)).cut(-1, 1, -2, 0, -1, 1)
             .box(-2, -1, -1, 0, -2, -1, WHITE).box(1, 2, -1, 0, 1, 2, (90, 180, 230))),
    ]

    def flash(t):
        k = env(t, 0.1, 1.5, 0.15)
        return {"UpperArm_R": (-88 * k, 0, -6), "LowerArm_R": (-14 + 10 * k, 0, 0), "Head": (6 * k, 0, 0), "Hips": (0, -12 * k, 0), "UpperLeg_L": (-8 * k, 0, 6 * k)}

    def cool(t):
        k = env(t, 0.0, 2.0, 0.35)
        return {"UpperArm_L": (-25 * k, 0, 6 - 12 * k), "UpperArm_R": (-25 * k, 0, -6 + 12 * k), "LowerArm_L": (-14 - 80 * k, -55 * k, 0), "LowerArm_R": (-14 - 80 * k, 55 * k, 0),
                "Hips": (-6 * k, 0, 7 * k), "Head": (0, 15 * k, -8 * k), "UpperLeg_R": (-6 * k, 0, -10 * k), "LowerLeg_R": (12 * k, 0, 0), "_hip": (0.02 * k, 0, 0)}

    def radio(t):
        k = env(t, 0.0, 1.6, 0.25)
        return {"UpperArm_R": (-40 * k, 0, -6 + 30 * k), "LowerArm_R": (-14 - 125 * k, 0, 0), "Head": (0, -20 * k, -10 * k), "UpperArm_L": (0, 0, 6 + 10 * k)}

    def freeze(t):
        k = env(t, 0.05, 1.5, 0.1)
        return {"UpperArm_R": (-90 * k, 0, -6), "LowerArm_R": (-14 + 14 * k, 0, 0), "UpperArm_L": (-20 * k, 0, 6 + 30 * k), "Hips": (8 * k, 0, 0),
                "UpperLeg_L": (-30 * k, 0, 0), "LowerLeg_L": (20 * k, 0, 0), "UpperLeg_R": (20 * k, 0, 0), "_hip": (0, -0.04 * k, 0), "Head": (6 * k, 0, 0)}

    def donut(t):
        k = env(t, 0.0, 1.8, 0.3)
        bite = env(t, 0.6, 1.1, 0.12)
        return {"UpperArm_L": (-35 * k - 20 * bite, 0, 6 - 20 * k), "LowerArm_L": (-14 - 90 * k - 20 * bite, 0, 0), "Head": (10 * bite, 0, 0), "Hips": (-4 * k, 0, 0)}

    clips = [
        ("Badge flash", 1.6, flash, lambda t: face("eyes_open", "brows_angry", "mouth_smirk"), lambda t: {"badge"}),
        ("Too cool", 2.0, cool, lambda t: face("eyes_open", "brows_normal", "mouth_smirk")),
        ("Radio in", 1.6, radio, lambda t: face("eyes_open", "brows_angry", "mouth_open" if (t % 0.3) < 0.15 and 0.3 < t < 1.3 else "mouth_flat"), lambda t: {"walkie"}),
        ("Freeze!", 1.5, freeze, lambda t: face("eyes_wide", "brows_angry", "mouth_open" if 0.2 < t < 0.7 else "mouth_grit")),
        ("Donut break", 1.8, donut, lambda t: face("eyes_open" if t < 0.6 else "eyes_shut", "brows_raised", "mouth_open" if 0.55 < t < 0.75 else ("mouth_grin" if t > 0.75 else "mouth_flat")), lambda t: {"donut"}),
    ]
    return dict(name="roasted_miami", label="Roasted Peanut (Miami cop)", parts=p, props=props, clips=clips,
                note="1980s Miami detective look: pastel teal jacket with pushed-up sleeves over a pink tee, white pants, loafers without socks, aviator shades.")


def cook():
    red, white, apron, hat_stripe, metal, fries = (200, 44, 44), (248, 248, 244), (246, 246, 240), (200, 44, 44), (186, 190, 196), (246, 200, 70)
    p = base(sleeve=red, pants=(44, 44, 52), shoe=BLACK, sock=BLACK)
    t = p["Hips"]
    t.paint(lambda x, y, z: y >= 2, red)
    t.paint(lambda x, y, z: z >= 1 and y <= 8, apron).paint(lambda x, y, z: y == 6, apron)   # apron and its tie
    t.box(-4, -2, 7, 8, 5, 6, WHITE).box(-4, -3, 7, 8, 5, 6, (60, 60, 200))    # name tag
    t.paint(lambda x, y, z: y == 3 and z >= 4 and x > 0, (220, 190, 120))       # grease smudge
    h = p["Head"]
    h.box(-6, 6, 10, 15, -4, 4, white).box(-6, 6, 10, 11, -4, 4, hat_stripe).cut(-6, 6, 14, 15, -4, -2).cut(-6, 6, 14, 15, 2, 4)   # paper fold hat
    props = [
        prop("spatula", "LowerArm_R", Vox().box(0, 1, -6, 1, 0, 1, BLACK).box(-2, 2, -10, -6, 0, 1, metal)),
        prop("basket", "LowerArm_R", Vox().box(0, 1, -5, 1, 0, 1, BLACK).box(-2, 3, -10, -5, -2, 2, metal).cut(-1, 2, -9, -6, -1, 1).box(-1, 2, -9, -6, -1, 1, fries)
             .box(-1, 0, -9, -8, 2, 3, fries).box(1, 2, -8, -7, 2, 3, fries)),
        prop("salt", "LowerArm_R", Vox().box(-1, 1, -4, 0, -1, 1, (220, 230, 240)).box(-1, 1, -5, -4, -1, 1, metal)),
        prop("bell", "LowerArm_L", Vox().box(-2, 2, -2, -1, -2, 2, (232, 190, 60)).box(-1, 1, -1, 0, -1, 1, (232, 190, 60)).box(0, 1, 0, 1, 0, 1, BLACK)),
        prop("patty", "LowerArm_R", Vox().box(-2, 2, -11, -10, 0, 1, (110, 64, 34)), ),
    ]

    def flip(t):
        k = env(t, 0.0, 1.6, 0.25)
        f = env(t, 0.55, 0.85, 0.08)
        return {"UpperArm_R": (-55 * k - 30 * f, 0, -6), "LowerArm_R": (-14 - 25 * k - 30 * f, 0, 0), "Head": (12 * k - 25 * f, 0, 0), "UpperArm_L": (-20 * k, 0, 10 * k)}

    def shake(t):
        k = env(t, 0.0, 1.6, 0.25)
        s = osc(t, 0.2, 1) * env(t, 0.3, 1.3, 0.1)
        return {"UpperArm_R": (-50 * k + 10 * s, 0, -6), "LowerArm_R": (-14 - 20 * k, 0, 0), "Hips": (6 * k, 0, 0), "Head": (10 * k, 0, 0), "UpperArm_L": (0, 0, 6 + 10 * k)}

    def order(t):
        k = env(t, 0.0, 1.4, 0.2)
        d = env(t, 0.35, 0.55, 0.06) + env(t, 0.75, 0.95, 0.06)
        return {"UpperArm_L": (-60 * k - 20 * d, 0, 6), "LowerArm_L": (-14 - 30 * k + 25 * d, 0, 0), "UpperArm_R": (0, 0, -6 - 40 * k), "LowerArm_R": (-14 - 80 * k, 0, 0),
                "Head": (-12 * k, 0, 0)}

    def salt(t):
        k = env(t, 0.0, 1.8, 0.3)
        s = osc(t, 0.25, 1) * env(t, 0.4, 1.4, 0.1)
        return {"UpperArm_R": (-160 * k, 0, -6 - 10 * k), "LowerArm_R": (-14 - 40 * k + 20 * s, 0, 0), "Head": (-25 * k, 0, 0), "Hips": (-6 * k, 0, 0), "UpperArm_L": (0, 0, 6 + 20 * k)}

    def wipe(t):
        k = env(t, 0.0, 1.6, 0.3)
        sw = env(t, 0.4, 1.1, 0.15)
        return {"UpperArm_L": (-100 * k, 0, 6 - 30 * k + 40 * sw), "LowerArm_L": (-14 - 100 * k, 0, 0), "Head": (-8 * k, 0, 10 * k), "Hips": (-4 * k, 0, 0), "UpperArm_R": (0, 0, -6 - 6 * k)}

    clips = [
        ("Burger flip", 1.6, flip, lambda t: face("eyes_open", "brows_raised", "mouth_grin"), lambda t: {"spatula", "patty"} if t < 0.6 or t > 0.85 else {"spatula"}),
        ("Basket shake", 1.6, shake, lambda t: face("eyes_squint", "brows_normal", "mouth_flat"), lambda t: {"basket"}),
        ("Order up!", 1.4, order, lambda t: face("eyes_open", "brows_raised", "mouth_open" if 0.35 < t < 1.0 else "mouth_grin"), lambda t: {"bell"}),
        ("Salt it", 1.8, salt, lambda t: face("eyes_shut", "brows_raised", "mouth_grin"), lambda t: {"salt"}),
        ("Wipe brow", 1.6, wipe, lambda t: face("eyes_shut", "brows_worried", "mouth_wavy")),
    ]
    return dict(name="roasted_cook", label="Roasted Peanut (fry cook)", parts=p, props=props, clips=clips,
                note="Fry cook: red uniform, white apron with a name tag, paper fold hat, black work shoes.")


def garbage():
    navy, vis, silver, glove, cap, bag, can = (40, 52, 84), (190, 230, 40), (210, 214, 220), (232, 196, 60), (232, 110, 30), (34, 34, 38), (120, 128, 136)
    p = base(sleeve=navy, pants=navy, shoe=(70, 52, 36), sock=(70, 52, 36), hand=glove)
    t = p["Hips"]
    t.paint(lambda x, y, z: y >= 2, vis).paint(lambda x, y, z: y < 2, navy)
    t.paint(lambda x, y, z: y in (4, 7), silver)                               # reflective bands
    t.paint(lambda x, y, z: y >= 8 and -1 <= x <= 0 and z >= 2, navy)          # vest opening at the neck
    h = p["Head"]
    h.box(-6, 6, 10, 13, -5, 5, cap).box(-5, 5, 10, 11, 5, 9, cap)             # cap and brim
    props = [
        prop("trashbag", "LowerArm_R", Vox().box(-1, 1, -2, 0, -1, 1, bag).box(-3, 3, -9, -2, -3, 3, bag).bevel(-3, 3, -9, -2, -3, 3)),
        prop("broom", "LowerArm_R", Vox().box(0, 1, -16, 3, 0, 1, (150, 110, 70)).box(-3, 4, -20, -16, -1, 2, (220, 190, 90))),
        prop("can", "LowerArm_L", Vox().box(-4, 4, -12, -2, -4, 4, can).box(-5, 5, -3, -2, -5, 5, (96, 102, 110)).box(-1, 1, -2, 0, -1, 1, (60, 60, 60))
             .paint(lambda x, y, z: y in (-6, -10) and abs(z) == 4 or y in (-6, -10) and abs(x + 0.5) > 3, (150, 158, 166))),
    ]

    def toss(t):
        wind = env(t, 0.0, 0.55, 0.2)
        thr = env(t, 0.5, 0.9, 0.08)
        return {"UpperArm_R": (60 * wind - 160 * thr, 0, -6), "LowerArm_R": (-14 - 40 * wind, 0, 0), "Hips": (-10 * wind + 20 * thr, 25 * wind - 15 * thr, 0),
                "UpperLeg_L": (-25 * thr, 0, 0), "Head": (-10 * thr, 0, 0)}

    def haul(t):
        w = walk(t, 1.0, 22)
        w["Hips"] = (14, 0, 0)
        w["UpperArm_L"] = (20, 0, 6)
        w["LowerArm_L"] = (-6, 0, 0)
        w["Head"] = (-8, 0, 0)
        return w

    def sweep(t):
        k = env(t, 0.0, 2.0, 0.3)
        s = osc(t, 0.7, 1) * k
        return {"UpperArm_R": (-40 * k, 30 * s, -6), "LowerArm_R": (-14 - 10 * k, 0, 0), "UpperArm_L": (-45 * k, 30 * s, 6 - 20 * k), "LowerArm_L": (-14 - 50 * k, 0, 0),
                "Hips": (12 * k, 20 * s, 0), "Head": (15 * k, -10 * s, 0)}

    def ride(t):
        k = env(t, 0.0, 2.0, 0.3)
        b = osc(t, 0.35, 1) * k
        return {"UpperArm_R": (0, 0, -6 - 160 * k), "LowerArm_R": (-14 + 10 * k, 0, 0), "UpperLeg_L": (-50 * k, 0, 30 * k), "LowerLeg_L": (60 * k, 0, 0),
                "Hips": (0, 0, -12 * k + 2 * b), "Head": (-8 * k, 20 * k, 6 * k), "UpperArm_L": (-30 * k, 0, 6 + 60 * k), "_hip": (0, 0.03 * b, 0)}

    def thumbs(t):
        k = env(t, 0.05, 1.4, 0.2)
        return {"UpperArm_R": (-80 * k, 0, -6), "LowerArm_R": (-14 - 70 * k, 0, 0), "Head": (0, 0, 10 * k), "Hips": (0, -10 * k, 0)}

    clips = [
        ("Bag toss", 1.3, toss, lambda t: face("eyes_open", "brows_angry", "mouth_grit" if t < 0.5 else "mouth_open"), lambda t: {"trashbag"} if t < 0.75 else set()),
        ("Haul the can", 2.0, haul, lambda t: face("eyes_squint", "brows_worried", "mouth_grit"), lambda t: {"can"}),
        ("Sweep", 2.1, sweep, lambda t: face("eyes_open", "brows_normal", "mouth_flat"), lambda t: {"broom"}),
        ("Ride the truck", 2.1, ride, lambda t: face("eyes_open", "brows_raised", "mouth_grin")),
        ("Thumbs up", 1.4, thumbs, lambda t: face("eyes_open", "brows_raised", "mouth_grin"), lambda t: {"hand_thumb"} if 0.25 < t < 1.25 else set()),
    ]
    return dict(name="roasted_garbage", label="Roasted Peanut (garbage collector)", parts=p, props=props, clips=clips,
                note="Garbage collector: lime hi-vis vest with silver reflective bands over navy work clothes, orange cap, yellow work gloves, brown boots.")


def riot():
    black, plate, pad, visor, shield, frame = (30, 32, 38), (52, 56, 64), (64, 68, 76), (170, 205, 225, 128), (190, 215, 230, 128), (24, 24, 28)
    p = base(sleeve=black, pants=black, shoe=BLACK, sock=BLACK, hand=(40, 40, 46))
    t = p["Hips"]
    t.paint(lambda *_: True, plate)
    t.box(-4, 4, 3, 9, 4, 5, black).box(-4, 4, 3, 9, -6, -5, black)            # chest and back plates
    t.box(-3, 3, 6, 7, -7, -6, WHITE)                                            # stripe on the back
    t.paint(lambda x, y, z: y == 1, BLACK)
    for side in ("L", "R"):
        p[f"UpperArm_{side}"].box(-2, 1, -1, 2, -2, 1, pad)                     # shoulder pads
        p[f"LowerLeg_{side}"].box(-2, 2, -2, 0, 1, 2, pad)                      # knee pads
    h = p["Head"]
    h.box(-7, 7, 8, 13, -7, 6, black).box(-6, 6, 13, 14, -6, 5, black)        # helmet
    h.box(-7, 7, -2, 8, -7, -6, black).box(-7, -6, -2, 8, -7, 5, black).box(6, 7, -2, 8, -7, 5, black)   # neck guard and sides
    h.box(-7, 7, -1, 8, 6, 7, visor)                                             # clear visor over the face
    h.box(-7, 7, 8, 9, 6, 8, frame)
    sh = Vox().box(-4, 4, -2, -1, -6, 8, shield)
    sh.box(-4, 4, -2, -1, -6, -5, frame).box(-4, 4, -2, -1, 7, 8, frame).box(-5, -4, -2, -1, -6, 8, frame).box(4, 5, -2, -1, -6, 8, frame)
    sh.box(-1, 1, -1, 0, 0, 2, frame)                                            # grip
    props = [
        prop("shield", "LowerArm_L", sh, at=(0.0, -0.12, 0.0)),
        prop("baton", "LowerArm_R", Vox().box(0, 1, -9, 1, 0, 1, BLACK).box(0, 1, -2, -1, 1, 2, BLACK)),
        prop("radio", "LowerArm_R", Vox().box(-1, 1, -1, 3, -1, 1, BLACK).box(0, 1, 3, 5, -1, 0, BLACK)),
    ]
    guard = {"UpperArm_L": (-75, 0, 6 - 20), "LowerArm_L": (-14 - 10, -30, 0)}

    def up(t):
        k = env(t, 0.0, 1.6, 0.25)
        return {"UpperArm_L": (-75 * k, 0, 6 - 20 * k), "LowerArm_L": (-14 - 10 * k, -30 * k, 0), "UpperArm_R": (-10 * k, 0, -6), "Hips": (4 * k, 0, 0), "Head": (4 * k, 0, 0),
                "UpperLeg_L": (-15 * k, 0, 0), "UpperLeg_R": (12 * k, 0, 0), "_hip": (0, -0.02 * k, 0)}

    def advance(t):
        w = walk(t, 1.0, 18)
        w.update({"UpperArm_L": (-75, 0, -14), "LowerArm_L": (-24, -30, 0), "Hips": (6, 0, 0), "UpperArm_R": (-10, 0, -6)})
        return w

    def hold(t):
        k = env(t, 0.0, 2.0, 0.3)
        b = osc(t, 1.0, 1) * k
        return {"UpperArm_L": (-75 * k, 0, 6 - 20 * k), "LowerArm_L": (-14 - 10 * k, -30 * k, 0), "UpperLeg_L": (-30 * k, 0, 0), "LowerLeg_L": (30 * k, 0, 0),
                "UpperLeg_R": (25 * k, 0, 0), "Hips": (10 * k + 2 * b, 0, 0), "_hip": (0, -0.06 * k, 0), "UpperArm_R": (-20 * k, 0, -6), "Head": (4 * k, 0, 0)}

    def kneel_cover(t):
        k = env(t, 0.0, 2.0, 0.3)
        return merge(kneel(k), {"UpperArm_L": (-75 * k, 0, 6 - 20 * k), "LowerArm_L": (-14 - 10 * k, -30 * k, 0), "Hips": (10 * k, 0, 0), "Head": (10 * k, 0, 0)})

    def radio(t):
        k = env(t, 0.0, 1.6, 0.25)
        return {"UpperArm_R": (-40 * k, 0, -6 + 30 * k), "LowerArm_R": (-14 - 125 * k, 0, 0), "Head": (0, 0, -12 * k)}

    clips = [
        ("Shield up", 1.6, up, lambda t: face("eyes_open", "brows_angry", "mouth_flat"), lambda t: {"shield", "baton"}),
        ("Advance", 2.0, advance, lambda t: face("eyes_open", "brows_angry", "mouth_grit"), lambda t: {"shield", "baton"}),
        ("Hold the line", 2.0, hold, lambda t: face("eyes_squint", "brows_angry", "mouth_grit"), lambda t: {"shield"}),
        ("Kneel and cover", 2.0, kneel_cover, lambda t: face("eyes_open", "brows_worried", "mouth_flat"), lambda t: {"shield"}),
        ("Radio check", 1.6, radio, lambda t: face("eyes_open", "brows_normal", "mouth_open" if (t % 0.3) < 0.15 and 0.3 < t < 1.3 else "mouth_flat"), lambda t: {"radio"}),
    ]
    return dict(name="roasted_riot", label="Roasted Peanut (riot officer)", parts=p, props=props, clips=clips,
                note="Riot officer: black helmet with a clear visor and neck guard, chest and back plates, shoulder and knee pads; clear shield, baton, radio.")


def plain():
    cap, capdark = (236, 128, 36), (196, 96, 24)
    p = base(shell=(222, 186, 128), ridge=(190, 150, 96), toast=(200, 160, 104), salt=False)
    h = p["Head"]
    h.box(-6, 6, 10, 13, -5, 5, cap).box(-5, 5, 13, 14, -4, 4, cap)
    h.box(-5, 5, 10, 11, 5, 9, capdark).box(-1, 1, 14, 15, -1, 1, capdark)
    return dict(name="peanut", label="Peanut", parts=p, props=[], clips=B.clips_for("peanut"),
                note="Original peanut character: raw two-lobed shell with a net texture, stick limbs, cream hands, orange ball cap, sneakers.")


ALL = [army, medic, miami, cook, garbage, riot, plain]


def main():
    j0, bin0 = B.load_glb(B.TEMPLATE)
    iv = j0["bufferViews"][j0["images"][0]["bufferView"]]
    from PIL import Image
    pal_img = Image.open(io.BytesIO(bin0[iv.get("byteOffset", 0):iv.get("byteOffset", 0) + iv["byteLength"]]))
    want = sys.argv[1:]
    (ROOT / "assets/props/roasted").mkdir(parents=True, exist_ok=True)
    (ROOT / "source/vox/props/roasted").mkdir(parents=True, exist_ok=True)
    for fn in ALL:
        spec = fn()
        if want and spec["name"] not in want:
            continue
        spec.update(mouth=(0, 0, 0), tail_at=None, tail_sway=None)
        glb, own, _ = B.build(spec, j0, bin0, pal_img)
        (ROOT / "assets/models" / f"{spec['name']}.glb").write_bytes(glb)
        spec = fn()
        spec.update(tail_at=None)
        B.write_vox(spec, ROOT / "source/vox/characters" / f"{spec['name']}.vox")
        for pr in spec["props"]:
            write_prop(pr, ROOT / "assets/props/roasted" / f"{pr['name']}.glb", ROOT / "source/vox/props/roasted" / f"{pr['name']}.vox", pal_img)
        print(f"{spec['name']}.glb  {len(glb) // 1024} KB  poses: {', '.join(own)}  props: {', '.join(p['name'] for p in spec['props'])}")


def write_prop(pr, glb_path, vox_path, pal_img):
    """standalone prop: one node, origin at the grip"""
    import json, struct
    import numpy as np
    out = B.Out()
    pal = B.Palette(pal_img)
    prims = []
    for tr in (False, True):
        p, n, u, ix = B.mesh_vox(pr["vox"], pal, tr)
        if len(p):
            p = p + np.array(pr.get("off", (0, 0, 0)), "f4") * B.V
            a = {"POSITION": out.acc(p, "VEC3", target=34962, minmax=True), "NORMAL": out.acc(n, "VEC3", target=34962), "TEXCOORD_0": out.acc(u, "VEC2", target=34962)}
            prims.append({"attributes": a, "indices": out.acc(ix, "SCALAR", 5125, target=34963), "material": 1 if tr else 0})
    buf = io.BytesIO()
    pal.img.save(buf, "PNG")
    iv = out.view(buf.getvalue())
    j = {"asset": {"version": "2.0", "generator": "block-avatar build_roasted.py"}, "scene": 0, "scenes": [{"nodes": [0]}],
         "nodes": [{"name": pr["name"], "mesh": 0}], "meshes": [{"name": pr["name"], "primitives": prims}],
         "materials": [{"name": "Voxel", "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "metallicFactor": 0.0, "roughnessFactor": 0.85}},
                       {"name": "VoxelTranslucent", "alphaMode": "BLEND", "doubleSided": True,
                        "pbrMetallicRoughness": {"baseColorTexture": {"index": 0}, "baseColorFactor": [1, 1, 1, 0.5], "metallicFactor": 0.0, "roughnessFactor": 0.3}}],
         "textures": [{"sampler": 0, "source": 0}], "samplers": [{"magFilter": 9728, "minFilter": 9728, "wrapS": 33071, "wrapT": 33071}],
         "images": [{"bufferView": iv, "mimeType": "image/png", "name": "palette"}]}
    if not any(pr2["material"] == 1 for pr2 in prims):
        j["materials"] = j["materials"][:1]
    j["accessors"], j["bufferViews"] = out.accs, out.views
    while len(out.bin) % 4:
        out.bin.append(0)
    j["buffers"] = [{"byteLength": len(out.bin)}]
    js = json.dumps(j, separators=(",", ":")).encode()
    while len(js) % 4:
        js += b" "
    glb = struct.pack("<III", 0x46546C67, 2, 28 + len(js) + len(out.bin)) + struct.pack("<II", len(js), 0x4E4F534A) + js + struct.pack("<II", len(out.bin), 0x004E4942) + bytes(out.bin)
    glb_path.write_bytes(glb)
    B.write_vox(dict(parts={"Hips": pr["vox"]}, tail_at=None), vox_path)


if __name__ == "__main__":
    main()
