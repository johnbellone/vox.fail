---
type: Decision
title: "Decision (proposed): Simulation State in the Sparse Voxel Octree"
description: "ADR 0001: the octree is simulation's index (activity, summaries, where to sample); fields are solved in octree-indexed bricks, effects on blocks are sparse per-block state; gravity is a world setting and spin means voxel objects."
tags: [decision, proposed, simulation, octree, weather, gravity, voxel-objects]
generated: { by: agent/mcp, at: "2026-11-28T04:24:01Z" }
status: draft
---

# Decision (proposed): Simulation State in the Sparse Voxel Octree

**ADR 0001**, `docs/decisions/0001-simulation-in-the-voxel-octree.md`, status **proposed**, 2026-11-28. Beads epic `vox-1lp` (11 beads). The owner asked for simulation that affects blocks (climate, gravity, spin) through the sparse voxel octree planned for far terrain ([chunk-streaming-design](../architecture/chunk-streaming-design.md) §5). They then added: use the octree as the index for sampling points.

## Decided (proposed)
The octree is simulation's **index**: of where things are happening, of what has happened (its summaries), and of where to sample. Plain arrays do the arithmetic.
1. **Fields live in bricks that the octree indexes.** Air (and liquids, if they come) is solved on 8 × 8 × 8-cell bricks, the octree's leaves. Bricks exist only where the field matters: fine near the ground and builds, coarse or none in empty sky. Stencils run on a brick's plain array; the tree is consulted once per brick edge, never per cell. This is the OpenVDB layout. Today's 8 m weather grid is the first, coarse brick layer, and finer bricks come only if the `vox-1lp.11` prototype shows they pay.
2. **The octree is the sampling index**:
   - **Coupling points**: fields change blocks only at exposed, awake surface nodes, at a depth set by distance to a watcher (every 0.25 m face up close, one sample per 2 to 8 m node far off).
   - **Weighted random ticks**: slow changes pick branches by their summaries and bits, using a pick seeded by world and tick.
   - **Ray queries**: rain reaching a block, sky seen, a path for water, each skipping whole empty or solid nodes.
3. **Effects on blocks are sparse per-block state**: a small record (kind and value), only for the blocks that have one, kept in a side table per chunk. It is stored with the chunk and sent with it.
4. **Node metadata**: activity bits (awake, exposed, and a reserved *grounded* bit) and a byte of summary per channel (wetness, snow, temperature, damage), built bottom-up. Far tiles carry the summaries, which also weight the random ticks.
5. **Only awake nodes are simulated**, within a per-world budget: bricks are stepped and coupling points sampled only there. Nodes wake on edits, field changes and awake neighbours, and sleep when quiet.
6. **Gravity is a world setting**: one vector, defaulting to 9.81 m/s² down, or declared bodies. Nothing is stored per node.
7. **Structural collapse**: left to its own ADR (`vox-1lp.9`).
8. **Spin means voxel objects**: separate small grids with a position, rotation and spin. A planet's spin is a climate number.
9. **The server is the authority**: a fixed tick, fixed-point state and seeded sampling. Clients get visible state and summaries, with no prediction.

## Rejected or kept as a start
- **Rejected**: one simulation cell per octree node (a tree walk for every neighbour read); dense state for every block; GPU simulation.
- **Kept as a start**: uniform grids everywhere stay as the coarse layer; uniform random ticks are kept, but weighted by the tree.

## Consequences
- **Good**: cost follows activity and how closely anyone is looking, and resolution goes where the field matters.
- **Costs**: a new chunk section and a protocol bump; summaries in far tiles; bookkeeping to make and drop bricks; determinism tests for every rule, sampling included.
- **Order**: after the far-field octree (vox-cti.11, .12) and streaming (vox-cti.7).

# Related Concepts
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): the octree this extends
- [Weather Simulation](../architecture/weather-simulation.md): the grid that becomes the first brick layer
- [Design (proposed): Worlds at 60 km x 60 km](../architecture/world-scaling-design.md): simulation owners
