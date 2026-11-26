#!/usr/bin/env python3
"""The Islands: floating islands in dark fog (once the first world; now the Void is a luminous
flat space, see build_void.py).

Floating islands in fog that goes on for ever, three moons faint through it, joined by basalt bridges lit with fire:

    The Threshold    (0, 0)      where you arrive: an obsidian plaza with a ring of glowing
                                 runes, four fire braziers and the First Gate to the north
    Ember Wastes     east        ash plains, magma pools, burning dead trees
    Crystal Hollow   west        a dark bowl of violet crystal spires, no fire at all
    The Shattered Keep north     a ruined castle on a high island, a beacon on its spire
    The Dark         north-east  a maze of basalt columns in near darkness, one fire in it
    Magma Falls      south-west  a magma river pouring off the island's edge into the void

plus scattered islets (some burning, some with crystals), embers glowing far below, and a ring
of obsidian shards at the world's edge. About 480 x 480 blocks (240 m across at half-metre
blocks), like a medium Minecraft map.

    python3 tools/assets/build_islands.py        # writes data/levels/islands.json

Coordinates are blocks: (0, -1, 0) is the ground where you arrive, -z is north (ahead).
The output is the level format (boxes), with identical neighbouring blocks merged into big
boxes so the file stays small.
"""
import json
import math
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "levels" / "islands.json"

# world bounds (the server's): x, z in [-256, 256), y in [-32, 96)
X0, Y0, Z0 = -256, -32, -256
SX, SY, SZ = 512, 128, 512

NAMES = ["air", "obsidian", "basalt", "magma", "ash", "crystal", "fire", "stone", "cobblestone", "bricks", "log", "gravel", "lamp"]
ID = {n: i for i, n in enumerate(NAMES)}

W = np.zeros((SY, SZ, SX), dtype=np.uint8)  # [y, z, x]
RNG = np.random.default_rng(1729)


def inb(x, y, z):
    return X0 <= x < X0 + SX and Y0 <= y < Y0 + SY and Z0 <= z < Z0 + SZ


def put(x, y, z, name):
    if inb(x, y, z):
        W[y - Y0, z - Z0, x - X0] = ID[name]


def get(x, y, z):
    return NAMES[W[y - Y0, z - Z0, x - X0]] if inb(x, y, z) else "air"


def box(a, b, name):
    (x0, y0, z0), (x1, y1, z1) = a, b
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    z0, z1 = sorted((z0, z1))
    x0, y0, z0 = max(x0, X0), max(y0, Y0), max(z0, Z0)
    x1, y1, z1 = min(x1, X0 + SX - 1), min(y1, Y0 + SY - 1), min(z1, Z0 + SZ - 1)
    if x0 > x1 or y0 > y1 or z0 > z1:
        return
    W[y0 - Y0:y1 - Y0 + 1, z0 - Z0:z1 - Z0 + 1, x0 - X0:x1 - X0 + 1] = ID[name]


def top(x, z, below=95):
    """The highest solid (not fire) block in a column at or below `below`, or None."""
    if not (X0 <= x < X0 + SX and Z0 <= z < Z0 + SZ):
        return None
    col = W[: below - Y0 + 1, z - Z0, x - X0]
    idx = np.nonzero((col != 0) & (col != ID["fire"]))[0]
    return int(idx[-1]) + Y0 if len(idx) else None


# ---------------------------------------------------------------- noise


class Noise:
    """Smooth value noise on a lattice, sampled at any (x, z) arrays."""

    def __init__(self, seed, cell):
        self.g = np.random.default_rng(seed).random((80, 80))
        self.cell = cell

    def __call__(self, x, z):
        u = np.asarray(x, float) / self.cell + 40.0
        v = np.asarray(z, float) / self.cell + 40.0
        i, j = np.floor(u).astype(int) % 79, np.floor(v).astype(int) % 79
        fu, fv = u - np.floor(u), v - np.floor(v)
        fu, fv = fu * fu * (3 - 2 * fu), fv * fv * (3 - 2 * fv)
        g = self.g
        a = g[i, j] * (1 - fu) + g[i + 1, j] * fu
        b = g[i, j + 1] * (1 - fu) + g[i + 1, j + 1] * fu
        return a * (1 - fv) + b * fv


