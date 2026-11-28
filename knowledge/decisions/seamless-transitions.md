---
type: Decision
title: "Decision (proposed): Seamless Transitions, Falling from Orbit into the Sea"
description: "ADR 0004: make-before-break crossings between worlds (Prepare, a second connection, streaming through a frame, velocity in the ticket, fail safe); orbit to sea by falling through a shared planet frame, orbit's compressed high descent, edge:down and the sea's tall sky."
tags: [decision, proposed, travel, worlds, orbit, streaming, physics]
generated: { by: agent/mcp, at: "2026-11-28T07:04:06Z" }
status: draft
---

# Decision (proposed): Seamless Transitions, Falling from Orbit into the Sea

**ADR 0004**, `docs/decisions/0004-seamless-transitions.md`, status **proposed**, amended 2026-11-28. Beads epic `vox-81l` (8 beads). Traversing servers in general, beyond one crossing, is [ADR 0005](traversing-servers.md).

## Facts it rests on
- **Travel is a hand-off with a gap.** `Through`, then `Travel { url, ticket, arrive }`; the client drops the connection, dials the new world and arrives. Before protocol 11 a ticket carried a position but no motion.
- **Orbit** is a station 250 km up. The planet is the sky shader's (`ORBIT_KM`), not blocks.
- **The sea** is a surface world 48 m tall.
- **The client and server disagree on falling**: the client falls at 22 m/s² with no top speed, and the server refuses falls faster than 70 m/s.

## Decided (proposed)
- **Make before break, for every crossing**:
  - **Prepare by time.** `Prepare { url, ticket, frame }` comes when a player would reach the crossing within speed × (setup + join + first chunks). A WebSocket takes about three round trips, 150 to 300 ms at home, so a walker is prepared 10 to 20 m out.
  - **Wake the destination.** A stopped destination starts on Prepare. A whole sleeping container can't be hidden, but only sleeps when the site is empty.
  - **Live echo.** The second connection starts as a **watcher** (a live echo), streams the far side, and draws it in the current scene through the frame. Between worlds on the same host it can share one socket.
  - **Cross with state.** The ticket carries velocity and facing. The old connection closes only after the new world accepts. If the destination isn't ready, the crossing waits (fail safe).
  - This needs a protocol bump.
- **Orbit to sea**:
  - **Shared frame**: worlds declare a planet frame (surface point and altitude).
  - **Sea**: its sky reaches 2 km (sparse).
  - **Compressed descent**: below the station, orbit maps 8 km of fall to 250 km down to 2 km of true altitude, on a log scale. The sky, curvature and glow are drawn for the true altitude, and a re-entry glow hides the switch.
  - **Crossing**: `edge:down` hands over to the sea's top, already streamed. The last 2 km is real, about 36 s at 55 m/s.
- **Falling settings are per world**: gravity, terminal velocity and the fastest accepted fall, shared by the client and the move check.
- **Doorways show their destination's state**, each differing in colour *and* motion, with a line of text up close:
  - **Open**: today's look, or the live picture once prepared.
  - **Waking**: the destination is starting; rings ripple out from the centre, quickening as it comes up, and you're held at the sill until it's open.
  - **Unlinked**: nothing answers; still, cold grey with a violet edge and static, and you bump back with a notice.
  - **Restricted**: the destination is there but not for you (a raid party only, full, sign-in needed); amber, slow pulse.
  - **Unknown**: not checked yet; today's look.
  - The world server decides from the directory (exists, last report, parties), from `vox-host` (running or starting) and from Prepare results. It sends `ToClient::DoorState`. A state describes only that doorway's own destination.
  - The shader gains a state and the time it began, and blends between looks over half a second.

## Built so far (2026-11-28, branch `claude/seamless-crossings`)
- **`vox-81l.2`, protocol 11.**
  - `vox_core::travel` holds the shared types:
    - `Frame {offset, turn}`. `Door::frame(side)` maps points exactly as `Door::carry`; an atlas edge's frame is a move of one tile.
    - `Motion {velocity, yaw}`. `sane()` holds it to the move check's limits.
    - `DoorState`.
  - Motion travels the whole way: the client sends it with `Through`, the origin world holds it sane and turns it through the frame, the directory signs it into the ticket, and the client arrives still falling.
  - `Prepare` is defined but not yet sent (`vox-81l.3` and `.7`).
- **`vox-81l.8`, doorway states.**
  - **The rule:** `bins/vox-server/src/doors.rs` decides the state per player:
    - a world on this host is open, waking or unlinked, from what `vox-host` says through its `Ways` handle (World state atomics); a stopped one is woken when looked at;
    - a dungeon is restricted without a raid party;
    - another world of a signed-in directory is restricted for guests;
    - an address on another host, or a plain vox-server with no host, stays unknown.
  - **When it's sent:** players within 24 m get `DoorState` on change, checked every second.
  - **Through a waking doorway:** you are held at the sill and sent on once the world is up (90 s at most; leaving the sill gives up the wait).
  - **Client:** it bumps you back from closed doorways with the reason. `PortalMaterial` has a `look` uniform with the state, the state before, the blend and the time held. A closed or waking doorway shows its look even without an echo.
  - **Checked:** a render through vox-host showed an unhosted destination as still grey with a violet edge, and a running one with its echo.
  - **Not done:** the directory as a source (whether a world exists, its last report) and Full.
- Waiting: per-world falling (`vox-81l.1`, which overlaps ADR 0001's `vox-1lp.7`); crossing make-before-break and Prepare by time (`.3`, `.7`, after streaming); the planet frame and the descent (`.4`, `.5`).

## Rejected or kept
- **Rejected**: today's gap; one 250 km-tall world; a cutscene.
- **Kept as a variant**: three stacked worlds (with a sky world), if the descent needs content of its own.

## Depends on
Streaming (vox-cti.7, .8) and the far-field octree (vox-cti.12, .13).

# Related Concepts
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): streaming the far side before crossing
- [Decision (proposed): Simulation State in the Sparse Voxel Octree](simulation-in-the-voxel-octree.md): gravity as a world setting
- [Worlds, Levels and Blocks](../architecture/worlds-catalogue.md): orbit and the sea
- [Decision (proposed): Traversing Servers Without Seams](traversing-servers.md): the general case
- [World Wire Protocol](../architecture/wire-protocol.md): protocol 11
