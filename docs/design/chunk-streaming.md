# Chunks, streaming and draw distance at quarter-metre blocks (proposed)

Status: proposed, 2026-11-27. Owner's request: more efficient chunking and streaming, a shorter
draw distance, and a comparison with Minecraft and other large voxel games.

This builds on [block-scale.md](block-scale.md), which moved every world to 0.25 m blocks,
and makes parts of [world-scaling.md](world-scaling.md) concrete: its *Streaming by area of
interest*, *Memory and meshes* and *Far terrain* sections. Where the two differ, this document
is the newer plan for those parts. Zones, regions, generated terrain and simulation owners stay
as world-scaling.md has them.

## Summary

- **Store chunks with a palette.** Most of our chunks hold two to four kinds of block. Storing
  each chunk as a short list of the block types it uses, plus a few bits per block, cuts block
  memory about nine times on the server and in the browser. The rest of the code doesn't
  change: `Chunk::get` and `Chunk::set` behave as before.
- **Never expand a level into a map of every block.** Loading a level today builds a hash map of
  all its blocks first. At the new size, seeding the sea needs 1.7 GB, and a portal echo builds
  the same map in the browser. Fix this before the quarter-metre branch ships.
- **Stream by view radius.** The client asks for the chunks within a radius it chooses. The
  server sends the nearest first, within a byte budget, and sends edits only to the players who
  can see them. A join then costs what is near you, not the whole world.
- **Full detail to 64 m in the browser and 128 m on the desktop.** Past that, coarser copies of
  the same chunks (0.5 m, 1 m and 2 m blocks) carry the view to the fog: 450 m in the Void.
  They use the same chunk format and the same mesher.
- **Then make meshing faster and drawing smarter**, guided by measurements: the binary
  greedy mesher, meshing off the main thread on the desktop, and culling chunks you can't see.

## Where we are (measured 2026-11-27)

Release builds of `vox-server`; a fresh world made from each level; numbers from
`/proc/<pid>/status` and the stored chunks.

| | Half-metre blocks | Quarter-metre blocks |
| --- | --- | --- |
| Sea: blocks, chunks | 4.3 M, 309 | 34.0 M, 2,179 |
| Sea: server memory, idle | 34 MB | 138 MB |
| Sea: server peak while seeding | 222 MB | **1,687 MB** |
| Sea: sent on join (encoded) | 0.20 MB | 0.67 MB |
| Islands: blocks, chunks | 0.4 M, 257 | 3.2 M, 716 |
| Islands: server memory, idle | 68 MB | 96 MB |

- **Chunks are 32 × 32 × 32 blocks, stored dense**: two bytes a block, 64 KB a chunk
  (`crates/vox-core/src/world.rs`). Empty chunks aren't stored. The server holds every chunk of
  its world, and so does each client: `predict::View` keeps one copy and a list of your
  pending edits.
- **A join sends every chunk** (`send_world` in `bins/vox-server/src/world.rs`, called from
  `join`). Watchers get the same. On the wire each chunk is run-length encoded, which is already small: the sea's
  2,179 chunks are 0.67 MB.
- **Levels expand into a hash map of every block** (`vox_core::level::load`) before they go
  into a world. That is where the 1.7 GB peak comes from. The client does the same for
  **portal echoes** (`crates/vox-world/src/portals.rs`, `crate::blocks::load_level`), and
  meshes the whole level. Standing in the Void, the Tide Door's echo builds the entire sea in
  the browser. The browser figure is not measured: the built-in browser pane has no WebGPU,
  and Chrome wasn't connected. It is very likely to be over a gigabyte.
- **Meshing** already merges neighbouring faces with the same tile and light
  (`mesh_cells(.., merge: true)`) and uses a packed 20-byte vertex. It runs on the main thread
  within 8 ms a frame on the desktop and 5 ms in the browser, nearest chunks first. Each chunk
  is up to four entities (opaque, glass, water, flame).
