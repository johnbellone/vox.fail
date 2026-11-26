#!/usr/bin/env python3
"""The Dark Sea: black water under a storm, rolling in long swells to the edge of the fog.

    the Landing      a basalt island where you arrive, with the Tide Door (back to the void)
    the Pier         planks on log posts running north, lanterns burning along it
    the Lighthouse   a stone tower on a rock at the pier's end, its lamp lit
    the Stacks       jagged basalt pillars standing out of the sea, one pair joined in an arch
    the Wreck        a ship's hull to the east, on its side in the shallows, its mast broken

The waves are the water shader's (the level's `"water": {"swell": ...}` sets how high they
roll, `"colour"` how dark the water is, `"opacity"` how much of the seabed shows) and the wind's (`weather`).

    python3 tools/assets/build_sea.py        # writes data/levels/sea.json

Coordinates are blocks (half a metre). Everything is built as if the sea's surface were the
top of y = -1 and the Landing's the top of y = 1, then lowered by `DROP` so that you walk at
y = 0 like every other level (the avatar app stands its character at the origin).
"""
import json
import math
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "levels" / "sea.json"
HALF = 256  # the world's default bounds: -256..255 across
SEA_TOP = -1  # the top layer of water
BED = -16  # nothing below this

DROP = 2  # how far everything is lowered when written

boxes = []
cells = []


def box(block, a, b, **kw):
    boxes.append({"block": block, "from": [a[0], a[1] - DROP, a[2]], "to": [b[0], b[1] - DROP, b[2]], **kw})


def put(x, y, z, block):
    cells.append([x, y - DROP, z, block])


def noise(x, z, scale, seed):
    """Value noise in 0..1, smooth between lattice points `scale` apart."""
    def lattice(i, j):
        h = (i * 374761393 + j * 668265263 + seed * 2147483647) & 0xFFFFFFFF
        h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
        return (h & 0xFFFF) / 0xFFFF

    fx, fz = x / scale, z / scale
    i, j = math.floor(fx), math.floor(fz)
    u, v = fx - i, fz - j
    u, v = u * u * (3 - 2 * u), v * v * (3 - 2 * v)
    a, b = lattice(i, j), lattice(i + 1, j)
    c, d = lattice(i, j + 1), lattice(i + 1, j + 1)
    return (a * (1 - u) + b * u) * (1 - v) + (c * (1 - u) + d * u) * v


def disc(block, cx, cz, y, r):
    """One layer of a round shape: a flat disc `r` blocks across from its middle."""
    r = max(r, 0.6)
    box(block, (math.floor(cx - r), y, math.floor(cz - r)), (math.ceil(cx + r), y, math.ceil(cz + r)), round=1.0)


def rock(cx, cz, top, radius, rnd, block="basalt", taper=0.18):
    """A rock rising from the seabed: discs widening downwards, a little ragged."""
    for y in range(BED, top + 1):
        r = radius + (top - y) * taper + rnd.uniform(-0.6, 0.6)
        disc(block, cx + rnd.uniform(-0.5, 0.5), cz + rnd.uniform(-0.5, 0.5), y, r)


def ring(cx, cz, y0, y1, r, block, door=None):
    """A round wall one block thick, `r` from its middle; `door` (dx, dz) leaves a gap there."""
    for y in range(y0, y1 + 1):
        for x in range(math.floor(cx - r - 1), math.ceil(cx + r + 1) + 1):
            for z in range(math.floor(cz - r - 1), math.ceil(cz + r + 1) + 1):
                d = math.hypot(x + 0.5 - cx, z + 0.5 - cz)
                if r - 0.5 <= d < r + 0.5:
                    if door and y < y0 + 5 and (x - cx) * door[0] + (z - cz) * door[1] > r * 0.8:
                        continue
                    put(x, y, z, block(y) if callable(block) else block)


