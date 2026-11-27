#!/usr/bin/env python3
"""The character catalogue for the web: who you can be and what you can wear.

Reads the models that have portraits (assets/portraits/<id>.png), their names from
data/cast_manifest.json, and the wardrobe from bins/block-avatar/src/character/wardrobe.rs, and writes
data/characters.json and data/wardrobe.json, which the directory serves to the character
creator.

    python3 tools/assets/build_catalogue.py
"""
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
cast = {c["id"]: c for c in json.loads((ROOT / "data/cast_manifest.json").read_text())["characters"]}
GROUPS = {"blue": "Main", "cat": "Animals", "dog": "Animals", "donkey": "Animals", "rhino": "Animals", "peanut": "Animals", "capybara": "Animals", "brain": "Comics"}
NAMES = {"blue": "Blue", "scene_all": "Lead", "peanut": "Peanut", "roasted_miami": "Miami Travis",
         "roasted_army": "Army peanut", "roasted_cook": "Fry cook peanut", "roasted_garbage": "Garbage collector peanut",
         "roasted_medic": "Medic peanut", "roasted_riot": "Riot peanut", "capybara": "Yuzu", "brain": "The Brain"}

# Vox's main character comes first, and is who a new player starts as
MAIN = "blue"
# from each builder's note (build_animals.py, build_roasted.py) and docs/avatar/manual.md
ABOUT = {"scene_all": "The original lead, no longer the main character: red cap, blue overalls over a white undershirt, short brown beard.",
         "cat": "Grey tabby cat: striped back and crown, white chest and socks, pink nose, whiskers, tall tail.",
         "dog": "Golden-furred dog: cream muzzle and paws, floppy brown ears, red bandana with a gold tag.",
         "donkey": "Donkey in a blue blazer over an open-collar white shirt and jeans; tall ears, dark mane, pale muzzle.",
         "rhino": "Rhino in a navy suit, white shirt, red tie and flag pin; big ivory horn, wrinkled grey hide.",
         "peanut": "Original peanut character: two-lobed dimpled shell, stick limbs, cream hands, orange ball cap, sneakers.",
         "roasted_army": "Roasted peanut as an army man: olive helmet with chin strap, webbing, belt pouches, radio pack, rifle slung on the back, combat boots.",
         "roasted_cook": "Roasted peanut as a fry cook: red uniform, white apron with a name tag, paper fold hat, black work shoes.",
         "roasted_garbage": "Roasted peanut as a garbage collector: lime hi-vis vest with silver reflective bands over navy work clothes, orange cap, yellow work gloves, brown boots.",
         "roasted_medic": "Roasted peanut as a medic: teal scrubs, white vest with a green first-aid cross front and back, stethoscope, headband, blue gloves.",
         "roasted_miami": "Roasted peanut as a 1980s Miami detective: pastel teal jacket with pushed-up sleeves over a pink tee, white pants, loafers without socks, aviator shades.",
         "roasted_riot": "Roasted peanut as a riot officer: black helmet with a clear visor and neck guard, chest and back plates, shoulder and knee pads; clear shield, baton, radio.",
         "brain": "The Brain, from the comic From the Brain of Michael Collins: Michael's brain in a jar, pink with round glasses, on a professor in a tweed jacket and bow tie. Computer scientist, dungeon master, lore expert; fact-checks everything.",
         "capybara": "Yuzu, a calm capybara: warm brown barrel body, big blunt muzzle, small round ears. Chills, soaks in the hot spring with a towel on its head, balances a yuzu, snacks on watermelon and nods off.",
         "blue": "Vox's main character: an American pit bull terrier with a blue nose, a blue-grey coat, white blaze, chest and paws, rose ears and an orange collar."}

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
        "about": ABOUT.get(cid) or (c or {}).get("note", ""),
    })
order = {"Main": -1, "Cast": 0, "Roast": 1, "Animals": 2, "Comics": 3}
chars.sort(key=lambda c: (c["id"] != MAIN, order[c["group"]], c["name"]))

src = (ROOT / "bins/block-avatar/src/character/wardrobe.rs").read_text()
body = src[src.index("pub const ITEMS"):]
body = body[:body.index("];")]
items = []
for m in re.finditer(r'it\(\s*"([^"]+)",\s*"([^"]+)",\s*Slot::(\w+),\s*"([^"]+)",?\s*\)', body):
    iid, label, slot, file = m.groups()
    items.append({"id": iid, "label": label, "slot": slot.lower(), "file": file})

(ROOT / "data/characters.json").write_text(json.dumps({"characters": chars}, indent=1) + "\n")
(ROOT / "data/wardrobe.json").write_text(json.dumps({"items": items}, indent=1) + "\n")
print(f"{len(chars)} characters, {len(items)} wardrobe items")
