#!/usr/bin/env python3
"""The character catalogue for the web: who you can be and what you can wear.

Reads the models that have portraits (assets/portraits/<id>.png), their names from
data/cast_manifest.json, and the wardrobe from src/wardrobe.rs, and writes
data/characters.json and data/wardrobe.json, which the directory serves to the character
creator.

    python3 tools/build_catalogue.py
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
cast = {c["id"]: c for c in json.loads((ROOT / "data/cast_manifest.json").read_text())["characters"]}
GROUPS = {"cat": "Animals", "dog": "Animals", "donkey": "Animals", "rhino": "Animals", "peanut": "Animals"}
NAMES = {"scene_all": "Lead", "peanut": "Peanut", "roasted_miami": "Miami tourist"}

chars = []
for p in sorted((ROOT / "assets/portraits").glob("*.png")):
    cid = p.stem
    if not (ROOT / f"assets/models/{cid}.glb").exists():
        continue
    c = cast.get(cid)
    name = NAMES.get(cid) or (c["name"] if c else cid.replace("roasted_", "").replace("_", " ").capitalize())
    group = GROUPS.get(cid) or ("Roast" if cid.startswith("roasted_") else "Cast")
    chars.append({
        "id": cid,
        "name": name,
        "group": group,
        "model": f"models/{cid}.glb",
        "portrait": f"portraits/{cid}.png",
        "about": (c or {}).get("note", ""),
    })
order = {"Cast": 0, "Roast": 1, "Animals": 2}
chars.sort(key=lambda c: (c["id"] != "scene_all", order[c["group"]], c["name"]))

src = (ROOT / "src/wardrobe.rs").read_text()
body = src[src.index("pub const ITEMS"):]
body = body[:body.index("];")]
items = []
for m in re.finditer(r'it\(\s*"([^"]+)",\s*"([^"]+)",\s*Slot::(\w+),\s*"([^"]+)",?\s*\)', body):
    iid, label, slot, file = m.groups()
    items.append({"id": iid, "label": label, "slot": slot.lower(), "file": file})

(ROOT / "data/characters.json").write_text(json.dumps({"characters": chars}, indent=1) + "\n")
(ROOT / "data/wardrobe.json").write_text(json.dumps({"items": items}, indent=1) + "\n")
print(f"{len(chars)} characters, {len(items)} wardrobe items")
