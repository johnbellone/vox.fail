---
type: Architecture
title: Weather Simulation
description: "A small server-side atmosphere model whose keyframes drive haze, fog, clouds, rain and wind in every client."
tags: [architecture, weather, simulation, vox-core]
generated: { by: agent/mcp, at: "2026-11-27T13:28:19Z" }
status: stable
code_refs: [crates/vox-core/src/weather.rs, crates/vox-world/src/outdoors.rs]
---

# Weather Simulation

Code: `crates/vox-core/src/weather.rs` (877 lines, engine-free) and the visible half in `crates/vox-world/src/outdoors.rs`.

## Model
A grid of air cells (8 m across, 5 m tall; default 32 x 10 x 32) holding temperature, water vapour, cloud and fog water, rain, and wind in three directions. Each step: forcing (wind pulled to the world's prevailing wind, slower near ground on a log profile, gusts from moving noise; the ground warms or cools the air), then transport, condensation and precipitation. A level picks a climate preset: clear, haze, mist, fog, luminous, rain, storm, windy, snow.

## Network
The **server** runs the simulation and sends keyframes (`ToClient::Weather`, droppable). Clients simulate locally between keyframes and round-trip them. Clients may ask to change weather (`world:weather:storm`, `world:wind:8:270`, `world:humidity:0.9`, `world:temperature:-3`).

## What it drives
Visibility and distance haze, a volume of fog baked round the camera (High quality), cloud cover, rain or snow, the direction flames and water move, and a HUD line showing the air where you stand. A storm gives about 2.9 mm/h of rain; the luminous void stays dry.

## Tests
Part of the 44 `vox-core` tests: it settles to its climate, rain falls from storms not from clear air, fog lowers visibility, wind follows the preset, keyframes round-trip, bad climates are made sane.

# Related Concepts
- [Design (proposed): Ground Relief and Puddles](ground-relief-design.md): Proposed: ground water per column, filling puddles in the generated relief.
