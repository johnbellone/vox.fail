# Design: the world format

Status: **phase 1 built** · 2026-11-26 · decisions recorded (see *Decisions*). `crates/vox-format`
has region blobs (blocks), the blob store and snapshots with a week's retention; the world server
snapshots every five minutes and on closing, and restores (`--restore`, `--restore-at`). Not yet:
the Oxen prototype, and object storage beyond a local folder (`DirStore`).

Related: [world-scaling.md](world-scaling.md) (regions, streaming, region hashes, simulation
owners), [world-graph-and-art.md](world-graph-and-art.md) (world graphs and recipes, which
become entities here), [repo-structure.md](repo-structure.md) (where the crate goes).

## Summary

A world is stored and sent as **regions** (256 m × 256 m, 16 × 16 chunk columns). Each region
is one compressed blob in two parts:

- **Blocks**: the region's edited chunks in today's run-length encoding, compressed together
  with zstd. Dense data in a dense binary format.
- **Entities**: everything that isn't a block (portals, spawn points, lamps and props, plots,
  signs, world-graph nodes, later creatures and inventories), as a Bevy `DynamicWorld`. Written
  as **RON** for authoring and review; stored and sent as **postcard** from the same types.

Region blobs are **content-addressed**: stored once under their hash. A **snapshot** is a small
manifest mapping region keys to hashes, whose root is the region hash tree from the scaling
design. Unchanged regions are shared between snapshots, so a snapshot of a large world costs
only what changed; the existing edit log covers the time between snapshots.

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
| Blocks on the server | SQLite: one row per chunk (`chunks(x, y, z, version, data)`), data in run-length encoding (varint run length, varint block id) | Millions of rows; regions are the better unit |
| Edit log | SQLite `edits(seq, at, player, x, y, z, old, new)`, kept for ever | Needs compaction into snapshots |
| Levels | `data/levels/*.json`: boxes and single blocks by name, plus a `world` object (sky, time, portals) | Entities need a real format |
| Entities | Only portals and settings, as JSON inside the level and the server's `meta` table | Plots, props, graph nodes, inventories |
| On the wire | Every chunk on join, one message each | Regions, only what changed (scaling design) |

## Measured

The islands world (257 stored chunks, about 405,000 blocks):

| Encoding | Bytes |
| --- | --- |
| Plain 16-bit blocks | 16,842,752 |
| Run-length (today) | 211,314 (822 a chunk on average) |
| Run-length + zstd, each chunk alone | 79,500 |
| Run-length + zstd, each chunk with a dictionary trained on them | 70,091 (optimistic: trained on the same chunks) |
| Run-length + zstd, a region's chunks together | 62,831 |

