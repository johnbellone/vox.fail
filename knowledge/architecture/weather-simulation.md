---
type: Architecture
title: Weather Simulation
description: "A small server-side atmosphere model whose keyframes drive haze, fog, clouds, rain, wind and the water standing on the ground in every client."
tags: [architecture, weather, simulation, vox-core]
generated: { by: agent/mcp, at: "2026-11-27T14:07:57Z" }
status: stable
code_refs: [crates/vox-core/src/weather.rs, crates/vox-world/src/outdoors.rs]
---

# Weather Simulation

Code: `crates/vox-core/src/weather.rs` (engine-free) and the visible half in `crates/vox-world/src/outdoors.rs` and `assets/shaders/block.wesl`.

## Model
A grid of air cells (8 m across, 5 m tall; default 32 x 10 x 32) holding temperature, water vapour, cloud and fog water, rain, and wind in three directions. Each step: forcing (wind pulled to the world's prevailing wind, slower near ground on a log profile, gusts from moving noise; the ground warms or cools the air), then transport, condensation and precipitation, then **ground water**: per column, how full the ground's dips are (0-1). Rain fills them at a game's pace (2 mm/h fills them in about four minutes) and warm, dry, windy air dries them (about fifteen minutes on a mild day; barely below 0 C). A world starts with water in its dips from its climate's `wetness`. A level picks a climate preset: clear, haze, mist, fog, luminous, rain, storm, windy, snow.

## Network
The **server** runs the simulation and sends keyframes (`ToClient::Weather`, droppable); from protocol 8 they carry the ground water (`Keyframe::ground`). Clients simulate locally between keyframes; on the desktop the client steps a copy on a worker thread and swaps it in, so a step (about 2 ms) never holds up a frame. Clients may ask to change weather (`world:weather:storm`, `world:wind:8:270`, `world:humidity:0.9`, `world:temperature:-3`).

## What it drives
Visibility and distance haze, a volume of fog round the camera (High quality; baked four rows a frame, so it costs about 0.6 ms a frame instead of 9 ms twice a second), cloud cover, rain or snow, the direction flames and water move, wet ground and puddles in the ground's relief (`architecture/ground-relief-design`), and a HUD line showing the air where you stand. A storm gives about 2.9 mm/h of rain; the luminous void stays dry.

## Tests
In `vox-core`: it settles to its climate, rain falls from storms not from clear air, fog lowers visibility, wind follows the preset, keyframes round-trip (ground water included, wrong lengths refused), bad climates are made sane, rain fills puddles and dry air empties them, soaked climates start wet, frozen puddles barely dry.

# Related Concepts
- [Design (proposed): Ground Relief and Puddles](ground-relief-design.md): Ground water per column fills the puddles drawn in the generated relief.
