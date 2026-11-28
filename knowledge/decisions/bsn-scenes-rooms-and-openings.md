---
type: Decision
title: "Decision (proposed): BSN Scenes for Drawing; Rooms and Openings for Indoor Culling"
description: "ADR 0002: world components stay engine-free while the client draws each kind as a bsn! scene (.bsn files later, through a native tool); authored Room and Opening components drive cells-and-portals culling indoors, failing open, and make rooms sheltered."
tags: [decision, proposed, bsn, bevy, culling, rooms, world-structure]
generated: { by: agent/mcp, at: "2026-11-28T04:29:00Z" }
status: draft
---

# Decision (proposed): BSN Scenes for Drawing; Rooms and Openings for Indoor Culling

**ADR 0002**, `docs/decisions/0002-bsn-scenes-rooms-and-openings.md`, status **proposed**, 2026-11-28. Beads epic `vox-3o4`.

## Facts it rests on
- **BSN in `bevy_scene` 0.20.1** is the `bsn!` macro only: scene composition, field-level overrides, and asset paths resolved from strings. There is **no `.bsn` file format** yet. Bevy plans one, and says file assets won't support expressions.
- **The server, directory and `vox-core` have no Bevy.** [World format](../architecture/world-format-design.md) already kept Bevy's `DynamicWorld` out of stored worlds.
- **"Portal" means a doorway to another place** in Vox, so the culling openings get other names.

## Decided (proposed)
- **A. BSN**
  - World structure stays as engine-free `vox-core` components: postcard, level JSON, checked by the server.
  - The client draws each component kind (doorway, prop, editor helpers, voxel objects later) as a `bsn!` scene, filled from the component by a template. Editor ghosts reuse the same scenes with an override.
  - Authored `.bsn` files come only after Bevy ships the format. A native tool will convert them into entities and level JSON, so the server never reads BSN.
- **B. Rooms and openings**
  - `Room` (boxes; sheltered; interior light) and `Opening` (a rectangle joining two rooms or a room and outside, optionally tied to a door prop) become new entity components, edited in Advanced mode and declared in levels.
  - Each frame the client walks from the camera's room through openings, narrowing a screen rectangle at each (Luebke and Georges 1995). Closed doors stop the walk, and the outside is drawn only if reached. The unit is the chunk. It runs after frustum culling, beside cave culling for terrain.
  - It **fails open**: no room, or an unsure state, means nothing is culled. A debug view shows what is culled.
  - The same graph makes rooms sheltered (ADR 0001's *exposed* bit is off inside), gives interior light, and later handles sound and voice.

## Rejected
- BSN or `DynamicWorld` as the stored format.
- Our own `.bsn` parser.
- Precomputed visible sets, because worlds are edited live.
- GPU occlusion for now, though it may come later if measured.

## Consequences
- A protocol bump for the new components, editor tools, and authoring work.
- Indoor scenes draw only what can be seen.
- Drawing code becomes composable scenes.

# Related Concepts
- [Design (phase 1 built): The World Format](../architecture/world-format-design.md): engine-free stored entities
- [Chunks, streaming and draw distance](../architecture/chunk-streaming-design.md): cave culling and the other culling plans
- [Decision (proposed): Simulation State in the Sparse Voxel Octree](simulation-in-the-voxel-octree.md): the exposed bit rooms clear
