#!/usr/bin/env python3
"""Orbit: a station 250 km above the planet, the world turning slowly beneath it.

    the Hub          where you arrive: a deck under a glass dome open at its crown, its outer
                     ring glass underfoot (the planet below), the Down
                     Gate (back to the dark sea) on its west side
    the Mast         an antenna mast and dish standing on the truss east of the Hub
    the Truss        a lattice spine running east and west, with a walkway along it
    the Arrays       four pairs of solar wings off the truss, dark cells in metal frames
    the Radiators    white fins standing up and down from the truss near the Hub
    the Habitats     a long module at each end of the truss, lit inside, with window bands
    the Lander       a small ship docked to the north of the Hub, on a short tunnel

The sky (`"sky": "orbit"`) draws the planet below, its rim 15.8° under eye level, and the sun
and stars; the station is all there is in the world. Nothing is under it but a long way down.

    python3 tools/assets/build_orbit.py        # writes data/levels/orbit.json

Coordinates are blocks (half a metre); you walk at y = 0 (the decks are at y = -1).
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "levels" / "orbit.json"

boxes = []
cells = []
carved = []  # openings: single blocks go in after boxes, so these are cut from both


def carve(a, b):
    box("air", a, b)
    carved.append((a, b))


def box(block, a, b, **kw):
    boxes.append({"block": block, "from": list(a), "to": list(b), **kw})


def put(x, y, z, block):
    cells.append([x, y, z, block])


def disc(block, cx, cz, y, r):
    box(block, (math.floor(cx - r), y, math.floor(cz - r)), (math.ceil(cx + r), y, math.ceil(cz + r)), round=1.0)


def dome(cx, cz, y0, r):
    """A glass hemisphere one block thick, ribbed with metal every 30 degrees, open where the
    truss comes in (east and west) and where the tunnel leaves (north)."""
    for x in range(math.floor(cx - r - 1), math.ceil(cx + r + 1) + 1):
        for z in range(math.floor(cz - r - 1), math.ceil(cz + r + 1) + 1):
            for y in range(y0, y0 + r + 2):
                d = math.sqrt((x + 0.5 - cx) ** 2 + (y - y0 + 0.5) ** 2 + (z + 0.5 - cz) ** 2)
                if not (r - 0.5 <= d < r + 0.5):
                    continue
                dx, dz = x - cx, z - cz
                # an open oculus at the top, rimmed in metal (nothing may stand over the middle:
                # the world's spawn is the top of whatever is there)
                ring = math.hypot(dx + 0.5, dz + 0.5)
                if ring < 4.5:
                    continue
                if ring < 5.5 and y > y0 + r - 2:
                    put(x, y, z, "metal")
                    continue
                # doorways: the truss east and west, the tunnel north
                if y < y0 + 5 and abs(dz) <= 2 and abs(dx) > r - 3:
                    continue
                if y < y0 + 5 and abs(dx) <= 2 and dz < -(r - 3):
                    continue
                a = math.degrees(math.atan2(dz, dx)) % 30
                rib = a < 2.2 or a > 27.8 or y == y0
                put(x, y, z, "metal" if rib else "glass")


def ring_wall(cx, cy, r, z0, z1, along_z=True, window=None):
    """A cylinder one block thick along z, `r` round its axis at (cx, cy); `window(y, z)` picks
    glass for window bands, concrete otherwise; metal caps at both ends."""
    for z in range(z0, z1 + 1):
        for x in range(math.floor(cx - r - 1), math.ceil(cx + r + 1) + 1):
            for y in range(math.floor(cy - r - 1), math.ceil(cy + r + 1) + 1):
                d = math.hypot(x + 0.5 - cx, y + 0.5 - cy)
                end = z in (z0, z1)
                if end and d < r + 0.5:
                    put(x, y, z, "metal")
                elif r - 0.5 <= d < r + 0.5:
                    put(x, y, z, "glass" if window and window(y, z) else "concrete")


def truss(x0, x1):
    """The spine: four metal beams along x with cross bracing every six blocks, and a tiled
    walkway down its middle."""
    lo, hi = min(x0, x1), max(x0, x1)
    for y in (-1, 4):
        for z in (-3, 3):
            box("metal", (lo, y, z), (hi, y, z))
    box("tile", (lo, -1, -2), (hi, -1, 2))
    for x in range(lo, hi + 1, 6):
        box("metal", (x, -1, -3), (x, 4, -3))
        box("metal", (x, -1, 3), (x, 4, 3))
        box("metal", (x, 4, -3), (x, 4, 3))
        # one diagonal per bay, alternating
        for k in range(6):
            y = -1 + k
            put(x + k, y, -3 if (x // 6) % 2 else 3, "metal")
    # lights along the walkway's edge
    for x in range(lo + 3, hi + 1, 12):
        put(x, -1, -2, "lamp")
        put(x, -1, 2, "lamp")


def array_wing(x, side):
    """A solar wing off the truss at `x`, reaching out to `side` (+1 south, -1 north): a mast
    out from the truss, then a sheet of dark cells in a metal frame."""
    z_in, z_out = 6 * side, 62 * side
    zl, zh = min(z_in, z_out), max(z_in, z_out)
    box("metal", (x, 1, 3 * side), (x, 1, z_in))  # the boom
    box("obsidian", (x - 7, 1, zl), (x + 7, 1, zh))  # the cells
    for zz in range(zl, zh + 1, 8):  # the frame
        box("metal", (x - 7, 1, zz), (x + 7, 1, zz))
    box("metal", (x - 8, 1, zl), (x - 8, 1, zh))
    box("metal", (x + 8, 1, zl), (x + 8, 1, zh))


def main():
    # ---- the Hub: a tiled middle, then a ring of glass decking out to the dome, so the
    # planet shows under your feet (from eye height its rim is ten blocks out), crossed by
    # metal spokes
    disc("metal", 0, 0, -1, 21.5)
    disc("glass", 0, 0, -1, 20.5)
    disc("metal", 0, 0, -1, 9.5)
    disc("tile", 0, 0, -1, 8.5)
    disc("metal", 0, 0, -1, 2.5)
    disc("metal", 0, 0, -2, 9.5)  # the underside, under the middle only
    for i in range(8):  # the spokes
        a = i / 8 * math.tau + math.tau / 16
        for k in range(10, 21):
            put(round(math.cos(a) * k), -1, round(math.sin(a) * k), "metal")
    for i in range(8):  # lamps set in the tile, round the middle
        a = i / 8 * math.tau
        put(round(math.cos(a) * 6), -1, round(math.sin(a) * 6), "lamp")
    dome(0, 0, -1, 22)
    # the Down Gate, west: a metal ring with lights, back to the dark sea
    gx = -12
    for dz, top in ((-5, 14), (4, 14)):
        box("metal", (gx, 0, dz), (gx, top, dz))
    box("metal", (gx, 14, -5), (gx, 15, 4))
    for y in (0, 5, 10):
        put(gx, y, -5, "lumen")
        put(gx, y, 4, "lumen")
    # benches and a console facing the view south
    for x in (-6, 6):
        box("concrete", (x - 2, 0, 9), (x + 2, 0, 10))
    box("metal", (-1, 0, 14), (1, 1, 14))
    box("crystal", (-1, 2, 14), (1, 2, 14))
    # the Mast, on the truss east of the Hub, and its dish
    mx = 46
    box("metal", (mx, 5, 0), (mx, 34, 0))
    box("metal", (mx - 1, 5, -1), (mx + 1, 7, 1))
    for y, r in ((35, 2.4), (36, 4.4), (37, 6.2)):
        disc("pearl", mx, 0, y, r)
    box("metal", (mx, 37, 0), (mx, 42, 0))
    put(mx, 43, 0, "lumen")

    # ---- the Truss, east and west, and what hangs off it
    truss(22, 150)
    truss(-150, -22)
    for x in (-118, -66, 66, 118):
        array_wing(x, 1)
        array_wing(x, -1)
    for x in (-34, 34):  # radiators: white fins up and down
        box("pearl", (x - 5, 5, 0), (x + 5, 22, 0))
        box("pearl", (x - 5, -20, 0), (x + 5, -2, 0))
        box("metal", (x - 6, -20, 0), (x - 6, 22, 0))

    # ---- the Habitats, one at each end: cylinders along z with window bands, lit inside
    for hx in (-163, 163):
        ring_wall(hx + 0.5, 3.5, 8, -26, 26, window=lambda y, z: y >= 5 and y <= 7 and (z // 4) % 2 == 0)
        box("tile", (hx - 6, -1, -25), (hx + 6, -1, 25))  # the floor
        for z in range(-21, 22, 7):
            put(hx, 11, z, "lamp")  # lights along the ceiling
        # the way in from the truss: a cut through the wall facing it
        side = -1 if hx > 0 else 1
        wx = hx + side * 8
        carve((wx - 1, 0, -2), (wx + 1, 3, 2))
        box("tile", (min(wx, hx + side * 6), -1, -2), (max(wx, hx + side * 6), -1, 2))
        box("tile", (min(wx, 150 * -side), -1, -2), (max(wx, 150 * -side), -1, 2))
        # bunks and tables
        for z in (-18, -9, 9, 18):
            box("planks", (hx - 5, 0, z), (hx - 3, 0, z + 2))
            box("planks", (hx + 3, 0, z), (hx + 5, 0, z + 2))
        box("concrete", (hx - 1, 0, -2), (hx + 1, 1, 2))

    # ---- the Lander, docked to the north: a tunnel out from the Hub, then the ship
    box("metal", (-3, -1, -40), (3, 4, -22), hollow=True)
    carve((-2, 0, -40), (2, 3, -21))
    box("tile", (-2, -1, -40), (2, -1, -21))
    for z in range(-38, -22, 6):
        put(0, 4, z, "lamp")
    # the ship: a stubby hull pointing north with a glass nose and swept wings
    for z in range(-70, -40):
        t = (z + 70) / 30  # 0 nose .. 1 tail
        r = 2.5 + 4.5 * min(1.0, t * 2.2) ** 0.6
        for x in range(-8, 9):
            for y in range(-6, 9):
                d = math.hypot((x + 0.5) / r, (y - 1 + 0.5) / (r * 0.8))
                if 0.82 <= d < 1.0 or (z in (-41, -70) and d < 1.0):
                    if z < -64 and y >= 0:
                        put(x, y, z, "glass")
                    else:
                        put(x, y, z, "pearl" if y > -2 else "metal")
    for z in range(-56, -42):  # wings
        span = (z + 56) // 2 + 3
        box("pearl", (-6 - span, -1, z), (-6, -1, z))
        box("pearl", (6, -1, z), (6 + span, -1, z))
    box("metal", (-2, -1, -64), (2, -1, -42))  # the cabin floor
    carve((-2, 0, -42), (2, 3, -40))  # the hatch through to the tunnel
    for x in (-3, 3):  # engine glow at the tail
        put(x, 1, -40, "magma")

    level = {
        "name": "Orbit",
        "about": "A station 250 km above the planet, which turns slowly beneath it: the Hub under its "
                 "glass dome (where you arrive, with the Down Gate back to the dark sea), a truss east and "
                 "west with solar wings and radiators, a habitat at each end, and a lander docked to the "
                 "north. Black sky, hard sunlight, stars. Made by tools/assets/build_orbit.py.",
        "world": {
            "sky": "orbit",
            "time": 10.0,
            # no air, so no weather: the world server runs no simulation and sends none
            "weather": "none",
            "portals": [
                {"name": "sea", "at": [gx + 0.5, 0, 0], "axis": "x", "size": [8, 14], "level": "sea",
                 "url": ":7783", "arrive": [8.5, 0, 0], "yaw": 270},
            ],
            "hotbar": ["metal", "glass", "concrete", "tile", "pearl", "lamp", "lumen", "obsidian", "crystal"],
        },
        "boxes": boxes,
        "blocks": [c for c in cells if not any(all(a[i] <= c[i] <= b[i] for i in range(3)) for a, b in carved)],
    }
    OUT.write_text(json.dumps(level, separators=(",", ":")))
    print(f"{OUT}: {len(boxes)} boxes, {len(cells)} blocks, {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