- **Draw distance**: none, beyond the fog. Every loaded chunk is meshed and drawn.

## How others do it

A survey of published descriptions. Claims marked *(unverified)* come from secondary sources:
hosting guides, generated wikis or blog analyses rather than the developers.

### Minecraft (Java Edition)

- **Blocks and sections.** Blocks are 1 m. A chunk is a 16 × 16 column split into
  **sections** of 16 × 16 × 16 blocks (4,096 blocks; 16 m on a side). One of our blocks is a
  sixty-fourth of theirs, so a cubic metre holds 64 times the blocks.
- **Palettes.** Each section stores its blocks in a *paletted container*: a byte gives the
  bits per entry, and 0 bits means a single-value palette with no data at all. Otherwise
  indices into a small local palette are packed into 64-bit words. Older protocol pages give
  4 to 8 bits for a local palette, with a fall-back to global ids above 8 bits; the details
  change between versions ([chunk format, wiki.vg](https://wiki.vg/Chunk_Format);
  [Anvil 1.18+ format](https://team-lodestone.github.io/Documentation/Java/Anvil_Docs/1.18+/Chunk_Format.html);
  [`PalettedContainer` (Yarn)](https://maven.fabricmc.net/docs/yarn-1.21.7+build.1/net/minecraft/world/chunk/PalettedContainer.html)).
- **View and simulation distance.** The server sends the chunks within `view-distance` of each
  player: 10 by default, 3 to 32 allowed, about 160 m at the default
  ([PaperMC server.properties](https://docs.papermc.io/paper/reference/server-properties)).
  A separate `simulation-distance` decides which chunks keep running; one hosting guide gives
  its default as 10 *(unverified)*
  ([Shockbyte](https://shockbyte.com/billing/knowledgebase/134/Adjusting-View-Distance-and-Simulation-Distance-on-Your-Minecraft-Server.html)).
- **Cave culling.** Each section keeps 15 bits: whether each pair of its six faces is connected
  through non-opaque blocks. A breadth-first walk from the camera through those connections
  skips sections that can't be seen, such as most of the underground
  ([Tommo's visibility posts](https://tomcc.github.io/2014/08/31/visibility-1.html),
  [part 2](https://tomcc.github.io/2014/08/31/visibility-2.html)). It has had long-running
  false-culling bugs ([MC-70850](https://bugs.mojang.com/browse/MC-70850)).
- **What mods add.** Sodium rewrites the chunk renderer and its culling. Its changelogs report
  10–15% fewer sections drawn at long distances from better occlusion culling
  ([Sodium changelog](https://modrinth.com/mod/sodium/changelog)). Nvidium draws with
  bindless multi-draw indirect where the driver offers it
  ([Nvidium](https://modrinth.com/mod/nvidium/changelog)).
- **Far terrain** is not in the game. Distant Horizons draws simplified, low-resolution models
  of far chunks, built on background threads, out to thousands of blocks
  ([Distant Horizons FAQ](https://blog.curseforge.com/distant-horizons-frequently-asked-questions/)).
  Voxy keeps a voxel hierarchy with five levels of detail, culls and draws it on the GPU with
  compute shaders and multi-draw indirect, and needs OpenGL 4.6 *(unverified: from a generated
  wiki)* ([Voxy](https://modrinth.com/mod/voxy), [DeepWiki](https://deepwiki.com/MCRcortex/voxy)).

### Other techniques

- **Greedy meshing** merges neighbouring faces into bigger quads; ambient occlusion survives if
  only faces with the same corner shading are merged
  ([0fps: meshing](https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/),
  [0fps: ambient occlusion](https://0fps.net/2013/07/03/ambient-occlusion-for-minecraft-like-worlds/)).
  We do this already.
- **Binary greedy meshing** keeps a 64-bit mask per row, so one bitwise operation tests 64
  faces. Chunks are 62³ plus a one-block border, so each padded row fits a `u64`. Reported
  times are 50 to 200 µs a chunk, and the Rust port reports 65 µs for its fast path on a
  Ryzen 5 5500 ([cgerikj/binary-greedy-meshing](https://github.com/cgerikj/binary-greedy-meshing),
  [Rust crate](https://docs.rs/crate/binary-greedy-meshing/0.5.2/source/README.md)). Our
  32-block chunks plus a border fit the same scheme.
- **Packed vertices and vertex pulling**: one small record per face or vertex in a storage
  buffer, expanded by the vertex shader. Write-ups report 4 bytes a vertex where they had 16 or
  28 ([bevy-vertex-pulling](https://github.com/superdump/bevy-vertex-pulling),
  [stb_voxel_render](https://flutter.googlesource.com/third_party/stb/+/refs/heads/dev/stb_voxel_render.h):
  20 bytes a quad). Ours is 20 bytes a vertex.
- **LOD seams.** Where coarse chunks meet fine ones, the usual fixes are skirts, transition
  seams, or drawing the finer chunk over the coarser one
  ([0fps: LOD for blocky voxels](https://0fps.net/2018/03),
  [clipmap engine notes](https://ngildea.blogspot.com/2015/03/engine-overview.html)).
  *Clipmaps* are nested rings round the viewer, each twice as coarse as the last, updated as the
  viewer moves ([Clipmap](https://en.wikipedia.org/wiki/Clipmap)).
- **GPU-driven voxel rendering**, such as Aokana, streams voxel data so the whole scene is never
  in GPU memory, and combines LOD with streaming
  ([Aokana](https://arxiv.org/html/2505.02017v1)).
- **Ray-marched volumes.** Teardown draws each object as its bounding box and ray-marches a 3D
  texture holding one byte per voxel (a palette index), so its cost follows screen pixels, not
  the voxel count *(unverified: from analyses, not the developer)*
  ([frame teardown](https://acko.net/blog/teardown-frame-teardown/)).
- **Sparse voxel DAGs** merge identical subtrees of an octree, often by orders of magnitude, and
  can be traced without decompressing; editing them is harder
  ([Kämpe et al.](https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf),
  [symmetry-aware DAGs](https://www.crs4.it/vic/data/papers/i3d2016-symmetry-dags.pdf)).
- **The browser.** WebGPU's multi-draw indirect is still experimental in Chrome, behind the
  "Unsafe WebGPU Support" flag ([Chrome: new in WebGPU 131](https://developer.chrome.com/blog/new-in-webgpu-131)).
  We can't count on it there.

### What fits Vox

| | Minecraft | Vox today | Vox proposed |
| --- | --- | --- | --- |
| Block | 1 m | 0.25 m | 0.25 m |
| Storage unit | 16³ section (16 m) | 32³ chunk (8 m) | 32³ chunk, 8 m to 64 m with LOD |
| Block storage | palette, 0 to ~8 bits | dense, 16 bits | palette, 0 to 8 bits (16 rare) |
| Sent on join | within view distance | the whole world | within your radius, nearest first |
| Full detail | 10 chunks: ~160 m | everything loaded | 64 m browser, 128 m desktop |
| Far terrain | none (mods add it) | none | coarser chunks to the fog |
| Culling | frustum, cave culling | frustum | frustum, then cave culling |

Ray marching and voxel DAGs suit static or GPU-resident scenes. They are a big step away from
our mesh renderer, Bevy's materials and the browser's limits, so they are left for later. A
palette, streaming and mip-mapped chunks give most of the gain at a fraction of the work, and
keep one chunk format everywhere.

## Design

### 1. Levels go straight into chunks (fix first)

- `vox_core::level` applies boxes to a `World` (or any `set(cell, id)` target) in file order,
  instead of building `HashMap<Cell, u16>`. Boxes are applied in order, so "later boxes
  overwrite" and `"air"` clear as before. `load` stays for tests and small levels, built on
  the same code.
- `vox-world`'s `load_level` and `mesh_level` take a `World`, so an echo is held as chunks.
- **An echo shows only what's near its arrival point**: the chunks within 48 m of `arrive`,
  plus whatever the echo camera looks at inside that. It is meshed at full detail near the
  doorway and from LOD chunks beyond (§5) once those exist. The far side is a picture through a
  doorway, not a second world.
- Acceptance: seeding the sea peaks under 400 MB on the server, and the Void with the Tide Door
  in view stays under 300 MB of wasm memory in Chrome.

### 2. Paletted chunks

`Chunk` keeps its API (`get`, `set`, `encode`, `decode`, `version`) and changes inside:

```text
enum Blocks {
    Uniform(u16),                                     // one block everywhere: no array
    Packed { palette: Vec<u16>, bits: u8, words: Vec<u64> },  // bits = 1, 2, 4, 8 or 16
}
```

- Indices are 1, 2, 4, 8 or 16 bits, so an index never straddles a 64-bit word. 16 bits
  holds any block, so there's no global-id fall-back.
- `set` adds a new block to the palette and repacks at the next width when it fills: rare,
  and amortised. A palette is never shrunk by `set`. `encode` drops unused entries, so a
  chunk compacts when it is saved or sent.
- Measured on the stored chunks: **sea 136 MB → 14.5 MB; islands 45 MB → 5.4 MB**. Ids per
  chunk in the sea: 757 chunks with 2, 1,001 with 3, 370 with 4, 51 with 5 or more.
- `get` is a shift and a mask. The mesher reads chunks through a copied 34³ grid
  (`grid` in `client/chunks.rs`) and isn't on this path. Benchmark `get`/`set` against today
  (`cargo bench` or a `#[test]` timing with a generous bound), then decide.
- The wire format (run-length encoded) and the stored formats (SQLite rows, `vox-format`
  zones) don't change: they already encode runs, not the in-memory array. Nothing needs a
  version bump.

### 3. Streaming by view radius

The protocol of world-scaling.md, *Streaming by area of interest*, simplified for one world
server and chunk keys:

- `ToServer::View { centre: ChunkKey, radius: u16, rings: u8, budget_kbps: u32 }`: radius in
  chunks of full detail, and how many coarser rings beyond (§5). Sent on join and when you
  cross a chunk border. The server clamps it to the world's limits (a setting, defaulting to
  128 m and three rings).
- The server keeps each player's **interest set**, plus a reverse index from chunk to
  players. `ToClient::Edits` and chunk updates go only to players interested in that chunk.
  Poses already go to everyone; they can follow later.
- **Sending**: chunks not yet sent are queued nearest first (distance from your chunk, ahead
  of where you're going first), paced to the budget. Poses and edits are never queued behind
  chunks. A chunk's encoded bytes are cached until it changes, so many players joining costs
  copies, not encoding.
- **Client**: a chunk is loaded inside the radius and dropped past radius + 2 chunks (16 m),
  so walking along the edge doesn't thrash. Dropping a chunk frees its blocks and meshes.
  Prediction (`vox-core::predict`) only touches cells within reach, which is always inside the
  radius.
- **Watchers** (the front page) ask for a small radius round the spawn.
- Protocol 11. Until a client sends `View`, a world sends everything, as today. That helps
  tests and the avatar app, which shares `vox-world`.

### 4. Draw distance

- **Full detail**: 64 m in the browser and 128 m on the desktop, as a client setting clamped
  by the world. At 0.25 m, a block 128 m away covers under 3 pixels at 1080p with Bevy's
  default 45° vertical view (about 2.7), and about 1.4 at 256 m. Past that radius, one block's
  detail is a few pixels at most, which is what the coarser rings are for.
- **Fog is matched to it**: fog ends where the last LOD ring ends, never short of what's
  loaded and never far past it. The Void keeps its 450 m haze, reached with LOD.
- Estimates with paletted chunks (about 6.7 KB a chunk, the sea's average) and about three
  non-empty chunks a column on built-up ground:

  | Full detail | Columns | Chunks | Block memory |
  | --- | --- | --- | --- |
  | 64 m | ~200 | ~600 | ~4 MB |
  | 96 m | ~450 | ~1,350 | ~9 MB |
  | 128 m | ~800 | ~2,400 | ~16 MB |

  Mesh memory is the bigger unknown and is measured in phase 0 (see *Plan*).

### 5. Far terrain: mip-mapped chunks

The clipmap idea, done with our own chunks:

- **LOD level L** has blocks 2^L times larger: 0.5 m, 1 m, 2 m for L = 1 to 3. A level-L chunk
  is still 32³ cells, so it covers 2^L × 8 m on a side. The format, the palette, the wire
  message (with `lod` added to the key) and the mesher are all the same.
- **Downsampling** is done on the server and cached. A coarse cell is solid when at least
  four of its eight finer cells are solid, and takes the commonest solid block among them.
  Ties go to the lowest id, so every machine gets the same result. Water and glass are counted
  as non-solid unless all eight are. A level-L chunk is rebuilt from its eight level-(L-1)
  chunks when one of them changes, at most once every few seconds.
- **Rings**: level L is drawn from R·2^(L-1) to R·2^L, where R is the full-detail radius. Each
  ring holds about three quarters as many chunks as the full-detail disc. Three rings from
  64 m reach 512 m, which covers the Void's 450 m haze, for about 3.25 times the chunks of the
  full-detail disc: some 2,000 chunks and 13 MB in the browser.
- **Seams**: a coarse chunk is not drawn where the finer ring covers it. When meshing a coarse
  chunk, cells outside it count as air, so its sides are closed. Those side faces act as
  skirts, and no cracks show where it meets a finer chunk. Finer chunks load before coarser
  ones unload.
- Generated ground (atlas tiles) could downsample on the client without the network. That
  waits for zones and the generator (world-scaling.md).

### 6. Meshing and drawing (after measuring)

- **Benchmark our mesher** against binary greedy meshing on real chunks (sea, islands) and
  adopt it if it is several times faster. Our 32-block chunks plus a border fit 64-bit masks.
  Keep merging only faces with the same tile and light, as now.
- **Off the main thread on the desktop** with Bevy's `AsyncComputeTaskPool` (the echo loader
  already does this). The browser build has no threads: it keeps the per-frame budget, nearest
  first. Workers would need wasm threads and cross-origin isolation headers; revisit only if
  measurements show the budget can't keep up.
- **Cave culling** (15 bits a chunk) for the islands, the sea's depths and the dungeons, after
  frustum culling. It matters little in the open Void, so it comes last.
- **Fewer draw calls**: count them first. If they bound frame time, merge each 2 × 2 × 2 group
  of coarse chunks into one mesh per material. Multi-draw indirect can't be relied on in
  browsers yet.

### 7. Server memory

The server gains the same palette (it uses `vox_core::World`), and loading levels straight into
chunks fixes the seeding peak. Loading zones on demand and evicting idle ones stays as
world-scaling.md plans: worlds today are small enough to keep in memory once palettes are in.

## Plan

The beads epic `vox-cti` carries these steps (`bd list --parent vox-cti`); vox-cti.2 and
vox-cti.3 block shipping the quarter-metre branch.

0. **Measure** (from vox-2go.6): commit the browser measurement page, and record wasm memory,
   frame time and time to first view in Chrome for the sea, the islands and the Void with each
   echo, before and after. Add the mesh memory: vertices and indices per world.
1. **Levels straight into chunks; echoes near their arrival point.** This blocks shipping the
   quarter-metre branch.
2. **Paletted `Chunk`** in `vox-core`, with benchmarks and server memory before and after.
3. **Streaming by view radius**: `View`, interest sets, nearest-first sending, hysteresis,
   watchers. Protocol 11.
4. **Draw distance setting and matched fog.**
5. **Mip-mapped chunks**: server downsampling, LOD keys on the wire, client rings and seams.
6. **Meshing**: benchmark binary greedy, adopt if faster; desktop meshing off the main thread.
7. **Cave culling**, then draw-call batching if measurements call for it.

## Budgets (first targets)

| | Browser | Desktop |
| --- | --- | --- |
| Full detail | 64 m | 128 m |
| LOD rings | 3 (to 512 m) | 3 (to 1,024 m) |
| Block memory | under 20 MB | under 60 MB |
| wasm memory in the Void, sea echo in view | under 300 MB | n/a |
| Meshing per frame | 5 ms | 8 ms, or off-thread |
| Join to first view (sea) | under 3 s on a home connection | under 2 s |

## Open questions

- Does Bevy 0.20 batch our chunk meshes in the browser, or is each one its own draw call?
  This needs the frame capture from phase 0.
- Water and glass at coarse levels: drop them, or keep a thin surface? Decide from screenshots.
- Should the world's limit on view radius depend on how many players are in it?

## Sources

- [Chunk format (wiki.vg)](https://wiki.vg/Chunk_Format)
- [Anvil chunk format 1.18+](https://team-lodestone.github.io/Documentation/Java/Anvil_Docs/1.18+/Chunk_Format.html)
- [`PalettedContainer` (Yarn mappings)](https://maven.fabricmc.net/docs/yarn-1.21.7+build.1/net/minecraft/world/chunk/PalettedContainer.html)
- [PaperMC: server.properties](https://docs.papermc.io/paper/reference/server-properties)
- [Shockbyte: view and simulation distance](https://shockbyte.com/billing/knowledgebase/134/Adjusting-View-Distance-and-Simulation-Distance-on-Your-Minecraft-Server.html)
- [Tommo: The Advanced Cave Culling Algorithm](https://tomcc.github.io/2014/08/31/visibility-1.html) and [part 2](https://tomcc.github.io/2014/08/31/visibility-2.html)
- [MC-70850](https://bugs.mojang.com/browse/MC-70850)
- [Sodium changelog](https://modrinth.com/mod/sodium/changelog), [Nvidium changelog](https://modrinth.com/mod/nvidium/changelog)
- [Distant Horizons FAQ](https://blog.curseforge.com/distant-horizons-frequently-asked-questions/)
- [Voxy](https://modrinth.com/mod/voxy), [DeepWiki on Voxy](https://deepwiki.com/MCRcortex/voxy)
- [0fps: Meshing in a Minecraft game](https://0fps.net/2012/06/30/meshing-in-a-minecraft-game/), [ambient occlusion](https://0fps.net/2013/07/03/ambient-occlusion-for-minecraft-like-worlds/), [LOD for blocky voxels](https://0fps.net/2018/03)
- [Binary greedy meshing](https://github.com/cgerikj/binary-greedy-meshing), [Rust crate](https://docs.rs/crate/binary-greedy-meshing/0.5.2/source/README.md)
- [bevy-vertex-pulling](https://github.com/superdump/bevy-vertex-pulling), [stb_voxel_render](https://flutter.googlesource.com/third_party/stb/+/refs/heads/dev/stb_voxel_render.h)
- [Clipmap](https://en.wikipedia.org/wiki/Clipmap), [engine overview with clipmap-style octree LOD](https://ngildea.blogspot.com/2015/03/engine-overview.html)
- [Aokana: GPU-driven voxel rendering](https://arxiv.org/html/2505.02017v1)
- [Teardown frame teardown](https://acko.net/blog/teardown-frame-teardown/)
- [High resolution sparse voxel DAGs](https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf), [symmetry-aware sparse voxel DAGs](https://www.crs4.it/vic/data/papers/i3d2016-symmetry-dags.pdf)
- [Chrome: new in WebGPU 131 (multi-draw indirect)](https://developer.chrome.com/blog/new-in-webgpu-131)
