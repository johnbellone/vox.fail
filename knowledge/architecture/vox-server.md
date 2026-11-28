---
type: Component
title: "vox-server: One Persistent World"
description: "A single-world game server: WebSocket in, one thread owns the blocks and the edit order, SQLite for persistence, optional directory reporting."
tags: [component, vox-server, axum, sqlite, websocket]
generated: { by: agent/mcp, at: "2026-11-28T03:24:20Z" }
status: stable
code_refs: [bins/vox-server/src/, bins/vox-server/tests/]
---

# vox-server: One Persistent World

Path `bins/vox-server/`, about 4,900 lines in 9 files (`lib.rs`, `connection.rs`, `world.rs`, `entities.rs`, `props.rs`, `rescale.rs`, `snapshot.rs`, `flags.rs`, `main.rs`), with integration tests over real sockets (`tests/`). Depends on `vox-core`, `vox-format`, `vox-art`, `vox-graph`, axum 0.8 (ws), tokio, `rusqlite` (bundled) and `reqwest` (rustls).

## Behaviour
- `GET /` (and `/w/{port}/`) returns a short JSON status. `GET /ws` and `/w/{port}/ws` open the game WebSocket, for players and read-only watchers. The `/w/<port>/` forms exist because the Cloudflare Tunnel cannot rewrite paths.
- **One thread owns the world** (blocks, entities, players, edit order), so every player sees the same edits in the same order.
- **Join**: `Hello`, then `Welcome`, every non-empty chunk, every entity (`Entities` batches), `Ready`, and a `Joined` for each player. Edits stream in numbered order (`Edits`, `seq`); refusals go only to the sender. The whole world is still sent on join; streaming by view radius is planned (`architecture/chunk-streaming-design`).
- **Limits**:
  - 20 edits/s per player
  - 100 players (`--max-players`) and 200 watchers (`--max-watchers`)
  - reach and bounds from `vox-core`
  - plots claimed at the directory refuse outsiders (`Why::Claimed`)
- **Bounds**: a world takes its level's `bounds` at every start (the Void is ±51,200 blocks), or `Bounds::default()`.
- **Seeding**: a new world is filled straight into chunks (`vox_core::level::fill`). Seeding the sea peaks at about 140 MB; building a map of every block first had cost 1.7 GB.
- **Move checks** (`vox-core::moves`): each player has a distance budget, refilled at flying speed with room for lag. An impossible position is not broadcast, and the player gets `ToClient::Correct` (at most twice a second). `--trust-moves` turns this off for tests. The first position after joining is still trusted.
- **Persistence**: SQLite tables `meta`, `chunks` and `edits`, plus the entity tables, written every few seconds and on shutdown.
- **Snapshots** (`snapshot.rs`, via `vox-format`):
  - Regions that changed since the last snapshot are stored as blobs every 5 minutes (`--snapshot-every`) and on closing, and kept a week.
  - The edit log is trimmed to match.
  - They go in `<db>.snapshots/` (`--snapshots <dir>`, `--no-snapshots`).
- **Restore**: `--list-snapshots`, `--restore <id|latest>`, `--restore-at <unix time>`. The world must be closed, and a restore can be undone.
- **Weather**: the server owns the atmosphere simulation and sends keyframes.

## Quarter-metre conversion (`rescale.rs`)
On its first start with protocol 10, a world saved at half-metre blocks is converted in one transaction:
- Each stored chunk becomes its 8 chunks in a new table, one at a time, then the new table replaces the old.
- Entities and settings move with the blocks (`vox_core::rescale`).
- The edit logs are emptied.

Meta `block_m` marks the world as done, and `block_m_at` records when. The first snapshot after the conversion is whole, and snapshots from before are converted when restored. A new world is only marked.

## Entities and the editor (protocol 9)
- `entities.rs`: the world's entities (portals, spawn, objective, settings, props, nodes) in memory by id, with the ones changed since the last write and an entity edit log (who changed what, old and new).
  - They are written in the same SQLite transaction as the blocks, and forgotten only after it commits.
  - Loading re-validates every row and skips a bad one.
  - On restore, the log replays onto a snapshot's entities.
