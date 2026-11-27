# Design: worlds at 60 km × 60 km

Status: **proposed** · 2026-11-25 · nothing here is built yet

## Summary

Vox worlds today are small enough to send whole: 512 × 128 × 512 blocks, about 1,000 chunks,
all sent on joining and all simulated all the time. The goal is worlds around **60 km × 60 km**,
in the browser and on the desktop. At 0.5 m blocks that is 120,000 blocks a side, roughly
14 million chunk columns, about 55,000 times today's size.

The design, in one paragraph: the world is divided into **zones**. Untouched ground is
**generated** from a seed on whichever machine needs it, so only edits are stored and sent.
Each client asks for the area around it at a **view radius it chooses** (small in the browser,
larger on the desktop), and gets chunks nearest-first within a bandwidth budget. Beyond that
radius it draws **low-detail terrain**. **Cosmetic simulation** (fine weather, particles,
ambient life) runs on one nearby client per zone, the *simulation owner*, and everyone else
interpolates its snapshots; **anything that changes the game** (blocks, plots, currency, harm)
stays authoritative on the server. Nothing simulates where no one is. Clients may **cache**
zones and check them against **zone hashes** arranged as a tree, so a reconnect sends only
what changed.

## Goals

- Worlds of about 60 × 60 km, with the same rules as today (server-authoritative edits).
- Playable in a browser tab on a laptop with integrated graphics, and better on a desktop.
- Server cost grows with players and edits, not with world area.
- Portal trips and reconnects are fast; a returning player downloads only what changed.

## Non-goals

- Seamless multi-server sharding of one world (the zone design leaves room for it; see
  *Later*).
- Deterministic lockstep simulation across clients.
- Changing the block size or the look of the game.

## Where the code is today

| Area | Today | Why it breaks at 60 km |
| --- | --- | --- |
| Bounds | `Bounds::default()` is 512 × 128 × 512 blocks (`core/src/world.rs`) | Fixed and small |
| Join | Server sends every chunk on join; its queue is sized for "a world is at most 1024 chunks" (`server/src/lib.rs`) | 56 M chunks cannot be sent |
| Server memory | Whole world in memory, saved as one row per chunk in SQLite | Millions of rows; cannot hold in RAM |
| Encoding | Each chunk re-encoded per joining player | CPU per join grows with world |
| Client memory | Every chunk kept as 32³ `u16` (64 KB), mostly air | 200 MB at a 256 m radius |
| Meshes | 64 bytes per vertex (position, normal, UV, tangent, colour as floats; `world/src/blocks.rs`), up to four meshes per chunk | Hundreds of MB to GB of GPU memory |
| Meshing | All dirty chunks meshed in one frame; one-block edits rebuild the whole chunk | Long stalls while walking |
| Weather | One 32 × 10 × 32 grid of 8 m cells, stepped on the server every tick and by each client between keyframes | A 60 km grid at 8 m is ~50 M cells |
| Precision | Positions in `f32` metres | ~4 mm steps 30 km from the origin: visible jitter |
| Browser build | Single-threaded (no wasm threads, no cross-origin isolation headers) | All of the above competes for one 16 ms frame |

## Constraints: browser and desktop

The browser sets the baseline; the desktop is allowed to go further.

| | Browser (wasm32 + WebGPU) | Desktop (native) |
| --- | --- | --- |
| Memory | 4 GB address space; tabs are often killed around 1 to 2 GB; phones far less | 64-bit; several GB is fine |
| CPU | One thread for everything unless work moves to Web Workers (separate wasm instance, message passing) | Bevy task pool across all cores |
| GPU | WebGPU: higher per-draw cost, buffer size limits; often integrated graphics | Native Vulkan/Metal/DX12; more VRAM |
| Storage | IndexedDB or the origin private file system; quota varies and the browser may evict it | Files on disk |
| Network | WebSocket over TCP: one big message delays everything behind it. WebTransport is an option to evaluate (browser support and the Cloudflare Tunnel are unverified) | WebSocket now; UDP or QUIC possible |

