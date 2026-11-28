---
type: Decision
title: "Decision (proposed): Seamless Transitions, Falling from Orbit into the Sea"
description: "ADR 0004: make-before-break crossings between worlds (Prepare, a second connection, streaming through a frame, velocity in the ticket, fail safe); orbit to sea by falling through a shared planet frame, orbit's compressed high descent, edge:down and the sea's tall sky."
tags: [decision, proposed, travel, worlds, orbit, streaming, physics]
generated: { by: agent/mcp, at: "2026-11-28T04:37:31Z" }
status: draft
---

# Decision (proposed): Seamless Transitions, Falling from Orbit into the Sea

**ADR 0004**, `docs/decisions/0004-seamless-transitions.md`, status **proposed**, amended 2026-11-28. Beads epic `vox-81l` (7 beads). Traversing servers in general, beyond one crossing, is [ADR 0005](traversing-servers.md).

## Facts it rests on
- **Travel is a hand-off with a gap.** `Through`, then `Travel { url, ticket, arrive }`; the client drops the connection, dials the new world and arrives. A ticket carries position and yaw, but no velocity.
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
