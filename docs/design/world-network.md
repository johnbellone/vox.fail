# Design: a network of worlds, and instances

Status: **proposed** · 2026-11-26 · phase 2 (tickets and server-driven travel) built; the rest not yet

Related: [world-scaling.md](world-scaling.md) (one world up to 60 km: regions, generated terrain,
streaming), [world-format.md](world-format.md) (region blobs, content-addressed snapshots),
[world-graph-and-art.md](world-graph-and-art.md) (generated buildings, which dungeons will use).

## Summary

world-scaling.md makes **one** world big. This document makes **many** worlds cheap, so that the
whole game can be far larger than any one server, and adds **instances**: private, short-lived
copies of a dungeon for one raid party.

The design, in one paragraph: a world stays what it is today, one `vox-server` with one thread
that owns its blocks and players. What changes is everything around it. Worlds get **ids instead
of ports**, and one **host** process runs many of them. The directory keeps a **world registry**
and starts worlds **when someone walks towards them** and stops them when they empty. Portal
travel becomes **server-driven**: the world you leave asks the directory for a short-lived
**ticket** for the world you're going to, naming it and your arrival point, so tokens can cross
hosts safely and arrival points are no longer taken on trust. An **instance** is a world started
from a **template** (a dungeon's snapshot), locked to one party, thrown away after. A very large
map is an **atlas**: a grid of ordinary worlds whose **edges are portals**, with terrain generated
from global coordinates so the horizon runs on across the seam.

## Goals

- Total world size limited by storage, not by any server: an atlas of thousands of tiles, of
  which only the occupied ones run.
- Dungeon instances: a party walks into a dungeon door and gets its own copy, which nobody else
  can enter, and which is gone when they're done.
- Worlds can run on more than one machine.
- Close two known gaps on the way: tokens with no audience, and the first position after a
  portal taken on trust (AGENTS.md, *Known gaps*).
- `tools/dev/play-local.sh` and hand-written `"url"` portals keep working.

## Non-goals

- Splitting one world across processes (world-scaling.md, *Later*). Portals and tiles avoid it.
- Seeing players or edits across an atlas seam before crossing it (see *Atlas*).
- Combat, loot and creatures. Instances are designed so these fit later; the first dungeon
  objective is a place to reach, not a boss to kill.

## Where the code is today

| Area | Today | Why it doesn't scale |
| --- | --- | --- |
| World addresses | A port each (`:7780` to `:7784`); portals say `"url": ":7781"`; the tunnel, nginx edge and Helm list every port | Adding a world is a deploy |
| Processes | One `vox-server` per world, started by systemd (`vox-world@.service`) or a Helm `StatefulSet` per entry in `values.yaml` | Thousands of worlds can't each be a unit or a pod; idle worlds cost a process |
| World list | The directory's `--world "void=The Void=:7780"` flags | Static; no notion of a world that isn't running |
| Travel | The client reconnects to the portal's `url` (`WorldClient::travel`); falls back after 20 s | The client decides where it goes and where it arrives |
| Sign-in on travel | The token goes only to the host you started on (`token_host`); a token is valid on every world | Can't cross hosts; a token leaked by one world works on all |
| Arrival | The world you arrive in takes your first position on trust | A modified client can arrive anywhere |
| World → directory | Report every minute and fetch rules, over localhost, one shared `VOX_WORLD_SECRET` | Hosts on other machines; one leaked secret speaks for every world |
| Raids | A party's leader picks a world; ten minutes there with two or more members wins (`game.rs`, `raids`) | No private space, no objective |

The server is ready for one thing already: `vox_server::Server::start(cfg)` and `router()` hold
no process-wide state, so one process can run several worlds side by side.

## Terms

- **World**: one `vox-server` world, as today. Has an **id** (`void`, `yard`, `crypt~a41f`).
- **Kind**: `persistent` (kept for ever, snapshots on), `template` (never run for players; a
  source for instances), `instance` (a copy of a template for one party; thrown away).
- **Host**: a process that runs many worlds on one port, routed by id.
- **Ticket**: a sign-in token for one world, for a minute, used once.
- **Atlas**: a grid of persistent worlds (**tiles**) that together make one large map.

## Design

### 1. Worlds by id, many per host

A new binary, `bins/vox-host`, runs many worlds in one process:

- It listens on one port and routes `/w/<id>/ws` (and `/w/<id>/` for status) to the world with
  that id. Each world is a `Server` as today, with its own thread, SQLite file and snapshots
  folder under `<data>/<id>/`.
- It takes orders from the directory: `start(id, kind, source, lease)`, `stop(id)`, and reports
  each world's players and its own load (worlds, players, memory) every few seconds.
- `vox-server` stays as it is, for one world and for development. `vox-host` is a thin wrapper
  around the same library.

World ids replace ports in every address: the tunnel and the nginx edge route `/w/` to hosts,
not ports. With one host, that's one rule. With several, see *Several machines*.

### 2. The world registry

The directory replaces its `--world` flags with a table:

```sql
CREATE TABLE worlds (
  id TEXT PRIMARY KEY,         -- 'void', 'crypt~a41f', 'atlas:ea:12:7'
  name TEXT NOT NULL,
  kind TEXT NOT NULL,          -- persistent | template | instance
  template TEXT,               -- for an instance: what it was made from
  party INTEGER,               -- for an instance: who it belongs to
  host TEXT,                   -- where it runs, or last ran (persistent worlds stay put; see 7)
  state TEXT NOT NULL,         -- cold | starting | live | draining
  epoch INTEGER NOT NULL,      -- goes up each start; see 7
  seen INTEGER,                -- last report
  expires INTEGER              -- an instance's hard end
);
```

- The existing tables already key on a world id (`plots`, `portals`, `world_state`,
  `whereabouts`, `crews.target`), so they keep working.
- `--world` flags become seed rows, so a small install and `play-local.sh` look the same.
- **Cold start**: when someone needs a cold world, the directory picks a host and sends it
  `start`. The world is `live` when its first report arrives.
- **Idle stop**: a persistent world with nobody in it for 10 minutes is told to stop (it saves
  and snapshots as on SIGTERM today). The Void and other entry worlds can be marked never to stop.

Starting a world means opening SQLite and loading its chunks, which should be quick for today's
small worlds (to be measured: it sets how long a cold portal takes). Streaming regions (world-scaling.md, phase 3) keeps that small for big worlds too.

### 3. Server-driven travel, with tickets

Today the client decides to travel. Instead, the world you're leaving does:

1. The client walks into a portal and sends `ToServer::Through { portal, side }` (new).
2. The world checks you're really in that doorway (its last checked position, within a few
   blocks of the doorway's plane; `vox-core::portal` holds the doorway geometry the client
   draws), works out where you come out, and calls the directory:
   `POST /api/world/travel { world, account, to, arrive }`, `to` being the portal's url.
