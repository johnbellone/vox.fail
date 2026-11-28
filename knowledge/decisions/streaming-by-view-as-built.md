---
type: Decision
title: "Streaming by View Radius, as Built (Paletted Chunks, Protocol 12)"
description: "What of the chunk-streaming design is built (2026-11-28): paletted chunks, View before Hello, interest sets with nearest-first paced sending, edits only to interested players, the client's view and dropping, and the rules settled by review."
tags: [decision, streaming, chunks, protocol, vox-server, vox-world, performance]
generated: { by: agent/mcp, at: "2026-11-28T13:54:24Z" }
status: stable
---

## Built (2026-11-28, branch `claude/streaming`)

Implements §1 and §3 of [the chunk-streaming design](../architecture/chunk-streaming-design.md).

- **Paletted chunks (`vox-cti.4`, `vox_core::world::Chunk`).**
  - A chunk is either `Uniform(id)`, storing nothing, or a palette of its ids with each block's index packed at 1, 2, 4, 8 or 16 bits into u64 words.
  - The palette grows and widens as new ids are set; an emptied chunk goes back to `Uniform(0)`. The palette never shrinks while the chunk is held; decode rebuilds the narrowest one.
  - The run-length encoding on the wire and on disk is unchanged.
  - Measured on an idle sea world (34 million blocks): 139.8 MB resident before, 38.1 MB after.
  - In release, `set` takes about 9 ns and `get` about 1.2 ns (dense: 0.8 and 0.5).
- **Protocol 12 (`vox-cti.6`).** `ToServer::View(View {centre, radius, rings, budget_kbps})` and `ToClient::FarTile {key, depth, bytes}` (far tiles are not sent yet). `View::sane` caps it at 64 chunks and 8 rings; `far_tile_ok` checks a far tile; a far tile covers 256 m.
- **Server (`vox-cti.7`, `bins/vox-server/src/interest.rs`).**
  - **Joining:** a client may send one `View` before its `Hello` or `Watch`. Joining then sends only the chunks with blocks within that view, nearest first, before `Ready`. The view is centred on where the world starts the player (the arrival point, or the spawn), not on the client's guess; a watcher's is centred on the spawn.
  - **Later views** queue what comes into view, sent each tick and paced to a budget: 1 MB/s by default, at most 8 MB/s, with a second's worth saved up.
  - **Holding:** chunks are forgotten past the radius and two more. A chunk still held just past the view keeps getting edits.
  - **Edits** go only to players whose view holds the chunk, with two exceptions:
    - an edit into a chunk still queued waits for the chunk;
    - a player always gets their own edits, without that marking the chunk as held.
  - **Abuse limits:** the view's centre is clamped to the world before any arithmetic, and views are capped at 10 a second per connection. A view that changes nothing does no work.
  - **Cache:** encoded chunk bytes are kept until the chunk changes.
  - **The world's limit:** `view_radius_m` (128 m) and `view_rings` (3). The radius granted goes out in the welcome's settings as `view_radius`.
  - Clients that send no view get everything, as before.
- **Client (`vox-cti.8`).**
  - **When it sends a view:** before the hello, and on entering each new chunk (but never while watching, since a watcher may send nothing after its `Watch`).
  - **Radius:** the device profile's full detail (`Detail::view_m`: 128 m on the desktop, 64 m in the browser, 32 m on a phone), or 48 m when watching. It is never more than the world granted.
  - **Dropping:** chunks past the radius and two more are dropped (`World::drop_chunks`), and their meshes go at the next remesh.
- **Tests:** `bins/vox-server/tests/streaming.rs` checks that a far player gets nothing from the other side, that the nearest chunk comes first, and that walking brings the rest. The interest unit tests cover the rules above. A client test checks that walking 200 m keeps the chunks held bounded.

## Left open
- The encode cache never evicts.
- A join burst over the 2048-message queue drops the player, as a whole-world join did before.
- There's a race where an edit recreates a chunk the client just dropped (a ghost chunk).
- An old (protocol 11) server answers a View with "say hello first".
- Fast editor flight can outrun the 1 MB/s budget.
- Far tiles and rings: `vox-cti.11` to `.14`.

# Related Concepts
- [World Wire Protocol](../architecture/wire-protocol.md): protocol 12
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): the design
