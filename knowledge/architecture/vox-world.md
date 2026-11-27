---
type: Component
title: "vox-world: Drawing a World (Blocks, Sky, Portals, Players)"
description: "The Bevy crate that shows a Vox world on screen and talks to a world server; shared by the streaming app and the player client, native and WebAssembly."
tags: [component, vox-world, bevy, rendering, wasm]
generated: { by: agent/mcp, at: "2026-11-27T20:45:05Z" }
status: stable
code_refs: [crates/vox-world/src/, assets/shaders/]
---

# vox-world: Drawing a World (Blocks, Sky, Portals, Players, Controls)

Path `crates/vox-world/`, about 17,700 lines in 40 files. Depends on Bevy 0.20.1, `vox-core`, `vox-link`, `vox-art`, `vox-graph`, `png`, `postcard`; `tungstenite`/`vox-link` natively and `web-sys` WebSocket on wasm32.

| Module | Role |
| --- | --- |
| `client/` (10 files) | Split by concern: `link` and `web_link` (server link), `receive` (messages), `chunks` (chunk meshes), `controls`, `camera` (Diablo-style 4-45 m zoom or behind-the-shoulder, first person, fixed shots), `avatar` (other players from poses, 100 ms behind), `hud`, `commands`. |
| `input/` | Action layer: `bindings`, `devices` (detects gamepads), `hints` (button glyphs per brand: Xbox, PlayStation, Nintendo, Steam). |
| `hud.rs`, `menu.rs` | Pixel HUD (crosshair, hotbar, quick wheel) and controller-navigable menus. |
| `blocks.rs` | Block definitions from `data/blocks.json`, texture atlas, mesher (merged faces, **packed 20-byte vertices**, per-frame meshing budget 8 ms desktop / 5 ms browser), glass, water and flame materials. Chunk meshes live only on the GPU. |
| `outdoors.rs` | Sky and light in three styles chosen by the level: **earth**, **void**, **islands**; environment map, haze, 12 nearest lamps and fires, fog volume baked from weather, rain/snow. |
| `portals.rs` | Doorways to other places with an echo of the far level; walking through switches server. They are the world's portal entities. |
| `entities.rs` | `WorldEntities`: every entity as the server sends it, by id with the seq of its latest change; portals become the doorways `portals.rs` draws. |
| `props.rs` | Props on screen. A prop entity's blob is fetched by hash (`data/props/<hash>.voxp`), decoded and greedy-meshed (one flat-shaded mesh per hash, at most 256 kept, 50,000 quads each) within a per-frame budget. **Incremental showing**: `PropWatch` diffs only the entities whose seq changed, takes a changed one down and up again, queues new ones and spawns at most `SPAWN_CAP` a frame. Colours come from `data/props/palette.json` by the prop's `palette_version` (a fixed ramp stands in while it is missing). The world's `catalogue.json` says which props it accepts; others are **orphans**, drawn but taking no cells, as the server counts them. Colliding props put their cells in `PropOccupancy`, which walking reads beside the blocks. Lights join the nearest lamps in `outdoors`; motion and flicker read each client's own clock. |
| `editor/` | The in-world editor (edit mode, browser client only, added by `EditorPlugin`). `bridge`: the JSON `Command` and `Event` messages between the page and the client (serde, tagged by `type`; mirrored in `apps/site/src/game/editor.ts`, one sample of each in `editor.samples.json`; commands are size-capped and validated on arrival). `pick`: the voxel ray and ray tests on doorway frames and props find what the pointer is on. `ghost`: the translucent portal or prop preview, red where `vox_core::edit::check_put` refuses (only the Editor role is checked; the server decides). `gizmo`: move, turn (15 degree steps for props) and resize handles whose drag is sent as a `Set` on release. `camera`: edit flight at a speed the server's move check allows, and fly-to. `undo`: undo and redo of the author's own changes, sent as a change expecting the seq the step sits at so another's change is refused `Changed`; an undone Build sends `Unbuild`. `node`: node footprints as outlines and the selected node's preview, expanded with the same `vox-graph` code the server builds with, cached by seq and palette and meshed over several frames; over `MAX_BUILD_CELLS` there is no ghost and a Build is refused before it is sent. The page hears `Done` and `Refused` with the `ref` it chose. |
| `pose.rs`, `proto.rs` | The pose format and show protocol. See `architecture/show-networking`. |
| `files.rs` | Desktop reads from disk; the browser fetches the files a world needs. |

## Shaders (`assets/shaders/`)
`block.wesl` and `block_vertex.wesl` (parallax occlusion on the atlas; unpacks the packed vertex; repeats the tile across merged faces), `water.wesl`, `sky.wesl`, `floor.wesl`, `flame.wesl`, `portal.wesl`, plus the retro shader.

## Rules and limits
Anything drawn with `BlockMaterial` must use `into_packed_mesh`. A one-block edit still rebuilds its whole chunk; weather and fog baking run on the main thread. Echoes show the other level as built from its file, drawn a second time at half size. Wasm has no compute shaders in this path, so the sky environment light falls back to flat ambient. The server still sends every chunk on join. Static props are not merged per chunk, and named parts of a prop are not animated on their own yet.

# Related Concepts
- [Worlds, Levels and Blocks](worlds-catalogue.md): the levels and blocks it draws
- [Weather Simulation](weather-simulation.md): the weather it displays
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): browser build
- [Design (phases 1-4 built): Block-Game Controls, HUD and Menus](controls-and-hud-design.md): its input, HUD and menus
- [Decision: The World Editor](../decisions/world-editor.md): the editor it hosts
