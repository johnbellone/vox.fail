---
type: Component
title: "vox-world: Drawing a World (Blocks, Sky, Portals, Players)"
description: "The Bevy crate that shows a Vox world on screen and talks to a world server; shared by the streaming app and the player client, native and WebAssembly."
tags: [component, vox-world, bevy, rendering, wasm]
generated: { by: agent/mcp, at: "2026-11-28T03:24:20Z" }
status: stable
code_refs: [crates/vox-world/src/, assets/shaders/]
---

# vox-world: Drawing a World (Blocks, Sky, Portals, Players, Controls)

Path `crates/vox-world/`, about 18,000 lines in 41 files. Depends on Bevy 0.20.1, `vox-core`, `vox-link`, `vox-art`, `vox-graph`, `png` and `postcard`; on `tungstenite`/`vox-link` natively and the `web-sys` WebSocket on wasm32.

| Module | Role |
| --- | --- |
| `client/` (10 files) | Split by concern: `link` and `web_link` (server link), `receive` (messages), `chunks` (chunk meshes), `controls`, `camera` (Diablo-style 4-45 m zoom or behind-the-shoulder, first person, fixed shots), `avatar` (other players from poses, 100 ms behind), `hud`, `commands`. The player's box, reach and speeds come from `vox-core` in metres. |
| `detail.rs` | **Device profiles**: `Profile::{Desktop, Browser, Mobile}`, each giving a `Detail` with full-detail distance, LOD rings, echo radius and meshing budget per frame. Desktop is 128 m, 3 rings, 64 m echoes, 8 ms; browser 64 m, 3, 48 m, 5 ms; mobile 32 m, 2, 24 m, 3 ms. The client's `Start.detail` picks one, as does the page's `deviceDetail` (`mobile` for touch screens) and `vox-client --detail`. Systems read `Option<Res<Detail>>`, falling back to the platform's default (the avatar app inserts none). Full detail and rings are for streaming, which is not built yet (`architecture/chunk-streaming-design`). |
| `input/` | Action layer: `bindings`, `devices` (detects gamepads), `hints` (button glyphs per brand: Xbox, PlayStation, Nintendo, Steam). |
| `hud.rs`, `menu.rs` | Pixel HUD (crosshair, hotbar, quick wheel) and controller-navigable menus. |
| `blocks.rs` | Block definitions from `data/blocks.json` (whose `block_size` must be `vox_core::BLOCK_M`), the texture atlas, the mesher (merged faces, **packed 20-byte vertices**, meshing budget from the device profile), and the glass, water and flame materials. Chunk meshes live only on the GPU. `load_level_world` and `mesh_world` read a level as chunks, clipped to a region, never as a map of every block. |
| `outdoors.rs` | Sky and light in styles chosen by the level: **earth**, **void** (a bright haze, 450 m visibility), **islands**, **sea**, **orbit**. Also the environment map, haze, the 12 nearest lamps and fires, a fog volume baked from the weather, and rain and snow. |
| `portals.rs` | Doorways to other places, showing an echo of the far level; walking through switches server. They are the world's portal entities. An echo loads only the level within the device's echo radius of its arrival point. A doorway arriving far from another gets its own scene, and a scene no doorway shows is dropped with everything it spawned. |
| `entities.rs` | `WorldEntities`: every entity as the server sends it, by id with the seq of its latest change. Portals become the doorways `portals.rs` draws. |
| `props.rs` | Props on screen. See below. |
| `editor/` | The in-world editor (edit mode, browser client only). See below. |
| `pose.rs`, `proto.rs` | The pose format and show protocol. See `architecture/show-networking`. |
| `files.rs` | Desktop reads from disk; the browser fetches the files a world needs. |

## Props (`props.rs`)
- A prop entity's blob is fetched by hash (`data/props/<hash>.voxp`), decoded and greedy-meshed within a per-frame budget: one flat-shaded mesh per hash, at most 256 kept, 50,000 quads each.
- **Incremental showing**: `PropWatch` diffs only the entities whose seq changed. It takes a changed one down and up again, queues new ones, and spawns at most `SPAWN_CAP` a frame.
- Colours come from `data/props/palette.json` by the prop's `palette_version`; a fixed ramp stands in while it is missing.
- The world's `catalogue.json` says which props it accepts. Others are **orphans**: drawn, but taking no cells, as the server counts them.
- Colliding props put their cells (half-metre footprint cells, 2 × 2 blocks each) in `PropOccupancy`, which walking reads beside the blocks.
- Prop lights join the nearest lamps in `outdoors`. Motion and flicker read each client's own clock.

## Editor (`editor/`)
Added by `EditorPlugin`; browser client only. The page hears `Done` and `Refused` with the `ref` it chose.
- `bridge`: the JSON `Command` and `Event` messages between the page and the client. They are serde, tagged by `type`, and mirrored in `apps/site/src/game/editor.ts` (one sample of each in `editor.samples.json`). Commands are size-capped and validated on arrival.
- `pick`: the voxel ray, and ray tests on doorway frames and props (a prop picks by its half-metre cell), find what the pointer is on.
- `ghost`: the translucent portal or prop preview, red where `vox_core::edit::check_put` refuses. Only the Editor role is checked; the server decides.
- `gizmo`: move, turn (15 degree steps for props) and resize handles, with limits in metres. A drag is sent as a `Set` on release.
- `camera`: edit flight at a speed the server's move check allows, and fly-to.
- `undo`: undo and redo of the author's own changes. Each is sent expecting the seq the step sits at, so another player's change is refused `Changed`. An undone Build sends `Unbuild`.
- `node`: node footprints as outlines, and the selected node's preview. The preview is expanded with the same `vox-graph` code the server builds with, cached by seq and palette, and meshed over several frames. Over `MAX_BUILD_CELLS` there is no ghost, and a Build is refused before it is sent.

## Shaders (`assets/shaders/`)
- `block.wesl` and `block_vertex.wesl`: parallax occlusion on the atlas; unpack the packed vertex; repeat the tile across merged faces; relief lattices of 8 and 2 blocks (relief version 2).
- `water.wesl`, `sky.wesl`, `floor.wesl`, `flame.wesl`, `portal.wesl`, plus the retro shader.

## Rules and limits
- Anything drawn with `BlockMaterial` must use `into_packed_mesh`.
- A one-block edit still rebuilds its whole chunk.
- Weather and fog baking run on the main thread.
- Wasm has no compute shaders in this path, so the sky's environment light falls back to flat ambient.
- The server still sends every chunk on join; streaming, LOD rings from a far-field octree and a ray-marching prototype are planned in `architecture/chunk-streaming-design`.
- Static props are not merged per chunk, and named parts of a prop are not yet animated on their own.

# Related Concepts
- [Worlds, Levels and Blocks](worlds-catalogue.md): the levels and blocks it draws
- [Weather Simulation](weather-simulation.md): the weather it displays
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): browser build
- [Design (phases 1-4 built): Block-Game Controls, HUD and Menus](controls-and-hud-design.md): its input, HUD and menus
- [Decision: The World Editor](../decisions/world-editor.md): the editor it hosts
- [Chunks, streaming and draw distance](chunk-streaming-design.md): what comes next for its chunks
