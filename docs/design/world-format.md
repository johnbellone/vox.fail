# Design: the world format

Status: **phase 1 built** · 2026-11-26 · decisions recorded (see *Decisions*). `crates/vox-format`
has zone and region blobs (blocks), the blob store and snapshots with a week's retention; the world server
snapshots every five minutes and on closing, and restores (`--restore`, `--restore-at`). Oxen
is the snapshot store when asked for (`--oxen`; see *Oxen*), a commit per snapshot with history
older than the week squashed daily. Structure entitlements (who may place, copy and edit) are
built in the directory (see *Entitlements*); structures themselves (placing them in worlds) are
phase 6. Not yet: object storage beyond a local folder.

Related: [world-scaling.md](world-scaling.md) (zones, regions, streaming, zone hashes, simulation
owners), [world-graph-and-art.md](world-graph-and-art.md) (world graphs and recipes, which
become entities here), [repo-structure.md](repo-structure.md) (where the crate goes).

## Summary

A world is stored and sent as **zones** (256 m × 256 m, 16 × 16 chunk columns). Each zone
is one compressed blob in two parts:

- **Blocks**: the zone's edited chunks in today's run-length encoding, compressed together
  with zstd. Dense data in a dense binary format.
- **Entities**: everything that isn't a block (portals, spawn points, lamps and props, plots,
  signs, world-graph nodes, later creatures and inventories), as a Bevy `DynamicWorld`. Written
  as **RON** for authoring and review; stored and sent as **postcard** from the same types.

Zone blobs are **content-addressed**: stored once under their hash. Zones are grouped into
**regions** of 32 × 32 zones (8,192 m across, exactly one atlas tile); a region is a small blob
of its own that lists the hash of each of its zones. A **snapshot** is a small manifest mapping
region keys to region hashes, whose root is the hash tree from the scaling design (snapshot root,
regions, zones). Unchanged regions are shared whole between snapshots, and inside a changed
region unchanged zones are shared, so a snapshot of a large world costs only what changed; the
existing edit log covers the time between snapshots.

Everything that reads or writes the format lives in a new crate, `crates/vox-format`, with no
rendering, so the server, the clients (native and wasm) and the authoring tool share it.

## Decisions (2026-11-26)

1. **Player-built structures are entities**, stored as a **reference to a specific version** of
   a structure, not as loose blocks (see *Structures*).
2. **Object storage** holds the blobs; **Oxen** is explored as the snapshot and history mechanism
   (see *Oxen*). The live connection stays the world server's WebSocket.
3. **World owners restore their worlds themselves**; snapshots are **kept for a week**.

## Where it is today

| | Today | At 60 km |
| --- | --- | --- |
| Blocks on the server | SQLite: one row per chunk (`chunks(x, y, z, version, data)`), data in run-length encoding (varint run length, varint block id) | Millions of rows; zones are the better unit |
| Edit log | SQLite `edits(seq, at, player, x, y, z, old, new)`, kept for ever | Needs compaction into snapshots |
| Levels | `data/levels/*.json`: boxes and single blocks by name, plus a `world` object (sky, time, portals) | Entities need a real format |
| Entities | Only portals and settings, as JSON inside the level and the server's `meta` table | Plots, props, graph nodes, inventories |
| On the wire | Every chunk on join, one message each | Zones, only what changed (scaling design) |

## Measured

The islands world (257 stored chunks, about 405,000 blocks):

| Encoding | Bytes |
| --- | --- |
| Plain 16-bit blocks | 16,842,752 |
| Run-length (today) | 211,314 (822 a chunk on average) |
| Run-length + zstd, each chunk alone | 79,500 |
| Run-length + zstd, each chunk with a dictionary trained on them | 70,091 (optimistic: trained on the same chunks) |
| Run-length + zstd, a zone's chunks together | 62,831 |

