---
type: Process
title: "Prop Authoring: Recipes, MagicaVoxel Import and the Touch-up Loop"
description: "The two lanes into one prop library: Rust recipes with prop-shaped operations, and hand-made .vox files imported with palette snapping and reserved-colour tags."
tags: [process, props, recipes, magicavoxel, import, vox-art]
generated: { by: agent/mcp, at: "2026-11-27T03:35:41Z" }
status: draft
stale_after: 2026-12-26
---

# Prop Authoring: Recipes, MagicaVoxel Import and the Touch-up Loop

Part of `architecture/prop-pipeline-design`; source `docs/design/props.md`. Both lanes end as a sparse `VoxelGrid` (palette indices) plus a `PropDef` (`architecture/prop-definition`) and then go through the same checks (`architecture/prop-checks-and-review`).

## Lane A: recipes
Data, written by a person or a language model, interpreted by Rust (`vox-art`, native and wasm, no Python). The interpreter owns proportions and edge treatment; the author says what, not which voxel. Only named palette materials; randomness only through the seeded RNG, so a recipe, seed and variant give the same voxels everywhere.

| Operation | Does |
| --- | --- |
| `box`, `slab`, `sphere`, `cyl` | solids (round ones voxel-true) |
| `lathe` | a profile revolved about the pivot axis: vases, barrels, lamps, mugs |
| `legs`, `frame`, `rail` | tables, chairs, beds, fences, ladders |
| `stairs`, `ramp` | steps and slopes |
| `mirror`, `repeat`, `ring` | symmetry and repetition |
| `cut`, `hollow` | openings, drawers, inside of a crate |
| `part`, `socket` | name a voxel group; name a point and direction |
| `bevel` | one voxel of rounding on big props, none on small |
| `shade` | the three-tone rule (base, shade, highlight) per material |
| `scatter` | seeded leaves, grass, pebbles within a shape |
| `weather` | seeded wear: chips, moss, rust, snow, salt |

## Lane B: hand-made `.vox` (first-class)
1. Read with a hardened reader (sizes capped, indices checked, as the WAV reader).
2. Orient (MagicaVoxel is z-up, the world y-up); the scale comes from the def, not the file.
3. Snap each colour to the nearest project palette entry, reporting the distance; past a limit the import fails naming the colours.
4. Pivot from a marker voxel in a reserved colour (fallback: base centre); parts and sockets from other reserved colours, so a person tags them in MagicaVoxel.
5. Clean: drop islands of one voxel; report floating parts rather than dropping them.
6. The file stays as the source; the def beside it holds what a file cannot say (behaviours, states, tags).

## The touch-up loop
`vox-forge` exports any recipe result as `.vox`; a person fixes what a recipe cannot; the import comes back as a new source with `derived_from` pointing at the recipe. The recipe stays for variants; the hand edit is a recorded override.

## Seeding the library
The usable existing avatar props come in through Lane B from `source/vox/props/`: bed, ladder, boat, lectern, chairs, lawnchair, pine, brick, can, burger, ball, book, binoculars, bag, paper, phone, rake, hammer, mic, coffee, notepad, pen, gavel, flag stand, podium. The politically specific ones (placards, vote cards, rally signs) stay with the avatar app until the open product question in `project/characters-and-content` is settled.

## Library, blobs and versions
Source lives in `data/props/<id>/` (def, recipe or `.vox`, accepted review sheet). Accepted versions are built to `VOXP` blobs (def plus run-length-coded zstd grid, BLAKE3-addressed, same caps as untrusted regions), served by hash over HTTP (the edge already caches `/assets/`). The avatar app keeps reading `.glb`: the exporter writes the same GLB the Python builders do, so the Python builders retire prop by prop.

Decision: `decisions/props-hybrid-authoring`.

# Related Concepts
- [Decision: Props Come from Recipes and Imported MagicaVoxel Files](../decisions/props-hybrid-authoring.md): Why there are two lanes.
