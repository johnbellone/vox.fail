---
type: Design
title: "Chunks, streaming and draw distance (proposed)"
description: "Paletted chunks, levels loaded straight into chunks, streaming by view radius, per-device draw distance, a sparse voxel octree far field meshed in rings, and a desktop ray-marching prototype."
tags: [design, proposed, chunks, streaming, lod, octree, ray-marching, performance]
generated: { by: agent/mcp, at: "2026-11-28T03:21:10Z" }
status: stable
---

# Chunks, streaming and draw distance (proposed)

Design: `docs/design/chunk-streaming.md`; beads epic `vox-cti` (21 beads). It refines the streaming, memory and far-terrain parts of [world-scaling-design](world-scaling-design.md) for 0.25 m blocks.

## Measured (2026-11-27)
- **Dense chunks**: 32³ blocks of u16, 64 KB each. With a palette the sea would be 136 MB → 14.5 MB, and the islands 45 MB → 5.4 MB. Most chunks hold 2 to 4 kinds of block.
- **Joining** sends every chunk. The download is small (sea 0.67 MB run-length encoded); memory is the cost.
- **Level loading** built a map of every block, and seeding the sea peaked at 1.7 GB. Fixed: `vox_core::level::apply` writes straight into chunks (now 142 MB), and portal echoes load only the region round their arrival point.

## Decided so far
- **Device profiles** (`vox_world::detail`, chosen by the page; `vox-client --detail`):

  | | Full detail | LOD rings | Echo radius | Meshing per frame |
  | --- | --- | --- | --- | --- |
  | Desktop | 128 m | 3 | 64 m | 8 ms |
  | Browser | 64 m | 3 | 48 m | 5 ms |
  | Mobile | 32 m | 2 | 24 m | 3 ms |

## Proposed, in order
1. Measure in Chrome (vox-cti.1).
2. **Paletted chunks**: `Uniform` or `Packed` storage, with indices of 1, 2, 4, 8 or 16 bits. The API is unchanged.
3. **Streaming by view radius**: `View` and `FarTile` messages (protocol 11), per-player interest sets, nearest chunks first within a byte budget, and edits sent only to interested players.
4. **Draw distance** with fog matched to it.
5. **Far field as a sparse voxel octree**: 256 m tiles with levels from 256 m down to 0.5 m.
   - Each node holds a child byte and a block that stands for the whole node.
   - Coarser cells come from finer ones: solid when at least 4 of 8 are, using the commonest block.
   - The server keeps the trees, and an edit updates at most nine nodes along the path to the root.
   - Tiles are sent breadth-first, as deep as the client's distance needs.
   - The client meshes LOD rings from the tree, with closed sides acting as skirts.
6. **Ray-marching prototype, desktop first.**
   - The GPU form is a 64-tree: 4³ children per node, masks as two u32, 512 m roots.
   - A full-screen pass runs after the meshes and writes `frag_depth`, so near meshes win.
   - It stops descending by pixel footprint, uses the same haze as the meshes, and keeps its buffer under 64 MB.
   - Measurements against the rings decide it per profile; phones keep the rings.
7. Binary greedy meshing; meshing off the main thread on the desktop.
8. Cave culling and draw-call batching, if measurements call for them.

## Why not alternatives
- **Octree for near chunks**: block reads would walk the tree, and edits, the move check and meshing read single blocks constantly.
- **DAGs** compress more but are hard to edit. They wait until parts of a world stop changing.
- **Multi-draw indirect** is still experimental in Chrome's WebGPU.

## Comparison
Minecraft uses 1 m blocks in 16³ paletted sections, sends chunks within its view distance (10 by default, about 160 m), and skips hidden sections with cave culling. Far terrain only comes from mods: Distant Horizons, and Voxy (an octree drawn on the GPU).

# Related Concepts
- [Quarter-metre blocks in every world, a 25 km Void](quarter-metre-blocks-decision.md): why this is needed
- [Design (proposed): Worlds at 60 km x 60 km](world-scaling-design.md): zones, regions and the streaming this refines
