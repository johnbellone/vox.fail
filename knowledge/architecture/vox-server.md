---
type: Component
title: "vox-server: One Persistent World"
description: "A single-world game server: WebSocket in, one thread owns the blocks and the edit order, SQLite for persistence, optional directory reporting."
tags: [component, vox-server, axum, sqlite, websocket]
generated: { by: agent/cli, at: "2026-11-26T23:26:41Z" }
status: stable
code_refs: [bins/vox-server/src/, bins/vox-server/tests/]
---

# vox-server: One Persistent World

Path `bins/vox-server/`, 1,842 lines in 6 files (`lib.rs`, `connection.rs`, `world.rs`, `snapshot.rs`, `main.rs`), 16 integration tests over real sockets (`tests/world.rs`). Depends on `vox-core`, `vox-format`, axum 0.8 (ws), tokio, `rusqlite` (bundled), `reqwest` (rustls).

## Behaviour
- `GET /` (and `/w/{port}/`) returns a short JSON status. `GET /ws` and `/w/{port}/ws` open the game WebSocket (players and read-only watchers). The `/w/<port>/` forms exist because the Cloudflare Tunnel cannot rewrite paths.
- **One thread owns the world** (blocks, players, edit order), so every player sees the same edits in the same order.
- Join: `Hello` -> `Welcome`, every non-empty chunk, `Ready`, then a `Joined` per player. Edits stream in numbered order (`Edits`, `seq`); refusals go only to the sender.
- Limits: **20 edits/s** per player, **100 players** (`--max-players`), **200 watchers** (`--max-watchers`), reach and bounds from `vox-core`, plots claimed at the directory refuse outsiders (`Why::Claimed`).
- **Move checks** (`vox-core::moves`): a distance budget per player, refilled at flying speed with room for lag; an impossible position is not broadcast and the player gets `ToClient::Correct` (at most twice a second). `--trust-moves` turns it off for tests. The first position after joining is still trusted.
- Persistence: SQLite tables `meta`, `chunks`, `edits`, written every few seconds and on shutdown. **Snapshots** (`snapshot.rs`, via `vox-format`): regions that changed since the last one are stored as blobs every 5 minutes (`--snapshot-every`) and on closing, kept a week, with the edit log trimmed to match, in `<db>.snapshots/` (`--snapshots <dir>`, `--no-snapshots`). **Restore**: `--list-snapshots`, `--restore <id|latest>`, `--restore-at <unix time>` (world closed; undoable).
- Weather: the server owns the atmosphere simulation and sends keyframes.

## Flags
`--listen 127.0.0.1:7780  --db world.db  --name void  --seed <level>  --blocks data/blocks.json  --max-players N  --max-watchers N  --trust-moves  --directory-key <hex|file> [--guests]  --directory-url http://127.0.0.1:7700  [--snapshots dir | --no-snapshots] [--snapshot-every s]`.
`--directory-key` makes the world require a signed token; `--guests` also lets unsigned players in.

## Directory link
With `--directory-url` and `VOX_WORLD_SECRET` the world POSTs a report every minute (who is signed in, where they stand, blocks placed) and fetches plots and portals every 30 s. `--name` must equal the world's id at the directory. These `/api/world/*` endpoints must not be public.

## Known gaps
No ownership beyond plots; no restore UI for owners; no chat or name tags; the whole world is sent on join (see `architecture/world-scaling-design`); a token is valid on every world. See `status/risks-and-open-issues`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 6)](wire-protocol.md): the protocol it speaks
- [SQLite Persistence (rusqlite, bundled)](../technologies/sqlite-persistence.md): its storage
- [vox-directory: Accounts, Characters, Economy and Web Pages](vox-directory.md): reports and plots flow through the directory
- [Design (phase 1 built): The World Format](world-format-design.md): snapshots and restore


## Voice relay (protocol 6)
The world thread relays `ToServer::Voice` as `ToClient::Voice`. The audience is decided from server-known state only: **nearby** = other players within `--voice-radius` blocks (default 32, 3D) of the sender's checked position; **party** = other players whose directory account is in the sender's raid party (parties come from `GET /api/world/{id}/rules` as `parties`, refreshed every `rules_every`, default 30 s), at any distance. Guests have no party, watchers neither send nor receive, the sender never hears herself. Packets must pass `VoicePacket::valid()`, a per-player bucket (60/s, burst 100) and a per-connection cap before they reach the world thread; sends use `try_send`, nothing is stored or logged. Tests: `voice_audience` unit tests, `tests/world.rs` and the real-recording `tests/voice_e2e.rs`.
