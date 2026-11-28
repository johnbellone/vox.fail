# Design: the gateway

- **Status:** proposed (vox-7jh.6), 2026-11-28
- **Implements:** [ADR 0005](../decisions/0005-traversing-servers.md) step 3: a gateway holds each
  player's one connection when worlds span hosts
- **Builds on:** [ADR 0004](../decisions/0004-seamless-transitions.md) (crossings with motion and a
  frame, protocol 11); the neighbour links (vox-7jh.1, `bins/vox-server/src/neighbours.rs`);
  [cloudflare-hosting.md](cloudflare-hosting.md) (the edge Worker, the Durable Object, the one
  container)

## What it is for

Today a player's socket goes straight to one world (`/w/<id>/ws`). Crossing to another world
means a ticket, a new socket and a new join, even when both worlds run in the same `vox-host`.
The gateway is the one place a player's socket ends. Behind it, the player can be in one world
and see into its neighbours, and cross from one to the next without ever reconnecting. The client
never learns how many servers there are.

## Where it runs on vox.fail

**In the container, inside `vox-host`, not in the Durable Object.**

| | In the Durable Object (`apps/edge/src/container.ts`) | In `vox-host` |
| --- | --- | --- |
| Language | TypeScript: the wire would be decoded twice, and `vox_core::wire`'s rules (caps, `Frame::valid`, `Motion::sane`) rewritten | Rust: the same `vox_core` types and checks |
| Hops to a world on the same host | the Worker's relay, then a socket into the container | none: a channel within the process |
| What a deploy does | restarts the object and drops every socket anyway | the same as today |
| State | durable storage it doesn't need | in memory, beside the worlds it routes to |

The Durable Object keeps doing what it does: relaying bytes, counting sockets to keep the
container awake, and showing the offline page.

When worlds span several hosts (several containers, or a self-hosted cluster), each host's
`vox-host` is the gateway for the players connected to it. It reaches worlds on other hosts over
**gateway links**: the neighbour link's handshake (both ends prove the world secret), carrying
player traffic. Such a link crosses a network, so it needs TLS, or a session key derived from the
handshake, before it carries anything (the handshake has no channel binding; see ADR 0005).

## A session

A player's session in the gateway holds:

- **who they are**, from their sign-in token, checked once on connecting, as a world checks it today;
- **their world**: the one with authority over them (it checks their moves, holds their place in
  the edit order, and decides what they may do);
- **slots**: the worlds whose streams they receive, numbered from 0. Slot 0 is always their own
  world; the others are neighbours within the margin (ADR 0005 step 2), each with its `Frame` from
  the player's world;
- **a resume token** (below).

## Routing by owner

What the player sends goes to the world it concerns:

- **`Pose`** goes to their own world, which checks the move and passes it on. Neighbours that show
  the player get it over the neighbour link (the margin), not from the gateway, so a world's view
  of who is near it comes from the world that checked them.
- **`Edit`, `Put` and `Remove`** go to the world that owns the cell (one owner per cell, ADR 0005
  step 2). It is reached through the frame of the slot it lives in, and the cell is mapped into that
  world's coordinates by the frame. The owner judges the edit with the player's rights, which come
  from their own world as today. The answer comes back on the owner's slot.
- **`Through` and edges** go to their own world, as today.
- **`Voice`** goes to their own world, which forwards it to the neighbours that should hear it.

## Merging streams

What comes back is tagged with its slot, so the client can tell where each part came from. That
takes a new message (a protocol bump):

```text
ToClient::Slot { slot: u8, frame: Frame, world: String }   // a slot opens, or its frame changes
ToClient::In { slot: u8, msg: Box<ToClient> }              // a message from that slot's world
ToClient::Close { slot: u8 }
```

- **Slot 0's messages are sent bare, as today**, so a client that has opened no other slot behaves
  exactly as it does now.
