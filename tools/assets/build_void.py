#!/usr/bin/env python3
"""The Void: the first world, where everyone arrives.

An ethereal, flat, luminous space: a pale polished floor that goes on for ever (drawn by the
floor shader; everything below it is solid) in bright fog, three moons faint in the light.
Almost nothing stands in it, so what does looks lost and far away:

    the Circle       where you arrive: twelve pale pillars round you, each with a glowing top
    the First Gate   a lone doorway to the north (-z), standing in nothing; a fixed camera
                     frames it as you walk up, and an echo of the yard appears in it (a portal:
                     walk through and you're there)
    the Ember Door   a black doorway to the west, with an echo of the islands in it
    the Stair        steps rising to the north-east and stopping in mid-air
    the Monoliths    a row of tall slabs far to the north-west, fading into the fog
    the Doors        doorframes scattered far out in every direction
    the Lights       glowing stones on thin stems, leading out from the Circle

    python3 tools/assets/build_void.py        # writes data/levels/void.json

Coordinates are blocks (half a metre); y = 0 is the floor's top.
"""
import json
import math
import random
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "data" / "levels" / "void.json"

boxes = []


def box(block, a, b, **kw):
    boxes.append({"block": block, "from": list(a), "to": list(b), **kw})


def doorway(x, z, facing_x=False, w=8, h=14, block="pearl"):
    """A free-standing doorframe, `w` wide inside, `h` tall."""
    if facing_x:
        box(block, (x, 0, z - w // 2 - 1), (x, h, z - w // 2 - 1))
        box(block, (x, 0, z + w // 2), (x, h, z + w // 2))
        box(block, (x, h, z - w // 2 - 1), (x, h + 1, z + w // 2))
    else:
        box(block, (x - w // 2 - 1, 0, z), (x - w // 2 - 1, h, z))
        box(block, (x + w // 2, 0, z), (x + w // 2, h, z))
        box(block, (x - w // 2 - 1, h, z), (x + w // 2, h + 1, z))


def main():
    rnd = random.Random(31)
    # ---- the Circle: twelve pillars round where you arrive, glowing at the top
    for i in range(12):
        a = i / 12 * math.tau
        x, z = round(math.cos(a) * 22), round(math.sin(a) * 22)
        box("pearl", (x, 0, z), (x + 1, 13, z + 1))
        box("lumen", (x, 14, z), (x + 1, 14, z + 1))
    # a low pale ring set into the floor where you stand
    for i in range(72):
        a = i / 72 * math.tau
        x, z = round(math.cos(a) * 9), round(math.sin(a) * 9)
        box("pearl", (x, 0, z), (x, 0, z))
    # ---- the First Gate, north: an echo of the yard shows in it as you come near
    doorway(0, -40, w=8, h=16)
    # ---- the Ember Door, west: an echo of the islands in their dark fog
    doorway(-38, 0, facing_x=True, w=8, h=14, block="obsidian")
    box("magma", (-38, 0, -6), (-38, 0, -6))
    box("magma", (-38, 0, 5), (-38, 0, 5))
    # ---- the Stair: rising north-east and stopping
    for k in range(18):
        box("pearl", (30 + k, 0, -30 - k), (32 + k, k, -28 - k))
    # ---- the Monoliths, far north-west
    for i in range(7):
        x = -120 + i * 18
        z = -150 - (i % 3) * 7
        h = 40 + (i * 13) % 17
        box("pearl", (x, 0, z), (x + 3, h, z + 9))
    # ---- doorframes far out, every which way
    placed = []
    while len(placed) < 14:
        a = rnd.random() * math.tau
        r = 60 + rnd.random() * 160
        x, z = round(math.cos(a) * r), round(math.sin(a) * r)
        if abs(x) > 236 or abs(z) > 236 or any(math.hypot(x - px, z - pz) < 30 for px, pz in placed):
            continue
        placed.append((x, z))
        doorway(x, z, facing_x=rnd.random() < 0.5, w=rnd.choice([6, 8, 10]), h=rnd.choice([12, 14, 18]))
    # ---- the Lights: glowing stones on thin stems, leading out four ways
    for a in (0.0, math.pi / 2, math.pi, 3 * math.pi / 2):
        for k in range(1, 9):
            r = 30 + k * 16
            x, z = round(math.cos(a + 0.12) * r), round(math.sin(a + 0.12) * r)
            box("pearl", (x, 0, z), (x, 3, z))
            box("lumen", (x, 4, z), (x, 4, z))
    level = {
        "name": "The Void",
        "about": "The first world: an ethereal, flat, luminous space. A pale floor that goes on for ever in "
                 "bright fog, three moons faint in the light, and almost nothing else: the Circle of pillars "
                 "where you arrive, the First Gate to the north (an echo of the yard in it), the Ember Door to the west, a stair to nowhere, monoliths and doorways "
                 "far off in the fog. Made by tools/assets/build_void.py.",
        "world": {
            "sky": "void",
            "time": 0.0,
            "floor": 0,
            "weather": "luminous",
            "camera": {"yaw": 0, "zoom": 14},
            "cameras": [
                {
                    "name": "gate",
                    "zone": {"from": [-14, 0, -36], "to": [14, 10, -20]},
                    "at": [5.5, 2.2, -6.0],
                    "look": [0.0, 4.0, -20.0],
                }
            ],
            # doorways to other worlds (see src/portals.rs): `at` is the middle of the sill,
            # `arrive` the matching point on the other side
            "portals": [
                {"name": "yard", "at": [0, 0, -39.5], "axis": "z", "size": [8, 16],
                 "level": "yard", "url": ":7781", "arrive": [0, 0, 11.5], "yaw": 0},
                {"name": "islands", "at": [-37.5, 0, 0], "axis": "x", "size": [8, 14],
                 "level": "islands", "url": ":7782", "arrive": [-13.5, 0, 0], "yaw": 270},
            ],
            "hotbar": ["pearl", "lumen", "glass", "concrete", "water", "crystal", "planks", "stone", "bricks"],
        },
        "boxes": boxes,
    }
    OUT.write_text(json.dumps(level, indent=1))
    print(f"{OUT}: {len(boxes)} boxes, {OUT.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