3. The directory resolves where it leads (`/w/<x>/` or the port names the world; later a
   template: see 4), starts the world if it's cold (phase 3), and signs a **ticket**
   (`vox_core::auth::Ticket`, `t1.` rather than a token's `v1.`, signed with a prefix so neither
   passes for the other): the usual claims plus `world` (the audience), `arrive` and a `nonce`,
   expiring a minute after it's made.
4. The world sends `ToClient::Travel { url, ticket, arrive }` (new). `url` is the destination's
   address on the directory's list; later, with several hosts, an absolute one.
5. The client reconnects and says `Hello` with the ticket. The new world checks the signature,
   that `world` is its own id, that the nonce is new (it remembers nonces until they expire),
   and puts you at `arrive`, not where you say you are.
6. Failure is as today: no welcome within the time limit and you're back where you were.

What this fixes:

- **Tokens with no audience** (a known gap): a ticket only works on one world, once, for a minute.
  The day-long token is only ever sent to the directory and the first world.
- **Arrival on trust** (a known gap): the arrival point comes from the world you left, which
  checked you were in the doorway, signed into the ticket by the directory.
- **Crossing hosts**: a ticket is safe to send anywhere. The day-long token still goes only to
  the host you started on (`token_host`).

Tokens are unchanged, so signed-in players keep theirs. A signed-in player who joins without a
ticket starts at the spawn; guests get no tickets, so their first position is still taken on
trust. The wire protocol goes from 5 to 6. Without a
directory (local play) the world answers `Travel` with no ticket. Still to do: refusing day-long
tokens on worlds that aren't entry worlds, so a token leaked by one world can't be used on
another.

