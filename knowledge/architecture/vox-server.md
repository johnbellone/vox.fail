---
type: Component
title: "vox-server: One Persistent World"
description: "A single-world game server: WebSocket in, one thread owns the blocks and the edit order, SQLite for persistence, optional directory reporting."
tags: [component, vox-server, axum, sqlite, websocket]
generated: { by: agent/mcp, at: "2026-11-27T20:45:05Z" }
status: stable
code_refs: [bins/vox-server/src/, bins/vox-server/tests/]
---

# vox-server: One Persistent World

Path `bins/vox-server/`, about 4,600 lines in 8 files (`lib.rs`, `connection.rs`, `world.rs`, `entities.rs`, `props.rs`, `snapshot.rs`, `flags.rs`, `main.rs`), integration tests over real sockets (`tests/`). Depends on `vox-core`, `vox-format`, `vox-art`, `vox-graph`, axum 0.8 (ws), tokio, `rusqlite` (bundled), `reqwest` (rustls).

## Behaviour
- `GET /` (and `/w/{port}/`) returns a short JSON status. `GET /ws` and `/w/{port}/ws` open the game WebSocket (players and read-only watchers). The `/w/<port>/` forms exist because the Cloudflare Tunnel cannot rewrite paths.
- **One thread owns the world** (blocks, entities, players, edit order), so every player sees the same edits in the same order.
- Join: `Hello` -> `Welcome`, every non-empty chunk, every entity (`Entities` batches), `Ready`, then a `Joined` per player. Edits stream in numbered order (`Edits`, `seq`); refusals go only to the sender.
- Limits: **20 edits/s** per player, **100 players** (`--max-players`), **200 watchers** (`--max-watchers`), reach and bounds from `vox-core`, plots claimed at the directory refuse outsiders (`Why::Claimed`).
- **Move checks** (`vox-core::moves`): a distance budget per player, refilled at flying speed with room for lag; an impossible position is not broadcast and the player gets `ToClient::Correct` (at most twice a second). `--trust-moves` turns it off for tests. The first position after joining is still trusted.
- Persistence: SQLite tables `meta`, `chunks`, `edits`, plus the entity tables, written every few seconds and on shutdown. **Snapshots** (`snapshot.rs`, via `vox-format`): regions that changed since the last one are stored as blobs every 5 minutes (`--snapshot-every`) and on closing, kept a week, with the edit log trimmed to match, in `<db>.snapshots/` (`--snapshots <dir>`, `--no-snapshots`). **Restore**: `--list-snapshots`, `--restore <id|latest>`, `--restore-at <unix time>` (world closed; undoable).
- Weather: the server owns the atmosphere simulation and sends keyframes.

## Entities and the editor (protocol 9)
- `entities.rs`: the world's entities (portals, spawn, objective, settings, props, nodes) in memory by id with the ones changed since the last write and an entity edit log (who changed what, old and new). They are written in the same SQLite transaction as the blocks and forgotten only after it commits; loading re-validates every row and skips a bad one; the log replays onto a snapshot's entities on restore.
- `props.rs`: `PropLibrary`, read once at open from the props folder (`--props`, default `data/props`: `catalogue.json` and `<hash>.voxp` blobs). It keeps only what the rules need per prop (footprint, height, whether it collides, the surfaces it stands on, whether it is a table). An index of colliding props by chunk means a footprint check looks only at the chunks a new prop's cells fall in, and per-column counts serve a plot's 64-prop cap. A placed prop whose hash left the catalogue is an orphan: it keeps its entity but takes no cells and cannot be moved or turned (`NoSuchProp`); whoever may remove it still may.
- `world.rs` judges a `Put` or `Remove` with `vox_core::edit` (the `Ctx` closures answered from the library and the blocks). A prop's owner is stamped by the server (a new prop gets the judging account; an existing one keeps its owner). Basic builders edit their plots; **advanced rights** are asked of the directory (`GET /api/world/{id}/editor?account=`) with the edit held until it answers, and the answer is cached for 5 s (`EDITOR_CACHE`). `--editors-all` (local play and tests only, refused with a directory) makes every signed-in player, guests too, an editor without asking. At most one edit waits on the directory per player.
- **Builds**: `Build` expands a node's recipe with `vox-graph` and applies it **4,096 blocks a tick** (ticks are 100 ms) so other edits interleave; a world starts a build at most every 10 s. What a build changed is kept per builder (the last 8, `KEPT_BUILDS`); `Unbuild` puts back those blocks except the ones that changed since, then sends a `Notice` with the counts.
- `drop_unsupported`: when a block is broken, replaced or turned to water, props that leaned on it and have no surface left are removed (same entity log, edit number and broadcast as an editor's removal), and what stood on a removed table goes too. Props already floating before the rule, and orphans, are left alone.
- **Closed atlas edges**: an edge the Settings entity's `atlas.edges` no longer lists refuses `edge:*` travel; a world whose settings do not mention edges stays open.

## Flags
`--listen 127.0.0.1:7780  --db world.db  --name void  --seed <level>  --blocks data/blocks.json  --props data/props  --max-players N  --max-watchers N  --trust-moves  --editors-all  --directory-key <hex|file> [--guests]  --directory-url http://127.0.0.1:7700  [--snapshots dir | --no-snapshots] [--snapshot-every s]`.
`--directory-key` makes the world require a signed token; `--guests` also lets unsigned players in. The flags are shared with `vox-host` (`vox_server::flags`).

## Directory link
With `--directory-url` and `VOX_WORLD_SECRET` the world POSTs a report every minute (who is signed in, where they stand, blocks placed) and fetches plots and portals every 30 s. `--name` must equal the world's id at the directory. These `/api/world/*` endpoints must not be public.

## Known gaps
No restore UI for owners; no chat or name tags; the whole world is sent on join (see `architecture/world-scaling-design`); a token is valid on every world. Nothing goes back to an inventory when a prop is dropped (there is none). See `status/risks-and-open-issues`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 9)](wire-protocol.md): the protocol it speaks
- [SQLite Persistence (rusqlite, bundled)](../technologies/sqlite-persistence.md): its storage
- [vox-directory: Accounts, Characters, Economy and Web Pages](vox-directory.md): reports and plots flow through the directory
- [Design (phase 1 built): The World Format](world-format-design.md): snapshots and restore
- [Decision: The World Editor](../decisions/world-editor.md): the entity edits it judges and applies


## Voice relay (protocol 7)
The world thread relays `ToServer::Voice` as `ToClient::Voice`. The audience is decided from server-known state only: **nearby** = other players within `--voice-radius` blocks (default 32, 3D) of the sender's checked position; **party** = other players whose directory account is in the sender's raid party (parties come from `GET /api/world/{id}/rules` as `parties`, refreshed every `rules_every`, default 30 s), at any distance. Guests have no party, watchers neither send nor receive, the sender never hears herself. Packets must pass `VoicePacket::valid()`, a per-player bucket (60/s, burst 100) and a per-connection cap before they reach the world thread; sends use `try_send`, nothing is stored or logged. Tests: `voice_audience` unit tests, `tests/world.rs` and the real-recording `tests/voice_e2e.rs`.
