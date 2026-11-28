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
- **Full detail to 64 m in the browser and 128 m on the desktop.** Past that, a **sparse voxel
  octree** holds the world at coarser and coarser sizes (0.5 m, 1 m, 2 m and up). Rings of it
  are meshed with the same mesher out to the fog: 450 m in the Void.
- **Then try ray-marching that octree instead of meshing it**, first on the desktop: the GPU
  traces each far pixel through the tree. That costs per pixel rather than per block, which is
  what a 25 km Void full of builds needs. A prototype measured against the mesh rings decides
  it for each device profile.
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
- **Sparse voxel octrees.** Laine and Karras's *Efficient Sparse Voxel Octrees* gives a compact
  layout and a GPU ray cast through it, and reports ray casting competitive with triangles
  ([paper](https://research.nvidia.com/publication/efficient-sparse-voxel-octrees),
  [technical report](https://research.nvidia.com/sites/default/files/publications/laine2010tr1_paper.pdf)).
- **Wider trees trace faster.** A tree with 4 × 4 × 4 children per node keeps each node's
  occupancy in one 64-bit mask. One write-up reports hierarchical grids like this beating a
  state-of-the-art octree traversal by as much as 60%
  ([sparse 64-trees](https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing)). A Rust
  crate splits the mask into two 32-bit halves for the GPU
  ([Sparse-Voxel-64Tree](https://docs.rs/crate/Sparse-Voxel-64Tree/latest/source/src/lib.rs)),
  which WGSL needs, having no 64-bit integers.
- **Sparse voxel DAGs** merge identical subtrees of an octree, often by orders of magnitude, and
  can be traced without decompressing; editing them is harder
  ([Kämpe et al.](https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf),
  [symmetry-aware DAGs](https://www.crs4.it/vic/data/papers/i3d2016-symmetry-dags.pdf)).
- **The browser.** WebGPU's multi-draw indirect is still experimental in Chrome, behind the
  "Unsafe WebGPU Support" flag ([Chrome: new in WebGPU 131](https://developer.chrome.com/blog/new-in-webgpu-131)).
  We can't count on it there. By default, one storage buffer binding is limited to 128 MiB
  and a buffer to 256 MiB; a page must ask for more, and the adapter may not have it
  ([MDN: GPUSupportedLimits](https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits)).

### What fits Vox

| | Minecraft | Vox today | Vox proposed |
| --- | --- | --- | --- |
| Block | 1 m | 0.25 m | 0.25 m |
| Storage unit | 16³ section (16 m) | 32³ chunk (8 m) | 32³ chunk, 8 m to 64 m with LOD |
| Block storage | palette, 0 to ~8 bits | dense, 16 bits | palette, 0 to 8 bits (16 rare) |
| Sent on join | within view distance | the whole world | within your radius, nearest first |
| Full detail | 10 chunks: ~160 m | everything loaded | 64 m browser, 128 m desktop |
| Far terrain | none (mods add it) | none | sparse octree: meshed rings, then ray-marched |
| Culling | frustum, cave culling | frustum | frustum, then cave culling |

Near the player, a palette and meshes are right. Edits, the move check and the mesher read
single blocks all the time, and a tree would make every read a walk down it. Far away the
other way round is true. Nobody edits there, a coarse copy is all that shows, and an octree's
levels *are* the coarse copies, with empty space left out for free. So the far field is a
sparse octree (§5). It is first drawn by meshing rings of it, and then, if a prototype earns
it, by ray-marching it (§6). Voxel DAGs, which share identical subtrees, compress more, but are
hard to edit; they wait until parts of a world stop changing.

## Design

### 1. Levels go straight into chunks (fix first)

- `vox_core::level` applies boxes to a `World` (or any `set(cell, id)` target) in file order,
  instead of building `HashMap<Cell, u16>`. Boxes are applied in order, so "later boxes
  overwrite" and `"air"` clear as before. `load` stays for tests and small levels, built on
  the same code.
- `vox-world`'s `load_level` and `mesh_level` take a `World`, so an echo is held as chunks.
- **An echo shows only what's near its arrival point**: the chunks within 48 m of `arrive`,
  plus whatever the echo camera looks at inside that. It is meshed at full detail near the
  doorway and from the far-field octree beyond (§5) once that exists. The far side is a picture through a
  doorway, not a second world.
- Acceptance: seeding the sea peaks under 400 MB on the server, and the Void with the Tide Door
  in view stays under 300 MB of wasm memory in Chrome.
- **Done (vox-cti.2, vox-cti.3).** Seeding the sea now peaks at 142 MB, which is also its
  memory at rest (it was 1,687 MB), and the islands at 97 MB. An echo holds the chunks within
  the device's echo radius of its arrival point: from the Void, the sea is 659 chunks on the
  desktop (64 m), 390 in a browser (48 m) and 123 on a phone (24 m), not its 2,179. The avatar
  app's level sets load the same way. Browser memory is still to be measured (vox-cti.1).
  An echo no doorway shows any more is dropped with what it spawned, and arrival points from
  the network are made finite and brought within 2^24 blocks first, so echoes can't pile up.
  An echo ends at the edge of its region: check in vox-cti.1 that haze hides that edge at the
  phone's 24 m.
- **Device profiles** (`crates/vox-world/src/detail.rs`): desktop, browser and mobile, each
  with a full-detail distance, LOD rings, an echo radius and a meshing budget. The page picks
  `mobile` for a touch screen or a browser that says it's mobile (`deviceDetail` in
  `apps/site/src/game/wasm.ts`); `vox-client --detail mobile` tries it on the desktop.

  | | Desktop | Browser | Mobile |
  | --- | --- | --- | --- |
  | Full detail | 128 m | 64 m | 32 m |
  | LOD rings | 3 | 3 | 2 |
  | Echo radius | 64 m | 48 m | 24 m |
  | Meshing per frame | 8 ms | 5 ms | 3 ms |

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

### 5. Far terrain: a sparse voxel octree

Beyond full detail, the world is held as a sparse voxel octree. Each level halves the detail of
the one below, so the levels are the coarse copies the rings need (the clipmap idea).

- **Tiles.** The far field is cut into **far tiles** 256 m on a side (1,024 blocks, 32 by 32
  chunks), each the root of an octree tall enough for the whole world's height. Its levels
  run from 256 m at the root down to 0.5 m at the bottom: nine levels. There is no 0.25 m level,
  since full detail is the chunks' job.
- **What a node holds**: which of its eight children exist (one byte), and one block standing
  for the whole node: what it looks like from far away. A missing child is all air. A node
  whose children would all be the same solid block has none: it is that block throughout,
  which prunes solid ground as well as air.
- **Coarser from finer.** A 0.5 m cell comes from its eight blocks, and each level above from
  the level below. A cell is solid when at least four of its eight are, and takes the
  commonest solid block among them; ties go to the lowest id, so every machine gets the same
  tree. Water and glass count as non-solid unless all eight are.
- **Built and kept on the server.** The server keeps each far tile's octree as a node array
  with child indices, built from the chunks. An edit changes at most one node per level
  (nine), along the path to the root, so updates are cheap. That is unlike a DAG, where shared
  subtrees would have to be unshared first. A tile's wire form is rebuilt from it at most every
  few seconds while edits come in.
- **On the wire and in the client**: breadth-first, level by level. Each node is its child
  byte plus an index into the tile's palette: about 2 bytes a node. A child's position comes
  from counting the set bits before it, with a running count every 64 nodes for fast lookup.
  A client asks for each tile only as deep as its distance needs, coarse levels first, and
  refines as it gets nearer. `ToClient::FarTile { key, depth, bytes }` joins the chunk messages,
  under the same `View` and byte budget.
- **Size**, an estimate to check against the sea and islands: nodes follow surface area. A flat
  256 m tile has 512 × 512 surface cells at 0.5 m, so about 350,000 nodes with the levels above,
  or about 700 KB. At 1 m it is about 175 KB, and every level up is a quarter of that.
- **Rings from the tree.** The client fills a 32³ grid from the octree at the ring's level
  (level L: blocks 2^L × 0.25 m, so a ring chunk covers 2^L × 8 m) and meshes it with the same
  mesher as a chunk. Level L is drawn from R·2^(L-1) to R·2^L, where R is the full-detail
  radius. Each ring holds about three quarters as many ring chunks as the full-detail disc has
  chunks. Three rings from 64 m reach 512 m, enough for the Void's 450 m haze.
- **Seams**: a coarse ring chunk is not drawn where the finer ring covers it. Cells outside a
  ring chunk count as air when it is meshed, so its sides are closed. Those side faces act as
  skirts, and no cracks show where it meets a finer chunk. Finer data arrives before coarser
  is dropped.
- **Simulation** uses the same tree as its index: awake bits, per-node summaries (wetness,
  snow, temperature, damage), where to sample (coupling points, weighted ticks, ray queries) and
  bricks for its fields, with sparse per-block state. See
  [ADR 0001](../decisions/0001-simulation-in-the-voxel-octree.md).
- Generated ground (atlas tiles) could build its octree on the client without the network.
  That waits for zones and the generator (world-scaling.md).

### 6. Ray-marching the far field (a prototype, desktop first)

The same octree, traced per pixel on the GPU instead of meshed:

- **Split by distance.** Chunks within the full-detail radius are drawn as meshes, as now. A
  full-screen pass after them marches each pixel's ray through the far field, starting a
  little inside the full-detail radius. It writes a colour and a depth (`@builtin(frag_depth)`)
  with the depth test on, so a near mesh in front always wins and the overlap shows no seam.
- **The GPU's form is a 64-tree**: nodes with 4 × 4 × 4 children, each node's occupancy a 64-bit
  mask held as two `u32`s, the layout reported to trace faster than an octree. It is built on
  the client from the far tiles it receives, so the wire carries one format. Its levels step
  by four, not two: from a 512 m root (four far tiles) they run 128 m, 32 m, 8 m, 2 m and
  0.5 m, so its leaves match the octree's finest level. Each node keeps the octree's
  representative block for its size.
- **Level of detail by footprint.** A ray stops descending once a node is smaller than its
  pixel at that distance. That keeps distant terrain from shimmering (the tree is its own mip
  chain), and bounds the steps per ray.
- **Shading kept simple.** Each block's colour is the average of its tiles, worked out once from
  the texture atlas. The hit face gives the normal. The sky's sun and ambient light come from
  `SkyLook`, and the haze uses the same visibility as `DistanceFog`, so near meshes and far
  pixels fade the same way. There are no shadows or corner shading that far out.
- **Into Bevy.** A render graph node after the main opaque pass that reads the depth texture.
  The details of Bevy 0.20's render graph are to be confirmed by the prototype; this is its
  main risk.
- **Limits.** The tree lives in one storage buffer. Kept under 64 MB, it stays inside WebGPU's
  default 128 MiB binding limit, so no page has to ask for more.
- **Why bother.** Meshing cost grows with the blocks in view; ray-marching cost grows with the
  pixels that show far terrain. Past a few hundred metres there are far more blocks than
  pixels. It also lets the desktop see farther than the mesh rings could afford: 1 to 4 km in
  clear weather, which a filled 25 km Void would want.
- **The decision.** Build it behind a setting, on the desktop. Measure it against the mesh rings
  at the same distances in the Void, the islands and the sea: GPU frame time, memory, CPU time
  saved on meshing, and the look (popping, shimmer, the seam at the handover). Then decide for
  each profile. A computer's browser follows only if it wins there too. Phones keep the mesh
  rings: their GPUs are weakest at long per-pixel loops.

### 7. Meshing and drawing (after measuring)

- **Benchmark our mesher** against binary greedy meshing on real chunks (sea, islands) and
  adopt it if it is several times faster. Our 32-block chunks plus a border fit 64-bit masks.
  Keep merging only faces with the same tile and light, as now.
- **Off the main thread on the desktop** with Bevy's `AsyncComputeTaskPool` (the echo loader
  already does this). The browser build has no threads: it keeps the per-frame budget, nearest
  first. Workers would need wasm threads and cross-origin isolation headers; revisit only if
  measurements show the budget can't keep up.
- **Cave culling** (15 bits a chunk) for the islands, the sea's depths and the dungeons, after
  frustum culling. It matters little in the open Void, so it comes last. Interiors we author
  get exact culling from rooms and openings instead
  ([ADR 0002](../decisions/0002-bsn-scenes-rooms-and-openings.md)).
- **Fewer draw calls**: count them first. If they bound frame time, merge each 2 × 2 × 2 group
  of coarse chunks into one mesh per material. Multi-draw indirect can't be relied on in
  browsers yet.

### 8. Server memory

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
5. **The far-field octree**: built and kept on the server, far tiles on the wire, rings meshed
   from it on the client, with seams closed.
6. **Ray-marching prototype** on the desktop: the 64-tree, the far-field pass, and measurements
   against the rings. Then a decision for each profile.
7. **Meshing**: benchmark binary greedy, adopt if faster; desktop meshing off the main thread.
8. **Cave culling**, then draw-call batching if measurements call for it.

## Budgets (first targets)

Per device profile (see §1, *Device profiles*, for mobile):

| | Browser | Desktop |
| --- | --- | --- |
| Full detail | 64 m | 128 m |
| LOD rings | 3 (to 512 m) | 3 (to 1,024 m) |
| Block memory | under 20 MB | under 60 MB |
| wasm memory in the Void, sea echo in view | under 300 MB | n/a |
| Meshing per frame | 5 ms | 8 ms, or off-thread |
| Join to first view (sea) | under 3 s on a home connection | under 2 s |
| Far-field octree held | under 16 MB | under 64 MB (the ray-march buffer too) |

## Open questions

- Does Bevy 0.20 batch our chunk meshes in the browser, or is each one its own draw call?
  This needs the frame capture from phase 0.
- Water and glass at coarse levels: drop them, or keep a thin surface? Decide from screenshots.
- Should the world's limit on view radius depend on how many players are in it?
- Ray-marching: is footprint-based level of detail enough against shimmer, or does the far pass
  need temporal anti-aliasing? Can Bevy 0.20 take a custom pass with depth in and out?
- When does part of a world stop changing enough to be worth a DAG (shared subtrees)?

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
- [Efficient Sparse Voxel Octrees (Laine and Karras)](https://research.nvidia.com/publication/efficient-sparse-voxel-octrees), [technical report](https://research.nvidia.com/sites/default/files/publications/laine2010tr1_paper.pdf)
- [A guide to fast voxel ray tracing using sparse 64-trees](https://dubiousconst282.github.io/2024/10/03/voxel-ray-tracing), [Sparse-Voxel-64Tree crate](https://docs.rs/crate/Sparse-Voxel-64Tree/latest/source/src/lib.rs)
- [MDN: GPUSupportedLimits](https://developer.mozilla.org/en-US/docs/Web/API/GPUSupportedLimits)
- [High resolution sparse voxel DAGs](https://www.cse.chalmers.se/~uffe/HighResolutionSparseVoxelDAGs.pdf), [symmetry-aware sparse voxel DAGs](https://www.crs4.it/vic/data/papers/i3d2016-symmetry-dags.pdf)
- [Chrome: new in WebGPU 131 (multi-draw indirect)](https://developer.chrome.com/blog/new-in-webgpu-131)
