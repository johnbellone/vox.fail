---
type: Architecture
title: World Wire Protocol (PROTOCOL = 11)
description: "The message set between a world server and its players, how it is encoded, and what each protocol version added (11: crossings carry motion and a frame; Prepare and DoorState)."
tags: [architecture, protocol, networking, postcard]
generated: { by: agent/mcp, at: "2026-11-28T06:27:35Z" }
status: stable
governance: constraint
code_refs: [crates/vox-core/src/wire.rs, crates/vox-world/src/proto.rs]
---

# World Wire Protocol

Defined in `crates/vox-core/src/wire.rs` (`pub const PROTOCOL: u32 = 11`). Each WebSocket **binary** message is one postcard-encoded value. Bump the constant whenever messages change in a way old players cannot read; the server refuses a wrong version.

## Messages
**ToServer**:
- `Hello {protocol, name, avatar: Vec<u8>, token: Option<String>}`, `Edit {id, cell, block}`, `Pose {at, blob}`, a weather change, and `Watch {protocol}`.
- `Through {portal, side, motion}` (protocol 6; `motion` since 11): go through a portal. The world checks you're in the doorway, holds `motion` to what a player can do, and answers `Travel`.
- `Voice(VoicePacket)` (protocol 7).
- The entity messages of protocol 9 (below), appended last: `Put`, `Remove`, `Build`, `Unbuild`.

**ToClient**:
- `Welcome {you, world, palette, block_size, bounds, spawn, settings (JSON string), name}`. `block_size` is 0.25 since protocol 10.
- `Chunk {key, version, data}` (run-length encoded), `Ready`, `Joined {player, name, avatar}`, `Left`.
- `Edits(Vec<Applied{seq, player, id, cell, block}>)`, `Refused {id, why}`, `Pose`, `Weather` (a `weather::Keyframe`), `Bye {why}`.
- Added last, so older clients ignore it, `Correct {at}`: the server's answer to an impossible position, telling the client to go back.
- `Travel {url, ticket, arrive, motion, frame}` (protocol 6; `motion` and `frame` since 11): connect there, say hello with the one-use ticket, and arrive where that world puts you, still moving as `motion` says.
- `Notice {text}`, added after `Travel` with the raid objectives under protocol 6: something everyone in the world should know, such as a dungeon's objective reached, shown for a moment. Protocol 9 also uses it for the counts after an `Unbuild`.
- `Voice {player, packet}` (protocol 7).
- The entity messages `Entities`, `EntityRefused`, `EntityDone` (protocol 9).
- `Prepare {url, ticket, frame}` and `DoorState {portal, state}` (protocol 11).

New messages always go last, so older ones keep their numbers; a test pins each one's first byte. `Pose`, `Weather` and `Voice` are *droppable*: a newer one follows, so a full queue drops them instead of disconnecting the player. Everything else is reliable.

**Voice (protocol 7).** `VoicePacket {channel (0 nearby, 1 party), seq, samples (320), pred, index (<= 88), data (160 bytes)}`: 20 ms of 16 kHz mono IMA ADPCM, about 190 bytes encoded. `VoicePacket::valid()` checks all four limits. The server decides who hears a packet (see `decisions/world-voice-chat`). Watchers neither send nor receive it, and the version check refuses old clients.

**Weather keyframes (protocol 8).** `Keyframe {climate, time, sun, size, cell, origin, seed, fields (7 per cell), rain (per column), ground (per column)}`, all quantised to `u16`. `ground` is how full the ground's dips are (puddles; see `architecture/ground-relief-design`). `Weather::from_keyframe` checks every length against the grid size.

**Entities (protocol 9).** The things placed in a world beside its blocks (`vox_core::entity::Component`: portals, spawn, objective, settings, props, nodes).
- After the chunks and before `Ready`, the server sends every entity in `Entities(Vec<EntityChange{seq, entity, component: Option}>)` batches. Each batch holds at most `ENTITIES_PER_MESSAGE` (512) and is split to stay under `MAX_MESSAGE` (64 KiB), which is also the server's WebSocket frame cap.
- A later change (placed, changed, or removed as `component: None`) takes the next edit `seq`, shared with block edits, so everyone sees one order.
- Client to server:
  - `Put {id, entity: Option<EntityId>, expect: Option<u64>, component}`: a new entity when `entity` is None. `expect` is the seq the sender last saw; an older one is refused `Changed`.
  - `Remove {id, entity, expect}`.
  - `Build {id, node}`: expand a node's recipe into blocks.
  - `Unbuild {id, build}`: undo the build made as request `build`, putting back blocks that have not changed since.
