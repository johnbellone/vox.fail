---
type: Process
title: Testing and Continuous Integration
description: "What the tests cover, how to run them (unit, integration, visual, browser end-to-end) and what CI does."
tags: [process, testing, ci, github-actions]
generated: { by: agent/pi-lead, at: "2026-11-27T14:10:42Z" }
status: stable
code_refs: [.github/workflows/visual.yml, .github/workflows/windows.yml, tools/test/visual_test.py, tools/test/web_e2e.py, testdata/]
---

# Testing and CI

## Commands
```
cargo test --workspace --release        # all unit and integration tests
python3 tools/test/visual_test.py       # render fixed scenes, compare with testdata/visual/ref/
python3 tools/test/visual_test.py --update   # accept new references (make your own on a new GPU)
python3 tools/test/web_e2e.py OUT_DIR   # browser: sign up, make a character, enter the Void
bun install && (cd apps/site && bun test)    # site tests
tools/dev/play-local.sh                 # run three local worlds and join as a guest
```
Linux needs `libasound2-dev libudev-dev`; CI also uses `libwayland-dev libxkbcommon-dev mesa-vulkan-drivers xvfb python3-pil`.

## Counts (2026-11-26, main at `7c4e517`: all passed, 2 min 20 s warm)
256 passed, 0 failed, 1 ignored. block_avatar 85, vox-world 48, vox-core 51, avatar-voice 21 (+1 ignored), vox-server 16, vox-relay 13, vox-directory 12, vox-format 7, vox-link 2, vox-client 1. Site: 1 test file (not run).

## What the tests pin
- **Voice**: real-voice regression (`crates/avatar-voice/src/tests_real.rs`): pitch vs Praat, vowels, no false tilts; laugh/sing on real recordings. Fixtures in `testdata/real/`.
- **Viewer questions**: nothing airs unheard, limits, bans across restarts, blocked words, broken WAVs, a full fake-Twitch login and upload with a real recording, the page exposes no control commands.
- **Chat**: IRC parsing against a fake IRC server, flood limits, dress-up permissions, head fit for every character.
- **Output**: OBS shared-memory header byte for byte, NV12 range, OBS-style reader, comic strip and cards.
- **Worlds**: chunk keys, RLE, edit checks, raycasts, box mover, moves budget, levels, prediction, weather, portals, sky; server over real sockets (one edit order, refusals only to sender, rate limit, restart keeps edit numbers, snapshots and restore).
- **Format**: region blob round trips, crafted blobs refused, store and snapshots.
- **Network**: pose packing, playout clock, ADPCM SNR, jitter buffer; relay fan-out, expiry, simulated delay/loss, new rate limits.
- **Directory**: accounts, economy (5), Steam/Discord against stand-ins (forged or repeated-key answers refused).
- **Sim clock**: identical motion tick for tick at 30, 60 and 144 fps.
Rendering and Bevy systems are verified by headless runs and screenshots, not assertions.

## Gaps
Tests write into the project folder; `web_e2e.py` is not in CI; no test of the control page's JavaScript; no macOS, real GPU or Windows run.

## CI (`.github/workflows/`)
- **visual.yml**: push to `main`, pull requests, manual. Ubuntu 24.04, Mesa software Vulkan, Xvfb, `cargo build --release -p block_avatar`, `visual_test.py`; uploads the report. References came from Linux software rendering.
- **windows.yml**: on `v*` tags: `cargo test --release -p block_avatar`, build, NSIS installer (`bins/block-avatar/installer/block-avatar.nsi`), portable zip (copies `docs/avatar/manual.md` as README). First run on tag `v0.0.0-test` (2026-11-27).
- **client-build.yml** (reusable; also on pull requests touching the client): builds vox-client for Windows (NSIS installer + zip), macOS (universal dmg, `macos-latest`) and Linux (AppImage + tar.gz, ubuntu-22.04) and runs `vox-client --check` on every package from outside the checkout (Windows: silent install, check, uninstall). First green run: 46008608571.
- **nightly.yml**: daily 05:17 UTC on the default branch, rolling `nightly` prerelease; skips unchanged commits. **release.yml**: `v*` tags, release `Vox <version>`. See `decisions/client-installers`, `docs/vox/releases.md`.
- **No workflow runs `cargo test --workspace`**, the site's tests, or the browser end-to-end test.

Related: `status/test-results`, `status/verification-gaps`.

# Related Concepts
- [Decision: vox-client installers built by GitHub Actions](../decisions/client-installers.md): the installer workflows
- [Test Results on main (2026-11-26 run)](../status/test-results.md): the latest measured results
