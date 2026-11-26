---
type: Architecture
title: World Wire Protocol (PROTOCOL = 5)
description: "The message set between a world server and its players, how it is encoded, and what each protocol version added."
tags: [architecture, protocol, networking, postcard]
generated: { by: agent/mcp, at: "2026-11-26T15:52:35Z" }
status: stable
governance: constraint
code_refs: [crates/vox-core/src/wire.rs, crates/vox-world/src/proto.rs]
---

# World Wire Protocol

Defined in `crates/vox-core/src/wire.rs` (`pub const PROTOCOL: u32 = 5`). One WebSocket **binary** message per postcard-encoded value. Bump the constant whenever messages change in a way old players cannot read; the server refuses a wrong version.

## Messages
**ToServer**: `Hello {protocol, name, avatar: Vec<u8>, token: Option<String>}`, `Edit {id, cell, block}`, `Pose {at, blob}`, weather change, leave.
**ToClient**: `Welcome {you, world, palette, block_size, bounds, spawn, settings (JSON string), name}`, `Chunk {key, version, data}` (run-length encoded), `Ready`, `Joined {player, name, avatar}`, `Left`, `Edits(Vec<Applied{seq, player, id, cell, block}>)`, `Refused {id, why}`, `Pose`, `Weather`, `Bye {why}`, and (added last, so older clients ignore it) `Correct {at}`: the server's answer to an impossible position, telling the client to go back.
`Pose` and `Weather` are *droppable* (a newer one follows); everything else is reliable.

## Version history (from docs/PROJECT.md; v3 is not documented)
| Version | Change |
| --- | --- |
| 1 | First slice: chunks, edits, poses |
| 2 | The world's settings (sky, time, clouds, hotbar) travel with the welcome |
| 4 | Hello carries the sign-in token; welcome carries the name you go by |
| 5 | `Why::Claimed`: edits inside someone else's plot are refused |

## Separate protocol
Shows (host, guest, viewers) use a different format in `crates/vox-world/src/proto.rs`, carried by `vox-relay`. See `architecture/show-networking`.

## Design rules
- Poses are opaque to the server: it only needs where the player stands.
- Everything is ordered by the server; clients predict and reconcile (`vox-core::predict`).
- Known protocol gap: the hello carries the model only, so other players do not see what you wear. Fixing it needs a version bump (risk 29).