def fbm(seed, cell):
    ns = [Noise(seed + k, cell / (2 ** k)) for k in range(3)]

    def f(x, z):
        return (ns[0](x, z) * 0.57 + ns[1](x, z) * 0.29 + ns[2](x, z) * 0.14)

    return f


# ---------------------------------------------------------------- islands


def island(cx, cz, ytop, radius, depth, surface, under, core, seed, bump=1.5, flat=0.0, bowl=0.0):
    """A floating island: a rough disc of ground on top of an upside-down cone of rock with
    hanging spikes. `flat` (0-1): the middle part that stays perfectly level. `bowl`: how much
    the middle sinks. Returns the surface height per column as a dict."""
    edge = fbm(seed, radius * 0.6)
    lump = fbm(seed + 10, 9.0)
    spike = fbm(seed + 20, 4.0)
    mix = fbm(seed + 30, 7.0)
    r = int(radius * 1.35) + 2
    heights = {}
    for x in range(cx - r, cx + r + 1):
        dx = x - cx
        zs = np.arange(cz - r, cz + r + 1)
        dz = zs - cz
        d = np.sqrt(dx * dx + dz * dz) / radius
        d = d * (1.0 + 0.32 * (edge(np.full_like(zs, x), zs) - 0.5))
        for z, dd in zip(zs, d):
            if dd >= 1.0:
                continue
            level = 0.0 if dd < flat else bump * (lump(x, z) - 0.35) * min(1.0, (dd - flat) * 3 + 0.2)
            level -= bowl * max(0.0, 1.0 - dd / 0.75) ** 1.5
            # the rim drops away a little
            level -= max(0.0, dd - 0.88) * 8.0
            yt = ytop + int(round(level))
            body = depth * (1 - dd * dd) ** 0.6 * (0.62 + 0.38 * spike(x, z))
            if spike(x + 300, z) > 0.68:
                body += (spike(x + 300, z) - 0.68) * depth * 2.2
            yb = int(round(ytop - body))
            for y in range(yb, yt + 1):
                if y == yt:
                    name = surface
                elif y >= yt - 2:
                    name = under
                else:
                    name = core if mix(x, z + y * 7) < 0.62 else "obsidian"
                put(x, y, int(z), name)
            heights[(x, int(z))] = yt
    return heights


def brazier(x, y, z, height=3, post="basalt", bowl="magma"):
    """A post with a burning bowl on top; returns the fire's cell."""
    box((x, y, z), (x, y + height - 1, z), post)
    put(x, y + height, z, bowl)
    put(x, y + height + 1, z, "fire")
    return (x, y + height + 1, z)


def bridge(a, b, width=3, block="basalt", edge_block="obsidian", posts=16, seed=0):
    """A straight bridge from a to b (each (x, y, z) on an island's surface), stepping up or
    down at most one block every two along, with fire posts on alternate sides."""
    (ax, ay, az), (bx, by, bz) = a, b
    length = math.hypot(bx - ax, bz - az)
    n = int(length * 2) + 1
    ux, uz = (bx - ax) / length, (bz - az) / length
    px, pz = -uz, ux
    half = width / 2.0
    last_post = -posts / 2
    side = 1
    for i in range(n + 1):
        t = i / n
        x, z = ax + (bx - ax) * t, az + (bz - az) * t
        y = int(round(ay + (by - ay) * t))
        for k in np.linspace(-half - 0.6, half + 0.6, int(width * 3) + 3):
            cx, cz = int(math.floor(x + px * k)), int(math.floor(z + pz * k))
            name = edge_block if abs(k) > half - 0.5 else block
            if get(cx, y, cz) == "air" and get(cx, y + 1, cz) == "air":
                put(cx, y, cz, name)
                # a keel underneath
                if abs(k) < 0.8:
                    put(cx, y - 1, cz, "basalt")
        along = t * length
        if along - last_post >= posts and 4 < along < length - 4:
            last_post = along
            side = -side
            cx, cz = int(math.floor(x + px * (half + 0.3) * side)), int(math.floor(z + pz * (half + 0.3) * side))
            if get(cx, y + 1, cz) == "air":
                brazier(cx, y + 1, cz, height=2)


