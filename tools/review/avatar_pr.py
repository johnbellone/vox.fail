#!/usr/bin/env python3
"""An avatar's review: what it has, whether it's complete, and its pull request's description.

    python3 tools/review/avatar_pr.py info capybara              # its clips, props and entries, as JSON
    python3 tools/review/avatar_pr.py check capybara             # files, data and model complete? (exit 1 if not)
    python3 tools/review/avatar_pr.py check capybara --body pr.md   # ... and the PR description in the avatar format
    python3 tools/review/avatar_pr.py body capybara --sha <commit> [--issue 15] > pr.md
    python3 tools/review/avatar_pr.py audit [--md]               # every character: names, files, clips, faces, props

The avatar format (docs/avatar/avatar-workflow.md): the portrait, the in-game sheet and the
close-up sheet at the top (images pinned to a commit), the avatar and its own poses as a table
(pose, face, prop), how it was made, and what was checked. `body` writes one from the model;
`check --body` checks one a person or an agent wrote.
"""
import json
import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REPO = "johnbellone/vox.fail"
# clips every character on the block rig has (100 body, 24 face); an avatar's own come after
SHARED_CLIPS = 124
FACE_PARTS = {"eyes", "brows", "mouth"}


def load_glb(path):
    b = path.read_bytes()
    if b[:4] != b"glTF":
        raise ValueError(f"{path} is not a binary glTF")
    n = struct.unpack("<I", b[12:16])[0]
    j = json.loads(b[20:20 + n])
    binlen = struct.unpack("<I", b[20 + n:24 + n])[0]
    return j, b[28 + n:28 + n + binlen]


def accessor(j, binb, i):
    import numpy as np
    a = j["accessors"][i]
    v = j["bufferViews"][a["bufferView"]]
    comp = {5126: ("f4", 4), 5123: ("u2", 2), 5125: ("u4", 4), 5121: ("u1", 1)}[a["componentType"]]
    n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
    off = v.get("byteOffset", 0) + a.get("byteOffset", 0)
    return np.frombuffer(binb[off:off + a["count"] * n * comp[1]], dtype=comp[0]).reshape(a["count"], n)


def info(cid):
    """What the avatar `cid` has: its own clips (with the face parts and props each shows), its
    props, and its catalogue entry."""
    glb = ROOT / "assets/models" / f"{cid}.glb"
    j, binb = load_glb(glb)
    names = [nd.get("name", "") for nd in j["nodes"]]
    props = [n[5:] for n in names if n.startswith("prop_")]
    clips = []
    for a in j.get("animations", [])[SHARED_CLIPS:]:
        dur, shown, faces = 0.0, set(), []
        for ch in a["channels"]:
            s = a["samplers"][ch["sampler"]]
            t = accessor(j, binb, s["input"])
            dur = max(dur, float(t[-1][0]))
            node = names[ch["target"]["node"]]
            if ch["target"]["path"] != "scale":
                continue
            on = bool((accessor(j, binb, s["output"]) > 0.5).any())
            if node.startswith("prop_") and on:
                shown.add(node[5:])
            elif node.split("_")[0] in FACE_PARTS and on and node not in ("eyes_open", "brows_normal", "mouth_flat"):
                faces.append(node)
        clips.append({"name": a["name"], "seconds": round(dur, 2), "props": sorted(shown), "faces": sorted(set(faces))})
    chars = json.loads((ROOT / "data/characters.json").read_text())
    chars = chars["characters"] if isinstance(chars, dict) else chars
    entry = next((c for c in chars if c["id"] == cid), None)
    return {
        "id": cid,
        "clips": len(j.get("animations", [])),
        "own": clips,
        "props": props,
        "entry": entry,
        "head_fit": cid + ".glb" in json.loads((ROOT / "data/head_fit.json").read_text()),
        "surfaces": f"models/{cid}.glb" in json.loads((ROOT / "data/surfaces.json").read_text()),
        "bytes": glb.stat().st_size,
    }