def doorway(x, z, w=8, h=14, block="obsidian", y=0):
    """A doorframe facing along x, `w` wide inside, `h` tall, standing on `y`."""
    box(block, (x, y, z - w // 2 - 1), (x, y + h, z - w // 2 - 1))
    box(block, (x, y, z + w // 2), (x, y + h, z + w // 2))
    box(block, (x, y + h, z - w // 2 - 1), (x, y + h + 1, z + w // 2))


def main():
    rnd = random.Random(7)
    # ---- the sea: water everywhere, in tiles (a level's boxes are capped in size)
    for x in range(-HALF, HALF, 128):
        for z in range(-HALF, HALF, 128):
            box("water", (x, BED, z), (x + 127, SEA_TOP, z + 127))
    # ---- the seabed: dark ash and gravel, rolling, shallower near the Landing
    step = 8
    for x in range(-HALF, HALF, step):
        for z in range(-HALF, HALF, step):
            n = noise(x, z, 64, 1) * 0.7 + noise(x, z, 19, 2) * 0.3
            near = max(0.0, 1.0 - math.hypot(x, z) / 90.0)
            top = round(-13 + n * 6 + near * 4)
            block = "gravel" if noise(x, z, 23, 3) > 0.62 else "ash"
            box(block, (x, BED, z), (x + step - 1, top, z + step - 1))

    # ---- the Landing: where you arrive, a metre above the water
    land = random.Random(11)
    for y in range(BED, 2):
        r = 15 + max(0, -y) * 0.35 + land.uniform(-0.4, 0.4)
        disc("basalt", 0, 0, y, r)
    for _ in range(26):  # ash and cobbles worn into the top
        a, d = land.random() * math.tau, land.random() * 12
        x, z = round(math.cos(a) * d), round(math.sin(a) * d)
        box(land.choice(["ash", "cobblestone", "stone"]), (x, 1, z), (x + land.randint(0, 2), 1, z + land.randint(0, 2)))
    # boulders round its edge
    for i in range(9):
        a = i / 9 * math.tau + 0.4
        x, z = math.cos(a) * 14.5, math.sin(a) * 14.5
        if abs(x) < 4 and z < 0:  # leave the pier's way clear
            continue
        if x < -5 and abs(z) < 7:  # and the door's
            continue
        disc("basalt", x, z, 2, 1.4)
        disc("basalt", x, z, 3, 0.8)
    # the Tide Door, west, back to the void; braziers either side
    doorway(-8, 0, w=8, h=14, block="obsidian", y=2)
    for dz in (-7, 6):
        box("basalt", (-8, 2, dz), (-8, 3, dz))
        box("fire", (-8, 4, dz), (-8, 4, dz))

    # ---- the Pier: north from the Landing to the Lighthouse
    z0, z1 = -14, -100
    box("planks", (-1, 1, z1), (1, 1, z0))
    for z in range(z0, z1 - 1, -8):
        for x in (-2, 2):
            box("log", (x, BED + 4, z), (x, 2, z))
        if (z - z0) % 16 == 0:  # a lantern on every other pair of posts
            box("log", (-2, 3, z), (-2, 3, z))
            box("fire", (-2, 4, z), (-2, 4, z))
    # a few planks missing, one sagging
    for z in (-37, -38, -61):
        box("air", (1, 1, z), (1, 1, z))
    box("planks", (1, 0, -38), (1, 0, -37))

    # ---- the Lighthouse, on a rock at the pier's end
    lx, lz = 0.5, -112.5
    rock(lx, lz, 1, 11, random.Random(5), taper=0.25)
    box("planks", (-1, 1, -102), (1, 1, -100))
    bands = lambda y: "bricks" if (y // 6) % 2 else "stone"
    ring(lx, lz, 2, 41, 4.6, bands, door=(0, 1))
    # floors inside every ten blocks, a lamp room on top
    for y in (11, 21, 31):
        disc("planks", lx, lz, y, 3.9)
    disc("stone", lx, lz, 42, 6.2)  # the gallery
    ring(lx, lz, 43, 43, 6.2, "metal")  # its railing
    ring(lx, lz, 43, 47, 3.6, "glass")
    box("lamp", (-1, 43, -114), (1, 45, -112))
    box("fire", (0, 46, -113), (0, 46, -113))
    for y, r in ((48, 4.6), (49, 3.4), (50, 2.2), (51, 1.0)):
        disc("basalt", lx, lz, y, r)

    # ---- the Stacks: jagged pillars out of the sea
    srnd = random.Random(23)
    placed = [(0, 0, 30), (0, -112, 22), (72, 34, 24)]
    stacks = []
    while len(stacks) < 15:
        a, d = srnd.random() * math.tau, 40 + srnd.random() * 190
        x, z = math.cos(a) * d, math.sin(a) * d
        if abs(x) > 236 or abs(z) > 236 or abs(x) < 10 and -110 < z < 0:
            continue
        if any(math.hypot(x - px, z - pz) < pr for px, pz, pr in placed):
            continue
        r = 2 + srnd.random() * 3.5
        top = round(6 + srnd.random() * 34)
        placed.append((x, z, r + 14))
        stacks.append((x, z, r, top))
        rock(x, z, top, r, srnd, block=srnd.choice(["basalt", "basalt", "obsidian"]), taper=0.12)
    # the Arch: two stacks joined at the top, south-west of the Landing
    ax, az = -46, 52
    for sx in (ax - 9, ax + 9):
        rock(sx, az, 22, 3.2, srnd, taper=0.1)
    for x in range(ax - 10, ax + 11):
        sag = round(2.5 * math.cos((x - ax) / 10 * math.pi / 2))
        box("basalt", (x, 20 + sag, az - 2), (x, 23 + sag, az + 2))

    # ---- the Wreck: a hull on its side in the shallows, east
    wx, wz, length = 72, 34, 34
    shallows = random.Random(3)
    for y in range(BED, -8):
        disc("gravel", wx, wz, y, 14 + (-8 - y) * 0.5 + shallows.uniform(-0.5, 0.5))
    for i in range(length):
        z = wz - length // 2 + i
        t = (i / (length - 1)) * 2 - 1  # -1 stern .. 1 bow
        half = max(1.0, 6.5 * (1 - t * t) ** 0.6)
        for x in range(-round(half), round(half) + 1):
            # the hull's curve, then rolled on its side (y up the deck, leaning east)
            depth = round(6 * max(0.0, 1 - (x / half) ** 2) ** 0.5)
            lean = round(x * 0.45)
            y_keel = -8 - depth + lean
            put(wx + x, y_keel, z, "planks")
            if abs(x) >= round(half) - 1:  # the sides, up to the gunwale
                for y in range(y_keel, 2 + lean):
                    if not (i % 7 == 3 and y == y_keel + 2):  # holes stove in
                        put(wx + x, y, z, "planks")
    # the broken mast, leaning off to the north-east, and its stump
    for k in range(22):
        put(wx + 1 + round(k * 0.55), -4 + k, wz - 3 - round(k * 0.3), "log")
    box("log", (wx, -8, wz + 6), (wx, 0, wz + 6))

    level = {
        "name": "The Dark Sea",
        "about": "Black water under a storm, rolling in long swells to the edge of the fog. The Landing "
                 "(where you arrive) with the Tide Door back to the void; a lantern-lit pier north to the "
                 "Lighthouse; stacks of basalt standing out of the sea, an arch, and a wreck in the shallows "
                 "to the east. Made by tools/assets/build_sea.py.",
        "world": {
            "sky": "sea",
            "time": 0.0,
            "weather": {"humidity": 1.04, "temperature": 7.0, "wind_speed": 10.0, "wind_from": 250.0,
                        "gusts": 0.7, "heating": 0.0, "wetness": 1.0, "ground": -1.5},
            # how the sea looks: its colour (multiplied with the water tile), how high the
            # swell rolls (metres from the calm surface to a crest), and how much it hides the
            # seabed (deep and dark: almost all of it)
            "water": {"colour": [0.07, 0.11, 0.13], "swell": 0.75, "opacity": 0.94},
            "portals": [
                {"name": "void", "at": [-7.5, 2 - DROP, 0], "axis": "x", "size": [8, 14], "level": "void",
                 "url": ":7780", "arrive": [38.5, 0, 0], "yaw": 90},
            ],
            "hotbar": ["basalt", "obsidian", "planks", "log", "stone", "glass", "lamp", "fire", "water"],
        },
        "boxes": boxes,
        "blocks": cells,
    }
    OUT.write_text(json.dumps(level, separators=(",", ":")))
    print(f"{OUT}: {len(boxes)} boxes, {len(cells)} blocks, {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
