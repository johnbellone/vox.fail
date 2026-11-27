#!/usr/bin/env python3
"""End to end in a browser: the world editor places a portal, and a second page sees it.

Needs what web_e2e.py needs, plus a world editor that can edit the yard: a directory on :7700
started with `--world yard` and the yard world started with the directory's key (so /edit can
sign in and the editor is the world's own), and the browser client built (tools/dev/build_web.sh).
It cannot run against tools/dev/play-local.sh, whose guest worlds have no sign-in: /edit sends
guests to the start page. Uses Chromium with WebGPU on SwiftShader, on a display (DISPLAY=:99
with Xvfb on a server), and needs playwright (pip install playwright; playwright install chromium).

    python3 tools/test/editor_e2e.py OUT_DIR [--base http://127.0.0.1:7700]

The flow: sign up an account and make a character; open /edit?world=yard; hold the Portal tool,
mark a spot on the canvas and create a portal toward the void (or pair it with one of the
account's own portals when no world is allowed); see its row in the entity list. A second
browser context signs up another account and sees that row on its own /edit?world=yard page.
Then the first account marks a spot outside the plot and the refusal's reason shows in the form.

Writes a screenshot of each step and console.log to OUT_DIR. Exits non-zero if a step fails.
With no arguments it prints this usage and exits 0; playwright is imported only for a run.
"""
import asyncio
import random
import re
import sys
from pathlib import Path

GPU = ["--enable-unsafe-webgpu", "--enable-features=Vulkan", "--use-vulkan=swiftshader",
       "--use-webgpu-adapter=swiftshader", "--use-angle=swiftshader", "--ignore-gpu-blocklist"]
DEFAULT_BASE = "http://127.0.0.1:7700"
EDIT = "/edit?world=yard"
# the editor's entity buttons read `Portal "<name>"` (labelOf in features/editor/state.ts)
PORTAL_NAME_MAX = 32


def usage() -> None:
    print(__doc__.strip())


async def sign_up(pg, base: str, name: str) -> None:
    """Makes an account and a character, as web_e2e.py does, and ends on the start page."""
    await pg.goto(base + "/")
    await pg.click("#tab-up")
    await pg.fill("#name", name)
    await pg.fill("#password", "a long password")
    await pg.click("#go")
    await pg.wait_for_url("**/create", timeout=10000)
    await pg.wait_for_timeout(3000)
    await pg.click('button[data-model="models/officer.glb"]')
    await pg.click('#hat button[data-id="crown"]')
    await pg.wait_for_timeout(3000)
    await pg.click("#save")
    await pg.wait_for_url(base + "/", timeout=10000)
    await pg.wait_for_selector("#enter", state="visible")


async def open_editor(pg, base: str) -> None:
    """Opens /edit and waits until the client says it is in: the page's loading text goes away."""
    await pg.goto(base + EDIT)
    # the Center that says "Opening ..." or "Loading ..." is drawn only until the client is ready
    await pg.locator("text=/^(Opening|Loading)/").first.wait_for(state="hidden", timeout=240000)
    await pg.wait_for_timeout(2000)


async def mark_spot(pg, dx: float = 0, dy: float = 0) -> None:
    """Holds the Portal tool and clicks the canvas where the ghost shows (the canvas centre,
    offset by dx, dy pixels)."""
    await pg.get_by_role("button", name="Portal", exact=True).click()
    box = await pg.locator("#editor-game").bounding_box()
    x = box["x"] + box["width"] / 2 + dx
    y = box["y"] + box["height"] / 2 + dy
    await pg.mouse.move(x, y)
    await pg.wait_for_timeout(800)
    await pg.mouse.click(x, y)


