---
type: Status
title: "Documentation Drift: Where the Docs Disagree with the Code"
description: "A list of stale or contradictory statements in docs/PROJECT.md and the READMEs, so agents know which source to trust."
tags: [status, documentation, drift]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Documentation Drift (checked 2026-11-26, main at 7c4e517)

**Trust order**: code and `git log` first, then this bundle, then `docs/avatar/PROJECT.md`, then `docs/avatar/manual.md`. `AGENTS.md` and `docs/design/` are current as of the restructure.

| Statement in docs | Reality |
| --- | --- |
| PROJECT.md: "about 12,800 lines in 26 files" for the app | 23,881 lines in 70 files (`bins/block-avatar`), plus 2,167 in `avatar-voice` |
| PROJECT.md: 200 tests | 256 pass (+1 ignored) |
| PROJECT.md: "Not a git repository" (risk 14) | Git repo, 35 commits, GitHub remote |
| PROJECT.md: `main.rs` 2,260 lines, flat `src/` module table, paths like `src/net.rs` | Folders under `bins/block-avatar/src/`; `main()` about 50 lines |
| PROJECT.md: Caddyfile deployment | Cloudflare Tunnel (`deploy/`) |
| PROJECT.md: world servers have no position checks (risk 22); relay has no limits (risk 16) | Move checks and relay limits exist now |
| PROJECT.md header: asks for review of `block-avatar.zip` | Dated Nov 22, updated through Nov 25; no zip |
| manual.md (lines 105, 112, 207, 266, 315) says `cargo run --release` | **Confirmed broken**: with a virtual workspace and five binaries cargo errors "could not determine which binary to run"; use `cargo run --release -p block_avatar` |
| App strings "see README: Viewer questions" | Installer and portable zip ship `docs/avatar/manual.md` as README.md, so the pointer works there |
| `visual.yml`/`windows.yml` use `lfs: true` | No `.gitattributes`; LFS not in use |
| `docs/vox/README.md` says a new database starts as the Void and the browser needs WebGPU | Still true |

## Verified as still correct
Port numbers, protocol version 5, 38 characters, 3 worlds, 19 directory tables, 24 h token lifetime, viewer-question limits, firewall rule, Ctrl+Alt defaults.

## Maintenance rule
When a number in this bundle disagrees with a fresh measurement, update the concept (`okf_update`) rather than adding a new one.

# Related Concepts
- [Restructure and Follow-on Work (the former branch, now on main)](restructure-and-follow-on-work.md): docs also changed with the restructure
