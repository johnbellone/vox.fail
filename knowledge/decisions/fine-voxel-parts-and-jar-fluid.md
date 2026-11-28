---
type: Decision
title: "Decision: Finer Voxel Parts and a Sloshing Fluid Material for Avatars"
description: "Avatar body parts may use voxels finer than 5 cm (the Brain's head and shirt use 1.25 cm), and a glTF material named VoxelFluid is drawn by a world-space slosh shader in both apps."
tags: [decision, avatars, voxels, shaders, brain]
generated: { by: agent/mcp, at: "2026-11-28T05:28:19Z" }
status: stable
---

# Decision

- `tools/assets/build_animals.py`: `Vox(res=N)` builds a part on a 5/N cm grid (one grid per part). Fine parts are greedy-merged to keep models under the 8 MB avatar limit; the build asserts under 40% of merged quad sides fall on the 5 cm grid, because block_avatar's bevel pass (`bevel/mesh.rs` `grid_of`) picks 5 cm when half the sides do.
- Patterned cloth (the Brain's plaid flannel) is painted per 1.25 cm voxel, not textured: the bevel pass and palette UVs stay unchanged.
- A model's fluid is its own primitive with the glTF material `VoxelFluid`. `vox_world::fluid::FluidPlugin` (block_avatar and vox-client) swaps it for `FluidMaterial` (`assets/shaders/fluid.wesl`): the fluid is cut at a world-space plane that moves with the jar, tilted by a damped spring fed by the jar's acceleration, with ripples, a meniscus and bubbles. The fluid is marked `NoBevel`, and `fluid::adopt` runs before `bevel::apply`.
- Fluid is tinted pale aqua, not green, so a green-screen key doesn't remove it.
- block_avatar `--snapshot-film <n>` saves n frames with time running (for reviewing motion).

# Why

The owner asked for more head detail, a plaid flannel shirt (finer voxels or texture) and sloshing fluid, shown as block_avatar renders on a green screen.