def edge_point(h, cx, cz, towards):
    """Where to land a bridge on an island (heights h): two blocks in from its edge on the
    side facing a point."""
    tx, tz = towards
    dx, dz = tx - cx, tz - cz
    L = math.hypot(dx, dz)
    # walk out from the centre to the last column with ground
    last = None
    for s in range(0, int(L)):
        x, z = int(round(cx + dx / L * s)), int(round(cz + dz / L * s))
        if (x, z) in h:
            last = (x, h[(x, z)], z)
        elif last is not None:
            break
    x, y, z = last
    # back off two blocks so the bridge lands on solid ground
    return (int(round(x - dx / L * 2)), y, int(round(z - dz / L * 2)))


# ---------------------------------------------------------------- the places


def threshold():
    h = island(0, 0, -1, 24, 22, "obsidian", "basalt", "basalt", seed=100, bump=1.2, flat=0.72)
    # the plaza: polished obsidian, a basalt ring, glowing runes
    for x in range(-18, 19):
        for z in range(-18, 19):
            d = math.hypot(x, z)
            if d <= 17.5:
                put(x, -1, z, "obsidian")
                if 15.5 <= d <= 16.5:
                    put(x, -1, z, "basalt")
                if 10.2 <= d <= 11.0:
                    put(x, -1, z, "magma")
                # four rune spokes from the ring inwards, stopping short of where you stand
                if 4.5 <= d <= 10.2 and (abs(x) <= 0 or abs(z) <= 0):
                    put(x, -1, z, "magma")
    fires = []
    for sx, sz in [(1, 1), (1, -1), (-1, 1), (-1, -1)]:
        fires.append(brazier(13 * sx, 0, 13 * sz, height=3))
    # the First Gate, north: an obsidian arch with crystal at its shoulders
    for x in range(-6, 7):
        for y in range(0, 14):
            for z in (-21, -20):
                inside = abs(x) <= 3 and y <= 9
                if not inside:
                    put(x, y, z, "obsidian")
    for x in (-5, 5):
        for y in (11, 12, 13):
            put(x, y, -22, "crystal")
    put(0, 14, -21, "crystal")
    put(0, 14, -20, "crystal")
    # fire bowls at its feet
    for x in (-5, 5):
        put(x, -1, -18, "magma")
        put(x, 0, -18, "fire")
    # the Pale Door, west on the plaza: a doorway back to the void (its echo glows in it)
    for z in (-4, 3):
        box((-14, 0, z), (-14, 9, z), "obsidian")
    box((-14, 10, -4), (-14, 10, 3), "obsidian")
    put(-14, 11, -1, "crystal")
    put(-14, 11, 0, "crystal")
    # broken obsidian stones lying about
    for x, z in [(19, 6), (-17, -9), (8, 19), (-12, 17)]:
        box((x, 0, z), (x + 1, 1, z), "obsidian")
    return h


