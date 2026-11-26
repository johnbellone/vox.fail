---
type: Decision
title: "Decision (done): Restructure into crates/, bins/ and apps/"
description: "The repository is a virtual workspace with libraries in crates/, programs in bins/ and Bun web apps in apps/; done and on main."
tags: [decision, repository, restructure, branch]
generated: { by: agent/mcp, at: "2026-11-26T17:38:31Z" }
status: stable
stale_after: 2027-01-25
---

# Decision: crates/, bins/, apps/

Source: `docs/design/repo-structure.md` (status **done**, 2026-11-26); implemented in nine "Restructure" commits, now on `main`.

## Problem on main
(Before the change) the root `Cargo.toml` was both the workspace and the avatar app, so a bare `cargo build` built only the avatar app. Crates sit beside non-code folders with no hint of role. The avatar app is flat (41 modules, ~26k lines; `main.rs` 2,078 lines with a 690-line `main()`; `bevel.rs` 1,962). `crates/vox-world/src/client/` is 2,029 lines of mixed concerns. One 1,100-line README serves both products. `tools/` mixes builders, dev scripts and test drivers.

## Principles
1. Three kinds of code, three folders: libraries in `crates/`, programs in `bins/`, Bun/TypeScript web apps in `apps/`.
2. **Virtual workspace** at the root: `cargo build` builds everything; shared dependency versions in `[workspace.dependencies]`.
3. Split crates for a reason (shared by two programs, must build without Bevy, or must build for wasm); inside a crate use module folders.
4. **One folder, one Bevy Plugin**; `main.rs` becomes a list of plugins.
5. `assets/` and `data/` stay at the root (runtime files programs and deploy expect).
6. Move with `git mv`, one buildable step at a time.

## Result
See the path map in `project/repository-layout`. New crates: `vox-link` (one WebSocket link, previously written twice), `avatar-voice` (pure-Rust voice analysis), `vox-format`. Planned later: `vox-art`, `vox-graph`, `vox-library`, `bins/vox-forge`, `apps/forge`.

## Consequence for this bundle
This bundle was re-based onto the new layout on 2026-11-26.
