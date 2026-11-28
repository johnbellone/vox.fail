# ADR 0002: BSN for how the world's things are built on screen; rooms and openings for indoor culling

- **Status:** proposed
- **Date:** 2026-11-28
- **Decider:** the owner
- **Builds on:** [world-format.md](../design/world-format.md) (*Entities*: stored components,
  engine-free); [chunk-streaming.md](../design/chunk-streaming.md) §7 (culling);
  [ADR 0001](0001-simulation-in-the-voxel-octree.md) (the *exposed* bit)
- **Beads:** epic `vox-3o4`

## Context

The owner asked for two things in one decision: using **BSN** (Bevy Scene Notation) in the
world's structure, and **portal occlusion culling** for indoor scenes that we define ourselves.

What BSN is in our Bevy (checked in `bevy_scene` 0.20.1, the version we pin):

- A scene system for composing entities: a scene names an entity's components and children,
  scenes include other scenes, and a reuse can override single fields of a component. Strings
  in a scene resolve to asset handles.
- Written with the **`bsn!` macro**, in Rust, at compile time. **There is no `.bsn` file format
  yet.** Bevy's docs say one is planned. Its files will not support expressions as the macro
  does, and until then games should use other formats or community loaders.
- It is part of Bevy. Our world server, the directory and `vox-core` have no Bevy, by design:
  world-format.md already kept Bevy's `DynamicWorld` out of stored worlds, because it would pull
  29 Bevy and wgpu crates into the server.

What we have for structure and culling today:

- A world's things are `vox_core::entity::Component`s: `Portal`, `Spawn`, `Objective`,
  `Settings`, `Prop` and `Node`. They are stored as postcard in SQLite and zones, authored in
  level JSON, sent as `Entities`, and checked by `vox_core::edit`.
- On screen, each kind is spawned by hand-written Bevy code (`portals.rs`, `props.rs`, the
  editor's ghosts), with the same patterns repeated.
- Culling is Bevy's frustum culling per chunk mesh. Cave culling (15 face-connection bits per
  chunk) is planned for terrain (vox-cti.17). Nothing knows what is *inside*: the Crypt, a
  house in the yard, a cave on the islands. Every chunk and prop in front of the camera is
  drawn, even behind solid walls.
- "Portal" already means something in Vox: a doorway to another world or place. This ADR's
  culling openings must not be confused with it.

## Decision

### Part A: BSN

1. **What a world *is* stays engine-free.** World structure is `vox-core` components, stored as
   postcard, authored as level JSON (RON later, per world-format.md), and checked on the server.
   BSN is **not** the stored or wire format: the server can't read it without Bevy, and Bevy has
   no file format for it yet.
2. **What a thing *looks like* is a BSN scene.** On the client, each component kind is drawn by
   a scene written with `bsn!`:
   - a doorway: frame, echo quad, glow
   - a prop: its mesh, parts and lights
   - room helpers in the editor
   - voxel objects, later (ADR 0001)

   A template turns the component's fields into the scene's overrides, for example the
   doorway's size or the prop's variant. Scenes include one another: a lit doorway is a doorway
   plus a light. The editor's ghosts are the same scenes with a translucent override. This
   replaces the hand-written spawn code in `portals.rs` and `props.rs`, one kind at a time,
   starting with the doorway.
3. **Authored structure files wait for Bevy's `.bsn` format.** When it ships, rooms, prefabs and
   dungeon pieces may be authored as `.bsn` files. A native build tool, which has Bevy,
   converts them into `vox-core` entities and level JSON, so the server still never reads BSN.
   Until then, level JSON stays. We don't write our own parser for a syntax Bevy hasn't
   released.

### Part B: rooms and openings (cells and portals)

4. **Two new world components**, called *rooms* and *openings* so they aren't confused with
   doorways to other worlds:
   - `Room`: a name, its volume as a few boxes in blocks, and what inside means: **sheltered**
     (no rain or snow; ADR 0001's *exposed* bit is off within it), its own ambient light instead
     of the sky's, and later sound.
   - `Opening`: a flat rectangle on a room's boundary (a sill point, an axis and a size, like a
     doorway), the two rooms it joins (or a room and *outside*), and optionally the door prop
     whose state opens and closes it.

   Both are ordinary entities: stored, sent, versioned and checked by `vox_core::edit`.
   - Caps: boxes per room, rooms per zone, opening size.
   - An opening must lie on its room's boundary.
   - Editors set them in Advanced mode; levels may declare them.
