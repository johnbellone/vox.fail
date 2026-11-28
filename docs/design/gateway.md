# Design: the gateway

- **Status:** proposed (vox-7jh.6), 2026-11-28
- **Implements:** [ADR 0005](../decisions/0005-traversing-servers.md) step 3: a gateway holds each
  player's one connection when worlds span hosts
- **Builds on:** [ADR 0004](../decisions/0004-seamless-transitions.md) (crossings with motion and a
  frame, protocol 11); the neighbour links (vox-7jh.1, `bins/vox-server/src/neighbours.rs`); ADR
  0005 step 2 (the border margin and one owner per cell, vox-7jh.2 and .3);
  [cloudflare-hosting.md](cloudflare-hosting.md) (the edge Worker, the Durable Object, the one
  container)

## What it is for

Today a player's socket goes straight to one world (`/w/<id>/ws`). Crossing to another world
means a ticket, a new socket and a new join, even when both worlds run in the same `vox-host`.
The gateway is the one place a player's socket ends. Behind it, the player can cross from one
world to the next without ever reconnecting, and the client never learns how many servers there
are.

The gateway doesn't replace ADR 0005 step 2. Neighbouring worlds still share their borders over
their links, and edits across a border are still forwarded to the cell's owner by the player's
own world. The gateway adds two things: one socket across crossings, and one socket across hosts.

## Where it runs on vox.fail

**In the container, inside `vox-host`, not in the Durable Object.**

| | In the Durable Object (`apps/edge/src/container.ts`) | In `vox-host` |
| --- | --- | --- |
| Language | TypeScript: the wire would be decoded twice, and `vox_core::wire`'s rules (caps, `Frame::valid`, `Motion::sane`) rewritten | Rust: the same `vox_core` types and checks |
| Hops to a world on the same host | the Worker's relay, then a socket into the container | none: a channel within the process (microseconds; not measured) |
| A deploy | restarts the object (the container may keep running, `container.ts` reattaches its timeout), and the sockets it relayed drop | the container's sockets drop only when the container restarts, as today |
| State | durable storage it doesn't need | in memory, beside the worlds it routes to |

The Durable Object keeps doing what it does: relaying bytes, counting sockets to keep the
container awake, and leaving the offline page to the Worker.

When worlds span several hosts (several containers, or a self-hosted cluster), each host's
`vox-host` is the gateway for the players connected to it. It reaches worlds on other hosts over
**gateway links**: the neighbour link's handshake (both ends prove the world secret), carrying
player traffic. Such a link crosses a network, so it needs TLS, or a session key derived from the
handshake, before it carries anything (the handshake has no channel binding; see ADR 0005).

## A session

A player's session in the gateway holds:

- **who they are**, from their sign-in token, checked once on connecting, as a world checks it today;
- **their world**: the one with authority over them. It checks their moves, holds their place in
  the edit order, decides what they may do, and sends them their view;
- **a prepared world**, during a crossing: the destination, joined ahead of time as ADR 0004's
  second connection, but over the gateway instead of a second socket;
- **a resume token** (below).

## What goes where

Everything the player sends goes to **their own world**, as it does today: poses, edits, entity
changes, `Through`, voice. The gateway doesn't route by cell.

