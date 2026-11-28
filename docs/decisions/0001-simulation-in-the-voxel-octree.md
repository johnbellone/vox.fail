# ADR 0001: Simulation state in the sparse voxel octree

- **Status:** proposed
- **Date:** 2026-11-28
- **Decider:** the owner
- **Builds on:** [chunk-streaming.md](../design/chunk-streaming.md) §5 (the far-field octree) and
  §6 (ray-marching it); [world-scaling.md](../design/world-scaling.md) (simulation owners);
  [block-scale.md](../design/block-scale.md) (quarter-metre blocks)
- **Beads:** epic `vox-1lp`

## Context

The owner wants simulation to affect the blocks themselves (climate, gravity, spin), and asked
whether the sparse voxel octree (SVO) planned for far terrain can carry it.

What exists today:

- **Blocks are static** apart from players' edits. A block is a palette id and nothing more: no
  per-block state, no block that changes by itself.
- **Weather** (`crates/vox-core/src/weather.rs`) runs on the server on a regular grid of air
  cells, 8 m across and 5 m tall, round the world: temperature, vapour, cloud, rain, wind. It
  touches the ground in one way only, the water standing in the ground's dips, kept per
  column of air. Clients get keyframes and simulate between them.
- **Nothing moves but players and props.** There is no structural rule (blocks hang in the air)
  and no rotating object; gravity acts on players' bodies only (`vox-core::body`).
- **The world is big and mostly still.** Blocks are 0.25 m, so the sea alone is 34 million
  blocks, and the Void is 25.6 km across. Almost none of it changes in any given minute.
- **The far-field octree is planned** (vox-cti.11): 256 m tiles, levels 256 m to 0.5 m, a child
  byte and a representative block per node, kept on the server, sent breadth-first to the
  depth a client needs.

Forces on the decision:

1. Simulation cost must follow **what is happening**, not how big the world is.
2. The server stays **authoritative and deterministic** (integer or fixed-point state, a fixed
   tick), as weather is and world-scaling.md's simulation owners need.
3. Editing must stay cheap, and the browser's limits (one thread, a few hundred MB) hold.
4. What a player sees far away should show the simulation's effect (snow on the far hills)
   without per-block data travelling that far.

## Decision

The octree is simulation's **index**: of where things are happening, of what has happened (its
summaries), and of where to sample. Plain arrays do the arithmetic. The tree only finds them.

1. **Fields live in bricks that the octree indexes.** Anything that reads its neighbours every
   step (air temperature, humidity, wind; liquids flowing, if they come) is solved on small
   regular grids, **bricks** of 8 × 8 × 8 cells. Bricks are leaves of the octree, and exist only
   where the field matters: fine bricks near the ground, builds and water, coarse ones or none in
   empty sky. A stencil runs on a brick's plain array. The tree is consulted once per brick edge
   to find the neighbouring brick, never once per cell. OpenVDB stores sparse volumes this way: a
   shallow tree over 8³ leaf bricks. Today's weather grid (8 m cells round the world) is the
   starting point: one coarse layer of bricks. Finer bricks are added only where a prototype
   shows they pay (`vox-1lp.11`).
2. **The octree is the sampling index.** Simulation reaches the blocks only at points the tree
   chooses:
   - **coupling points**: a field changes blocks where it is sampled, at exposed, awake surface
     nodes, at a depth set by distance. Near a player that is every 0.25 m block face; on a far
     hillside it is one sample per 2 m or 8 m node, written into that node's summary. Cost follows
     both what is happening and how closely anyone is looking.
   - **weighted random ticks**: slow changes (snow settling, drying, moss) pick branches in
     proportion to how likely they are to change. The weights come from the nodes' summaries and
     bits: exposed and wet, near heat, recently edited. The pick then descends to a block. The
     same budget lands where change is likely, not on bedrock. The randomness is seeded from the
     world and the tick, so every machine picks the same blocks.
   - **ray queries**: "does rain reach this block?", "how much sky does it see?" (for snow,
     drying and light), "is there a way through for water?". Each is a short ray through the tree
     that skips whole empty or solid nodes at once.
3. **Effects on blocks are sparse per-block state.** What the simulation leaves *in a block*
   (soil moisture, snow lying on a top face, ice, heat in a forge or lava, damage) is a small
   record: a kind and a value, a few bytes. It is kept only for the blocks that have it, in a
   side table per chunk keyed by the cell; a block without state has no entry. It is stored with
   the chunk (SQLite row and `vox-format` zone, a new section) and sent with the chunk to clients
   that can see it.
4. **Each octree node gains simulation metadata**, built bottom-up like its representative
   block:
   - **activity bits**: *awake* (something below is changing), *exposed* (open to the sky: rain
     and snow can reach), and one reserved for *grounded* (see 7);
   - **summaries**: a byte each for the channels a world uses (wetness, snow cover,
     temperature, damage): the average, or the most, of what is below.
   A far tile carries the summaries for the channels its world uses, so the far rings and the
   ray-marcher can tint distant terrain (snowy, wet, scorched) from the tree alone. The summaries
   are also the weights for weighted random ticks (2).