Rough cost of the area around a player, at today's formats (estimates; assumes four chunks tall):

| View radius | Chunks loaded | Raw block data | Meshes |
| --- | --- | --- | --- |
| 128 m | ~800 | ~50 MB | ~200 MB |
| 256 m | ~3,200 | ~200 MB | ~800 MB |
| 512 m | ~13,000 | ~800 MB | several GB |

Most loaded chunks are air or solid underground, so real numbers are lower, but the formats
must shrink before the radius can grow (see *Memory and meshes*).

## Design

### Coordinates, zones and regions

- **Blocks** stay `i32` cells (±2 billion; 120,000 is fine). A **chunk** stays 32³ blocks (16 m).
- A **zone** is 16 × 16 chunk columns, the full height of the world: 256 m × 256 m (512 blocks).
  A 60 km world is about 235 × 235 ≈ 55,000 zones. Zones are the unit of storage, hashing,
  streaming priority and simulation ownership.
- A **region** is 32 × 32 zones: 8,192 m × 8,192 m (16,384 blocks), exactly one atlas tile (see
  [world-network.md](world-network.md)). A 60 km world is about 8 × 8 regions (7.3 across). A
  region is a grouping of zones for the hash tree and the snapshot manifest, not a unit of
  streaming or simulation.
- Worlds are about 128 blocks tall, so all "spheres" in this document are circles on the map
  measured in chunk columns, not 3D spheres.
- **Floating origin.** Rendering and physics work relative to an origin near the player. When
  the player moves more than 1 km from it, the origin moves and everything is rebased. Network
  and storage positions are a zone or chunk key plus an `f32` offset inside it, never an
  absolute `f32`.

### Terrain is generated, edits are stored

- Each world has a **generator**: a seed plus parameters (heights, biomes, water level). Given a
  chunk key it produces that chunk's blocks. The same function runs on the server and on both
  clients.
- The generator must give **identical results in native and wasm builds**: integer or
  fixed-point maths and a fixed hash, no platform floats or `HashMap` iteration order. It lives
  in `vox-core` (no Bevy) and is covered by golden tests that run on both targets.
- The server stores **only edited chunks**. A chunk with no stored row is "as generated". The
  directory and portals keep working unchanged.
- Hand-built levels (`data/levels/*.json`) become a stored overlay on a generator (the Void's
  generator is the flat floor it already draws).

### Streaming by area of interest

The client tells the server where it is looking and how much it can take; the server sends only
what is needed, nearest first.

New messages (sketch; names are not final; protocol version bumps):

- `ToServer::View { center: ChunkKey, radius: u16, budget_kbps: u32 }`: sent on join and when the
  player crosses a chunk boundary or changes settings. The server clamps `radius` to a
  per-world maximum.
- `ToServer::Have { zone: ZoneKey, hash: [u8; 16] }` (batched): what the client already has
  cached (see *Caching*).
- `ToClient::Zone { key, hash, chunks: Vec<(ChunkKey, version, bytes)> }`: the **edited**
  chunks of a zone (generated ones are omitted), or an "unchanged" reply when the client's
  hash matches.
- `ToClient::Edit` and `ToClient::Pose` go only to players whose view includes that chunk.

Server side:

- Each player has a **view set** of zones. The server keeps a reverse index, zone to players,
  so an edit is sent to a handful of players, not broadcast.
- Sending is **priority-ordered** (distance from the player, then direction of travel) and paced
  to the player's budget. Large messages are split so poses and edits are never stuck behind a
  zone download.
- **Encode once.** A zone's wire bytes are cached and invalidated by edits, so a join costs
  copies, not encoding.

Client side:

- Chunks enter and leave the loaded set with **hysteresis** (load inside the radius, unload
  past radius + 32 m) so walking along an edge does not thrash.
- Received and generated chunks go through the same path into meshing.

### Storage

- The server loads zones **on demand** and evicts idle ones. A zone is saved as one row (or
  one file) holding its edited chunks, compressed, plus its hash.