So: keep the run-length encoding (it does the heavy lifting, 80× smaller), and compress a
zone's chunks together (another 3.4×), which beats a dictionary without having to ship and
version one. A dictionary stays an option for sending single chunks (an edit's neighbourhood).

## The zone blob

```
"VOXR"                      magic (kept from when zones were called regions, so stored hashes stay valid)
u16    format version
i32 × 2 zone key (x, z in zones)
u32    flags (bit 0: has entities)
u16    block palette version   (which data/blocks.json the ids refer to)
u32    blocks section length   (compressed)
[u8]   zstd frame: for each edited chunk, in (y, z, x) order:
           varint chunk index within the zone (column x, z and height y)
           varint chunk version
           varint length, then the run-length bytes
u32    entities section length (compressed; 0 when none)
[u8]   zstd frame: postcard DynamicWorld (see Entities)
[u8;16] hash of everything above (BLAKE3, first 16 bytes)
```

- Chunks not in the blob are **as generated** (the scaling design's generator) or air.
- The hash is over the uncompressed sections, so recompressing at another level doesn't change
  identity; it is the zone's address in the store and its leaf in the hash tree.
- Block ids are the world's palette order (as on the wire today); the header names the palette
  version so an old blob is remapped when the palette changes.
- Limits on reading untrusted blobs: at most 4,096 chunks (16 × 16 × 16 heights), each
  decompressed chunk checked by `Chunk::decode` (already bounds-checked), and the
  decompressed size capped before allocating.

## The region blob

A region groups 32 × 32 zones (1,024 at most) and is stored as a small blob that lists its zones:

```
"VOXG"                      magic
u16    format version
i32 × 2 region key (x, z in regions)
u32    zones
       for each zone that has blocks, sorted by (z, x):
           u8 × 2   zone x, z within the region (0 to 31)
           [u8;16]  the zone blob's hash
```

- A region blob is content-addressed like a zone: its hash (BLAKE3, first 16 bytes) is its
  address. It names zones by hash, so it holds no block data and stays under 20 KB.
- Reading untrusted bytes: the count is capped at 1,024, the length must match exactly, and
  zones must be inside the region, sorted and unique.
- A zone with no blocks is left out. An atlas tile is exactly one region.

## Entities

Entities are stored in **our own blob**, modelled on Bevy's scene format (`DynamicWorld`) but
not using it: `DynamicWorld` lives in `bevy_world_serialization`, which (checked in our lockfile,
2026-11-26) pulls in 29 Bevy and wgpu crates, `bevy_camera`, `bevy_mesh`, `bevy_image`,
`bevy_window` and `wgpu-types` among them. That would end the server's freedom from Bevy and
weigh on the browser build. See [bevy-ecosystem.md](bevy-ecosystem.md).

```
ZoneEntities { entities: [ { id: u32, components: [ (stable_name, version, bytes) ] } ] }
```

- postcard bytes; entity links (a portal and its key, a road between two houses) are the local
  `id`s inside the blob, mapped to real entities on load, as `DynamicWorld` maps its references.
- A **version per component**, so migrations run on the blob before anything reaches an ECS.
- **Saved versus drawn** (moonshine_save's pattern): only world components are saved; what is
  drawn (meshes, particles, lights) is rebuilt from them on load and never stored.
- RON is the authoring form (levels, templates); postcard is what is stored and sent.
- **Component types live in `vox-format`**, not in `vox-world` (which renders): the server needs
  them to validate what it stores, without a renderer. `vox-format` depends on **`bevy_reflect`
  alone** (for the types, stable names and a limited registry), fine without rendering and on
  wasm; the client turns the blob into Bevy entities in `vox-world`.
- **First components**: `Portal { to, size, axis, arrive, yaw }`, `Spawn`, `Light { kind, power }`,
  `Prop { model, at, yaw }`, `Plot { area, owner, builders }`, `Sign { text }`, and the world graph's
  `Node { kind, detail, params, recipe, seed }` with `Link { to, rel }`.
- **Stable names**: RON names components by Rust type path (`vox_format::portal::Portal`), so
  moving a type would break saved worlds. Every world component gets a fixed type path
  (`#[reflect(type_path = "vox::Portal")]`) and a version; a load runs migrations from older
  versions, with tests that load files saved by each earlier version.
- **Untrusted input**: anything that came from a client is deserialised against a type registry
  holding only the world components (never the app's own), with a cap on entities per zone and
  on string lengths, so a crafted blob can't create arbitrary components or exhaust memory.

## Structures

What a player builds is a **structure**: its blocks (and its own entities: lamps, signs, props
inside it) stored as a content-addressed blob of its own, the same format as a zone's blocks
and entities but in the structure's own coordinates. A world holds **instances**:

```ron
(
  structure: "a3f9c1...",        // the structure's version: the hash of its blob
  at: (412, 64, -380),          // where its origin sits, in blocks
  turn: 1,                      // quarter turns about y
)
```

- **Versions are immutable.** Building on a placed structure edits a working copy; the next
  snapshot (or an explicit "save") writes a new version and points that instance at it. Other
  instances of the old version, in this world or another, don't change: copying a house to a
  friend's world copies a pointer, not the blocks.
- **Where structures come from**: a plot's contents become its structure automatically (the
  plot is the natural boundary, and it already has an owner and builders); a player can also
  save a selection as a structure, and place one from their inventory.
- **The world graph's buildings** (world-graph-and-art.md) are structures too: a node's recipe
  expands into a structure version, so generated and player-built buildings are one thing.
- **Rendering and collision** expand an instance into blocks once, when its zone loads; the
  server keeps the expanded blocks in the zone (so edits, reach and plots work as today) and
  the instance as the record of where they came from.
- **Limits**: a structure is at most 64 × 64 × 64 blocks to begin with; bigger builds are several
  structures. Untrusted structure blobs get the same caps as zones.
- Open: when two instances of one version are edited differently, they become two versions
  (copy on write); a "propagate this edit to every instance" action is a later feature.

### Entitlements

A structure has an **owner** and, for everyone else, **rights**: *use* (place instances of it),
*copy* (make a structure of their own from it) and *edit* (save new versions of it); each
includes the ones before it. The rules are `vox_core::entitle` (no database in them, so the
directory that keeps the records and the world server that enforces them agree), the records
and API are the directory's (`bins/vox-directory/src/structures.rs`).

- **The owner** has all three, and alone grants and revokes, opens the structure to everyone, and
  deletes it. An editor can't pass it on. A structure is shared with at most 256 accounts.
- **Open to everyone** (`public`): every signed-in account has use, or use and copy, as a floor
  (a named grant above it still counts). Never edit. Guests have no rights at all: a world with
  guests can't place anyone's structures.
- **Versions** are immutable and a structure's *head* is its newest. Edit saves a new version and
  moves the head; an instance placed from a version keeps it. Only editors may place or see an
  older version; everyone else places the head.
- **Copying** makes a new structure owned by the copier at the same head, with `derived_from`
  recording the source, so a copy's lineage survives (until the source is deleted, which
  frees it). The copy is theirs to change; the original is untouched.
- **No right, no existence**: someone with no right to a structure is told it doesn't exist
  (404), not that it's forbidden.
- **A world server asks before placing** (`GET /api/world/structures/{id}?account=&version=`,
  with the world secret): the answer is the version to place and the account's right, or 404.
  The world checks again on every placement, so revoking takes effect at once; instances already
  placed stay, as blocks are the world's.
- **Plots**: a plot's contents become a structure owned by the plot's owner. Its builders (the
  owner's group) get *edit* while they are builders: a plot's entitlements follow the plot's
  (to build, when plots become structures).
- Limits: 200 structures an account, 500 versions each.

## Store and snapshots

```
store/
  blobs/ab/cdef...          zone and region blobs, by hash (written once, never changed)
  snapshots/2026-11-26T...  manifests
  log/                      edits since the last snapshot (the existing edit log)
```

- A **manifest** (format 2) is a list of (region key, region hash), sorted, plus a root hashing
  those. A region blob lists its zones' hashes, so the tree is snapshot root, regions, zones,
  blocks. A 60 km world is about 8 × 8 regions, so a manifest is a few kilobytes; the region
  blobs hold the zone hashes (18 bytes a zone, so 10,000 edited zones is about 180 KB).
  Manifests from before regions (format 1, zone hashes listed directly) still load, restore and
  prune; the next snapshot after one writes region blobs.
- **Saving**: the world server keeps writing changed chunks and the edit log as now (SQLite stays
  as the working store). Every few minutes, and on shutdown, it writes the blobs of zones
  changed since the last snapshot, the regions they are in, and a new manifest.
- **Restoring** to any moment: the snapshot before it, then the edit log replayed up to it.
- **Copying a world** (a preview, an event, a branch): copy a manifest; blobs are shared.
- **Retention: a week.** Snapshots older than seven days are dropped (with Oxen, see below,
  they leave the restorable list); a blob no kept snapshot names is deleted. The edit log before
  the oldest kept snapshot is dropped, which ends today's unbounded `edits` table.
- **Owners restore their own worlds**: the directory lists a world's snapshots of the last
  seven days to its owner (and to people they allow), on the site and in the game's menu. A
  restore is a new snapshot whose contents are the chosen one's, so it can itself be undone; the
  world server reloads, players in it are moved to the spawn, and the restore is logged.
- **Sync**: a client's zone cache is the same blobs under the same hashes; the scaling design's
  `Have { zone, hash }` compares them.

## Oxen

[Oxen](https://github.com/Oxen-AI/Oxen) is a version control system for large data, written in
Rust: repositories, commits, branches and workspaces over a Merkle tree down to file chunks,
with a self-hostable server (`oxen-server`, Linux and macOS) whose version store can be local or
S3, an HTTP API, a CLI, and an embeddable library (`liboxen`). The repository is Apache-2.0
(the crate listings say "non-standard": check the LICENSE of the version used).

What fits:

- **Snapshots as commits.** One Oxen repository per world; zone blobs and structure blobs are
  files (`zones/12_-3.voxr`, `structures/a3/f9c1...voxs`); a snapshot is a commit. Unchanged
  files are deduplicated, as the manifests above would do by hand.
- **Restore** is checking out a commit's tree into the next commit; **copying a world** is a
  branch; **history and diffs** (which zones changed when) come for free.
- **Object storage underneath**: the S3-compatible store (Cloudflare R2 for vox.fail) keeps the
  bytes; Oxen keeps the history.

What doesn't:

- **Not the live wire.** Oxen moves whole commits (seconds); play moves single edits in tens of
  milliseconds. Edits, poses and zone streaming stay on the world server's WebSocket.
- **Not in the browser.** `liboxen` builds in RocksDB and DuckDB (C and C++) and has no
  WebAssembly target, so clients can't embed it; a browser could only use Oxen's HTTP API.
  Clients instead fetch zone, region and structure blobs **by hash** over plain HTTP (immutable, so a
  CDN caches them for ever), from object storage or the world server.
- **Retention**: Oxen keeps history; a week's retention means either squashing older commits into
  a fresh base commit (if Oxen supports rewriting history; to verify) or keeping them and only
  listing the last week as restorable (storage grows only with changed zones).

So the proposal: **the world server writes snapshots as Oxen commits** (embedding `liboxen`, or
talking to a self-hosted `oxen-server` over its HTTP API), Oxen's store sits on object storage,
and **clients read blobs by hash from object storage through the CDN**. A prototype should
measure a commit of a few hundred changed zones, a checkout, and the store's size after a week
of edits, before the world server depends on it. The store layer in `vox-format` stays behind a
trait (`Snapshots`: commit, list, restore, prune), with a plain object-storage implementation
(the manifests above) as the fallback.

### The prototype (2026-11-26, Oxen 0.68.2)

`tools/dev/oxen_probe.sh` (with the `oxen_probe` example in `vox-format`) builds a world of
tiled islands, makes rounds of building (a small house in each of a few zones per round), and
stores every round both ways: as a `DirStore` snapshot, and as the changed `.voxr` files
committed to an Oxen repository. The pushes went to a local `oxen-server`.

| | 16 zones, 20 rounds × 3 | 256 zones (4 × 4 km), 100 rounds × 10 |
| --- | --- | --- |
| plain store | 4.8 MB | 85.7 MB |
| Oxen store | 10.2 MB (4.6 MB of it a fixed cache) | 97.4 MB, and a 17.4 MB working tree beside it |
| Oxen commit | 22 ms | 29 ms |
| Oxen checkout, oldest or newest | 10 ms | 13 ms |
| push all to `oxen-server` / one round | 110 ms / 21 ms | |
| fresh clone (latest only, 1 MB) / pull one round | 66 ms / 26 ms | |

What it showed:

- **Speed is no problem.** Commits, checkouts, pushes and pulls are tens of milliseconds, far
  inside a five-minute snapshot interval.
- **No saving on bytes.** Zone blobs are already compressed and addressed by hash, so Oxen
  stores the same changed zones the manifests do, plus about 14% of its own (and the CLI
  needs a working tree: one more copy of the newest world).
- **Retention doesn't fit.** Oxen has no way to rewrite history (no squash, reset or rebase);
  `oxen prune` only removes what no commit refers to. Keeping a week means starting a new
  repository each week, or keeping everything and listing only the last week as restorable.
- **It is one more server.** The CLI or `oxen-server` runs beside each world (no wasm, and
  `liboxen` brings RocksDB and DuckDB into the world server's build).

**Decision (first, 2026-11-26): the plain store.** The numbers above favoured it.

**Decision (revised, 2026-11-26): Oxen is the snapshot store, squashed.** The owner's call: a
server's cost doesn't matter, and an Oxen history (diffs, branches to copy a world) is worth
having. What changed from the measurements:

- **Squashing** isn't an Oxen command, so `OxenStore::squash` rebuilds the repository: the
  newest commit before the cutoff (a week ago) becomes the base, the commits after it are
  replayed on top in a fresh repository, swapped in for the old one. It runs when there is a day
  of older history to drop, so about once a day, not every five minutes. What is lost is only
  history older than the week, which the snapshots no longer list anyway.
- **It sits behind the same `BlobStore`** as the plain folder: `OxenStore` keeps blobs and
  manifests as files (the same keys) in an Oxen working tree, and `commit` is called after a
  snapshot is written and after a pruning. So `Snapshots`, restore and `--list-snapshots` are
  unchanged; Oxen adds history. A folder of plain snapshots is taken in as the first commit,
  so switching is `--oxen` and nothing else.
- **The CLI, not the library** (the owner's choice): `vox-server` runs the `oxen` binary
  (`--oxen [path]` or `VOX_OXEN`), so RocksDB and DuckDB stay out of the build. The container
  image has it (version and checksum pinned in the Dockerfile; the release publishes none).
- The cost: the store holds each blob twice (working tree and Oxen's versions), about 14% more
  with Oxen's own data, as measured. Accepted.
- Without `--oxen` the plain folder is used as before: the default for local play.

## Compression in the browser

- The `zstd` crate wraps C, which is awkward for wasm. `ruzstd` is pure Rust, decodes zstd and is
  already in the lock file (Bevy brings it), so clients decompress with it natively and in the
  browser. The server compresses with `zstd` (native, faster, better levels).
- Clients that send blocks (simulation owners in the scaling design) can use `ruzstd`'s basic
  encoder, or send uncompressed: the server recompresses what it stores anyway.

## Levels as files

The hand-built levels become authored zones:

```
data/worlds/void/
  world.ron            settings: sky, time, weather, generator and its seed, palette version
  zones/0_0.ron        entities (portals, spawn, lamps), readable and reviewable in git
  zones/0_0.blocks     the zone's blocks (binary, written by the tools)
```

`tools/assets/build_void.py` and friends move to Rust (`vox-forge`, per the world-graph design)
and write these. The server seeds a new world from them as it seeds from `levels/*.json` today;
the JSON loader stays until every level is converted.

## Crate

```
crates/vox-format/
  src/
    lib.rs           versions, limits, errors
    zone.rs          the zone blob: write, read, hash
    region.rs        the region blob: a list of zone hashes
    blocks.rs        chunks in a zone (on vox-core's Chunk encoding)
    entities/        the world components, their stable names, migrations, the registry
    store.rs         content-addressed blobs, manifests, the hash tree, pruning
    level.rs         authored levels (world.ron, zones/*.ron and *.blocks)
  tests/             round trips, old versions loading, crafted blobs refused, ruzstd vs zstd
```

Depends on `vox-core`, `bevy_ecs`, `bevy_reflect`, `bevy_world_serialization`, `serde`, `ron`,
`postcard`, `ruzstd`, `blake3`, and `zstd` on native targets only.

## Phases

1. (built; Oxen the snapshot store when asked for) `vox-format` with zone and region blobs for blocks only, the `Snapshots` trait with the plain
   object-storage implementation (manifests), a week's retention; the world server writes
   snapshots alongside SQLite; restore from a snapshot plus the log. Round-trip and
   crafted-input tests. In parallel: an Oxen prototype behind the same trait (commit, checkout,
   store size), to choose between them.
2. Entities: the first components (portals, spawn, lights, plots) moved out of JSON settings and
   the `meta` table into zone entities, with stable names and a migration test.
3. Levels as files: the Void, the yard and the islands converted; the seeding path reads them.
4. Zone blobs on the wire, with the streaming messages from the scaling design.
5. Client zone cache (disk and browser storage) checked by hash.
6. Structures: plots' contents as structure versions, instances by reference, saving and placing
   from the inventory.
7. Owners' restore: the snapshot list and restore on the site and in the game.

## Open questions

- Oxen: can older commits be squashed for the week's retention, or is history kept and only the
  last week listed? What does a self-hosted `oxen-server` on R2 cost to run?
- Structures: how big before a build must be split; who may copy someone else's structure
  (ownership, credit, a marketplace later)?
- Restore: may an owner let others (their group) restore, and does a restore need a cooldown?
