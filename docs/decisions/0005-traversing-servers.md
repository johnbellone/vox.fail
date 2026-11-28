# ADR 0005: Traversing servers without seams

- **Status:** proposed
- **Date:** 2026-11-28
- **Decider:** the owner
- **Builds on:** [ADR 0004](0004-seamless-transitions.md) (make-before-break crossings; the
  orbit fall is its worked example); [world-network.md](../design/world-network.md) (tickets,
  atlas edges); [world-scaling.md](../design/world-scaling.md) (zones, simulation owners);
  [cloudflare-hosting.md](../design/cloudflare-hosting.md) (the edge Worker and Durable Object)
- **Beads:** epic `vox-7jh`

## Context

ADR 0004 makes a single crossing seamless: connect to the next world before leaving this one,
then cross still moving. The owner asked the wider question: how seamless can moving **between
servers** be? That covers:

- atlas tiles served by different worlds;
- worlds on different hosts;
- later, one large world split across several machines.

What exists today:

- **One world, one server thread**, which owns its blocks, entities, players and edit order.
  `vox-host` runs many worlds in one process, each on its own path (`/w/<id>/ws`), with worlds
  started on first connection and stopped when idle (`--idle-stop`).
- **Players connect straight to one world.** A crossing is a ticket and a new WebSocket.
- **Neighbouring worlds don't talk to each other.** Atlas tiles agree at their edges only
  because their ground is generated from the same seed. Edits, players, voice and weather stop
  at the edge.
- **On vox.fail**, the edge Worker's Durable Object already relays every world socket to the
  one container (`apps/edge/src/container.ts`). It starts the container on the first request:
  restoring from R2 and starting the servers may take up to two minutes (`READY_MS`). It keeps
  the container awake while sockets are open, and lets it sleep after 15 minutes without
  requests or players.
- `vox-link` keeps a WebSocket open on its own thread, reconnecting with backoff. The avatar
  app uses it to reach the relay; nothing uses it between world servers yet.

## Decision

Moving between servers is made seamless in four steps. Each one removes a seam the previous
step left. A step is built only when something needs it.

### 1. Make before break, everywhere (ADR 0004)

Every crossing between worlds prepares early, streams the far side, and crosses with velocity.
Two refinements apply across servers:

- **Prepare by time, not distance.** How far ahead to prepare is speed × (connection setup +
  join + first chunks) + a margin. A walker is prepared 10 to 20 m out; a faller or a vehicle
  much further.
- **Wake the destination early.** Preparing also starts the destination world if it is stopped
  (`--idle-stop`), so its start-up hides behind the approach. A crossing can't hide a whole
  container waking from sleep; on vox.fail that only happens when nobody is on the site, so it
  never happens mid-crossing.

### 2. Neighbouring servers share their borders

Worlds that meet each other (atlas tiles side by side, the two ends of a vertical edge, a
doorway pair) keep a **server-to-server link** with `vox-link`. Over it they exchange what lies
within a **margin** of their shared border (32 m to start):

- **Chunks and edits** in the margin, so each server can show its players the neighbour's side.
- **Entities, player poses and voice** in the margin, so players see and hear each other across
  the line through their one connection.
- **Weather cells** along the edge, every step, so the air has no seam.

Every block, cell and entity has **one owner**, the server whose area it lies in (world-scaling's
zones are the unit). A player's edit across the line goes to their own server, which forwards it
to the owner. The owner decides and answers both servers, and the result reaches both sides'
players in the owner's edit order. Nothing is changed two ways at once. A forwarded edit is
checked by the owner with the editor's rights, carried with it and signed by the forwarding
server's world secret.

### 3. A gateway holds the player's one connection

When worlds span hosts, players connect to a **gateway** instead of to a world. The gateway:

- routes each message to the server that owns where it applies;
- merges what comes back (chunks and poses from both sides of a border);
- moves a player's authority to the next server at a crossing.

