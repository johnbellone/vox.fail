---
type: Decision
title: "Decision: Props Are a Third Kind of Thing, Beside Blocks and Structures"
description: "A prop has its own finer voxel grid, behaviours and versioned entity form; it is neither a block in chunk data nor a structure of blocks."
tags: [decision, props, structures, blocks, world-format]
generated: { by: agent/mcp, at: "2026-11-27T03:35:24Z" }
status: stable
---

# Decision: Props Are a Third Kind of Thing, Beside Blocks and Structures

**Decision (proposed with the design, 2026-11-26)**: a world holds three kinds of thing. **Blocks** (0.5 m cells with atlas textures, the only thing in chunk data) for terrain, walls and floors. **Structures** (versioned blueprints of blocks, with an owner and grants) for houses and bridges. **Props** (small objects with their own 5 cm or 10 cm voxel grid, a flat palette and declared behaviours) for what is in the house. Instances of structures and props are entities pinned to a version hash.

**Why**: making props out of blocks would force every lamp and mug to 0.5 m and cannot carry behaviours or moving parts; making them structures would inherit block-edit semantics (players reshaping them) that do not fit a lever or a door. A separate kind lets props mesh and batch on their own, keep blocks pure for the server's rules, and reuse the avatar props' format and look.

**Consequences**: props need a collision layer beside blocks (prop occupancy) and their own placement checks; they are never edited voxel by voxel; a player-made prop is the same kind of directory record as a structure, so `vox_core::entitle` is the one rule for who may use, copy and edit either.

See `architecture/prop-definition`, `architecture/props-in-worlds` and `decisions/structure-entitlements`.