- Edits are appended to a log as today; old log entries are compacted into the zone rows.
- SQLite remains fine at this size if rows are per zone (about 55,000 at most, far fewer in
  practice), with `synchronous = NORMAL` and a writer thread off the world thread.

### Memory and meshes

Before raising any radius:

1. **Uniform chunks** (all air, all stone) are stored as one value, not 64 KB.
2. **Packed vertices**: position inside the chunk, face direction, texture index and light fit in
   8 to 16 bytes instead of 64. The shader unpacks them.
3. **Greedy meshing**: merge adjacent identical faces into larger quads.
4. **Fewer draw calls**: batch chunks of a zone into one mesh per material, and keep glass,
   water and flame meshes only where they exist.
5. **Per-frame meshing budget**: mesh at most N chunks or M milliseconds per frame, nearest first.
   One-block edits remesh only the affected chunk, and later only the affected section.
6. **Off the main thread**: on the desktop, mesh and generate in Bevy's task pool. In the browser,
   start with the per-frame budget; move to Web Workers if measurements show it is needed. Both
   sit behind one interface so game code does not know which is used.

Together these are expected to cut memory and GPU use by roughly ten times; to be confirmed by
the prototype (see *Rollout*).

### Far terrain

Beyond the full-detail radius, the client draws a **low-detail surface**: a heightmap mesh per
zone at decreasing resolution with distance.

- For generated ground the client computes it locally: no network.
- For edited zones the server sends a small **zone summary** (a coarse height and colour
  grid, a few KB), updated when edits there change it. Summaries hang off the same zone
  records as the hashes.

### Simulation: owners, interpolation, and authority

Nothing simulates where no one is. Where players are, simulation splits by whether a wrong
result could matter to anyone else.

**Client-owned, interpolated** (cosmetic): fine-detail weather, particles, cosmetic water, foliage
movement, ambient creatures that cannot affect anything.

- For each active zone the server picks a **simulation owner** among nearby clients,
  preferring the strongest (desktop over browser, then by reported frame time).
- The owner sends compact, quantised **snapshots** at a low rate; the server relays them to
  players in range, who interpolate. The existing weather keyframes (quantised to 16 bits) are
  the model.
- Determinism is not required, because others interpolate rather than re-simulate.
- **Handoff**: when the owner leaves, stops sending or falls behind, the server picks another
  and gives it the last snapshot. Snapshots must be complete enough to resume from.
- Owners only ever affect cosmetic state, so a modified client can at worst show itself and
  its neighbours odd weather.

**Server-authoritative** (gameplay): block changes, plots, raids, currency, anything that hurts or
blocks a player, and anything that crosses zone edges in ways that matter. These are
event-driven, so they stay cheap on the server.

**Rotating authority with server-chosen snapshots** (for when clients share the compute of
anything that matters, not only cosmetics):

