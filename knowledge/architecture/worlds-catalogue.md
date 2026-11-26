---
type: Fact
title: "Worlds, Levels and Blocks"
description: "The level file format, the block palette, the three shipped worlds and how portals connect them."
tags: [worlds, levels, blocks, portals]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [data/levels/, data/blocks.json, assets/blocks/tiles/, tools/assets/build_void.py, tools/assets/build_islands.py, tools/assets/build_blocks.py]
---

# Worlds, Levels and Blocks

## Level file (`data/levels/*.json`, reader in `vox-core/level.rs`)
`{ "name", "about", "world": {...}, "boxes": [{block, from, to, hollow?, round?}], "blocks": [[x,y,z,"name"]] }`. Coordinates are in block units; (0,0,0) is where the character stands, the floor's top is `y = 0`, `-z` is away from the camera. Later boxes overwrite earlier ones; `"air"` cuts holes. The `"world"` object chooses sky (`earth|void|islands`), time, weather, clouds, camera, fixed-shot "cameras" with zones, hotbar and `"portals"` (where, which way, how big, which level, which server, matching point on the far side). `":7781"` means another port on the same host.

User levels go in `levels/` in the user folder (`set:<file>` in the avatar app). A point-and-click editor is the planned next step.

## Blocks (`data/blocks.json`, tiles in `assets/blocks/tiles/`)
Original 16: grass, dirt, stone, cobblestone, bricks, planks, log, sand, gravel, snow, leaves, glass, concrete, metal, tile, lamp (glows). Added for worlds: water, obsidian, basalt, magma, ash, crystal, fire, pearl, lumen. Each names tiles for top/side/bottom, roughness, metal, bump depth, glass or glow. Textures are 128x128, original, drawn by `tools/assets/build_blocks.py`, seamless.

## Shipped worlds
- **The Void** (`void.json`, made by `tools/assets/build_void.py`): luminous, flat, endless floor; arrival Circle; First Gate (north, an echo of the yard); Ember Door (west, shows the islands).
- **The yard** (`yard.json`): back-yard set, earth sky; doorway back.
- **The islands** (`islands.json`, 2.25 MB, `tools/assets/build_islands.py`): the Threshold, Ember Wastes, Crystal Hollow, Shattered Keep, The Dark, Magma Falls, bridges, 44 islets.

## Portals
Walk towards a doorway and an echo fades in; walk through and you arrive at the matching point, facing on. If nobody answers on the far side you are put back (refused, or after 20 s). `world:portal:<name>` goes through by command. Player-opened portals (a feat) are merged into settings that new arrivals receive.

## Backlog world
A January 6 Capitol world is on the backlog but **not started**, and is blocked on open content and platform-rule questions; see `convention/hard-constraints` and `status/roadmap`.

# Related Concepts
- [Vox Online: Player Journey and Worlds](../project/vox-online.md): how players reach the worlds
