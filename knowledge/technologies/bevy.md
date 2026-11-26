---
type: Technology
title: "Bevy 0.19.1: How and Why It Is Used"
description: "What the project uses Bevy for, the version rule, and the engine quirks that have shaped the code."
tags: [technology, bevy, rendering, ecs]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
code_refs: [Cargo.toml]
---

# Bevy 0.19.1

**Rule: stay on Bevy 0.19.1** (owner constraint; see `decisions/bevy-0-19-1-pin`). `cpal` is kept at 0.17 to match the rodio backend that `bevy_audio` uses, so both share one ALSA/CoreAudio/WASAPI path.

## Used for
ECS app loop and schedules (Startup, Update, PostUpdate, FixedUpdate at 120 Hz); glTF loading and the animation graph; PBR materials, shadows (cascades for sun), bloom, depth of field, tonemapping; custom WGSL materials (retro, block, water, sky, floor, flame, portal); multiple cameras and render layers; the desktop window; on wasm32, the `webgpu` feature.

## Quirks that shaped the code
- A system may take at most **16 parameters**, so resources are bundled into tuples.
- Face, joint and scale writes must happen in **PostUpdate after animation sampling** to win over clips.
- An **HDR main camera with a non-HDR overlay on the same window froze the window**; bloom is limited and the overlay handled separately.
- Software rendering (lavapipe) is used for CI and headless checks: about 1-5 fps.
- Time is Bevy virtual time (clamped on slow frames), so effects slow down on stalled machines; the fixed 120 Hz clock compensates (`decisions/fixed-120hz-simulation`).
- Screen-space reflections need the deferred renderer, which the show's materials do not use.
- In the browser, no compute shaders in the path used, so sky environment light becomes flat ambient.

## Cargo detail
`bevy = "0.19.1"` is a caret requirement; `Cargo.lock` holds 0.19.1 exactly. A plain `cargo update` could move to a later 0.19.x patch. Use `=0.19.1` if a hard pin is needed.

See `architecture/avatar-app-runtime`, `architecture/rendering-and-output`, `architecture/vox-world`.

# Related Concepts
- [Decision: Stay on Bevy 0.19.1](../decisions/bevy-0-19-1-pin.md): the version rule