- Answers go to the sender only:
  - `EntityRefused {id, why: EntityWhy}`.
  - `EntityDone {id, entity, seq}`, for an applied Put or Remove, sent after the `Entities` that carries it.
  - An applied Unbuild is answered under its `id`, followed by a `Notice` with the counts.
- `EntityWhy`: `NotYours, Outside, TooFar, TooMany, TooBig, Blocked, BadDest, NoSuchProp, CantAfford, TooFast, Changed, Unavailable, Invalid`, and last `NoSupport` (a prop with nothing under, behind or over it to stand on). `Why` (block edits) gained `Blocked` at the end: a placed prop stands in that cell.

**Quarter-metre blocks (protocol 10).** No message changed. Every position, size and level is now in 0.25 m blocks, so a client from before must not come in.

**Crossings (protocol 11, ADR 0004, `vox-81l.2`).** `vox_core::travel` holds the shared rules:
- `Frame {offset, turn}`: a point here is `turn(p) + offset` there (a rotation about y, as glam's). `Door::frame(side)` gives a doorway's, which maps points exactly as `Door::carry`; an atlas edge's is a move of one tile. `Frame::valid()` checks one from the network.
- `Motion {velocity (blocks/s), yaw}`: `sane(block_size)` holds it to the move check's limits (`moves::MAX_SPEED` sideways, `MAX_RISE` up, `MAX_FALL` down) and makes nonsense still; `through(frame)` turns it into the far frame.
- The client sends its motion with `Through`; the origin world holds it sane, turns it through the frame, and asks the directory (`/api/world/travel` takes an optional `motion`, held sane again). The signed `Ticket` carries `motion`. The client arrives still falling (`Arrival::vy`); walking comes from the player's own input.
- `Prepare` (the far side ahead of time) and `DoorState` (`travel::DoorState`: Unknown, Open, Waking, Unlinked, Restricted(Party | Full | SignIn)) are defined; the client hears and ignores them until vox-81l.3 and vox-81l.8.

Planned next, protocol 12: `ToServer::View` and `ToClient::FarTile` for streaming by view radius and the far-field octree (`architecture/chunk-streaming-design`).

## Version history (from docs/PROJECT.md; v3 is not documented)
| Version | Change |
| --- | --- |
| 1 | First slice: chunks, edits, poses |
| 2 | The world's settings (sky, time, clouds, hotbar) travel with the welcome |
| 4 | Hello carries the sign-in token; welcome carries the name you go by |
| 5 | `Why::Claimed`: edits inside someone else's plot are refused |
| 6 | Travel by the server: `ToServer::Through`, `ToClient::Travel` with a ticket |
| 7 | Voice: `ToServer::Voice`, `ToClient::Voice` (droppable); the world's settings carry `voice_radius` |
| 8 | Weather keyframes carry the ground's water (`Keyframe::ground`) for puddles; world settings may carry `relief` (no version change needed for that) |
| 9 | World entities: `Put`, `Remove`, `Build`, `Unbuild`; `Entities`, `EntityRefused`, `EntityDone` (see `decisions/world-editor`) |
| 10 | Quarter-metre blocks in every world (`architecture/quarter-metre-blocks-decision`) |
| 11 | Crossings carry motion and a frame; `Prepare`, `DoorState` (ADR 0004) |

## Separate protocol
Shows (host, guest, viewers) use a different format in `crates/vox-world/src/proto.rs`, carried by `vox-relay`. See `architecture/show-networking`.

## Design rules
- Poses are opaque to the server: it only needs where the player stands.
- The server orders everything; clients predict and reconcile (`vox-core::predict`).
- Known protocol gap: the hello carries the model only, so other players do not see what you wear. Fixing it needs a version bump (risk 29).

# Related Concepts
- [Decision: The World Editor](../decisions/world-editor.md): why the entity messages exist
- [vox-core](vox-core.md): the entity and edit rules behind them
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): protocol 10
- [Seamless transitions](../decisions/seamless-transitions.md): protocol 11
