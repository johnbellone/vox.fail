#!/usr/bin/env python3
"""The in-game review sheet of an avatar: the avatar app's own renderer (snapshot mode), full body
on a stage set, standing and in each of its own poses with their props.

    python3 tools/review/ingame.py capybara                       # writes docs/avatar/capybara-ingame.png
    python3 tools/review/ingame.py capybara --exe target/debug/block_avatar --set diner --out sheet.png

Needs a built block_avatar (release by default; a debug build works and takes less disk) and a
display. On a Linux machine without one, with Mesa's software Vulkan:

    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \\
      xvfb-run -a -s "-screen 0 1600x1000x24" python3 tools/review/ingame.py capybara

Each own clip is held a little past its middle (`--snapshot-do play:<clip> --snapshot-at <s>`).
"""
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

sys.path.insert(0, str(Path(__file__).resolve().parent))
from avatar_pr import info  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def shot(exe, cid, out, set_, clip=None, at=0.0):
    """One snapshot; True if a picture was saved."""
    home = Path(tempfile.mkdtemp(prefix="avatar-home-"))
    try:
        # full body (framing 2), whatever the machine's own settings say
        (home / "avatar_settings.txt").write_text("framing=2\n")
        env = dict(os.environ, BLOCK_AVATAR_HOME=str(home), BLOCK_AVATAR_NO_BROWSER="1")
        args = [str(exe), "--no-browser", "--port", "7790", "--ask-port", "7791", "--model", f"models/{cid}.glb",
                "--snapshot", str(out), "--snapshot-set", set_]
        if clip:
            args += ["--snapshot-do", f"play:{clip}", "--snapshot-at", f"{at:.2f}"]
        r = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=600)
        if r.returncode != 0 or not out.exists():
            print(f"  {clip or 'standing'}: failed\n{r.stderr[-1500:]}", file=sys.stderr)
            return False
        return True
    finally:
        shutil.rmtree(home, ignore_errors=True)


def font(size):
    for f in ("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
              "C:/Windows/Fonts/arialbd.ttf"):
        if Path(f).exists():
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


def main():
    a = sys.argv[1:]
    if not a or a[0].startswith("-"):
        sys.exit(__doc__)
    cid = a[0]
    opt = lambda k, d: a[a.index(k) + 1] if k in a and a.index(k) + 1 < len(a) else d
    exe = Path(opt("--exe", ROOT / "target/release/block_avatar"))
    set_ = opt("--set", "diner")
    out = Path(opt("--out", ROOT / "docs/avatar" / f"{cid}-ingame.png"))
    if not exe.exists():
        sys.exit(f"build the app first: cargo build --release -p block_avatar ({exe} is missing)")
    own = info(cid)["own"]
    shots = [("Standing", None, 0.0)] + [(c["name"], c["name"], round(c["seconds"] * 0.55, 2)) for c in own]
    tmp = Path(tempfile.mkdtemp(prefix="avatar-shots-"))
    pics = []
    try:
        for i, (label, clip, at) in enumerate(shots):
            p = tmp / f"{i}.png"
            print(f"  {label}…", file=sys.stderr)
            if not shot(exe, cid, p, set_, clip, at):
                sys.exit(1)
            pics.append((label, Image.open(p).convert("RGB")))
        w, h = pics[0][1].size
        cols = 3
        W, H = w // 2, h // 2
        rows = (len(pics) + cols - 1) // cols
        sheet = Image.new("RGB", (W * cols, (H + 34) * rows), (32, 36, 44))
        d, f = ImageDraw.Draw(sheet), font(18)
        for i, (label, im) in enumerate(pics):
            x, y = (i % cols) * W, (i // cols) * (H + 34)
            sheet.paste(im.resize((W, H)), (x, y))
            d.text((x + (W - d.textlength(label, font=f)) / 2, y + H + 7), label, fill=(232, 236, 242), font=f)
        out.parent.mkdir(parents=True, exist_ok=True)
        sheet.save(out, optimize=True)
        print(f"{out}: {len(pics)} pictures on the {set_} set")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
