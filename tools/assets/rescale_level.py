#!/usr/bin/env python3
"""Rescale a level file to smaller blocks, keeping everything its size in metres.

Blocks went from half a metre to a quarter (docs/design/block-scale.md). The level builders
still draw in half-metre units and pass their level through `rescale(level, 2)` before writing
it; levels made by hand (the Crypt) were converted once with this script.

With factor k, every place and size in blocks is multiplied by k:

    boxes      a box over blocks lo..hi covers k*lo .. k*hi + k - 1; a hollow box keeps its
               shell its thickness in metres ("thick": k); a rounded one keeps its roundness
    blocks     each single block becomes a k*k*k box (written after the boxes, as blocks were)
    world      portals (at, size, arrive), cameras (zone; `at` and `look` are metres and stay),
               the objective (at, radius), the floor, and the level's bounds

Metres stay as they are: weather, water, the camera's zoom, times and colours.

    python3 tools/assets/rescale_level.py data/levels/dungeons/crypt.json     # in place, x2
    python3 tools/assets/rescale_level.py --factor 2 in.json out.json
"""
import argparse
import json
from pathlib import Path


def _scale3(v, k):
    return [c * k for c in v]


def _box(b, k):
    lo = [min(a, c) for a, c in zip(b["from"], b["to"])]
    hi = [max(a, c) for a, c in zip(b["from"], b["to"])]
    out = {key: val for key, val in b.items() if key not in ("from", "to")}
    out["from"] = [c * k for c in lo]
    out["to"] = [c * k + k - 1 for c in hi]
    if b.get("hollow"):
        out["thick"] = b.get("thick", 1) * k
    return out


def rescale(level, k):
    """`level` (a parsed level file) with every place and size in blocks multiplied by `k`."""
    if not isinstance(k, int) or k < 1:
        raise ValueError(f"the factor must be a whole number from 1, not {k!r}")
    out = dict(level)
    boxes = [_box(b, k) for b in level.get("boxes", [])]
    # single blocks became boxes, after the level's own boxes (load applies boxes, then
    # blocks, so they still win over the boxes)
    for x, y, z, name in level.get("blocks", []):
        boxes.append({"block": name, "from": [x * k, y * k, z * k], "to": [x * k + k - 1, y * k + k - 1, z * k + k - 1]})
    out["boxes"] = boxes
    out.pop("blocks", None)
    if "bounds" in level:
        out["bounds"] = {"lo": _scale3(level["bounds"]["lo"], k), "hi": _scale3(level["bounds"]["hi"], k)}
    world = level.get("world")
    if isinstance(world, dict):
        w = dict(world)
        if "floor" in w:
            w["floor"] = w["floor"] * k
        if "portals" in w:
            w["portals"] = [_portal(p, k) for p in w["portals"]]
        if "cameras" in w:
            w["cameras"] = [_camera(c, k) for c in w["cameras"]]
        if "objective" in w:
            o = dict(w["objective"])
            o["at"] = _scale3(o["at"], k)
            o["radius"] = o["radius"] * k
            w["objective"] = o
        out["world"] = w
    return out


def _portal(p, k):
    p = dict(p)
    for key in ("at", "arrive"):
        if key in p:
            p[key] = _scale3(p[key], k)
    if "size" in p:
        p["size"] = [c * k for c in p["size"]]
    return p


def _camera(c, k):
    c = dict(c)
    if "zone" in c:
        z = c["zone"]
        # the zone is inclusive of its `to` block, like a box
        lo = [min(a, b) for a, b in zip(z["from"], z["to"])]
        hi = [max(a, b) for a, b in zip(z["from"], z["to"])]
        c["zone"] = {"from": [v * k for v in lo], "to": [v * k + k - 1 for v in hi]}
    return c


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("src", type=Path)
    ap.add_argument("dst", type=Path, nargs="?", help="where to write it (default: over src)")
    ap.add_argument("--factor", type=int, default=2)
    a = ap.parse_args()
    level = json.loads(a.src.read_text())
    out = rescale(level, a.factor)
    dst = a.dst or a.src
    dst.write_text(json.dumps(out, indent=1))
    print(f"{dst}: x{a.factor}, {len(out['boxes'])} boxes")


if __name__ == "__main__":
    main()
