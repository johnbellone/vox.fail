---
type: Status
title: "Restructure and Follow-on Work (the former branch, now on main)"
description: "The 29 commits of 2026-11-25/26 now on main: restructure into crates/bins/apps, controls, site rewrite, world format, hardening and design docs."
tags: [status, branch, restructure, worktree]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Restructure and Follow-on Work (now on main)

These 29 commits were developed on the local branch `claude/project-build-daed3d` (worktree `.claude/worktrees/project-build-daed3d`) and are now **on `origin/main`** (head `7c4e517`, pushed 2026-11-26). Merge base was `ca58e25`. Author John Bellone, co-authored by Claude Opus 5.5. 291 files changed, +23.9k / -22.9k lines. The branch and worktree still exist locally.

## What they delivered (newest first)
1. **World format phase 1**: `vox-format` (regions, BLAKE3-addressed zstd blobs, snapshots every 5 minutes and on close, kept a week; server flags `--snapshots`, `--no-snapshots`, `--snapshot-every`, `--list-snapshots`, `--restore`, `--restore-at`). Design: `architecture/world-format-design`.
2. **Rendering memory**: world faces merged, packed 20-byte vertex (was 64), chunk meshes only on the GPU, chunk-by-chunk voxel copy with a per-frame meshing budget (8 ms desktop, 5 ms browser).
3. **Avatar app**: `main()` now about 50 lines; systems grouped as plugins by pipeline (`Setup`, `Frame`, `Motion`, `Network`, `WorldGlue` in `app/plugins.rs`); resources built in `app/configure.rs`.
4. **Security hardening**: `vox-core::moves` budget (server refuses impossible positions and sends `ToClient::Correct`; `--trust-moves` for tests); Steam replies must be plain and signed over every field read; Argon2 at most 4 at a time (503 after 5 s); sign-in counters capped; relay limits shows per address (10 per 10 min) and wrong codes (20 per address, 50 per show); guest/viewer text frames dropped.
5. **Controls and HUD phases 1-4**: action layer, gamepad detection with brand button hints, first person with pointer lock, pixel HUD, controller-navigable menus, block palette, quick wheel. Design: `architecture/controls-and-hud-design`.
6. **Restructure steps 1-9**: workspace dependencies; `crates/` and `bins/`; `apps/site` replaces `web/`; tools and docs split; short README; avatar modules in folders; `avatar-voice` crate; `vox-link` crate; `vox-world/client.rs` split.
7. **Site rewrite**: React 19 + Mantine 9 + TypeScript 7 on Bun; watch mode for the front page's live view.
8. **Design documents** in `docs/design/`: repo-structure (done), controls-and-hud (phases 1-4 built), world-format (phase 1 built), world-scaling (proposed), world-graph-and-art (proposed); also decisions "rotating authority" and "new worlds default to creative".
9. `tools/dev/play-local.sh`; the installer ships `docs/avatar/manual.md` as its README, and `windows.yml` copies it into the portable zip.

## Old path to new path
See `project/repository-layout`.

## Known gaps recorded by the authors (AGENTS.md, "Known gaps")
First position after joining is still on trust (a portal's arrival point comes from the other world). A world's sign-in token is valid on every world (no audience). A one-block edit rebuilds its whole chunk; weather and fog baking run on the main thread. Duplicated helpers (xorshift, value noise, child-spawning). No Linux webcam output.

## Consequences
- The okf bundle was re-based onto this layout on 2026-11-26.
- Risks fixed here are marked in `status/risks-and-open-issues`.
- The 140 GB `target/` in the old worktree is the largest item on disk (`status/repository-hygiene`).

# Related Concepts
- [Decision (done): Restructure into crates/, bins/ and apps/](../decisions/restructure-into-crates-bins-apps.md): the restructure decision
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): world format, phase 1 built
- [Design (proposed): Worlds at 60 km x 60 km](../architecture/world-scaling-design.md): 60 km worlds, proposed
- [Design (proposed): World Graph and Generated Art](../architecture/world-graph-and-art-design.md): generated art, proposed
- [Design (phases 1-4 built): Block-Game Controls, HUD and Menus](../architecture/controls-and-hud-design.md): controls and HUD, phases 1-4 built
