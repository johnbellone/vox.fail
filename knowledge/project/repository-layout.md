---
type: Fact
title: Repository Layout
description: "Every top-level directory of the restructured repository (crates/, bins/, apps/), what it holds, sizes, and the old-to-new path map."
tags: [project, layout, repository]
generated: { by: agent/mcp, at: "2026-11-26T17:38:53Z" }
status: stable
---

# Repository Layout

Describes `main` at `7c4e517` (2026-11-26), after the restructure into a **virtual workspace**. 984 files are tracked, 193 of them `.glb` models. Rule: libraries in `crates/`, programs in `bins/`, Bun web apps in `apps/`. See `decisions/restructure-into-crates-bins-apps`.

## Rust workspace (root `Cargo.toml`: `members = ["bins/*","crates/*"]`, resolver 3, shared `[workspace.dependencies]`)

| Path | Package | Rust lines | Tests |
| --- | --- | --- | --- |
| `crates/vox-core/` | `vox-core` (no Bevy) | 2,543 in 10 files | 51 |
| `crates/vox-world/` | `vox-world` (Bevy) | 8,732 in 23 files | 48 |
| `crates/vox-format/` | `vox-format` (region blobs, snapshots) | 737 in 3 files | 7 |
| `crates/vox-link/` | `vox-link` (reconnecting WebSocket thread) | 241 in 1 file | 2 |
| `crates/avatar-voice/` | `avatar-voice` (lip sync, pitch, formants, laugh/sing, WAV) | 2,167 in 6 files | 21 (+1 ignored) |
| `bins/block-avatar/` | `block_avatar`, the streaming app | 23,881 in 70 files | 85 |
| `bins/vox-client/` | player client, native and wasm | 662 in 2 files | 1 |
| `bins/vox-server/` | one world server | 1,842 in 6 files | 16 |
| `bins/vox-directory/` | accounts, economy, pages API | 3,114 in 11 files | 12 |
| `bins/vox-relay/` | show relay | 1,119 in 3 files | 13 |

Total about 45k lines of Rust. The block-avatar source is organised in folders: `app/` (configure, plugins, sim, startup, applog), `audience/` (ask, chat, guest, questions, streamchat, poll, events, cohost), `character/` (bevel, face, facecap, jiggle, puppet, wardrobe), `controls/` (control page, commands, hotkeys), `network/`, `output/` (webcam, clips, cards, snapshot), `scene/` (stage, lighting, camera, show, sets, fx, particles, bubbles, crowd), `sound/` (audio, netvoice, voicefx), plus `cli.rs`, `settings.rs`, `model.rs`, `frame.rs`, `world_glue.rs`.

## Other directories
| Path | Contents |
| --- | --- |
| `apps/site/` | The vox.fail pages: React 19, Mantine 9, TypeScript 7, built with Bun into `dist/` (about 2.6k lines, pages `index`, `create`, `play`, `inventory`, `friends`). `public/pkg/` holds the generated wasm client (ignored). |
| `package.json`, `bun.lock` | Bun workspace (`workspaces: ["apps/*"]`) |
| `assets/` | 110 MB: models (44 GLB), props (143), sets (6), sounds, portraits (38), block tiles (63), shaders, fonts, UI |
| `data/` | JSON: `characters.json`, `blocks.json`, `wardrobe.json`, `head_fit.json`, `bare_heads.json`, `surfaces.json`, clip manifests, `levels/` (`void`, `yard`, `islands`) |
| `source/vox/` | MagicaVoxel sources |
| `tools/assets/` | 13 Python asset builders |
| `tools/dev/` | `build_web.sh` (wasm client into `apps/site/public/pkg`), `play-local.sh` (Void, yard, islands, then the client joins as guest) |
| `tools/test/` | `visual_test.py`, `web_e2e.py`, `eval_voice.py` |
| `deploy/` | Cloudflare Tunnel config, two systemd units, env templates, `purge-cache.sh` |
| `bins/block-avatar/installer/` | NSIS script for the Windows installer |
| `docs/avatar/` | The avatar manual (`manual.md`), `PROJECT.md` (design and review document, partly stale), `STREAMDECK.md`, line-up images |
| `docs/vox/` | Running worlds, the directory, authority and raids |
| `docs/design/` | Five design documents (`architecture/world-scaling-design`, `world-format-design`, `world-graph-and-art-design`, `controls-and-hud-design`; repo structure is `decisions/restructure-into-crates-bins-apps`) |
| `testdata/`, `demo/` | Real voice recordings and sample questions |
| `.github/workflows/` | `visual.yml`, `windows.yml` |
| `knowledge/`, `.agents/`, `AGENTS.md`, `.mcp.json` | okf agent memory and agent rules |

## Old flat layout (before 2026-11-26) to current
`src/` -> `bins/block-avatar/src/` (voice code to `crates/avatar-voice/`); `core/` -> `crates/vox-core/`; `world/` -> `crates/vox-world/`; `client/` -> `bins/vox-client/`; `server/` -> `bins/vox-server/`; `directory/` -> `bins/vox-directory/`; `relay/` -> `bins/vox-relay/`; `web/` -> `apps/site/`; `tools/*.py` -> `tools/assets|dev|test/`; the 1,100-line README -> `docs/avatar/manual.md`; `installer/` -> `bins/block-avatar/installer/`.

## Not source (ignored or local)
`target/`, `*.db`, `*.log`, `node_modules/`, `apps/*/dist/`, `apps/site/public/pkg/`, `directory.key*`, `gags.txt`. Runtime files that appear when the app runs from source (`avatar_settings.txt`, `hotkeys.txt`, `clips/`, `questions/`) are **not** ignored; see `status/repository-hygiene`.

# Related Concepts
- [Restructure and Follow-on Work (the former branch, now on main)](../status/restructure-and-follow-on-work.md): the restructure that produced this layout
- [Repository and Disk Hygiene Findings](../status/repository-hygiene.md): untracked and oversized directories
- [Decision (done): Restructure into crates/, bins/ and apps/](../decisions/restructure-into-crates-bins-apps.md): why the layout is crates/bins/apps
