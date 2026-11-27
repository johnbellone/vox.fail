# Quarter-metre blocks and a 25 km Void (proposed)

Status: proposed, 2026-11-27. Owner's request: smaller blocks in every world, the Void 100
times wider, less fog there, more places to build.

[world-scaling.md](world-scaling.md) listed "changing the block size" as out of scope. This
document takes it on.

## What changes for a player

| | Today | After |
| --- | --- | --- |
| A block | 0.5 m | 0.25 m (in every world) |
| A player | 3.6 blocks tall, 1.2 wide | 7.2 blocks tall, 2.4 wide (same 1.8 m) |
| Stepping up | one block (0.5 m) | two blocks (0.5 m, the same height) |
| The Void | 512 × 512 blocks (256 m) | 102,400 × 102,400 blocks (25.6 km) |
| Fog in the Void | 130 m visibility | 450 m |
| A plot | 16 × 16 blocks (8 m) | 64 × 64 blocks (16 m) |
| Plots each, per world | 3 | 6 |

Other worlds keep their size in metres, so they hold twice as many blocks along each side.
The yard, the islands, the sea, orbit and the Crypt look the same as today, built from blocks
half as big. Players' builds keep their size: each saved block becomes 2 × 2 × 2 small
blocks.

## The rule: metres in the code, blocks from one constant

Today about forty constants are in blocks and only make sense at 0.5 m: body size, walking
and running speed, gravity, jump height, reach, eye height, the move checker's budget, voice
radius, plot size, spawn clearance, the atlas tile size, portal echo distances, the editor's
limits. There are also seven literal `0.5`s (`moves.rs:94`, `editor/camera.rs:175`, prop cells
in `body.rs` and `entity.rs`, `vox-art::Scale::World`).

- `vox_core::BLOCK_M: f32 = 0.25` becomes the only block size. `blocks.json` keeps
  `block_size`, and loading checks that it equals `BLOCK_M`.
- Constants are written in metres and converted once (`const HEIGHT: f32 = 1.8 / BLOCK_M`).
  A later size change then edits one number.
- The protocol goes from 9 to 10. A client from before the change is refused with "update the
  game", not let in at the wrong scale.

## Levels, props and structures

- **Levels:** a new `vox_core::level::rescale(level, 2)` doubles every box, portal, camera,
  spawn, arrival point and entity. The Python builders (`build_void.py`, `build_islands.py`,
  `build_sea.py`, `build_orbit.py`) take `SCALE = 2`. The Crypt is hand-made, so it goes
  through the rescaler once.
- **Per-world bounds:** a level may give `"bounds"`. The Void gives ±51,200 blocks across and
  -64 to 384 high. Other worlds keep `Bounds::default()` doubled (±512, -64 to 192). The Void's
  floor is drawn by a shader and the world stores only built chunks, so the empty space costs
  nothing.
- **Props:** props keep their size in metres. Their footprint cells are 0.5 m today and
  become 2 × 2 blocks.
- **Structures (`vox-art`):** `Scale::World` voxels follow `BLOCK_M`. Saved structures go
  through the same 2 × 2 × 2 conversion as world blocks.

## The live data on vox.fail (a one-time migration)

Each world's SQLite database and the directory's database are converted once, at start-up,
behind a schema version:

1. Every stored chunk becomes eight chunks, each old block filling a 2 × 2 × 2 cube.
2. Plots, portals, entities, spawns, last-seen positions and raid objectives are doubled.
3. Snapshots taken before the change are converted when restored (the manifest format goes up
   one version), so the week of history survives.
4. Litestream has the pre-migration copy in R2. The migration refuses to run twice, and logs
   what it changed.

## Risks

- **Eight times the blocks for the same space.** A world sends every chunk when you join
  (`bins/vox-server/src/world.rs:500`), so the yard, islands and sea send about eight times as
  much, and the browser holds eight times the voxels. Meshing is budgeted per frame (5 ms in
  the browser), so worlds fill in more slowly. Before shipping, measure the join size and
  frame time in the browser. If they are too high, streaming by view radius (phase 3 of
  world-scaling.md) comes first.
- **A 25.6 km Void takes a long walk.** At running speed (3 m/s) it is over two hours from edge
  to edge. The doorways stay near the Circle. Builders spread out from there, and later
  waypoints or portals can link far plots.
- **Chunk and view distances in chunks** cover half as many metres. Draw and fog distances are
  set in metres and stay put; per-chunk budgets have to be re-tuned.

## Order of work (beads under one epic)

1. `BLOCK_M` and metre-based constants across `vox-core`, `vox-world`, `vox-server` and
   `vox-directory`; protocol 10. Tests updated.
2. Props and `vox-art` at the new scale; prop cells as 2 × 2 blocks.
3. `level::rescale` and per-level bounds; levels regenerated at ×2; the Void's bounds and
   fog.
4. Plots: 64 × 64 blocks, 6 per world, spawn clearance 48 blocks (12 m, as today).
5. Migration of world, directory and snapshot data, with a test on a copy of a real
   database.
6. Browser measurement (join size, memory, frame time) on the yard and the islands, before
   release.
7. Help pages and README (`docs/help/building.md` and others that say "half a metre").