The echo in a doorway still shows the destination level as built (`portals.rs`); for an
instance that's its template, which is exactly what the party will find.

### 4. Instances

**Templates.** A dungeon is built like any world: a level file (`data/levels/dungeons/crypt.json`)
or a world built in by hand and then frozen. Freezing takes a `vox-format` snapshot; the
template **is** that snapshot (its manifest hash, or its commit when the world uses the Oxen
store). Because region blobs are content-addressed,
starting a copy costs nothing: the instance starts from the same manifest and writes new blobs
only for what its party changes.

**Entrances.** A portal can lead to a template instead of a world:

```json
{"name": "crypt", "at": [40, 0, 12], "axis": "x", "size": [4, 6], "template": "crypt"}
```

**Making one.** When a party member goes through, the directory:

1. Looks for the party's live instance of that template. If there is one, the ticket is for it.
2. Otherwise checks the party is allowed one (the lockout, below), makes an id
   (`crypt~<random>`), records `party`, `template`, `expires = now + RAID_LENGTH`, picks a host
   and starts it with `kind = instance`, `source = <manifest>`, `max_players = 8` (a party's size).
3. The return point (the world and doorway you came through) is stored with the instance; its
   exit portal leads there.

**Who may enter.** The ticket is the only way in, and the directory issues tickets for an
instance only to members of its party. A player who leaves the party can't come back in.

**The end.** An instance stops when it's been empty for 5 minutes, when its party is done, or at
`expires`. It doesn't snapshot and its files are deleted after a day (kept that long for
reports of trouble). Anyone still inside goes back to the return point.

**Lockout.** One live instance per party per template, and a limit of new instances per party
per hour (say 3), so a party can't roll copies looking for a better one, and nobody can fill the
hosts by making instances.

**Raids.** A raid today is ten minutes together in a world. A raid on a template is won when the
instance reports its **objective** done: an `Objective` entity in the template (world-format.md
entities: a place to reach, a block to break, later a creature to defeat) that the world server
watches. The existing rules stay: the cost (10 authority), the reward (15), the trophy, and
`RAID_REWARDS_PER_DAY`. Raids on persistent worlds keep the current rule.

### 5. The atlas: very large maps

A map bigger than one world is a grid of **tiles**, each an ordinary persistent world with its
own id (`atlas:<name>:<x>:<z>`). Tiles are 8,192 m across (16,384 blocks, 32 × 32 regions) by
default; the atlas sets its own size, up to world-scaling.md's 60 km.

For scale: a map the size of western Europe, 2,400 × 2,100 km, is about 300 × 260 = 75,000
tiles of 8 km. Untouched tiles cost nothing: no row in storage, no process. Only tiles with
people in them run.

**Edges are portals.** Each tile's four edges act as doorways to its neighbours: walk off the
east edge at `z` and you arrive on the west edge of the next tile at the same `z`, through the
same ticket path as any portal. The client keeps its momentum and view and starts the
connection early (from 64 m away), so the crossing is short; meshes for the far side are
already there (below).

**One terrain across the seams.** The generator (world-scaling.md, *Terrain is generated*)
takes **atlas coordinates** (tile × tile size + local), not tile-local ones, so the ground lines
up exactly at every edge. The client generates the neighbours' terrain itself for far terrain
and for the last stretch before an edge, so the horizon is continuous.

**What doesn't cross a seam** until you do: other players, and edits in the neighbour tile. Far
terrain for edited regions there comes from the neighbour's **region summaries** (the small
height and colour grids from world-scaling.md, *Far terrain*), which the directory or storage can
serve without the tile running. People near a seam are the cost of this design; dense places
(cities) should sit away from tile edges, which an atlas builder can choose.