def check(cid, body=None):
    """Problems with the avatar `cid` (and its PR description `body`, if given); empty if none."""
    bad = []
    files = {
        "model": f"assets/models/{cid}.glb",
        "voxel source": f"source/vox/characters/{cid}.vox",
        "portrait": f"assets/portraits/{cid}.png",
        "close-up sheet": f"docs/avatar/{cid}-poses.png",
        "in-game sheet": f"docs/avatar/{cid}-ingame.png",
    }
    for what, f in files.items():
        if not (ROOT / f).exists():
            bad.append(f"missing the {what}: {f}")
    if not re.fullmatch(r"[a-z][a-z0-9_]{1,31}", cid):
        bad.append(f"'{cid}' isn't a plain id (lower-case letters, digits, _)")
    if bad and not (ROOT / files["model"]).exists():
        return bad
    try:
        i = info(cid)
    except Exception as e:  # a model that won't load is the finding
        return bad + [f"the model doesn't load: {e}"]
    if i["clips"] < SHARED_CLIPS + 1:
        bad.append(f"the model has {i['clips']} clips: the {SHARED_CLIPS} shared ones and at least one of its own are needed")
    if len(i["own"]) > 8:
        bad.append(f"{len(i['own'])} own clips: at most 8 (F9-F12 play the first four; the rest are on the control page)")
    for c in i["own"]:
        if not c["faces"]:
            bad.append(f"own clip '{c['name']}' has no facial animation (every pose changes the face)")
    for p in i["props"]:
        if not any(p in c["props"] for c in i["own"]):
            bad.append(f"prop '{p}' is never shown by any of its clips")
    if not i["entry"]:
        bad.append("no entry in data/characters.json (run tools/assets/build_catalogue.py)")
    elif not i["entry"].get("about"):
        bad.append("its data/characters.json entry has no 'about'")
    if not i["head_fit"]:
        bad.append("no entry in data/head_fit.json (hats won't sit on it)")
    if not i["surfaces"] and cid not in _surface_skip():
        bad.append("no entry in data/surfaces.json (or the id in guess_surfaces.py's SKIP, with a reason)")
    manual = (ROOT / "docs/avatar/manual.md").read_text()
    if f"`{cid}.glb`" not in manual:
        bad.append(f"docs/avatar/manual.md doesn't list `{cid}.glb`")
    if i["bytes"] > 8 * 1024 * 1024:
        bad.append(f"the model is {i['bytes'] // 1024} KB: keep it under 8 MB")
    if body is not None:
        bad += check_body(cid, body, i)
    return bad


def check_body(cid, body, i):
    """Problems with a PR description against the avatar format."""
    bad = []
    for img in (f"assets/portraits/{cid}.png", f"docs/avatar/{cid}-ingame.png", f"docs/avatar/{cid}-poses.png"):
        m = re.search(r"https://github\.com/" + re.escape(REPO) + r"/blob/([0-9a-f]{40})/" + re.escape(img) + r"\?raw=true", body)
        if not m:
            bad.append(f"the description doesn't show {img} (pinned to a commit: /blob/<sha>/{img}?raw=true)")
    for heading in ("## How it's made", "## Checked"):
        if heading not in body:
            bad.append(f"the description has no '{heading}' section")
    rows = {r.strip().split("|")[1].strip() for r in body.splitlines() if r.strip().startswith("|") and r.count("|") >= 4}
    for c in i["own"]:
        if c["name"] not in rows:
            bad.append(f"the poses table doesn't list '{c['name']}'")
    return bad


