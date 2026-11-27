---
type: Design
title: "Design (proposed): Props and the Prop Pipeline"
description: "A prop is a small authored object with behaviours, made as a recipe or an imported MagicaVoxel file, checked, reviewed, and placed in worlds as a versioned entity; with a seventy-prop catalogue."
tags: [design, proposed, props, art, vox-art, pipeline]
generated: { by: agent/mcp, at: "2026-11-27T02:46:20Z" }
status: draft
stale_after: 2026-12-26
---

# Design: Props and the Prop Pipeline

Source: `docs/design/props.md` (proposed 2026-11-26; nothing built). Builds on `architecture/world-graph-and-art-design` (recipes, `vox-art`, library) and `architecture/world-format-design` (entities, structures, entitlements).

## What a prop is
A small authored object with its own voxel grid (5 cm "fine" or 10 cm "prop" voxels, at most 64 a side), distinct from blocks (0.5 m, atlas tiles) and structures (blueprints of blocks). A `PropDef` is immutable and content-addressed: size, pivot, placement surfaces and footprint, collision, named parts, sockets, states, variants and a **closed vocabulary of behaviours** (light, seat, door, switch, readable, sound, motion, marker), never code.

## Pipeline
brief, source, normalise, evaluate, check, render, review, accept, library, pack, worlds. Two lanes into one library: **recipes** (data interpreted by Rust, native and wasm) and hand-made **MagicaVoxel `.vox`** imports (palette snapped to the project palette, pivot/parts/sockets tagged by reserved colours). A touch-up loop exports a recipe's result to `.vox` and re-imports the edit. Checks return structured reasons (size, voxel and triangle budgets, palette, floating parts, thin walls, footprint, sockets, behaviours, names). Review is a rendered contact sheet; a model can pre-screen later.

## In a world
`Prop { prop: hash, at (5 cm units), yaw (15 degree steps), variant, state }` is a region entity, so snapshots, Oxen history and instances carry props. The server checks placement (known version, use right, reach, free footprint, surface, caps); clients mesh by hash, batch static props per chunk and light with the nearest socket lights. Collision is a prop-occupancy layer beside blocks. Props are the dressing and puzzles of dungeon templates.

## Catalogue
Seventy props in eight groups (light, home and village, nature, dark sea, station, islands, dungeon and play, void); twelve are the **first wave** that exercises every pipeline feature (barrel, bench, signpost, street lamp, brazier, door, lever, pine, console, hanging lantern, rowboat, crate). The usable existing avatar props seed the library through the import lane.

## Open questions
A third scale; whether decorative props block by default; removal when supporting blocks go; who may place; skeletons versus part states; review before player-made props are public; filtering for readable text.

Related: `architecture/asset-pipeline`, `architecture/world-graph-and-art-design`, `architecture/world-format-design`, `decisions/structure-entitlements`.
