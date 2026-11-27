---
type: Design
title: "Props in Worlds: Entity, Placement, Drawing, Collision and Use"
description: "How a world stores, validates, draws, collides with and lets players use prop instances, and how dungeons use them."
tags: [design, props, entities, vox-server, vox-world, dungeons]
generated: { by: agent/mcp, at: "2026-11-27T03:35:41Z" }
status: draft
stale_after: 2026-12-26
---

# Props in Worlds: Entity, Placement, Drawing, Collision and Use

Part of `architecture/prop-pipeline-design`; source `docs/design/props.md`. Builds on `architecture/world-format-design` (entities, snapshots) and `decisions/structure-entitlements`.

## The entity
`Prop { prop: "<hash>", at: (x, y, z), yaw: 0..23, variant, state }`. `at` is an `i32` triple in 5 cm units (exact and identical everywhere); `yaw` in 15 degree steps. One more world component in `vox-format`, stored in its region's entities, so snapshots, Oxen history, restores and instances carry props. Props inside a structure are part of its blob, in the structure's coordinates.

## Placing
The server checks, in order: the hash is a known accepted version; the player may use it (catalogue props are open to every signed-in account; a player-made one needs `use`, via `vox_core::entitle`); the position is in reach and bounds; the footprint is free of solid blocks and other footprints; an accepted surface is under, beside or over it; plot and region caps hold (to begin with 64 props a plot, 4,000 a region). Moving and removing use the same checks.

## Drawing
Clients expand an instance from its blob into a small mesh (cached by hash) with the avatar props' look (shared palette image, flat shading).
- Static props of a region merge into one mesh per material per chunk, rebuilt with the chunk, within the existing per-frame budget (8 ms desktop, 5 ms browser).
- Repeats (fence rows, street lamps) are instanced.
- Moving parts are separate nodes only on `door` and `motion` props, and only nearby.
- Small props fade out by size (a mug at 30 m, a boat at 300 m).
- The nearest few `light` sockets join the lamps and fires already picked in `outdoors.rs`.

## Standing in the way
Blocks stay the only thing in chunk data. A `collide: footprint` prop adds its cells to a **prop occupancy** layer that `vox-core::body` and the placement checks read beside the blocks; server and client derive it from the same instances, so prediction agrees. A closed `door` blocks, an open one does not. If the blocks it stands on are broken the prop is removed (returned to its owner's inventory once there is one).

## Using
The `use` action targets the nearest prop under the cursor within reach (voxel raycast against prop boxes). The server decides (reach, the behaviour exists, plot rules, rate limit), then changes the instance state, sets who sits where, or sends the `readable` text, and tells the world. States persist.

## Dungeons
Dungeon templates (`docs/design/world-network.md`: worlds frozen as snapshots) are dressed with props: `switch` and `door` on a channel are the puzzles (a lever opens a gate, a plate lowers a bridge), `marker` is the objective a raid is won on, torches and braziers are the light. Because props are in region blobs, an instance gets them for free.

## Fit
`vox_core::entitle` is the one rule for who may use, copy and edit a blueprint, so player-made props are the same kind of directory record as structures (owner, grants, versions). A graph node's `dressing` detail level (world-graph design) is where props are placed. The browser needs no threads or blocking: blobs fetched by hash, meshes built within the chunk budget.

# Related Concepts
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](../decisions/structure-entitlements.md): Player-made props use the same owner and grants rule as structures.
