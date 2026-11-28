---
type: Decision
title: "Decision (proposed): Simulation State in the Sparse Voxel Octree"
description: "ADR 0001: fields run on grids, effects on blocks are sparse per-block state, and the far-field octree carries awake bits and summaries so simulation follows activity; gravity is a world setting and spin means voxel objects."
tags: [decision, proposed, simulation, octree, weather, gravity, voxel-objects]
generated: { by: agent/mcp, at: "2026-11-28T04:21:54Z" }
status: draft
---

# Decision (proposed): Simulation State in the Sparse Voxel Octree

**ADR 0001**, `docs/decisions/0001-simulation-in-the-voxel-octree.md`, status **proposed**, 2026-11-28. Beads epic `vox-1lp`. The owner asked for simulation that affects blocks (climate, gravity, spin) through the sparse voxel octree planned for far terrain ([chunk-streaming-design](../architecture/chunk-streaming-design.md) §5).

## Decided (proposed)
1. **Fields run on grids.** Air, and liquids if they come, are simulated on regular grids or bricks, as weather is now with its 8 m cells. They never run in tree nodes, because every neighbour read would be a walk through the tree.
2. **Effects on blocks are sparse per-block state.** Moisture, snow on a top face, ice, heat and damage are each a small record (kind and value), kept in a side table per chunk only for the blocks that have one. That state is stored with the chunk and sent with it.
3. **Octree nodes carry simulation metadata.** Each node gets activity bits (awake, exposed to the sky, and a reserved *grounded* bit) and a byte of summary per enabled channel (wetness, snow, temperature, damage), built bottom-up. Far tiles carry the summaries, so distant terrain can show snow or wet ground.
4. **Only awake nodes are simulated.** The server works within a per-world time budget, wakes nodes on edits, on field changes over exposed nodes, and on awake neighbours, and puts quiet ones to sleep. A still Void costs almost nothing.
5. **Gravity is a world setting**: one vector, defaulting to 9.81 m/s² down. A world may declare bodies to fall towards instead. Nothing is stored per node.
6. **Structural collapse is not decided here.** It gets its own ADR, bead `vox-1lp.9`; the grounded bit is reserved for it.
7. **Spin means voxel objects.** Doors, windmills, ships and loose pieces are separate small grids, each with a position, rotation and spin, simulated on the server and indexed per tile. Their blocks are never in the world's chunks. A planet's spin is a number in the climate settings.
8. **The server is the authority**, on a fixed tick with fixed-point state. Clients get the state of the chunks they can see and summaries in far tiles, and none of it needs prediction.

## Rejected
- Simulating fields in the octree.
- Dense state for every block: 34 M blocks in the sea alone.
- GPU simulation: not authoritative, and the browser has no compute shaders in our path.
- Random ticks alone. They stay as a tool for slow changes, such as snow settling.

## Consequences
- **Good**: simulation cost follows activity, the far field shows weather, and moving objects never disturb the world's chunks.
- **Costs**: a new chunk section, which means a protocol bump and a zone format version; summaries in far tiles; determinism tests for every rule; and care with wake and sleep, so no region stays awake for ever.
- **Order**: this comes after the far-field octree (vox-cti.11, .12) and streaming (vox-cti.7).

# Related Concepts
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): the octree this extends
- [Weather Simulation](../architecture/weather-simulation.md): the grid that stays a grid
- [Design (proposed): Worlds at 60 km x 60 km](../architecture/world-scaling-design.md): simulation owners