- **Each slot keeps its own world's edit order** (`seq`). The client keeps a world per slot and
  draws the others through their frames, as ADR 0004's make-before-break crossing draws the far
  side.
- **Nothing is merged into one order across worlds.** Two worlds never change the same cell (one
  owner), so their orders don't need to agree.
- **Chunks in a neighbour's slot are only those within the margin** (a view radius applied to a
  neighbour, so this waits on streaming, vox-cti.7 and .8).

## Moving authority at a crossing

A crossing becomes a switch of slots, with no new socket:

1. **The old world decides the crossing as now.** The player sends `Through` (or reaches an edge).
   The old world checks it and asks the directory for a ticket, which carries the arrival point and
   the motion.
2. **The ticket stays in the gateway.** It doesn't go to the client: the gateway presents it to the
   new world as the player's hello, over a channel (same host) or a gateway link (another host).
3. **The new world admits the ticket once, as now**, and welcomes the player at the arrival point,
   moving as the ticket says.
4. **The gateway swaps the slots.** The new world becomes slot 0 and the old one a neighbour slot,
   or closes if it is out of the margin. It sends the client `Slot` messages with the frames turned
   around. The client already has the new world's chunks on screen, since it was a neighbour slot,
   so no frame is drawn without a world.
5. **If the new world refuses the ticket, or doesn't answer in time, nothing changes.** The player
   stays where they were, as a refused crossing does today.

On one host, steps 2 to 4 happen within `vox-host` (vox-7jh.5, "one socket, several worlds").

## Restarting without dropping players

- **When the gateway is part of `vox-host`, it restarts with the worlds**, which drops sockets as a
  restart does today. Nothing new is lost.
- **When a player's socket drops for any other reason** (a deploy, a network blip, the container
  waking), the client reconnects with its **resume token**. This is a ticket (`vox_core::auth::Ticket`)
  that the gateway refreshes every 30 s from the player's own world:
  - it names their own world;
  - its arrival point is their last checked position, and it carries their motion;
  - it lasts a minute.

  On reconnecting, the gateway presents it like any ticket, so the player comes back where they
  were, without signing in again, within a minute.
- **A gateway on another host that goes away** costs its players their sockets, which then resume
  through any gateway, since the token names a world and not a gateway. Its gateway links to other
  hosts are dialled again with `vox-link`'s backoff.

## What a hop costs

| Path | Added time |
| --- | --- |
| Player to a world on the gateway's own host | none: an in-process channel |
| Player to a world on another host in the same region, over a gateway link | 1 to 5 ms |
| Between regions | the network's real delay, 10 to 50 ms and more |

- **Bandwidth.** A neighbour slot carries only the margin: its chunks once, then edits, poses and
  weather near the border. A player at a corner of an atlas has at most three neighbour slots.
- **Memory.** The gateway decodes only the envelope and the message kind, to route; the payload is
  passed on as it came.

## What stays visible

- **An edit across a border waits for the owner's answer** before the player's prediction is
  confirmed: one more hop than an edit at home.
- **A neighbour that isn't running shows its border as unlinked**, using the doorway states of ADR 0004
  extended to edges. Edits there are refused as unavailable.
- **Players on gateways in different regions** see each other with their real delay.

## Rejected

- **The Durable Object as the gateway**: see the table above.
- **The client connected to every nearby world**: ADR 0005 rejects it as the end state (bandwidth,
  phones). It survives only as ADR 0004's brief second connection, until the gateway exists.
- **One merged edit order for all slots**: it would need cross-world sequencing for no gain,
  since every cell has one owner.

## Steps

1. **One socket, several worlds, in `vox-host`** (vox-7jh.5): sessions, slots on one host,
   crossings as slot swaps, no envelope yet (slot 0 only).
2. **The envelope** (`Slot`, `In`, `Close`) and neighbour slots, once the margin exists (vox-7jh.2).
3. **Resume tokens.**
4. **Gateway links between hosts**, with TLS or a derived session key.
