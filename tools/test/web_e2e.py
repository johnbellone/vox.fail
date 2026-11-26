#!/usr/bin/env python3
"""End to end in a browser: make an account, make a character, walk into the Void.

Needs a directory on :7700 and the Void on :7780 started with the directory's key, and the
browser client built (tools/dev/build_web.sh). Uses Chromium with WebGPU on SwiftShader, on a
display (DISPLAY=:99 with Xvfb on a server).

    python3 tools/test/web_e2e.py OUT_DIR [--name "Test Player"]

Writes screenshots of each step to OUT_DIR and exits non-zero if a step fails.
"""
import asyncio
import random
import sys
from pathlib import Path

from playwright.async_api import async_playwright

BASE = "http://127.0.0.1:7700"
GPU = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader",
       "--use-webgpu-adapter=swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"]


async def main(out: Path, name: str):
    out.mkdir(parents=True, exist_ok=True)
    async with async_playwright() as p:
        # headed (on a display, e.g. Xvfb): headless Chromium doesn't put WebGPU canvases in
        # its screenshots
        b = await p.chromium.launch(args=GPU, headless=False)
        pg = await b.new_page(viewport={"width": 1280, "height": 800})
        logs = []
        pg.on("console", lambda m: logs.append(f"{m.type}: {m.text}"))
        pg.on("pageerror", lambda e: logs.append(f"pageerror: {e}"))
        try:
            await pg.goto(BASE + "/")
            await pg.click("#tab-up")
            await pg.fill("#name", name)
            await pg.fill("#password", "a long password")
            await pg.click("#go")
            await pg.wait_for_url("**/create", timeout=10000)
            await pg.wait_for_timeout(3000)
            await pg.click('button[data-model="models/officer.glb"]')
            await pg.click('#hat button[data-id="crown"]')
            await pg.wait_for_timeout(3000)
            await pg.screenshot(path=str(out / "1_create.png"))
            await pg.click("#save")
            await pg.wait_for_url(BASE + "/", timeout=10000)
            await pg.wait_for_selector("#enter", state="visible")
            await pg.screenshot(path=str(out / "2_home.png"))
            await pg.click("#enter")
            await pg.wait_for_url("**/play", timeout=10000)
            # the client says when it's in
            await pg.wait_for_selector("#opening.gone", state="attached", timeout=240000)
            await pg.wait_for_timeout(8000)
            await pg.screenshot(path=str(out / "3_play.png"))
            # walk: hold the mouse down ahead of the character, then W for a while
            await pg.mouse.move(640, 200)
            await pg.mouse.down()
            await pg.wait_for_timeout(3000)
            await pg.mouse.up()
            await pg.keyboard.down("w")
            await pg.wait_for_timeout(3000)
            await pg.keyboard.up("w")
            await pg.wait_for_timeout(2000)
            await pg.screenshot(path=str(out / "4_walked.png"))
            print("ok")
        except Exception as e:
            await pg.screenshot(path=str(out / "failed.png"))
            print("FAILED:", e)
            (out / "console.log").write_text("\n".join(logs))
            await b.close()
            sys.exit(1)
        (out / "console.log").write_text("\n".join(logs))
        await b.close()


if __name__ == "__main__":
    args = sys.argv[1:]
    name = args[args.index("--name") + 1] if "--name" in args else f"Tester {random.randint(100, 999)}"
    asyncio.run(main(Path(args[0]), name))