The client never reconnects between servers and never learns there are several.

- On vox.fail the Durable Object that relays world sockets already sits in that path. A gateway
  per session belongs there, or in the container in front of `vox-host`.
- On one host, `vox-host` can do it in-process. Several worlds share one WebSocket, and a
  crossing is a switch of which world you are in.
- It costs one extra hop for every message: about 1 to 5 ms within a region.

### 4. Boundaries that move with load

With a gateway in front, a server's area can split or merge while players are in it: a crowd at
the Circle gets its own machine, and a quiet district goes back to its neighbour's. The handover
is the same as a crossing (step 3) for every player and entity in the area. Players don't see
it. This waits until a world needs more than one machine.

## What stays visible, and how it is softened

- **Interactions on the boundary** (a collision, a hit, a build spanning the line) pay one
  forwarding hop: about 1 to 5 ms on one host, 10 to 50 ms between regions. One owner per cell
  keeps them correct; they are only slower.
- **Very fast movers** cross many borders. They are prepared by their speed (step 1), and their
  next servers are linked to their current one early.
- **Different hosts and owners** must trust each other: a directory accepting another's tickets
  (federation) is a decision of its own. Their block palettes and props must agree, or the
  client falls back (to stone for an unknown block).
- **Players far apart in the world network** see each other with their real network delay, as
  in any online game.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A | Hand-offs with a gap (today) | Rejected: visible seams. |
| B | Make before break only (ADR 0004) | Step 1: no blank frame, but you can't see or reach across a border until you cross. |
| C | Client connected to every nearby server at once | Rejected as the end state: the client does the merging, the bandwidth multiplies, and phones can't afford it. Kept briefly during a crossing (B). |
| D | Border sharing between servers, one owner per cell | **Chosen** (step 2): seamless across borders on a single connection. |
| E | A gateway holding each player's connection | **Chosen** (step 3), when worlds span hosts: no reconnects at all. |
| F | One huge server per world, scaled up rather than out | Rejected for large worlds: one thread can't keep up with a crowded 60 km world (world-scaling.md). |
| G | Moving boundaries (server meshing) | Step 4, when needed. |

## Consequences

- **Good:**
  - Walking, building and talking across server borders look and feel like one place.
  - Worlds can be spread over machines and hosts without players noticing.
  - The same machinery serves atlas tiles, vertical edges and doorway pairs.
- **Costs:**
  - Server-to-server links and a margin protocol (chunks, edits, entities, poses, voice, weather).
  - Edit forwarding, with ownership rules and their tests.
  - A gateway: one more component to run, one more hop, and session state to keep if it
    restarts.
  - More bandwidth between servers near busy borders.
- **Depends on** ADR 0004's crossing (vox-81l.2, .3) and on streaming by view radius
  (vox-cti.7, .8). The margin is a view radius applied to a neighbour.
- **Open:**
  - The margin's width.
  - Whether a server forwards a neighbour's voice or the gateway does.
  - Federation between directories.
  - Where the gateway runs on vox.fail (the Durable Object or the container).
  - How a gateway restarts without dropping players.

## Follow-up beads (epic `vox-7jh`)

1. `vox-7jh.1` Server-to-server links between neighbouring worlds (`vox-link`, the world
   secret).
2. `vox-7jh.2` The border margin: chunks, edits, entities, poses, voice and weather (after
   streaming, vox-cti.7).
3. `vox-7jh.3` One owner per cell; cross-border edits forwarded to the owner.
4. `vox-7jh.4` Review of the three above.
5. `vox-7jh.5` One socket, several worlds, in `vox-host`.
6. `vox-7jh.6` Design: the gateway for worlds on several hosts.
7. `vox-7jh.7` Decision: federation between directories.

Step 1 is ADR 0004's epic (`vox-81l`, with `vox-81l.7` for preparing by time, waking the
destination and the live echo).