- `props.rs`: `PropLibrary`, read once at open from the props folder (`--props`, default `data/props`: `catalogue.json` and `<hash>.voxp` blobs).
  - It keeps only what the rules need per prop: footprint (in half-metre cells), height, whether it collides, the surfaces it stands on, and whether it is a table.
  - An index of colliding props by chunk means a footprint check looks only at the chunks a new prop's cells fall in. Per-column counts serve a plot's 64-prop cap.
  - A placed prop whose hash left the catalogue is an **orphan**: it keeps its entity but takes no cells, and cannot be moved or turned (`NoSuchProp`). Whoever may remove it still may.
- `world.rs` judges a `Put` or `Remove` with `vox_core::edit`, answering the `Ctx` closures from the library and the blocks.
  - The server stamps a prop's owner: a new prop gets the judging account, and an existing one keeps its owner.
  - Basic builders edit their plots. **Advanced rights** are asked of the directory (`GET /api/world/{id}/editor?account=`), with the edit held until it answers; the answer is cached for 5 s (`EDITOR_CACHE`). At most one edit waits on the directory per player.
  - `--editors-all` makes every signed-in player, guests too, an editor without asking. It is for local play and tests only, and refused with a directory.
- **Builds**: `Build` expands a node's recipe with `vox-graph` and applies it **4,096 blocks a tick** (ticks are 100 ms), so other edits interleave. A world starts a build at most every 10 s. What a build changed is kept per builder (the last 8, `KEPT_BUILDS`). `Unbuild` puts back those blocks, except ones that changed since, then sends a `Notice` with the counts.
- `drop_unsupported`: when a block is broken, replaced or turned to water, props that leaned on it and have no surface left are removed. This uses the same entity log, edit number and broadcast as an editor's removal, and whatever stood on a removed table goes too. Props already floating before the rule, and orphans, are left alone.
- **Closed atlas edges**: an edge the Settings entity's `atlas.edges` no longer lists refuses `edge:*` travel. A world whose settings don't mention edges stays open. An edge step lands a metre inside the next tile.

## Flags
`--listen 127.0.0.1:7780  --db world.db  --name void  --seed <level>  --blocks data/blocks.json  --props data/props  --max-players N  --max-watchers N  --trust-moves  --editors-all  --directory-key <hex|file> [--guests]  --directory-url http://127.0.0.1:7700  [--snapshots dir | --no-snapshots] [--snapshot-every s]`

- `--directory-key` makes the world require a signed token; `--guests` also lets unsigned players in.
- The flags are shared with `vox-host` (`vox_server::flags`).
- The palette's `block_size` must be 0.25.

## Directory link
With `--directory-url` and `VOX_WORLD_SECRET`, the world POSTs a report every minute (who is signed in, where they stand, blocks placed) and fetches plots and portals every 30 s. `--name` must equal the world's id at the directory. These `/api/world/*` endpoints must not be public.

## Known gaps
- No restore UI for owners.
- No chat or name tags.
- The whole world is sent on join (see `architecture/chunk-streaming-design`).
- A token is valid on every world.
- Nothing goes back to an inventory when a prop is dropped (there is none).

See `status/risks-and-open-issues`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 10)](wire-protocol.md): the protocol it speaks
- [SQLite Persistence (rusqlite, bundled)](../technologies/sqlite-persistence.md): its storage
- [vox-directory: Accounts, Characters, Economy and Web Pages](vox-directory.md): reports and plots flow through the directory
- [Design (phase 1 built): The World Format](world-format-design.md): snapshots and restore
- [Decision: The World Editor](../decisions/world-editor.md): the entity edits it judges and applies
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): the conversion of saved worlds

## Voice relay (protocol 7)
The world thread relays `ToServer::Voice` as `ToClient::Voice`. The audience is decided from server-known state only:
- **Nearby**: other players within `--voice-radius` blocks (default 64, which is 16 m, in 3D) of the sender's checked position.
- **Party**: other players whose directory account is in the sender's raid party, at any distance. Parties come from `GET /api/world/{id}/rules` as `parties`, refreshed every `rules_every` (default 30 s).

Guests have no party, watchers neither send nor receive, and the sender never hears their own voice.

Before they reach the world thread, packets must pass `VoicePacket::valid()`, a per-player bucket (60/s, burst 100) and a per-connection cap. Sends use `try_send`, and nothing is stored or logged.

Tests: the `voice_audience` unit tests, `tests/world.rs`, and the real-recording `tests/voice_e2e.rs`.