5. **The simulation only visits awake nodes.** Each tick the server walks the awake branches of
   the tiles it simulates, within a per-world time budget. Only there are bricks stepped and
   coupling points sampled. A node wakes on an edit below it, on a change in the field around
   it (rain starting wakes *exposed* nodes in the rain), or on an awake neighbour. It sleeps after
   a few quiet ticks. A still Void costs almost nothing.
6. **Gravity is a world setting, not node data.** One vector per world (9.81 m/s² down by
   default), read by bodies, falling things and liquids. A world with several bodies to fall
   towards (orbit, asteroids) declares them, and the pull is worked out from them where it is
   needed. Nothing is stored per node.
7. **Structural collapse is not decided here.** Whether unsupported blocks fall changes how
   building feels, and needs its own decision (bead `vox-1lp.9`). The octree reserves the
   *grounded* bit so that, if it is adopted, "is this still held up?" is answered by the changed
   branch alone.
8. **Spin means moving voxel objects.** Anything that turns or moves (doors, windmills, ships,
   a piece that broke off) is a **voxel object**: its own small grid (and octree), a position,
   a rotation and a spin, simulated on the server and drawn as its own mesh. The world's octree
   records which objects are in each tile; their blocks are never in the world's grid. A
   planet's spin (the Coriolis turn of the wind, the length of a day) is a number in the
   world's climate settings.
9. **Authority.** The server runs all of it on a fixed tick with integer or fixed-point state and
   seeded sampling, so any machine replays it the same. Clients get per-block state for the
   chunks they can see and summaries in far tiles; none of it needs prediction. Cosmetic parts
   may later move to world-scaling.md's simulation owners.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A | Simulate fields in the octree's own nodes, one cell per node | Rejected: every neighbour read is a tree walk, stencils are irregular, and it is hard to keep deterministic and fast, worse on one wasm thread. The tree indexes bricks and sampling points instead (1, 2). |
| B | Dense per-block state: a second array in every chunk | Rejected: memory scales with the world (34 M blocks in the sea) though almost no block has state. |
| C | Fields in bricks indexed by the octree; the octree as the sampling index; sparse per-block state; activity bits and summaries | **Chosen.** |
| F | One uniform grid per field everywhere (this ADR's first draft) | Kept as the starting point: today's weather grid is one coarse brick layer. Finer bricks only where they pay. |
| D | Simulation on the GPU (compute shaders) | Rejected for now: the server must be the authority; the browser has no compute in our path. |
| E | Uniform random ticks only (a few blocks per chunk each tick, as Minecraft does for crops and snow) | Kept *inside* C, but weighted by the tree (2), for slow changes. Not enough alone for fast ones (water, fire). |

## Consequences

- **Good:**
  - Simulation cost follows activity, and how closely anyone is looking.
  - Resolution goes where the field matters, with no per-cell tree walks.
  - The far field shows the weather's effects.
  - Per-block state costs memory only where it exists.
  - Moving objects never disturb the world's chunks.
- **Costs:**
  - A new chunk section (stored and sent) and per-node summaries in far tiles, so a protocol bump
    and a zone format version.
  - More server CPU while much is awake, bounded by the budget.
  - Determinism tests for every rule, sampling included.
  - The wake and sleep rules need care to avoid regions that never sleep.
  - Bricks add bookkeeping: creating and dropping them as activity moves, and the field's
    values across a brick edge.
- **Order:** it builds on the far-field octree (vox-cti.11, .12) and on streaming (vox-cti.7),
  so it comes after them. Weather's per-column ground water stays until per-block wetness
  replaces it.
- **Open:**
  - Which channels a world may enable.
  - How many bytes per-block state may take before a chunk refuses more.
  - Whether voxel objects collide with each other or only with the world.
  - The brick size (8³ to start) and the finest brick level.
  - How coupling-point depth follows distance when several players watch the same place (the
    finest anyone needs).

## Follow-up beads (epic `vox-1lp`)

1. `vox-1lp.1` Activity bits and summaries in the octree (extends vox-cti.11).
2. `vox-1lp.2` Sparse per-block state in `vox-core` chunks, its storage and wire form.
3. `vox-1lp.3` The awake scheduler on the server, with a per-world budget and tests that a
   still world sleeps. Reviewed together with the two above (`vox-1lp.4`).
4. `vox-1lp.5` Weather leaves wetness and snow on exposed blocks.
5. `vox-1lp.6` Far rings and the ray-marcher tint from summaries.
6. `vox-1lp.7` World gravity setting (and declared bodies).
7. `vox-1lp.8` Design: voxel objects (spin): their format, simulation, collision and
   networking.
8. `vox-1lp.9` Decision: structural collapse (an ADR of its own, accepted or rejected).
9. `vox-1lp.10` The octree as the sampling index: coupling points by depth and distance,
   weighted random ticks with a seeded pick, and ray queries for exposure and sky.
10. `vox-1lp.11` Prototype: one field (air temperature and humidity) in octree-indexed bricks
    against today's grid: cost, memory, results; keep the grid unless bricks win.

## Sources

- Museth, *VDB: High-resolution sparse volumes with dynamic topology*, ACM Transactions on
  Graphics 32(3), 2013, and [OpenVDB](https://www.openvdb.org/): a shallow tree over 8³ leaf
  bricks.
- [Laine and Karras, *Efficient Sparse Voxel Octrees*](https://research.nvidia.com/publication/efficient-sparse-voxel-octrees):
  ray casts through a sparse octree.
