---
type: Decision
title: "Quarter-metre blocks in every world, a 25 km Void"
description: "Blocks are 0.25 m everywhere (vox_core::BLOCK_M), sizes are written in metres, the Void is 102,400 blocks across, and saved data is converted once on start."
tags: [decision, blocks, scale, void, migration, plots]
generated: { by: agent/mcp, at: "2026-11-28T03:20:55Z" }
status: stable
---

# Quarter-metre blocks in every world, a 25 km Void

**Decided 2026-11-27, at the owner's request** ("blocks smaller, the Void 100x larger, less fog, more places to build"). The owner chose 0.25 m blocks in *every* world, not only the Void, and read "100x" as 100x across, not 100x the area. Plan and as-built notes: `docs/design/block-scale.md` (beads epic `vox-2go`).

## The rule
- `vox_core::BLOCK_M = 0.25` is the only block size. `data/blocks.json` must agree; servers and the client refuse a palette that doesn't.
- Sizes that are really metres are written in metres and converted with `vox_core::in_blocks(m)` or `whole_blocks(m)`:
  - the player is 1.8 m tall (7.2 blocks), with eyes at 1.6 m
  - reach is 8 m
  - a step up is 0.5 m
  - voice carries 16 m
  - portal sides are 0.5 to 32 m
- Protocol 10 keeps older clients out. Relief is version 2: the same ground at the new scale.

## What changed in the worlds
- **Levels** keep their size in metres. Builders draw in half-metre units and write through `tools/assets/rescale_level.py` (x2). Hollow boxes keep their thickness (`thick`), and a level may set `bounds`.
- **The Void** is ±51,200 blocks across (25.6 km) and -64..384 blocks high.
  - Sky visibility is 450 m (it was 130 m).
  - The luminous weather sits just short of saturated.
  - The fog volume is thinner.
- **Plots** are 64 × 64 blocks (16 m), six per world, kept 12 m from the spawn.
- **Authority** is one per 200 blocks placed (it was 50). Atlas tiles are 1,024 blocks (256 m).
- **Props** keep their 5 cm positions. Each half-metre footprint cell covers 2 × 2 blocks.

## Saved data
- **Worlds** convert once on first start (`bins/vox-server/src/rescale.rs`, meta `block_m`):
  - each block becomes a 2 × 2 × 2 cube, one stored chunk at a time
  - entities and settings move with the blocks
  - the edit logs are emptied
  - the next snapshot is whole
- **The directory** converts once (`bins/vox-directory/src/rescale.rs`, counter `block_scale`): plots, portals, last-seen places, spawns, deeds and keys.
- **Snapshots** taken before the change are converted when restored.
- **Not converted:** world-graph node sizes and recipes, and structure blobs.

## Costs found, and what was done
- The same space holds 8x the blocks.
  - The sea went from 309 to 2,179 chunks, and from 34 MB to 138 MB of server memory at rest.
  - Loading a level used to build a map of every block, so seeding the sea peaked at 1.7 GB. Portal echoes did the same in the browser.
- Fixed 2026-11-28 (`vox-cti.2`, `vox-cti.3`):
  - `vox_core::level::apply` writes levels straight into chunks, clipped to a region. Seeding the sea now peaks at 142 MB.
  - Echoes hold only the region round their arrival point.
  - Device profiles came in at the same time: `vox_world::detail`, desktop, browser and mobile.
- The rest of the plan is [Chunks, streaming and draw distance](chunk-streaming-design.md).

# Related Concepts
- [Worlds, Levels and Blocks](worlds-catalogue.md): the level format this changes
- [Chunks, streaming and draw distance](chunk-streaming-design.md): the follow-on plan for memory and view distance
- [Design (proposed): Worlds at 60 km x 60 km](world-scaling-design.md): streaming and zones, which this makes more urgent
