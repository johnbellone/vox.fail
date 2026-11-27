---
type: Design
title: "Design (phases 1-4 built): Ground Relief and Puddles"
description: "Natural ground has generated dips of a few centimetres (a seeded integer function drawn per pixel in the block shader), and rain pools in them from ground water kept by the weather simulation; geometric dips (phase 5) are not built."
tags: [design, proposed, terrain, generation, weather, rendering, shaders]
generated: { by: agent/mcp, at: "2026-11-27T14:07:57Z" }
status: stable
---

# Design (proposed): Ground Relief and Puddles

Source: `docs/design/ground-relief.md` (2026-11-27). **Status: proposed; nothing built.** Asked for by the owner: block heights not uniform, so water pools when it rains, as part of world generation.

## Shape
- **Relief is generation, not data**: `vox_core::relief::depth(seed, x, z)` (integer, u32 hashing so WGSL can mirror it exactly; two octaves: swales of 3-8 blocks, roughness of about one block). It depends only on world (atlas) coordinates, so it has no seams at block, chunk, zone or tile edges. Largest dip 0.12 block (6 cm).
- **Per material**: `relief` and `soak` in `data/blocks.json`; built and glowing blocks are flat. The amount rides in the tile height texture's spare blue channel.
- **Edges pinned** by the corner ambient occlusion the mesher already packs, so ground meets walls and placed blocks without gaps.
- **Per world**: settings `"relief": {seed, amount, version}`; on for atlas tiles, opt-in for hand-made levels; no protocol change (settings travel in the welcome). `version` pins the function (golden tests).
- **Puddles from the weather**: `Weather::ground` (mm per column) = rain x (1 - soak) - evaporation, capped at the dips' capacity, sent in keyframes (`Keyframe::ground`, protocol 8). The block shader gets `wet` (darken, less rough) and `fill`: pixels deeper than (1 - fill) of the largest dip are water (flat, mirror-like, rain rings; ice below 0 C).

## Why per pixel first
Meshes merge neighbouring flat faces into big quads; geometric dips would unmerge every natural top face. Per pixel costs no vertices, no meshing and no collision change, and 6 cm reads through lighting and water, not silhouette. Geometric dips near the camera are phase 5, only if wanted.

## Rollout
1 the function, settings and block data, with golden and seam tests; 2 dry relief in `block.wesl`; 3 ground water in the weather (protocol 8); 4 puddles, rings and ice; 5 optional geometric dips.

## Open questions
The largest dip (set by eye); whether puddles do anything (splashes use real CC0 recordings); whether players can change relief; snow filling dips first.

Related: `architecture/world-scaling-design`, `architecture/weather-simulation`, `architecture/vox-world`.

# Related Concepts
- [Weather Simulation](weather-simulation.md): Ground water that fills the puddles is a new field of the weather simulation.
- [Design (proposed): Worlds at 60 km x 60 km](world-scaling-design.md): Relief is part of the generated terrain and follows its integer-only rules.
