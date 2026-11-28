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

Simulation is split by what each part needs. The octree is its index and its summary, not its
solver.

1. **Fields are simulated on grids.** Anything that reads its neighbours every step (air
   temperature, humidity, wind; liquids flowing, if they come) runs on regular grids or uniform
   bricks of cells, as weather does now. A tree makes every neighbour a walk, so it is not where
   a field lives. If a field needs fine cells near the ground and coarse ones in open sky, that
   is done with grid patches of different sizes, not with octree nodes.
2. **Effects on blocks are sparse per-block state.** What the simulation leaves *in a block*
   (soil moisture, snow lying on a top face, ice, heat in a forge or lava, damage) is a small
   record (a kind and a value, a few bytes) kept only for the blocks that have it, in a side
   table per chunk keyed by the cell. A block without state has no entry. It is stored with the
   chunk (SQLite row and `vox-format` zone, a new section) and sent with the chunk to clients
   that can see it.
3. **Each octree node gains simulation metadata**, built bottom-up like its representative
   block:
   - **activity bits**: *awake* (something below is changing), *exposed* (open to the sky: rain
     and snow can reach), and one reserved for *grounded* (see 6);
   - **summaries**: a byte each for the channels a world uses (wetness, snow cover,
     temperature, damage): the average, or the most, of what is below.
   A far tile carries the summaries for the channels its world uses, so the far rings and the
   ray-marcher can tint distant terrain (snowy, wet, scorched) from the tree alone.
4. **The simulation only visits awake nodes.** Each tick the server walks the awake branches of
   the tiles it simulates, within a per-world time budget. A node wakes on an edit below it, on a
   change in the field above it (rain starting wakes *exposed* nodes in the rain), or on an awake
   neighbour; it sleeps after a few quiet ticks. A still Void costs almost nothing.
5. **Gravity is a world setting, not node data.** One vector per world (9.81 m/s² down by
   default), read by bodies, falling things and liquids. A world with several bodies to fall
   towards (orbit, asteroids) declares them, and the pull is worked out from them where it is
   needed. Nothing is stored per node.
6. **Structural collapse is not decided here.** Whether unsupported blocks fall changes how
   building feels, and needs its own decision (bead `vox-1lp.9`). The octree reserves the
   *grounded* bit so that, if it is adopted, "is this still held up?" is answered by the changed
   branch alone.
7. **Spin means moving voxel objects.** Anything that turns or moves (doors, windmills, ships,
   a piece that broke off) is a **voxel object**: its own small grid (and octree), a position,
   a rotation and a spin, simulated on the server and drawn as its own mesh. The world's octree
   records which objects are in each tile; their blocks are never in the world's grid. A
   planet's spin (the Coriolis turn of the wind, the length of a day) is a number in the
   world's climate settings.
8. **Authority.** The server runs all of it on a fixed tick with integer or fixed-point state, so
   any machine replays it the same. Clients get per-block state for the chunks they can see and
   summaries in far tiles; none of it needs prediction. Cosmetic parts may later move to
   world-scaling.md's simulation owners.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A | Simulate fields in the octree's nodes (adaptive cells everywhere) | Rejected: every neighbour read is a tree walk; irregular stencils; hard to keep deterministic and fast, worse on one wasm thread. |
| B | Dense per-block state: a second array in every chunk | Rejected: memory scales with the world (34 M blocks in the sea) though almost no block has state. |
| C | Grids for fields, sparse per-block state, the octree for activity and summaries | **Chosen.** |
| D | Simulation on the GPU (compute shaders) | Rejected for now: the server must be the authority; the browser has no compute in our path. |
| E | Random ticks only (sample a few blocks per chunk each tick, as Minecraft does for crops and snow) | Kept as a tool *inside* C for slow changes (snow settling, moss); not enough alone for fast ones (water, fire). |

## Consequences

- **Good:** simulation cost follows activity; the far field shows the weather's effects; per-block
  state costs memory only where it exists; moving objects never disturb the world's chunks.
- **Costs:** a new chunk section (stored and sent) and per-node summaries in far tiles, so a
  protocol bump and a zone format version; more server CPU while much is awake (bounded by the
  budget); determinism tests for every rule; the wake and sleep rules need care to avoid regions
  that never sleep.
- **Order:** it builds on the far-field octree (vox-cti.11, .12) and on streaming (vox-cti.7),
  so it comes after them. Weather's per-column ground water stays until per-block wetness
  replaces it.
- **Open:** which channels a world may enable; how many bytes per-block state may take before a
  chunk refuses more; whether voxel objects collide with each other or only with the world.

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
8. `vox-1lp.9` Decision: structural collapse (ADR 0002, accepted or rejected).
