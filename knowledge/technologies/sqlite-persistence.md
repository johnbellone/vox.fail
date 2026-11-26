---
type: Technology
title: "SQLite Persistence (rusqlite, bundled)"
description: "Where SQLite is used, the table sets, and the operational notes (backups, ignored files)."
tags: [technology, sqlite, persistence, database]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# SQLite Persistence

`rusqlite` 0.37 with the `bundled` feature (no system SQLite needed). Each service owns one file:

- **World servers** (`world.db`, `void.db`, `yard.db`, `islands.db`): tables `meta`, `chunks` (x, y, z, version, RLE data) and `edits` (seq, time, player, cell, old and new block). Written every few seconds and on shutdown. Beside the SQLite file, `vox-format` **snapshots** (content-addressed zstd region blobs plus manifests) are written every 5 minutes and on close to `<db>.snapshots/`, kept a week, and the edit log is trimmed to match; `--restore`/`--restore-at` roll a closed world back. A new file is seeded from a level JSON. The edit log keeps old and new block, so an undo feature is feasible.
- **Directory** (`directory.db`): 19 tables for accounts, sessions (SHA-256 of the cookie only), characters, identities, currencies, balances, ledger, items, world ownership and state, whereabouts, activity, plots, portals, friends and crews.
- Avatar app: **no database**; flat text and JSON files (`avatar_settings.txt`, `questions/`, `presets.json`).

## Operations
`*.db` and `*.db-*` are git-ignored. Production files live in `/var/lib/vox/`. No backup job exists in the repo. A `void.db` (53 KB) sits in the repository root from a local run.

See `architecture/vox-server`, `architecture/vox-directory`.
