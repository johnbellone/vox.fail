---
type: Architecture
title: World Wire Protocol (PROTOCOL = 7)
description: "The message set between a world server and its players, how it is encoded, and what each protocol version added."
tags: [architecture, protocol, networking, postcard]
generated: { by: agent/cli, at: "2026-11-26T23:26:41Z" }
status: stable
governance: constraint
code_refs: [crates/vox-core/src/wire.rs, crates/vox-world/src/proto.rs]
---

# World Wire Protocol

Defined in `crates/vox-core/src/wire.rs` (`pub const PROTOCOL: u32 = 7`). One WebSocket **binary** message per postcard-encoded value. Bump the constant whenever messages change in a way old players cannot read; the server refuses a wrong version.

## Messages
**ToServer**: `Hello {protocol, name, avatar: Vec<u8>, token: Option<String>}`, `Edit {id, cell, block}`, `Pose {at, blob}`, weather change, `Watch {protocol}`, `Through {portal, side}` (protocol 6: go through a portal; the world checks you're in the doorway and answers `Travel`), and `Voice(VoicePacket)` (protocol 7).
**ToClient**: `Welcome {you, world, palette, block_size, bounds, spawn, settings (JSON string), name}`, `Chunk {key, version, data}` (run-length encoded), `Ready`, `Joined {player, name, avatar}`, `Left`, `Edits(Vec<Applied{seq, player, id, cell, block}>)`, `Refused {id, why}`, `Pose`, `Weather`, `Bye {why}`, and (added last, so older clients ignore it) `Correct {at}`: the server's answer to an impossible position, telling the client to go back; `Travel {url, ticket, arrive}` (protocol 6: connect there, saying hello with the one-use ticket, and arrive where that world puts you); `Notice {text}` (added after `Travel` with the raid objectives, under protocol 6: something everyone in the world should know, such as a dungeon's objective reached, shown for a moment); and `Voice {player, packet}` (protocol 7). New messages always go last, so older ones keep their numbers (a test pins each first byte).
`Pose`, `Weather` and `Voice` are *droppable* (a newer one follows; a full queue drops them instead of disconnecting the player); everything else is reliable.

**Voice (protocol 7).** `VoicePacket {channel (0 nearby, 1 party), seq, samples (320), pred, index (<= 88), data (160 bytes)}`: 20 ms of 16 kHz mono IMA ADPCM, about 190 bytes encoded. `VoicePacket::valid()` checks all four limits. The server decides who hears a packet (see `decisions/world-voice-chat`), watchers neither send nor receive it, and old clients are refused by the version check.

## Version history (from docs/PROJECT.md; v3 is not documented)
| Version | Change |
| --- | --- |
| 1 | First slice: chunks, edits, poses |
| 2 | The world's settings (sky, time, clouds, hotbar) travel with the welcome |
| 4 | Hello carries the sign-in token; welcome carries the name you go by |
| 5 | `Why::Claimed`: edits inside someone else's plot are refused |
| 6 | Travel by the server: `ToServer::Through`, `ToClient::Travel` with a ticket |
| 7 | Voice: `ToServer::Voice`, `ToClient::Voice` (droppable); the world's settings carry `voice_radius` |

## Separate protocol
Shows (host, guest, viewers) use a different format in `crates/vox-world/src/proto.rs`, carried by `vox-relay`. See `architecture/show-networking`.

## Design rules
- Poses are opaque to the server: it only needs where the player stands.
- Everything is ordered by the server; clients predict and reconcile (`vox-core::predict`).
- Known protocol gap: the hello carries the model only, so other players do not see what you wear. Fixing it needs a version bump (risk 29).
