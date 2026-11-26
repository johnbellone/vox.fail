---
type: Component
title: "vox-world: Drawing a World (Blocks, Sky, Portals, Players)"
description: "The Bevy crate that shows a Vox world on screen and talks to a world server; shared by the streaming app and the player client, native and WebAssembly."
tags: [component, vox-world, bevy, rendering, wasm]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
code_refs: [crates/vox-world/src/, assets/shaders/]
---

# vox-world: Drawing a World (Blocks, Sky, Portals, Players, Controls)

Path `crates/vox-world/`, 8,732 lines in 23 files, 48 tests. Depends on Bevy 0.19.1, `vox-core`, `vox-link`, `png`, `postcard`; `tungstenite`/`vox-link` natively and `web-sys` WebSocket on wasm32.

| Module | Role |
| --- | --- |
| `client/` (10 files) | Split by concern: `link` and `web_link` (server link), `receive` (messages), `chunks` (chunk meshes), `controls`, `camera` (Diablo-style 4-45 m zoom or behind-the-shoulder, first person, fixed shots), `avatar` (other players from poses, 100 ms behind), `hud`, `commands`. |
| `input/` | Action layer: `bindings`, `devices` (detects gamepads), `hints` (button glyphs per brand: Xbox, PlayStation, Nintendo, Steam). |
| `hud.rs`, `menu.rs` | Pixel HUD (crosshair, hotbar, quick wheel) and controller-navigable menus. |
| `blocks.rs` | Block definitions from `data/blocks.json`, texture atlas, mesher (merged faces, **packed 20-byte vertices**, per-frame meshing budget 8 ms desktop / 5 ms browser), glass, water and flame materials. Chunk meshes live only on the GPU. |
| `outdoors.rs` | Sky and light in three styles chosen by the level: **earth**, **void**, **islands**; environment map, haze, 12 nearest lamps and fires, fog volume baked from weather, rain/snow. |
| `portals.rs` | Doorways to other places with an echo of the far level; walking through switches server. |
| `pose.rs`, `proto.rs` | The pose format and show protocol. See `architecture/show-networking`. |
| `files.rs` | Desktop reads from disk; the browser fetches the files a world needs. |

## Shaders (`assets/shaders/`)
`block.wgsl` and `block_vertex.wgsl` (parallax occlusion on the atlas; unpacks the packed vertex; repeats the tile across merged faces), `water.wgsl`, `sky.wgsl`, `floor.wgsl`, `flame.wgsl`, `portal.wgsl`, plus the retro shader.

## Rules and limits
Anything drawn with `BlockMaterial` must use `into_packed_mesh`. A one-block edit still rebuilds its whole chunk; weather and fog baking run on the main thread. Echoes show the other level as built from its file, drawn a second time at half size. Wasm has no compute shaders in this path, so the sky environment light falls back to flat ambient. The server still sends every chunk on join.

# Related Concepts
- [Worlds, Levels and Blocks](worlds-catalogue.md): the levels and blocks it draws
- [Weather Simulation](weather-simulation.md): the weather it displays
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): browser build
- [Design (phases 1-4 built): Block-Game Controls, HUD and Menus](controls-and-hud-design.md): its input, HUD and menus
