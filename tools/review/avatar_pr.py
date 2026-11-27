#!/usr/bin/env python3
"""An avatar's review: what it has, whether it's complete, and its pull request's description.

    python3 tools/review/avatar_pr.py info capybara              # its clips, props and entries, as JSON
    python3 tools/review/avatar_pr.py check capybara             # files, data and model complete? (exit 1 if not)
    python3 tools/review/avatar_pr.py check capybara --body pr.md   # ... and the PR description in the avatar format
    python3 tools/review/avatar_pr.py body capybara --sha <commit> [--issue 15] > pr.md

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
    if not i["surfaces"]:
        bad.append("no entry in data/surfaces.json")
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


def main():
    a = sys.argv[1:]
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