So: keep the run-length encoding (it does the heavy lifting, 80× smaller), and compress a
region's chunks together (another 3.4×), which beats a dictionary without having to ship and
version one. A dictionary stays an option for sending single chunks (an edit's neighbourhood).

## The region blob

```
"VOXR"                      magic
u16    format version
i32 × 2 region key (x, z in regions)
u32    flags (bit 0: has entities)
u16    block palette version   (which data/blocks.json the ids refer to)
u32    blocks section length   (compressed)
[u8]   zstd frame: for each edited chunk, in (y, z, x) order:
           varint chunk index within the region (column x, z and height y)
           varint chunk version
           varint length, then the run-length bytes
u32    entities section length (compressed; 0 when none)
[u8]   zstd frame: postcard DynamicWorld (see Entities)
[u8;16] hash of everything above (BLAKE3, first 16 bytes)
```

- Chunks not in the blob are **as generated** (the scaling design's generator) or air.
- The hash is over the uncompressed sections, so recompressing at another level doesn't change
  identity; it is the region's address in the store and its leaf in the hash tree.
- Block ids are the world's palette order (as on the wire today); the header names the palette
  version so an old blob is remapped when the palette changes.
- Limits on reading untrusted blobs: at most 4,096 chunks (16 × 16 × 16 heights), each
  decompressed chunk checked by `Chunk::decode` (already bounds-checked), and the
  decompressed size capped before allocating.

## Entities

Bevy's scene format (`bevy_world_serialization::DynamicWorld` in 0.19, `.scn.ron`) serialises
entities and their reflected components, and maps entity references on load, so links between
entities (a portal and its key, a road between two houses) survive saving and loading. Bevy's
own tests round-trip a `DynamicWorld` through postcard, so the binary form needs no new code.

- **Component types live in `vox-format`**, not in `vox-world` (which renders): the server needs
  them to validate what it stores, without a renderer. `vox-format` depends on `bevy_ecs` and
  `bevy_reflect` only, both fine without rendering and on wasm.
- **First components**: `Portal { to, size, axis, arrive, yaw }`, `Spawn`, `Light { kind, power }`,
  `Prop { model, at, yaw }`, `Plot { area, owner, builders }`, `Sign { text }`, and the world graph's
  `Node { kind, detail, params, recipe, seed }` with `Link { to, rel }`.
- **Stable names**: RON names components by Rust type path (`vox_format::portal::Portal`), so
  moving a type would break saved worlds. Every world component gets a fixed type path
  (`#[reflect(type_path = "vox::Portal")]`) and a version; a load runs migrations from older
  versions, with tests that load files saved by each earlier version.
- **Untrusted input**: anything that came from a client is deserialised against a type registry
  holding only the world components (never the app's own), with a cap on entities per region and
  on string lengths, so a crafted blob can't create arbitrary components or exhaust memory.

## Structures

What a player builds is a **structure**: its blocks (and its own entities: lamps, signs, props
inside it) stored as a content-addressed blob of its own, the same format as a region's blocks
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
- **Rendering and collision** expand an instance into blocks once, when its region loads; the
  server keeps the expanded blocks in the region (so edits, reach and plots work as today) and
  the instance as the record of where they came from.
- **Limits**: a structure is at most 64 × 64 × 64 blocks to begin with; bigger builds are several
  structures. Untrusted structure blobs get the same caps as regions.
- Open: when two instances of one version are edited differently, they become two versions
  (copy on write); a "propagate this edit to every instance" action is a later feature.

## Store and snapshots

```
store/
  blobs/ab/cdef...          region blobs, by hash (written once, never changed)
  snapshots/2026-11-26T...  manifests
  log/                      edits since the last snapshot (the existing edit log)
```

- A **manifest** is a list of (region key, hash), sorted, plus the root of the region hash tree
  (a quadtree over region keys, as in the scaling design). A 60 km world with 10,000 edited
  regions is about 300 KB of manifest.
- **Saving**: the world server keeps writing changed chunks and the edit log as now (SQLite stays
  as the working store). Every few minutes, and on shutdown, it writes the blobs of regions
  changed since the last snapshot and a new manifest.
- **Restoring** to any moment: the snapshot before it, then the edit log replayed up to it.
- **Copying a world** (a preview, an event, a branch): copy a manifest; blobs are shared.
- **Retention: a week.** Snapshots older than seven days are dropped (with Oxen, see below,
  they leave the restorable list); a blob no kept snapshot names is deleted. The edit log before
  the oldest kept snapshot is dropped, which ends today's unbounded `edits` table.
- **Owners restore their own worlds**: the directory lists a world's snapshots of the last
  seven days to its owner (and to people they allow), on the site and in the game's menu. A
  restore is a new snapshot whose contents are the chosen one's, so it can itself be undone; the
  world server reloads, players in it are moved to the spawn, and the restore is logged.
- **Sync**: a client's region cache is the same blobs under the same hashes; the scaling design's
  `Have { region, hash }` compares them.

## Oxen

[Oxen](https://github.com/Oxen-AI/Oxen) is a version control system for large data, written in
Rust: repositories, commits, branches and workspaces over a Merkle tree down to file chunks,
with a self-hostable server (`oxen-server`, Linux and macOS) whose version store can be local or
S3, an HTTP API, a CLI, and an embeddable library (`liboxen`). The repository is Apache-2.0
(the crate listings say "non-standard": check the LICENSE of the version used).

What fits:

- **Snapshots as commits.** One Oxen repository per world; region blobs and structure blobs are
  files (`regions/12_-3.voxr`, `structures/a3/f9c1...voxs`); a snapshot is a commit. Unchanged
  files are deduplicated, as the manifests above would do by hand.
- **Restore** is checking out a commit's tree into the next commit; **copying a world** is a
  branch; **history and diffs** (which regions changed when) come for free.
- **Object storage underneath**: the S3-compatible store (Cloudflare R2 for vox.fail) keeps the
  bytes; Oxen keeps the history.

What doesn't:

- **Not the live wire.** Oxen moves whole commits (seconds); play moves single edits in tens of
  milliseconds. Edits, poses and region streaming stay on the world server's WebSocket.
- **Not in the browser.** `liboxen` builds in RocksDB and DuckDB (C and C++) and has no
  WebAssembly target, so clients can't embed it; a browser could only use Oxen's HTTP API.
  Clients instead fetch region and structure blobs **by hash** over plain HTTP (immutable, so a
  CDN caches them for ever), from object storage or the world server.
- **Retention**: Oxen keeps history; a week's retention means either squashing older commits into
  a fresh base commit (if Oxen supports rewriting history; to verify) or keeping them and only
  listing the last week as restorable (storage grows only with changed regions).

So the proposal: **the world server writes snapshots as Oxen commits** (embedding `liboxen`, or
talking to a self-hosted `oxen-server` over its HTTP API), Oxen's store sits on object storage,
and **clients read blobs by hash from object storage through the CDN**. A prototype should
measure a commit of a few hundred changed regions, a checkout, and the store's size after a week
of edits, before the world server depends on it. The store layer in `vox-format` stays behind a
trait (`Snapshots`: commit, list, restore, prune), with a plain object-storage implementation
(the manifests above) as the fallback.

## Compression in the browser

- The `zstd` crate wraps C, which is awkward for wasm. `ruzstd` is pure Rust, decodes zstd and is
  already in the lock file (Bevy brings it), so clients decompress with it natively and in the
  browser. The server compresses with `zstd` (native, faster, better levels).
- Clients that send blocks (simulation owners in the scaling design) can use `ruzstd`'s basic
  encoder, or send uncompressed: the server recompresses what it stores anyway.

## Levels as files

The hand-built levels become authored regions:

```
data/worlds/void/
  world.ron            settings: sky, time, weather, generator and its seed, palette version
  regions/0_0.ron      entities (portals, spawn, lamps), readable and reviewable in git
  regions/0_0.blocks   the region's blocks (binary, written by the tools)
```

`tools/assets/build_void.py` and friends move to Rust (`vox-forge`, per the world-graph design)
and write these. The server seeds a new world from them as it seeds from `levels/*.json` today;
the JSON loader stays until every level is converted.

## Crate

```
crates/vox-format/
  src/
    lib.rs           versions, limits, errors
    region.rs        the region blob: write, read, hash
    blocks.rs        chunks in a region (on vox-core's Chunk encoding)
    entities/        the world components, their stable names, migrations, the registry
    store.rs         content-addressed blobs, manifests, the hash tree, pruning
    level.rs         authored levels (world.ron, regions/*.ron and *.blocks)
  tests/             round trips, old versions loading, crafted blobs refused, ruzstd vs zstd
```

Depends on `vox-core`, `bevy_ecs`, `bevy_reflect`, `bevy_world_serialization`, `serde`, `ron`,
`postcard`, `ruzstd`, `blake3`, and `zstd` on native targets only.

## Phases

1. (built, but for Oxen) `vox-format` with region blobs for blocks only, the `Snapshots` trait with the plain
   object-storage implementation (manifests), a week's retention; the world server writes
   snapshots alongside SQLite; restore from a snapshot plus the log. Round-trip and
   crafted-input tests. In parallel: an Oxen prototype behind the same trait (commit, checkout,
   store size), to choose between them.
2. Entities: the first components (portals, spawn, lights, plots) moved out of JSON settings and
   the `meta` table into region entities, with stable names and a migration test.
3. Levels as files: the Void, the yard and the islands converted; the seeding path reads them.
4. Region blobs on the wire, with the streaming messages from the scaling design.
5. Client region cache (disk and browser storage) checked by hash.
6. Structures: plots' contents as structure versions, instances by reference, saving and placing
   from the inventory.
7. Owners' restore: the snapshot list and restore on the site and in the game.

## Open questions

- Oxen: can older commits be squashed for the week's retention, or is history kept and only the
  last week listed? What does a self-hosted `oxen-server` on R2 cost to run?
- Structures: how big before a build must be split; who may copy someone else's structure
  (ownership, credit, a marketplace later)?
- Restore: may an owner let others (their group) restore, and does a restore need a cooldown?
