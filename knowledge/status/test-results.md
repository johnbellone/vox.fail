---
type: Status
title: Test Results on main (2026-11-26 run)
description: "Measured output of cargo test --workspace --release on main at 7c4e517, per crate, and what was not run."
tags: [status, tests, quality]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Test Results

Command: `cargo test --workspace --release` on `main` at `7c4e517`, 2026-11-26, rustc 1.100.0, Linux, warm cache. Wall time **2 min 20 s**.

| Target | Passed | Failed | Ignored |
| --- | --- | --- | --- |
| `block_avatar` | 85 | 0 | 0 |
| `avatar-voice` | 21 | 0 | 1 |
| `vox-core` | 51 | 0 | 0 |
| `vox-world` | 48 | 0 | 0 |
| `vox-server` (tests/world.rs) | 16 | 0 | 0 |
| `vox-directory` (directory 3, game 5, social 4) | 12 | 0 | 0 |
| `vox-relay` (tests/relay.rs) | 13 | 0 | 0 |
| `vox-format` | 7 | 0 | 0 |
| `vox-link` | 2 | 0 | 0 |
| `vox-client` | 1 | 0 | 0 |
| **Total** | **256** | **0** | **1** |

History: 206 passed on the old flat layout (`ca58e25`); the extra tests come from moves checks, relay and sign-in hardening, snapshots, input, HUD and menus. `docs/avatar/PROJECT.md` still says 200.

## Not run in this analysis
- `tools/test/visual_test.py` (needs Xvfb and software Vulkan; CI runs it)
- `tools/test/web_e2e.py` (needs a built wasm client, directory and world, Chromium)
- `bun test` for `apps/site`
- Anything on Windows; `--features whisper`

## Side effect of running tests
Tests write runtime files into the repo root (`gags.txt`, `questions/`, settings). They are untracked; see `status/repository-hygiene`.

## Coverage gaps (known)
Rendering and Bevy systems are verified by screenshots, not assertions; no test of the control page's JavaScript; `windows.yml` tests only `block_avatar` (`-p block_avatar`) and no workflow runs `cargo test --workspace`.

See `architecture/testing-and-ci`.

# Related Concepts
- [Testing and Continuous Integration](../architecture/testing-and-ci.md): how tests and CI are set up