- Each active zone runs in **epochs** of 30 to 60 seconds. At each epoch the **server picks**
  the owner at random from capable nearby clients; nobody can ask for a zone, and nobody gets
  one where they gain from the result (their own plot, a raid they're in).
- **One chain**: every epoch starts from the last snapshot the server accepted, and the server
  records which that was. Handoff is "start from snapshot N".
- **Snapshots must be checkable**, so choosing one means something:
  1. *Redundancy* for gameplay state: two or three clients run the same epoch from the same
     snapshot and inputs and send hashes; the server takes the majority and flags the rest.
  2. *Spot audits* for single-owner (cosmetic) work: the server re-runs a random few percent of
     epochs itself; a cheater can't know which.
  3. *Rule checks* on every snapshot: conservation, limits, nothing changed that the rules forbid.
- A client caught disagreeing loses authority; a per-account trust score decides who is eligible.
- **This needs a bit-identical simulation** on every machine and target: fixed-point or integer
  maths in `vox-core` (floats' `sin`, `exp` and the like differ between math libraries), with
  golden tests on native and wasm. The current float weather would have to move to it first.
- Cost: redundancy doubles compute and upload for the clients involved (prefer desktops), and
  accepting a snapshot waits for the hashes.

**Weather in two layers**:

- A **coarse global layer** on the server, for example 1 km cells (60 × 60 = 3,600 cells). It is
  cheap enough to run everywhere all the time, so fronts and storms move across the map and
  arrive from somewhere.
- A **fine local layer** run by simulation owners inside active zones, seeded at its edges by
  the coarse layer. Overlapping areas merge into one patch so nearby players see the same rain.
  A browser client may skip local stepping and only interpolate.

**Elapsed-time state**: anything that should progress while no one is there (crops, decay,
timers) is computed from timestamps when next observed, not stepped.

### Caching and sync

- Clients may keep a **zone cache**: on disk natively; IndexedDB or the origin private file
  system in the browser. The browser cache may be evicted at any time, so an empty cache must
  always work.
- Each zone has a **hash** of its edited chunks' bytes (not their version numbers, so a no-op
  edit does not invalidate caches). The hashes form a **tree of three levels**: the snapshot
  root over the regions, each region over its zones (a region blob lists the hash of each of its
  zones that has blocks, at most 1,024). A 60 km world is about 8 × 8 regions, so a quadtree
  inside a region is not needed.
- On join or reconnect, the client sends the hashes of the cached zones in its view
  (`ToServer::Have`); the server replies "unchanged" or sends the zone. For a world with
  very many edited zones, comparing near the top of the tree first narrows the differences in
  a few round trips.
- **Bloom filters are not used.** A false positive would leave a missing chunk that nothing
  corrects; the exact hash comparison is already cheap at this scale.

### Security notes

- Everything a client sends stays validated on the server: view radius and budget are clamped,
  `Have` batches are capped, and position-dependent rules (reach, plot claims) need server-side
  speed and bounds checks, which are missing today (see `AGENTS.md`, *Known gaps*).
- Simulation-owner snapshots are size-capped and rate-limited like poses, and are relayed only
  to players in range.

## Budgets (first targets, to be measured)

| | Browser | Desktop |
| --- | --- | --- |
| Full-detail radius | 160 m | 384 m (user may raise) |
| Far terrain | to 2 km | to 4 km |
| Meshing per frame | 2 ms | task pool, 4 ms main-thread apply |
| Memory target | under 1 GB total | under 3 GB total |
| Download budget | 2 Mbit/s default | 8 Mbit/s default |

## Rollout

Each phase ships on its own and is measured before the next.

1. **Measure.** In Chrome and on the desktop, on the developer's laptop (Radeon 890M): frame time
   and memory at 128, 192 and 256 m with today's formats, then with packed vertices, uniform
   chunks and a per-frame meshing budget. These numbers fix the real budgets above.
2. **Memory and meshes.** Packed vertices, uniform chunks, greedy meshing, meshing budget, per-zone
   batching. No protocol change.
3. **Zones, generator, streaming.** Zone keys, the generator in `vox-core` with cross-target
   golden tests, `View`/`Zone` messages, server view sets and pacing, encode-once cache, zone
   storage. Protocol version bump; vox.fail upgraded in one release.
4. **Far terrain.** Local low-detail terrain, zone summaries.
5. **Simulation owners and two-layer weather.**
6. **Client cache and zone hashes.**
7. **Floating origin**, as soon as worlds larger than a few km are opened to players.

## Later

- Splitting one world across server processes by zone (the zone keys and the view index are
  designed so this is possible).
- WebTransport or QUIC for unordered streams, if measurements show head-of-line blocking hurts.

## Open questions

- World height: is 128 blocks (64 m) enough for 60 km worlds with mountains and caves?
- How much of a 60 km world is hand-built versus generated? This decides how big zone rows
  get.
- Should the per-world view-radius limit be set by the world owner (a directory setting)?
- Does WebTransport work through the Cloudflare Tunnel in front of vox.fail?
