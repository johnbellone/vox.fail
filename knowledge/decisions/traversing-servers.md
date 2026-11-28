---
type: Decision
title: "Decision (proposed): Traversing Servers Without Seams"
description: "ADR 0005: make-before-break crossings everywhere, neighbouring servers sharing a border margin over vox-link with one owner per cell and forwarded edits, a gateway holding each player's single connection when worlds span hosts, and moving boundaries later."
tags: [decision, proposed, networking, servers, gateway, atlas, travel]
generated: { by: agent/mcp, at: "2026-11-28T07:03:47Z" }
status: draft
---

# Decision (proposed): Traversing Servers Without Seams

**ADR 0005**, `docs/decisions/0005-traversing-servers.md`, status **proposed**, 2026-11-28. Beads epic `vox-7jh`. It generalises [ADR 0004](seamless-transitions.md), which keeps the orbit fall as its worked example.

## Facts it rests on
- Each world is one server thread. `vox-host` runs many worlds in a process, starting them on first connection and stopping them when idle.
- Players connect straight to one world. Neighbouring worlds don't talk to each other.
- On vox.fail the edge Worker's Durable Object already relays every world socket to the one container. A cold container start can take up to 2 minutes, and happens only when the site is empty.
- `vox-link` keeps WebSockets open with backoff, but is not yet used between servers.

## Decided (proposed), in steps
1. **Make before break everywhere** (ADR 0004, amended):
   - Prepare by time: speed × (setup + join + first chunks).
   - Wake a stopped destination when preparing.
   - A live echo: the prepared connection starts as a watcher.
   - On the same host, one socket carries several worlds.
2. **Neighbouring servers share a border margin** (32 m to start) over `vox-link`: chunks, edits, entities, poses, voice and weather cells. You see and hear across the line on one connection.
   - Every cell has **one owner**. A cross-border edit is forwarded to the owner with the editor's rights, signed by the world secret, and both sides get the result in the owner's edit order.
3. **A gateway holds each player's one connection** when worlds span hosts. It routes to owners, merges streams, and moves authority at crossings, at the cost of one extra hop (about 1 to 5 ms in a region). On vox.fail it belongs in the Durable Object or the container.
4. **Boundaries move with load** (server meshing), later.

## Built so far (2026-11-28, branch `claude/seamless-crossings`)
- **`vox-7jh.1`, server-to-server links.** Neighbours are the open atlas edges and the doorways whose url is on this host (`":7781"`, `/w/<id>/ws`, never a whole address elsewhere).
  - **One link per pair:** the world whose id sorts lower dials, using `vox-link` and its backoff. The other accepts on `GET /link`, `/w/{port}/link`, and in vox-host `/w/{name}/link`. vox-host's link route never starts a stopped world (503), so tiles don't wake each other in a chain.
  - **The handshake** (`vox_core::neighbour`, `LINK_PROTOCOL` 1): Hello with the dialler's nonce, a Challenge with the listener's proof, then the dialler's Proof, then Linked. Each proof is a keyed BLAKE3 mac: key derived from the world secret, over role, nonce and length-prefixed world ids. The secret never goes over the wire; nonces come from `getrandom`.
  - **Liveness:** pings every few seconds; 15 s silent counts as down.
  - **Where to see it:** the status page lists `neighbours` and whether each is linked.
  - **Config:** links need both `world_secret` and `link_base`, which defaults from `--listen` or is set by `--link-base`.
  - **Open:** `/w/*/link` is likely reachable through the edge Worker. It can't be passed without the secret, but the Worker could refuse it as it refuses `/api/world/*`.
- Not started: the border margin (`.2`), one owner per cell (`.3`), one socket for several worlds (`.5`), the gateway (`.6`).

## What stays visible
- **Interactions on the line** pay one forwarding hop: correct, but slower.
- **Fast movers** need preparing further ahead.
- **Federation** (trust between directories, matching assets) is a separate decision.
- **Cross-region players** see each other with their real network delay.

## Rejected
- Gaps.
- Clients connected to every nearby server (bandwidth, phones).
- Scaling one server up instead of out for large worlds.

# Related Concepts
- [Decision (proposed): Seamless Transitions, Falling from Orbit into the Sea](seamless-transitions.md): the first step and its worked example
- [Design (proposed): Worlds at 60 km x 60 km](../architecture/world-scaling-design.md): zones as the unit of ownership
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): the margin is a view radius applied to a neighbour