def body(cid, sha, issue=None):
    """A PR description in the avatar format, from the model and its catalogue entry."""
    i = info(cid)
    name = (i["entry"] or {}).get("name", cid.capitalize())
    about = (i["entry"] or {}).get("about", "")
    img = lambda p: f"https://github.com/{REPO}/blob/{sha}/{p}?raw=true"
    face = lambda c: ", ".join(f.replace("_", " ") for f in c["faces"]) or "—"
    rows = "\n".join(f"| {c['name']} | {c['seconds']} s | {face(c)} | {', '.join(c['props']) or ''} |" for c in i["own"])
    keys = "The first four are on F9–F12" + ("; the rest are on the control page." if len(i["own"]) > 4 else ".")
    closes = f"\n\nCloses #{issue}." if issue else ""
    return f"""A new avatar, {name}, on the shared block rig.{closes}

<img src="{img(f'assets/portraits/{cid}.png')}" width="160" alt="{name}'s portrait">

**In game:** the avatar app's own renderer, on the diner set, in each of its own poses.

![{name} in the app]({img(f'docs/avatar/{cid}-ingame.png')})

**Close up:** its own poses, two shared clips and eight face animations, rendered from `assets/models/{cid}.glb`.

![{name}'s poses, props and faces]({img(f'docs/avatar/{cid}-poses.png')})

## {name}

{about}

It plays all {SHARED_CLIPS} shared clips (100 body, 24 face) and has {len(i['own'])} of its own. {keys}

| Pose | Length | Face | Prop |
|---|---|---|---|
{rows}

Props on the model: {', '.join(i['props']) or 'none'}.

## How it's made

From the asset scripts, not by hand: `tools/assets/build_animals.py` (the model and its `.vox`), `guess_surfaces.py`, `make_portraits.py`, `build_catalogue.py`; the sheets with `tools/review/render_sheet.mjs` and `tools/review/ingame.py`. See `docs/avatar/avatar-workflow.md`.

## Checked

- `python3 tools/review/avatar_pr.py check {cid} --body <this description>`: passes.
- The close-up sheet and the in-game sheet above.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
"""


# ---- every character

# the 24 shared face clips ("Face: Blink" ...) that the expressions and reactions play by name
def _clip_names(cid):
    j, _ = load_glb(ROOT / "assets/models" / f"{cid}.glb")
    return [a["name"] for a in j.get("animations", [])]


def _surface_skip():
    """The ids tools/assets/guess_surfaces.py skips on purpose."""
    import ast, re
    m = re.search(r"^SKIP = (\{.*?\})", (ROOT / "tools/assets/guess_surfaces.py").read_text(), re.M | re.S)
    return ast.literal_eval(m.group(1)) if m else set()


