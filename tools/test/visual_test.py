#!/usr/bin/env python3
"""Visual regression tests: render a fixed list of characters and sets with the app's snapshot
mode and compare each picture with its reference in testdata/visual/ref/.

    python3 tools/test/visual_test.py              # render, compare, write testdata/visual/out/report.html
    python3 tools/test/visual_test.py --update     # accept the new pictures as the references
    python3 tools/test/visual_test.py --only night_lead

Needs a release build (cargo build --release) and a display. On a Linux machine without one:
    xvfb-run -s "-screen 0 1600x1000x24" python3 tools/test/visual_test.py
(with Mesa's software Vulkan: VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json).

A picture passes when, against its reference, the average difference is under MEAN_MAX (of 255)
and under CHANGED_MAX of the pixels differ by more than 40. That allows for driver and timing
noise but catches a black window, a missing set, wrong colours or a broken mesh. Reference
pictures depend on the renderer: make them on the machine (or CI runner) that checks them.
"""
import html
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parents[2]
REF = ROOT / "testdata" / "visual" / "ref"
OUT = ROOT / "testdata" / "visual" / "out"
EXE = ROOT / "target" / "release" / ("block_avatar.exe" if os.name == "nt" else "block_avatar")
MEAN_MAX = 2.0
CHANGED_MAX = 0.01

# name: (model, set or None, framing 0-2, background, extra settings)
CASES = {
    "diner_lead": ("models/scene_all.glb", "diner", 1, "studio", {}),
    "night_lead": ("models/scene_all.glb", "night", 1, "studio", {}),
    "rally_full": ("models/scene_all.glb", "rally", 2, "studio", {}),
    "hospital_judge": ("models/judge.glb", "hospital", 1, "studio", {}),
    "studio_dog": ("models/dog.glb", None, 1, "studio", {}),
    "beard_closeup": ("models/rep_beard.glb", "diner", 0, "studio", {}),
    "green_peanut": ("models/peanut.glb", None, 1, "green", {}),
    "yard_blocks": ("models/scene_all.glb", "yard", 2, "studio", {"studio_blur": "0"}),
    "block_gallery": ("models/scene_all.glb", "blocks", 2, "studio", {"studio_blur": "0"}),
    "bevels_off": ("models/scene_all.glb", "diner", 0, "studio", {"bevel": "false"}),
    # a confetti burst on the 120 Hz clock: fired on the first frame, shot 40 ticks later
    "confetti_burst": ("models/scene_all.glb", "diner", 2, "studio", {"snapshot_fx": "confetti"}),
}


def render(name, case, out):
    model, set_, framing, background, extra = case
    home = Path(tempfile.mkdtemp(prefix=f"visual_{name}_"))
    settings = {"model": model, "framing": str(framing), "background": background, "hud": "false", "bubbles": "false", "reactions": "false", "quality": "high", "weather": "off"}
    extra = dict(extra)
    fx = extra.pop("snapshot_fx", None)
    settings.update(extra)
    (home / "avatar_settings.txt").write_text("".join(f"{k}={v}\n" for k, v in settings.items()))
    env = dict(os.environ, BLOCK_AVATAR_HOME=str(home), BLOCK_AVATAR_NO_BROWSER="1")
    args = [str(EXE), "--no-browser", "--port", "7790", "--ask-port", "7791", "--model", model, "--snapshot", str(out)]
    if set_:
        args += ["--snapshot-set", set_]
    if fx:
        args += ["--snapshot-fx", fx]
    try:
        r = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=900)
        ok = r.returncode == 0 and out.exists()
        log = r.stderr[-2000:]
    except subprocess.TimeoutExpired:
        ok, log = False, "timed out"
    shutil.rmtree(home, ignore_errors=True)
    return ok, log


def compare(a, b):
    """(mean difference 0-255, fraction of pixels off by more than 40, amplified diff image)"""
    a, b = Image.open(a).convert("RGB"), Image.open(b).convert("RGB")
    if a.size != b.size:
        return 255.0, 1.0, None
    d = ImageChops.difference(a, b).convert("L")
    hist = d.histogram()
    n = sum(hist)
    mean = sum(i * v for i, v in enumerate(hist)) / n
    changed = sum(hist[41:]) / n
    return mean, changed, d.point(lambda v: min(255, v * 4))


def main():
    update = "--update" in sys.argv
    only = sys.argv[sys.argv.index("--only") + 1].split(",") if "--only" in sys.argv else None
    if not EXE.exists():
        raise SystemExit(f"build first: cargo build --release ({EXE} is missing)")
    OUT.mkdir(parents=True, exist_ok=True)
    REF.mkdir(parents=True, exist_ok=True)
    rows, failed = [], 0
    for name, case in CASES.items():
        if only and name not in only:
            continue
        out = OUT / f"{name}.png"
        out.unlink(missing_ok=True)
        ok, log = render(name, case, out)
        ref = REF / f"{name}.png"
        if not ok:
            failed += 1
            rows.append((name, "no picture", None, None))
            print(f"FAIL {name}: no picture\n{log}")
            continue
        if update or not ref.exists():
            shutil.copy(out, ref)
            rows.append((name, "new reference" if not update else "updated", 0.0, 0.0))
            print(f"REF  {name}")
            continue
        mean, changed, diff = compare(ref, out)
        good = mean <= MEAN_MAX and changed <= CHANGED_MAX
        if diff is not None:
            diff.save(OUT / f"{name}.diff.png")
        failed += not good
        rows.append((name, "pass" if good else "FAIL", mean, changed))
        print(f"{'ok  ' if good else 'FAIL'} {name}: mean {mean:.2f}, changed {changed * 100:.2f}%")
    # a page to look at the pictures side by side
    cells = []
    for name, status, mean, changed in rows:
        stat = status if mean is None else f"{status} (mean {mean:.2f}, changed {changed * 100:.2f}%)"
        imgs = "".join(f'<figure><img src="{src}" alt="{cap} {html.escape(name)}"><figcaption>{cap}</figcaption></figure>' for src, cap in [(f"../ref/{name}.png", "reference"), (f"{name}.png", "now"), (f"{name}.diff.png", "difference x4")])
        cells.append(f'<section class="{"bad" if "FAIL" in status or "no picture" in status else ""}"><h2>{html.escape(name)}: {html.escape(stat)}</h2><div class="row">{imgs}</div></section>')
    (OUT / "report.html").write_text(
        "<!doctype html><meta charset=utf-8><title>Visual tests</title><style>body{font:14px system-ui;margin:16px;background:#fff;color:#111}"
        ".row{display:flex;gap:8px;flex-wrap:wrap}figure{margin:0}img{width:300px;height:auto;border:1px solid #ccc}"
        "section.bad h2{color:#b00}</style><h1>Visual tests</h1>" + "".join(cells)
    )
    print(f"{len(rows) - failed}/{len(rows)} passed; report: {OUT / 'report.html'}")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