**Crowds.** A tile is still one thread. A tile that fills up refuses newcomers with a clear
message; events that need more people than a tile holds use instances.

Tile edges only exist on atlas worlds. Hand-made worlds and their portals don't change.

### 6. Several machines

- **Hosts register** with the directory at start, with a public base URL
  (`wss://h3.vox.fail`) and capacity. A ticket's `url` names the host, so the client needs no
  routing of its own.
- **Placement**: instances go to the host with the most room; persistent worlds stay on the host
  that has their files until 7 is built.
- **Public routing**: each host gets its own hostname through the tunnel (or a path prefix on the
  edge). No gateway carries game traffic.
- **World → directory** calls now cross machines, so they go over TLS, and each world gets **its
  own credential** instead of the shared `VOX_WORLD_SECRET`: the directory hands the host a
  per-world secret (an HMAC of the world id under the directory's key) with `start`, and accepts
  reports, travel requests and objective results only for that id. An instance can report only
  for itself, so a compromised instance can't pay authority out for the Void.

### 7. Moving worlds between machines (later)

To start a persistent world on any host, its storage moves off the host's disk: region blobs and
snapshot manifests in object storage (already decided in world-format.md), the SQLite edit log
replaced by a write-ahead log that is flushed into new region blobs.

Two hosts must never run the same world. The registry's `epoch` is a lease: each `start` raises
it, the host must renew it with every report, and every write to storage carries it. Storage
refuses writes with an older epoch, so a host that lost contact and kept running can't overwrite
the world. Instances never need this: they're new each time.

## Security notes

- Tickets: signed, one world, one use (nonces), a minute long. The day-long token goes only to
  the directory and the entry worlds.
- `Through` is checked against the world's own idea of where you are; the arrival point comes
  from the directory.
- Instance creation is rate-limited per party and per account, and capped per host.
- Per-world credentials limit what a compromised world or host can claim.
- Template snapshots are read-only to instances; an instance writes only its own blobs.

## Rollout

Each phase ships on its own.

1. **Ids and hosts.** `vox-host` running the five worlds on one port, routed by id; tunnel, edge
   and Helm route `/w/` to it. Portals keep `"url"`, now `"/w/yard/ws"`.
2. **Tickets and server-driven travel.** `Through`, `Travel`, `/api/world/travel`, ticket claims,
   protocol 6. Closes the two known gaps.
3. **Cold start and idle stop**, on one machine: built into `vox-host` (`--idle-stop`,
   `--keep`), for the worlds it's given. A known world starts on its first connection, so the
   directory needn't be involved yet.
4. **Registry and instances**: the directory's world registry (worlds that exist only once
   asked for), templates from snapshots, template portals, party lock, lockout, objective,
   raids on templates. The first dungeon is a hand-built level with a reach-the-end objective.
5. **Several hosts**: registration, placement, per-world credentials.
6. **Atlas**: atlas coordinates in the generator (needs world-scaling.md phase 3), edge portals,
   early connect, neighbour far terrain.
7. **Object storage and leases**: persistent worlds on any host.

Phases 1 to 4 need nothing from world-scaling.md and give instances on today's machine.

## Open questions

- Instance lifetime: is an hour (today's `RAID_LENGTH`) right for a dungeon, and should a
  cleared instance stay open for a few minutes to look around?
- Should a party be able to reset its instance (a fresh copy) before the lockout ends?
- Tile size: 8 km keeps a tile's server light and makes crossings more frequent; 60 km the
  reverse. Measure a crossing first.
- Should ownership, plots and currencies be per tile, or per atlas? Today they're per world,
  which would make each tile its own country.
- Is a hostname per host acceptable for vox.fail, or should one edge route by id to many hosts
  (a lookup per connection)?
- Where does a player's state live once there's health and an inventory (controls-and-hud.md)?
  The proposal: in the directory, loaded on join and saved on leave, so travel never carries it.
