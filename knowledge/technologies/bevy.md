---
type: Technology
title: "Bevy 0.20.1: How and Why It Is Used"
description: "What the project uses Bevy for, the version rule, and the engine quirks that have shaped the code."
tags: [technology, bevy, rendering, ecs]
generated: { by: agent/mcp, at: "2026-11-27T04:40:31Z" }
status: stable
code_refs: [Cargo.toml]
---

# Bevy 0.20.1

**Rule: stay on Bevy 0.20.1** (owner constraint; upgraded from 0.19.1 on 2026-11-27, see `decisions/bevy-0-20-upgrade`). `cpal` is kept at 0.17 to match the rodio backend that `bevy_audio` uses, so both share one ALSA/CoreAudio/WASAPI path. That is unchanged in 0.20.

## Used for
- The ECS app loop and schedules (Startup, Update, PostUpdate, FixedUpdate at 120 Hz).
- glTF loading and the animation graph.
- PBR materials, shadows (cascades for the sun), bloom, depth of field and tonemapping.
- Custom **WESL** materials in `assets/shaders/*.wesl`: retro, block, block_vertex, water, sky, floor, flame, portal, rain.
- Multiple cameras and render layers; the desktop window; on wasm32, the `webgpu` feature.
- Picking for menu hover and clicks (`Hovered`, `PointerClick`).

## Shaders (since 0.20)
Bevy 0.20 dropped the naga_oil preprocessor. Shaders are WESL:
- imports read `import bevy_pbr::render::forward_io::VertexOutput;` (every `bevy_pbr` module moved under `render::`);
- conditionals are `@if(X)`, `@elif` and `@else` on imports, statements or blocks, and they scope like blocks;
- bind groups use `@group(constants::MATERIAL_BIND_GROUP)`;
- the UI vertex output is in `bevy_ui_render`;
- `alpha_discard(flags, alpha_cutoff, colour)`;
- a custom vertex shader on standard meshes must `decompress_vertex` first.

## Quirks that shaped the code
- A system may take at most **16 parameters**, so resources are bundled into tuples.
- Face, joint and scale writes must happen in **PostUpdate after animation sampling** to win over clips.
- An **HDR main camera with a non-HDR overlay on the same window froze the window**, so bloom is limited and the overlay is handled separately.
- Software rendering (lavapipe) is used for CI and headless checks, at about 1 to 5 fps.
- Time is Bevy virtual time (clamped on slow frames), so effects slow down on stalled machines; the fixed 120 Hz clock compensates (`decisions/fixed-120hz-simulation`).
- Screen-space reflections need the deferred renderer, which the show's materials do not use.
- In the browser, the path used has no compute shaders, so the sky's environment light becomes flat ambient.
- `Tonemapping::None` is a full passthrough since 0.20; the old behaviour is `Tonemapping::Linear`.
- The old UI `Button` and `Interaction` are deprecated; use `Hovered` and pointer events, or `ui_widgets::Button`.

## Cargo detail
`bevy = "0.20.1"` is a caret requirement; `Cargo.lock` holds 0.20.1 exactly. A plain `cargo update` could move to a later 0.20.x patch. Use `=0.20.1` if a hard pin is needed.

See `architecture/avatar-app-runtime`, `architecture/rendering-and-output`, `architecture/vox-world`.

# Related Concepts
- [Decision: Upgrade to Bevy 0.20.1, and Stay There](../decisions/bevy-0-20-upgrade.md): The version rule, and what the upgrade from 0.19.1 took.
- [Bevy Ecosystem Crates: Use, Copy, Watch or Skip](bevy-ecosystem.md): Which ecosystem crates to use, copy, watch or skip.
