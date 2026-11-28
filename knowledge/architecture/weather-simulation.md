---
type: Architecture
title: Weather Simulation
description: "A small server-side atmosphere model whose keyframes drive haze, fog, clouds, rain, wind and the water standing on the ground in every client."
tags: [architecture, weather, simulation, vox-core]
generated: { by: agent/mcp, at: "2026-11-28T03:25:04Z" }
status: stable
code_refs: [crates/vox-core/src/weather.rs, crates/vox-world/src/outdoors.rs]
---

# Weather Simulation

Code: `crates/vox-core/src/weather.rs` (engine-free), with the visible half in `crates/vox-world/src/outdoors.rs` and `assets/shaders/block.wesl`.

## Model
- A grid of air cells, each 8 m across and 5 m tall (default 32 x 10 x 32), holding temperature, water vapour, cloud and fog water, rain, and wind in three directions. Weather works in metres, so the change to 0.25 m blocks didn't touch it.
- Each step:
  1. Forcing: wind is pulled towards the world's prevailing wind, slower near the ground on a log profile, with gusts from moving noise, and the ground warms or cools the air.
  2. Transport, condensation and precipitation.
  3. **Ground water**: per column, how full the ground's dips are (0-1). Rain fills them at a game's pace (2 mm/h fills them in about four minutes). Warm, dry, windy air dries them: about fifteen minutes on a mild day, and barely at all below 0 °C.
- A world starts with water in its dips from its climate's `wetness`.
- A level picks a climate preset: clear, haze, mist, fog, luminous, rain, storm, windy or snow.
- **Luminous**, the Void's preset, is a bright haze just short of saturation (humidity 0.98, since 2026-11-27). It was a saturated fog bank before, at 1.03. Combined with the sky's 450 m visibility and a thinner fog volume in `outdoors.rs`, you can see across the Void's builds (`architecture/quarter-metre-blocks-decision`).

## Network
- The **server** runs the simulation and sends keyframes (`ToClient::Weather`, droppable). From protocol 8 they carry the ground water (`Keyframe::ground`).
- Clients simulate locally between keyframes. On the desktop, the client steps a copy on a worker thread and swaps it in, so a step (about 2 ms) never holds up a frame.
- Clients may ask to change the weather: `world:weather:storm`, `world:wind:8:270`, `world:humidity:0.9`, `world:temperature:-3`.

## What it drives
- Visibility and distance haze.
- A volume of fog round the camera, at High quality. It is baked four rows a frame, about 0.6 ms a frame instead of 9 ms twice a second.
- Cloud cover, rain or snow, and the direction flames and water move.
- Wet ground and puddles in the ground's relief (`architecture/ground-relief-design`).
- A HUD line showing the air where you stand.

A storm gives about 2.9 mm/h of rain; the luminous Void stays dry and clear of fog banks.

## Tests
In `vox-core`:
- It settles to its climate.
- Rain falls from storms, not from clear air, and the luminous haze has no fog bank and no rain.
- Fog lowers visibility, and wind follows the preset.
- Keyframes round-trip, ground water included, and wrong lengths are refused.
- Bad climates are made sane.
- Rain fills puddles and dry air empties them; soaked climates start wet; frozen puddles barely dry.

# Related Concepts
- [Design (proposed): Ground Relief and Puddles](ground-relief-design.md): Ground water per column fills the puddles drawn in the generated relief.
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): the Void's thinner fog
