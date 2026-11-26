---
type: Event
title: Project History and Timeline
description: "The 35 git commits so far (2026-11-25 to 26): the Block Avatar snapshot, the Vox online stack, then the 29-commit restructure and follow-on work."
tags: [project, history, git]
generated: { by: agent/mcp, at: "2026-11-26T17:38:53Z" }
status: stable
---

# Project History and Timeline

The git history is short. It starts with a snapshot of a large existing code base, so earlier work (Block Avatar's many features) is not visible commit by commit. `docs/avatar/PROJECT.md` gives dated notes from 2026-11-22 to 2026-11-25. Dates come from `git log`.

## Phase 1: Block Avatar becomes Vox (2026-11-25, 6 commits)
| Commit | What it did |
| --- | --- |
| `bb4f6f5` | "snapshot before warren-world extraction": first commit, 135 files, about 60.6k lines. The Block Avatar app with its assets and docs. |
| `94dd059` | "web MVP: directory, warren-world, warren-client": 666 files. The directory (accounts), the world code split into a shared crate and a browser client. Worlds were still called "warren". |
| `4647d46` | "rename to vox; vox.pub deployment": all crates and binaries become `vox-*`. |
| `3e40e76` | "deploy at vox.fail": the domain changes from vox.pub. |
| `9de8028` | "Steam and Discord sign-in; Cloudflare Tunnel deployment": the Caddyfile is replaced by a tunnel. |
| `ca58e25` | "Blue, the main character; authority, world currencies, inventory explorer, feats; friends, groups, raid parties; worlds report to the directory (protocol 5)". |

## Phase 2: restructure and follow-on work (2026-11-25 to 26, 29 commits, head `7c4e517`)
Developed in a separate worktree and pushed to `origin/main` on 2026-11-26 (the repo shows a push at 15:45 UTC). Highlights in order: AGENTS.md and design docs; React/Mantine/Bun site; block-game controls and HUD (phases 1-4); nine restructure steps into `crates/`, `bins/`, `apps/`; server move checks and sign-in/relay hardening; GPU memory work (packed vertices, merged faces, meshing budget); avatar `main()` into plugins; world format design and phase 1 (`vox-format`, snapshots, restore). Details: `status/restructure-and-follow-on-work`.

## Phase 3: agent memory (2026-11-26)
okf agent memory set up (CLI v0.9.0, MCP server in `.mcp.json`, this `knowledge/` bundle, AGENTS.md okf block), a work-tracking convention (GitHub Issues, beads, okf; `convention/work-tracking`).

## Naming history
"warren" (early name for worlds and the client) became "Vox". The domain moved from vox.pub to vox.fail. The streaming app keeps its package name `block_avatar` and user-facing name "Block Avatar".

## Provenance note
Anything earlier than 2026-11-25 comes from the project's own documents and cannot be checked against git.

# Related Concepts
- [Restructure and Follow-on Work (the former branch, now on main)](../status/restructure-and-follow-on-work.md): the 29 later commits