async def run(out: Path, base: str) -> None:
    from playwright.async_api import async_playwright

    out.mkdir(parents=True, exist_ok=True)
    suffix = random.randint(100, 999)
    portal = f"E2E {suffix}"
    far = f"E2E far {suffix}"
    assert len(portal) <= PORTAL_NAME_MAX and len(far) <= PORTAL_NAME_MAX
    logs: list[str] = []

    async with async_playwright() as p:
        # headed, as in web_e2e.py: headless Chromium does not put WebGPU canvases in screenshots
        b = await p.chromium.launch(args=GPU, headless=False)
        ctx1 = await b.new_context(viewport={"width": 1280, "height": 800})
        ctx2 = await b.new_context(viewport={"width": 1280, "height": 800})
        pg = await ctx1.new_page()
        other = await ctx2.new_page()
        for name, page in (("a", pg), ("b", other)):
            page.on("console", lambda m, n=name: logs.append(f"[{n}] {m.type}: {m.text}"))
            page.on("pageerror", lambda e, n=name: logs.append(f"[{n}] pageerror: {e}"))
        try:
            # 1. the editor opens for an account that may edit the yard
            await sign_up(pg, base, f"Editor {suffix}")
            await open_editor(pg, base)
            await pg.screenshot(path=str(out / "1_editor.png"))

            # 2. the Portal tool marks a spot; the New portal form opens
            await mark_spot(pg)
            dialog = pg.get_by_role("dialog", name="New portal")
            await dialog.wait_for(state="visible", timeout=15000)
            await dialog.get_by_label("Name", exact=True).fill(portal)
            await pg.screenshot(path=str(out / "2_spot.png"))

            # 3. its destination: a world, the void; a pair with one of the account's own portals
            #    when no world is allowed (the form then offers no world to choose)
            await dialog.get_by_label("World", exact=True).click()
            void = pg.get_by_role("option", name=re.compile("void", re.IGNORECASE))
            if await void.count() and await void.first.is_visible():
                await void.first.click()
            else:
                await pg.keyboard.press("Escape")
                await dialog.get_by_label("Leads to", exact=True).click()
                await pg.get_by_role("option", name="my other portal").click()
                await dialog.get_by_label("Portal", exact=True).click()
                await pg.get_by_role("option").first.click()
            await dialog.get_by_role("button", name="Create portal").click()

            # 4. the edit is done when the form closes, and the portal's row is in the list
            await dialog.wait_for(state="detached", timeout=15000)
            row = pg.get_by_role("button", name=f'Portal "{portal}"')
            await row.wait_for(state="visible", timeout=15000)
            await pg.screenshot(path=str(out / "3_placed.png"))

            # 5. a second account, on its own page, sees the portal
            await sign_up(other, base, f"Visitor {suffix}")
            await open_editor(other, base)
            await other.get_by_role("button", name=f'Portal "{portal}"').wait_for(state="visible", timeout=30000)
            await other.screenshot(path=str(out / "4_second_page.png"))

            # 6. a refusal: a portal outside the plot is refused, and the reason shows in the form
            await mark_spot(pg, dx=60, dy=60)
            dialog = pg.get_by_role("dialog", name="New portal")
            await dialog.wait_for(state="visible", timeout=15000)
            await dialog.get_by_label("Name", exact=True).fill(far)
            await dialog.get_by_label("Position x").fill("100000")
            await dialog.get_by_label("World", exact=True).click()
            await pg.get_by_role("option", name=re.compile("void", re.IGNORECASE)).first.click()
            await dialog.get_by_role("button", name="Create portal").click()
            reason = dialog.get_by_role("alert")
            await reason.wait_for(state="visible", timeout=15000)
            text = (await reason.inner_text()).strip()
            if not text:
                raise AssertionError("the refusal shows no reason")
            print("refusal:", text)
            await pg.screenshot(path=str(out / "5_refused.png"))
            print("ok")
        except Exception as e:  # noqa: BLE001 - every failure is a failed run
            await pg.screenshot(path=str(out / "failed.png"))
            print("FAILED:", e)
            (out / "console.log").write_text("\n".join(logs))
            await b.close()
            sys.exit(1)
        (out / "console.log").write_text("\n".join(logs))
        await b.close()


def main() -> None:
    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        usage()
        return
    base = DEFAULT_BASE
    if "--base" in args:
        base = args[args.index("--base") + 1]
        args = [a for i, a in enumerate(args) if i != args.index("--base") and i != args.index("--base") + 1]
    asyncio.run(run(Path(args[0]), base))


if __name__ == "__main__":
    main()