def audit():
    """Every character in data/characters.json, checked against its own family: (rows, problems).

    Families: the lead and the characters built on the full block rig (the 100 shared body clips,
    the 24 shared face clips, then their own) and the cast (built from data/cast_manifest.json: the
    100 body clips, then their own). Everyone needs the 23 face parts, a name, a portrait, a voxel
    source, a head fit and their own clips; every prop must be shown by some clip, and every own
    clip should change the face."""
    chars = json.loads((ROOT / "data/characters.json").read_text())
    chars = chars["characters"] if isinstance(chars, dict) else chars
    lead = _clip_names("scene_all")
    body, faces = lead[:100], [c for c in lead if c.startswith("Face: ")]
    cast = {c["id"]: c for c in json.loads((ROOT / "data/cast_manifest.json").read_text())["characters"]}
    fit = json.loads((ROOT / "data/head_fit.json").read_text())
    surfaces = json.loads((ROOT / "data/surfaces.json").read_text())
    names = [c["name"] for c in chars]
    rows, problems = [], []
    for c in chars:
        cid, bad = c["id"], []
        j, binb = load_glb(ROOT / "assets/models" / f"{cid}.glb")
        nodes = [nd.get("name", "") for nd in j["nodes"]]
        anims = j.get("animations", [])
        clips = [a["name"] for a in anims]
        own = [a for a in anims if a["name"] not in lead] if cid != "scene_all" else []
        props = [n[5:] for n in nodes if n.startswith("prop_")]
        face_parts = [n for n in nodes if n.split("_")[0] in FACE_PARTS]
        shown, faceless = set(), []
        for a in anims:
            changes = set()
            for ch in a["channels"]:
                if ch["target"]["path"] != "scale":
                    continue
                node = nodes[ch["target"]["node"]]
                on = bool((accessor(j, binb, a["samplers"][ch["sampler"]]["output"]) > 0.5).any())
                if node.startswith("prop_") and on:
                    shown.add(node[5:])
                elif node.split("_")[0] in FACE_PARTS and on and node not in ("eyes_open", "brows_normal", "mouth_flat"):
                    changes.add(node)
            # the cast's clips come pre-built (no builder in this repo) and some keep a straight face
            # on purpose (No comment); the rule is for avatars built by build_animals/build_roasted
            if a in own and not changes and cid not in cast:
                faceless.append(a["name"])
        # names and words
        if not c.get("name") or c["name"] == cid:
            bad.append("no display name of its own")
        if names.count(c["name"]) > 1:
            bad.append(f"its name '{c['name']}' is used twice")
        if not c.get("about"):
            bad.append("no 'about' (the character picker shows nothing)")
        # files and data
        vox = "lead" if cid == "scene_all" else cid
        for what, f in (("portrait", f"assets/portraits/{cid}.png"), ("voxel source", f"source/vox/characters/{vox}.vox")):
            if not (ROOT / f).exists():
                bad.append(f"no {what} ({f})")
        if cid + ".glb" not in fit:
            bad.append("no head fit (hats float or sink)")
        # guess_surfaces.py leaves out its SKIP list on purpose (flat-painted on purpose)
        if f"models/{cid}.glb" not in surfaces and cid != "scene_all" and cid not in _surface_skip():
            bad.append("no surfaces entry (no fur or cloth texture)")
        # clips
        missing_body = [b for b in body if b not in clips]
        missing_faces = [f for f in faces if f not in clips]
        if missing_body:
            bad.append(f"{len(missing_body)} of the 100 shared body clips missing")
        # the app sets face parts directly (character/face.rs); the Face: clips only feed the
        # review sheet, so the cast (which never had them) is fine
        if missing_faces and cid not in cast:
            bad.append(f"{len(missing_faces)} of the {len(faces)} shared face clips missing (the review sheet's face row is empty)")
        if len(face_parts) < 23:
            bad.append(f"{len(face_parts)} face parts, not 23 (lip sync and expressions need them all)")
        if cid != "scene_all" and not own:
            bad.append("no clips of its own (F9-F12 do nothing)")
        if cid in cast and len(own) != len(cast[cid].get("own_clips", [])):
            bad.append(f"{len(own)} own clips, but data/cast_manifest.json lists {len(cast[cid]['own_clips'])}")
        if faceless:
            bad.append(f"own clips that never change the face: {', '.join(faceless)}")
        unshown = [p for p in props if p not in shown]
        if unshown:
            bad.append(f"props no clip shows: {', '.join(unshown)}")
        rows.append({"id": cid, "name": c["name"], "group": c["group"], "clips": len(clips), "own": [a["name"] for a in own],
                     "props": props, "face_parts": len(face_parts), "problems": bad})
        problems += [f"{cid}: {b}" for b in bad]
    return rows, problems


def main():
    a = sys.argv[1:]
    if a and a[0] == "audit":
        rows, problems = audit()
        if "--md" in a:
            print("| Id | Name | Group | Clips | Own clips | Props | Problems |\n|---|---|---|---|---|---|---|")
            for r in rows:
                print(f"| `{r['id']}` | {r['name']} | {r['group']} | {r['clips']} | {', '.join(r['own']) or '—'} | {len(r['props'])} | {'; '.join(r['problems']) or 'none'} |")
        else:
            for p in problems:
                print(f"- {p}")
        print(f"\n{len(rows)} characters, {sum(1 for r in rows if r['problems'])} with problems, {len(problems)} problems", file=sys.stderr)
        sys.exit(1 if problems else 0)
    if len(a) < 2 or a[0] not in ("info", "check", "body"):
        sys.exit(__doc__)
    cmd, cid = a[0], a[1]
    opt = lambda k: a[a.index(k) + 1] if k in a and a.index(k) + 1 < len(a) else None
    if cmd == "info":
        print(json.dumps(info(cid), indent=1))
    elif cmd == "check":
        b = opt("--body")
        problems = check(cid, Path(b).read_text() if b else None)
        for p in problems:
            print(f"- {p}")
        print(f"{cid}: {'ok' if not problems else f'{len(problems)} problem(s)'}")
        sys.exit(1 if problems else 0)
    else:
        sha = opt("--sha")
        if not sha or not re.fullmatch(r"[0-9a-f]{40}", sha):
            sys.exit("body needs --sha <the commit the images are in, 40 hex>")
        print(body(cid, sha, opt("--issue")), end="")


if __name__ == "__main__":
    main()