def ember_wastes():
    cx, cz = 122, 30
    h = island(cx, cz, 3, 44, 26, "ash", "ash", "basalt", seed=200, bump=3.0)
    pools = fbm(210, 11.0)
    burn = np.random.default_rng(211)
    for (x, z), y in h.items():
        p = pools(x, z)
        if p > 0.66 and math.hypot(x - cx, z - cz) < 38:
            put(x, y, z, "air")
            put(x, y - 1, z, "magma")
            h[(x, z)] = y - 1
        elif p > 0.6:
            put(x, y, z, "gravel" if burn.random() < 0.3 else "basalt")
    # burning dead trees
    for _ in range(26):
        a, r = burn.random() * math.tau, burn.random() * 36
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) not in h or get(x, h[(x, z)], z) == "magma":
            continue
        y = h[(x, z)] + 1
        tall = int(burn.integers(6, 12))
        box((x, y, z), (x, y + tall - 1, z), "log")
        for _ in range(int(burn.integers(2, 5))):
            bx, bz = [(1, 0), (-1, 0), (0, 1), (0, -1)][int(burn.integers(0, 4))]
            by = y + int(burn.integers(tall // 2, tall))
            ln = int(burn.integers(2, 4))
            for k in range(1, ln + 1):
                put(x + bx * k, by + (k // 2), z + bz * k, "log")
            if burn.random() < 0.7:
                put(x + bx * ln, by + ln // 2 + 1, z + bz * ln, "fire")
        if burn.random() < 0.6:
            put(x, y + tall, z, "fire")
    # fires burning on the ash
    for _ in range(70):
        a, r = burn.random() * math.tau, burn.random() * 40
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) in h and get(x, h[(x, z)], z) in ("ash", "basalt", "gravel"):
            put(x, h[(x, z)] + 1, z, "fire")
    # basalt boulders
    for _ in range(14):
        a, r = burn.random() * math.tau, burn.random() * 38
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) in h:
            y = h[(x, z)]
            s = int(burn.integers(1, 3))
            box((x - s, y, z - s), (x + s, y + s + 1, z + s), "basalt")
    return h, (cx, cz)


def crystal_hollow():
    cx, cz = -125, -55
    h = island(cx, cz, 9, 36, 30, "obsidian", "basalt", "basalt", seed=300, bump=2.0, bowl=6.0)
    g = np.random.default_rng(301)
    for _ in range(16):
        a, r = g.random() * math.tau, g.random() * 26
        x0, z0 = cx + math.cos(a) * r, cz + math.sin(a) * r
        if (int(x0), int(z0)) not in h:
            continue
        y0 = h[(int(x0), int(z0))]
        tall = int(g.integers(7, 24))
        lean = (g.random() - 0.5) * 0.35, (g.random() - 0.5) * 0.35
        rad = 1.6 + g.random() * 1.6
        for k in range(tall):
            rr = rad * (1 - k / tall) ** 0.8 + 0.3
            xc, zc = x0 + lean[0] * k, z0 + lean[1] * k
            for x in range(int(xc - rr) - 1, int(xc + rr) + 2):
                for z in range(int(zc - rr) - 1, int(zc + rr) + 2):
                    if math.hypot(x + 0.5 - xc, z + 0.5 - zc) <= rr:
                        put(x, y0 + 1 + k, z, "crystal")
    # small shards
    for _ in range(60):
        a, r = g.random() * math.tau, g.random() * 33
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) in h:
            box((x, h[(x, z)] + 1, z), (x, h[(x, z)] + int(g.integers(1, 4)), z), "crystal")
    return h, (cx, cz)


def shattered_keep():
    cx, cz = 35, -150
    h = island(cx, cz, 15, 32, 28, "cobblestone", "stone", "stone", seed=400, bump=1.0, flat=0.55)
    g = np.random.default_rng(401)
    fires = []
    # the curtain wall: 30 x 30, broken
    for x in range(cx - 15, cx + 16):
        for z in range(cz - 15, cz + 16):
            if x in (cx - 15, cx + 15) or z in (cz - 15, cz + 15):
                gap = (z == cz + 15 and abs(x - cx) <= 2)
                if gap:
                    continue
                wall = 10 + int(3 * math.sin(x * 0.7) * math.cos(z * 0.5))
                if g.random() < 0.12:
                    wall = int(g.integers(2, 6))
                for y in range(16, 16 + wall):
                    put(x, y, z, "bricks" if (y + x + z) % 5 else "cobblestone")
    # corner towers, broken at different heights
    for sx, sz in [(1, 1), (1, -1), (-1, 1), (-1, -1)]:
        tx, tz = cx + 15 * sx, cz + 15 * sz
        tall = int(g.integers(14, 24))
        for y in range(16, 16 + tall):
            for x in range(tx - 3, tx + 4):
                for z in range(tz - 3, tz + 4):
                    d = math.hypot(x - tx, z - tz)
                    if 2.0 <= d <= 3.4 and not (y > 16 + tall - 3 and g.random() < 0.4):
                        put(x, y, z, "stone")
        fires.append(brazier(tx, 16 + tall, tz, height=1, post="stone"))
    # the spire: obsidian, tapering, a beacon of fire on a magma cap
    for y in range(16, 52):
        rr = 3.2 * (1 - (y - 16) / 40) + 0.8
        for x in range(cx - 4, cx + 5):
            for z in range(cz - 4, cz + 5):
                if math.hypot(x - cx, z - cz) <= rr:
                    put(x, y, z, "obsidian")
    box((cx - 1, 52, cz - 1), (cx + 1, 52, cz + 1), "magma")
    for x in range(cx - 1, cx + 2):
        for z in range(cz - 1, cz + 2):
            put(x, 53, z, "fire")
    # fires along the walls inside
    for k in range(-12, 13, 6):
        for (x, z) in [(cx + k, cz - 13), (cx + k, cz + 13), (cx - 13, cz + k), (cx + 13, cz + k)]:
            if (x, z) in h:
                fires.append(brazier(x, h[(x, z)] + 1, z, height=2, post="cobblestone"))
    # rubble
    for _ in range(40):
        a, r = g.random() * math.tau, g.random() * 28
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) in h and get(x, h[(x, z)] + 1, z) == "air":
            put(x, h[(x, z)] + 1, z, "cobblestone" if g.random() < 0.6 else "stone")
    return h, (cx, cz)


def the_dark():
    cx, cz = 165, -112
    h = island(cx, cz, -5, 38, 24, "obsidian", "ash", "basalt", seed=500, bump=0.8, flat=0.6)
    g = np.random.default_rng(501)
    # a maze of basalt columns and walls
    for x in range(cx - 30, cx + 31, 4):
        for z in range(cz - 30, cz + 31, 4):
            if (x, z) not in h or math.hypot(x - cx, z - cz) < 5:
                continue
            y = h[(x, z)] + 1
            tall = int(g.integers(5, 10))
            box((x, y, z), (x, y + tall, z), "basalt")
            dx, dz = [(1, 0), (0, 1)][int(g.integers(0, 2))]
            if g.random() < 0.75:
                for k in range(1, 4):
                    if (x + dx * k, z + dz * k) in h:
                        box((x + dx * k, y, z + dz * k), (x + dx * k, y + tall - int(g.integers(0, 3)), z + dz * k), "basalt")
    # one fire at the heart, on a pedestal
    fire = brazier(cx, h[(cx, cz)] + 1, cz, height=2, post="obsidian")
    # a few crystals hidden in corners
    for _ in range(6):
        a, r = g.random() * math.tau, 15 + g.random() * 15
        x, z = int(cx + math.cos(a) * r), int(cz + math.sin(a) * r)
        if (x, z) in h and get(x, h[(x, z)] + 1, z) == "air":
            put(x, h[(x, z)] + 1, z, "crystal")
    return h, (cx, cz)


def magma_falls():
    cx, cz = -95, 115
    h = island(cx, cz, 22, 30, 26, "ash", "basalt", "basalt", seed=600, bump=2.0)
    g = np.random.default_rng(601)
    # a basalt cliff along the west
    for (x, z), y in list(h.items()):
        if x < cx - 12:
            rise = int(min(9, (cx - 12 - x) * 0.9 + 2 * math.sin(z * 0.4)))
            if rise > 0:
                box((x, y + 1, z), (x, y + rise, z), "basalt")
    # the river: from the middle to the east edge, then over
    edge_x = None
    for x in range(cx - 4, cx + 40):
        zc = cz + int(round(3 * math.sin((x - cx) * 0.18)))
        found = False
        for z in range(zc - 1, zc + 2):
            if (x, z) in h:
                y = h[(x, z)]
                put(x, y, z, "air")
                put(x, y - 1, z, "magma")
                put(x, y - 2, z, "basalt")
                found = True
        if found:
            edge_x = (x, zc, h.get((x, zc), 22))
            if g.random() < 0.18:
                put(x, h.get((x, zc + 3), 22) + 1, zc + 3, "fire")
        elif edge_x is not None:
            break
    # the fall: a curtain of magma from the lip down into the void
    if edge_x:
        x, zc, y = edge_x
        for yy in range(-30, y - 1):
            for z in range(zc - 1, zc + 2):
                put(x + 1, yy, z, "magma")
        # a glowing pool far below catches it
        island(x + 1, zc, -30, 6, 2, "magma", "basalt", "basalt", seed=602, bump=0.3)
    return h, (cx, cz)


def islets(avoid):
    g = np.random.default_rng(700)
    placed = []
    tries = 0
    while len(placed) < 44 and tries < 4000:
        tries += 1
        x, z = int(g.integers(-225, 226)), int(g.integers(-225, 226))
        r = int(g.integers(3, 10))
        if any(math.hypot(x - ax, z - az) < ar + r + 8 for ax, az, ar in avoid + placed):
            continue
        y = int(g.integers(-22, 40))
        h = island(x, z, y, r, r * 2 + 3, "basalt" if g.random() < 0.5 else "ash", "basalt", "obsidian", seed=710 + tries, bump=1.0)
        if not h:
            continue
        placed.append((x, z, r))
        if g.random() < 0.4 and (x, z) in h:
            brazier(x, h[(x, z)] + 1, z, height=1)
        elif g.random() < 0.3 and (x, z) in h:
            box((x, h[(x, z)] + 1, z), (x, h[(x, z)] + int(g.integers(2, 6)), z), "crystal")
    # embers far below
    for _ in range(8):
        x, z = int(g.integers(-200, 200)), int(g.integers(-200, 200))
        if any(math.hypot(x - ax, z - az) < ar + 6 for ax, az, ar in avoid):
            continue
        island(x, z, int(g.integers(-30, -24)), int(g.integers(3, 7)), 3, "magma", "basalt", "basalt", seed=800 + x, bump=0.5)


def shard_ring():
    g = np.random.default_rng(900)
    for i in range(26):
        a = i / 26 * math.tau + g.random() * 0.1
        r = 214 + g.random() * 24
        x0, z0 = int(math.cos(a) * r), int(math.sin(a) * r)
        tall = int(g.integers(14, 46))
        ybase = int(g.integers(-24, 30))
        lean = (g.random() - 0.5) * 0.5, (g.random() - 0.5) * 0.5
        w = 1.4 + g.random() * 1.6
        for k in range(tall):
            # thickest a third of the way up, pointed at both ends
            t = k / tall
            rr = w * math.sin(math.pi * min(1.0, t * 1.4 if t < 0.35 else 1.0 - (t - 0.35) / 0.65 * 0.98) ** 0.7) + 0.4
            xc, zc = x0 + lean[0] * k, z0 + lean[1] * k
            for x in range(int(xc - rr) - 1, int(xc + rr) + 2):
                for z in range(int(zc - rr) - 1, int(zc + rr) + 2):
                    if math.hypot(x + 0.5 - xc, z + 0.5 - zc) <= rr:
                        put(x, ybase + k, z, "obsidian")
        if g.random() < 0.35:
            put(int(x0 + lean[0] * tall), ybase + tall, int(z0 + lean[1] * tall), "crystal")


# ---------------------------------------------------------------- merge into boxes


def to_boxes():
    """Greedy merge: runs along x, then rectangles across z, then boxes across y."""
    boxes = []
    open3 = {}
    for yi in range(SY):
        layer = W[yi]
        rows = np.nonzero(layer.any(axis=1))[0]
        rects = {}
        open2 = {}
        prev_z = None
        for zi in list(rows) + [None]:
            runs = set()
            if zi is not None:
                row = layer[zi]
                change = np.nonzero(np.diff(row.astype(np.int16)))[0] + 1
                starts = np.concatenate([[0], change])
                ends = np.concatenate([change, [SX]])
                for s, e in zip(starts, ends):
                    if row[s] != 0:
                        runs.add((int(s), int(e) - 1, int(row[s])))
            contiguous = zi is not None and prev_z is not None and zi == prev_z + 1
            for key in list(open2):
                if not contiguous or key not in runs:
                    z0 = open2.pop(key)
                    rects[(key[0], key[1], z0, prev_z, key[2])] = True
            for key in runs:
                if key not in open2:
                    open2[key] = zi
            prev_z = zi
        for key in list(open3):
            if key not in rects:
                y0 = open3.pop(key)
                boxes.append((key, y0, yi - 1))
        for key in rects:
            if key not in open3:
                open3[key] = yi
    for key, y0 in open3.items():
        boxes.append((key, y0, SY - 1))
    out = []
    for (x0, x1, z0, z1, b), y0, y1 in boxes:
        out.append({
            "block": NAMES[b],
            "from": [int(x0) + X0, int(y0) + Y0, int(z0) + Z0],
            "to": [int(x1) + X0, int(y1) + Y0, int(z1) + Z0],
        })
    return out


def main():
    hs = threshold()
    ember, ember_c = ember_wastes()
    crystal, crystal_c = crystal_hollow()
    keep, keep_c = shattered_keep()
    dark, dark_c = the_dark()
    falls, falls_c = magma_falls()
    # bridges between them
    def link(h1, c1, h2, c2, seed):
        a = edge_point(h1, c1[0], c1[1], c2)
        b = edge_point(h2, c2[0], c2[1], c1)
        bridge(a, b, seed=seed)

    origin = (0, 0)
    # north through the gate to the keep: start just beyond the gate
    bridge(edge_point(hs, 0, 0, keep_c), edge_point(keep, keep_c[0], keep_c[1], origin), seed=1)
    link(hs, origin, ember, ember_c, 2)
    link(hs, origin, crystal, crystal_c, 3)
    link(keep, keep_c, dark, dark_c, 4)
    link(crystal, crystal_c, falls, falls_c, 5)
    link(hs, origin, falls, falls_c, 6)
    islets([(0, 0, 30), (ember_c[0], ember_c[1], 52), (crystal_c[0], crystal_c[1], 44), (keep_c[0], keep_c[1], 40),
            (dark_c[0], dark_c[1], 46), (falls_c[0], falls_c[1], 40), (40, -70, 14), (60, 15, 10), (-60, -25, 10),
            (100, -130, 10), (-50, 60, 12), (-110, 30, 14)])
    shard_ring()
    # keep where you arrive clear, and nothing overhead
    W[0 - Y0:, 0 - Z0, 0 - X0] = 0
    box((0, -1, 0), (0, -1, 0), "obsidian")
    count = int((W != 0).sum())
    fires = int((W == ID["fire"]).sum())
    level = {
        "name": "The Islands",
        "about": "Floating islands in endless dark fog, three moons faint through it. "
                 "The Threshold (where you arrive) with the First Gate north, Ember Wastes east, Crystal Hollow west, "
                 "the Shattered Keep north, The Dark north-east and Magma Falls south-west, joined by basalt bridges "
                 "lit with fire; the Pale Door on the plaza leads back to the void. Made by tools/assets/build_islands.py.",
        "world": {"sky": "islands", "time": 0.0,
                  "weather": {"humidity": 1.02, "temperature": 8.0, "wind_speed": 2.0, "gusts": 0.5, "heating": 0.0,
                              "wetness": 0.9, "ground": -16.0},
                  # back to the void through the Pale Door (see src/portals.rs)
                  "portals": [{"name": "void", "at": [-13.5, 0, 0], "axis": "x", "size": [6, 10], "level": "void",
                               "url": ":7780", "arrive": [-37.5, 0, 0], "yaw": 270}],
                  "hotbar": ["obsidian", "basalt", "magma", "ash", "crystal", "fire", "stone", "cobblestone", "bricks"]},
        "boxes": to_boxes(),
    }
    OUT.write_text(json.dumps(level, separators=(",", ":")))
    print(f"{OUT}: {count} blocks ({fires} fires), {len(level['boxes'])} boxes, {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