5. **Culling walks the rooms through their openings** (Luebke and Georges). Each frame:
   1. Find the room the camera is in.
   2. Walk to neighbouring rooms through openings, narrowing a screen-space rectangle at each
      opening (the opening's projected rectangle clipped to the current one). A closed door, or
      an opening outside the current rectangle, stops that branch.
   3. Draw what lies in the rooms reached: chunks that overlap them, and the props and entities
      inside them.
   4. Draw the outside only if an opening to it is reached.
   - From outdoors, rooms are entered through their outward openings that are in view, so a
     house's inside is drawn only through its windows and door.
   - Chunks are the first unit: a chunk is drawn if it overlaps any reached room, or the outside
     is reached. Interior chunk meshes are split per room only if measurement shows chunks are
     too coarse.
   - It runs after frustum culling, and beside cave culling, which stays for terrain and
     unauthored caves. Rooms take over where they exist.
6. **It fails open.** Camera in no room: rooms cull nothing. A room with no route to where the
   camera is: still drawn if anything is unsure (a door entity not loaded yet). The editor has a
   view that outlines rooms and openings and shows what is culled, so a missing opening is seen
   at once rather than as vanishing walls.
7. **The same graph serves more than drawing:**
   - **Weather and simulation**: rooms are sheltered, so no rain, snow or puddles fall inside
     them (ADR 0001).
   - **Light**: interior ambient light instead of the sky's.
   - **Sound and voice, later**: nearby voice heard through open openings and muffled by closed
     doors.
   - **Streaming, later and with care**: interiors behind closed doors sent later. They must
     still be sent before anyone can see them, so nothing leaks or pops.

## Options considered

| | Option | Verdict |
| --- | --- | --- |
| A1 | BSN (or Bevy's `DynamicWorld`) as the stored and wire format | Rejected: puts Bevy in the server; no `.bsn` files yet; world-format.md already chose engine-free components. |
| A2 | Our own `.bsn` parser in `vox-core` now | Rejected: it would track a syntax Bevy hasn't released, and may diverge. |
| A3 | BSN for how each component is drawn on the client; `.bsn` files later through a native tool | **Chosen.** |
| B1 | Cave culling only (automatic, per chunk) | Kept for terrain and unauthored caves; it can't know a door is shut or a room is sealed. |
| B2 | Precomputed potentially-visible sets (Teller and Séquin) | Rejected: worlds are edited live, so any precomputation goes stale. |
| B3 | GPU occlusion (depth pyramid, occlusion queries) | Later, if measured: not dependable in browsers today, and it costs a GPU pass every frame. |
| B4 | Authored rooms and openings, walked each frame | **Chosen** for interiors: cheap, exact for what we author, and also gives shelter, light and sound. |

## Consequences

- **Good:**
  - Indoor scenes (the Crypt, houses, caves) draw only what can be seen.
  - Rooms give weather, light and sound an "inside".
  - Drawing code becomes composable scenes that the editor's ghosts reuse.
  - The server stays free of Bevy.
- **Costs:**
  - Two new components, so a protocol bump (new `Component` variants go last), editor tools and
    validation.
  - Authoring work: rooms must be drawn by someone. A wrong room hides things, hence fail-open
    and the debug view.
  - Moving spawn code to BSN touches portals, props and the editor.
- **Open:**
  - The caps (boxes per room, rooms per zone).
  - Whether builders may define rooms inside their own plots (Basic mode) or only editors may.
  - Whether rooms can be suggested automatically from enclosed spaces.
  - How the far-field octree (chunk-streaming.md §5) treats rooms: interiors are never drawn far
    away.

## Follow-up beads (epic `vox-3o4`)

1. `vox-3o4.1` `Room` and `Opening` components in `vox-core`, with checks and edit rules.
2. `vox-3o4.2` Rooms in level JSON, and the Crypt authored with them.
3. `vox-3o4.3` The culling walk in `vox-world`, chunk by chunk, failing open.
4. `vox-3o4.4` A review of those three.
5. `vox-3o4.5` Measurements in the Crypt and the islands; whether to split chunk meshes by room.
6. `vox-3o4.6` Editor tools and the debug view.
7. `vox-3o4.7` Sheltered rooms in the weather (after ADR 0001's `vox-1lp.1`).
8. `vox-3o4.8` The doorway as the first `bsn!` scene, then props.
9. `vox-3o4.9` A decision once Bevy ships `.bsn` files.

## Sources

- Luebke and Georges, *Portals and Mirrors: Simple, Fast Evaluation of Potentially Visible
  Sets*, Symposium on Interactive 3D Graphics, 1995: frustum culling narrowed through convex
  portals over a graph of cells.
- [Teller and Séquin, *Visibility Preprocessing for Interactive Walkthroughs*, SIGGRAPH 1991](https://www.cs.princeton.edu/courses/archive/spr01/cs598b/papers/teller91.pdf):
  precomputed potentially visible sets.
- [Potentially visible set](https://wikipedia.com/wiki/Potentially_visible_set), and the
  [MIT 6.837 lecture notes](https://groups.csail.mit.edu/graphics/classes/6.837/F98/Lecture17/lecture17.html)
  on cells and portals.
- `bevy_scene` 0.20.1 crate documentation (`bsn!`, *.bsn Asset Format*): no `.bsn` files yet,
  and file assets will not support expressions.