- **An edit across a border** is forwarded by their own world to the cell's owner over the
  neighbour link, carrying the player's rights and signed with the world secret (ADR 0005 step 2,
  vox-7jh.3). The owner answers both worlds, and the player hears the answer from their own world.
  So request ids (`Edit` and `Put` carry the player's own `id`) stay in one place, their world,
  which matches each answer to its request as today.
- **What they see across a border** (the margin's chunks, edits, entities, poses, weather) comes
  from their own world too. It gets them from its neighbours over their links and passes them on.
  There is one stream to the player, in their own world's coordinates.
- **Voice** goes through their own world, which forwards it to the neighbours that should hear it.
  This answers ADR 0005's open question: the world forwards voice, not the gateway.

So the gateway reads only what it must: the message kind, to see `Through` and travel answers, and
its own session messages. Everything else passes through as it came.

## Two worlds at once: a crossing

The only time a player's stream comes from two worlds is during a crossing, which is ADR 0004's
make-before-break done over one socket:

```text
ToClient::Slot { slot: u8, frame: Frame, world: String }   // the prepared world opens (slot 1)
ToClient::In { slot: u8, msg: Box<ToClient> }              // a message from it
ToClient::Close { slot: u8 }
```

- **Slot 0 is the player's own world, and its messages are sent bare, as today.**
- **The envelope is only for a client that asks for it**, with a flag in its `Hello`, under a
  protocol bump. A client that doesn't ask gets today's crossing: `Travel`, then a reconnect to
  the gateway with the ticket.
- **Each slot keeps its own world's edit order.** Nothing is merged across worlds, since every cell
  has one owner.
- **Slot 1's chunks are those the prepared crossing streams** (ADR 0004 decision 2), so it waits on
  streaming (vox-cti.7, .8).

## Moving authority at a crossing

1. **The old world decides the crossing as now.** The player sends `Through` (or reaches an edge).
   The old world checks it and asks the directory for a ticket, which carries the arrival point and
   the motion.
2. **The ticket stays in the gateway.** The gateway sees the `Travel` answer and keeps it from the
   client: it presents the ticket to the new world as the player's hello, over a channel (same host)
   or a gateway link (another host).
3. **The new world admits the ticket once, as now**, and starts the player at the arrival point,
   moving as the ticket says. *Prerequisite:* the arriving world must read `Ticket.motion`. Today it
   is signed in but unused; the client takes its motion from `Travel` instead.
4. **The gateway swaps the slots.** The new world becomes slot 0 and the old one closes. The client
   is told with a `Slot` message carrying the frame from the old world to the new, and redraws in
   the new world's coordinates. It already has the new world's chunks, from slot 1, so no frame is
   drawn without a world.
5. **If the new world refuses the ticket, or doesn't answer in time, nothing changes.** The player
   stays where they were, as a refused crossing does today.

On one host, steps 2 to 4 happen within `vox-host` (vox-7jh.5, "one socket, several worlds").

## Coming back after a dropped socket

A ticket can't serve for this: the directory signs tickets (`/api/world/travel`), worlds hold only
its public key, and a ticket lasts a minute while the container may take two minutes to start
(`READY_MS`).

- **A resume token is the gateway's own**, a new type beside `Ticket`, MACed with a key derived from
  the world secret, which every world on the host already holds. It says:
  - the account and the name and character from their sign-in;
  - their world;
  - their last checked position and motion, which the gateway asks of their world as part of its
    own session messages;
  - when it was made, and a nonce.
- **The gateway makes a fresh one every 30 s, locally**, and sends it to the client. There is no
  call to the directory.
- **It lasts five minutes**, which covers a container restart.
- **On reconnecting**, the client brings its day-long sign-in token and the resume token. The
  gateway checks both: the same account, the resume token MAC'd here and not used before. It then
  starts the player in that world at that position.
  - A stolen resume token is no use without the account's sign-in token.
  - Each nonce is admitted once, so it can't be replayed.
  - Without a resume token, or with a stale one, the player starts at the spawn, as today.
- **The world evicts an older connection of the same account** when a resumed player joins. Today
  `join` doesn't check this, and a socket the world hasn't yet noticed was dropped would otherwise
  leave a ghost beside them.
- **A gateway that restarts with its host** loses its sessions, but not the players' tokens. They
  come back within five minutes to where they were.

## What a hop costs

| Path | Added time |
| --- | --- |
| Player to a world on the gateway's own host | an in-process channel (microseconds) |
| Player to a world on another host in the same region, over a gateway link | 1 to 5 ms (ADR 0005) |
| Between regions | the network's real delay, 10 to 50 ms and more (ADR 0005) |

- **Bandwidth.** The gateway adds nothing to the stream beyond the envelope during a crossing.
  What a border costs is in the margin between worlds (ADR 0005 step 2).
- **Memory.** One session per player, and during a crossing one prepared join.

## What stays visible

- **An edit across a border** waits for the owner's answer, through the player's own world.
- **A neighbour that isn't running** shows its border as unlinked, using the doorway states of ADR
  0004 extended to edges, and edits there are refused as unavailable.
- **Players on gateways in different regions** see each other with their real delay.

## Rejected

- **The Durable Object as the gateway**: see the table above.
- **The gateway routing edits straight to the owner**: it would duplicate ADR 0005's forwarding, and
  it would need a way to carry the player's rights apart from their own world. It saves one hop,
  only for edits across a border.
- **Each neighbour streaming to the player in its own slot all the time**: the player's own world
  already gathers the margin, so this would double the work and the frames for nothing.
- **The client connected to every nearby world**: ADR 0005 rejects this as the end state
  (bandwidth, phones).

## Steps

1. **Prerequisites:**
   - the arriving world reads `Ticket.motion`;
   - a world evicts an older connection of the same account.
2. **One socket, several worlds, in `vox-host`** (vox-7jh.5): sessions, the prepared world, and a
   crossing as a swap on one host. Old clients keep today's flow.
3. **The envelope** (`Slot`, `In`, `Close`, and the `Hello` flag), once streaming lets a prepared
   world stream (vox-cti.7, .8; vox-81l.3).
4. **Resume tokens.**
5. **Gateway links between hosts**, with TLS or a derived session key.
