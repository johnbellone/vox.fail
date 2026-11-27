---
type: Analysis
title: "Bevy Ecosystem Crates: Use, Copy, Watch or Skip"
description: "Verdicts on 42 Bevy ecosystem crates for vox.fail as of 2026-11-26, the Bevy 0.20 upgrade question, and what belongs in the world format and a pack."
tags: [technologies, bevy, dependencies, analysis, ecosystem, world-format]
generated: { by: agent/mcp, at: "2026-11-27T03:55:11Z" }
status: draft
stale_after: 2027-01-26
---

# Bevy Ecosystem Crates: Use, Copy, Watch or Skip

Source: `docs/design/bevy-ecosystem.md` (checked 2026-11-26 against crates.io, docs.rs, each repository, and this repository's `Cargo.lock` and code). Recommendations only; nothing adopted yet.

## The upgrade question
Done on 2026-11-27, on the owner's say-so: the workspace is on Bevy 0.20.1 (`decisions/bevy-0-20-upgrade`). The analysis had advised waiting, because six crates (big_space, bevy_firework, bevy_svg, bevy_feronia, transform-gizmo-bevy, bevy_animation_graph) have no 0.20 release. None of them was adopted, so nothing was lost; adopting them now waits on their releases.

## Use directly (on 0.19.1)
- bevy_seedling 0.8: the audio standard, with buses for props, ambience and voice.
- bevy_mod_outline 0.13.1: prop and avatar outlines.
- bevy_vector_shapes 0.13.1: the quick wheel and world markers.
- bevy_firework 0.10: CPU particles for weather and fire, if the look fits.
- bevy_hanabi 0.19: GPU particles, in the avatar app only.
- bevy_lookup_curve 0.12 (optional): RON curves.
- bevy_egui 0.42 and transform-gizmo-bevy 0.11: behind an editor feature.
- big_space 0.13 (later): a floating origin for big worlds, client only.
- `bevy_reflect` alone, in `vox-format`.
- bytemuck (already in the tree).
- The Bevy-free rerecast core comes later, in the server.

## Copy the pattern
- big-brain: utility AI in `vox-core`.
- bevy_proto: template inheritance, resolved when the pack is built.
- bevy_console: one typed command table.
- bevy_animation_graph: a RON state table on Bevy's own `AnimationGraph`.
- tnua: coyote time, jump buffering, variable jump and smooth step-up in `vox-core::body`.
- moonshine_save: save only marked components; rebuild what is drawn on load.
- `DynamicWorld`: our own entity blob.
- bevy_materialize: material inheritance in the pack schema.
- bevy_feronia: height-weighted wind sway from `vox-core::weather`.
- bevy_enoki: RON effect files.
- voronator and noise: our own deterministic `vox-core::noise` and jittered-grid cells, with golden tests on native and wasm, replacing the two copies.
- dot_vox: our own hardened `.vox` reader, with dot_vox as the test oracle.

## Skip
- In-game scripting (bevy_mod_scripting is not wasm and not server-side; Rune; mlua). Rhai is on watch for trusted tools only.
- bevy_kira_audio and bevy_easings (Bevy's own curves cover easing).
- The three camera crates.
- Avian and avian_pickup for players: physics stays engine-free and shared with the Bevy-less server.
- bevy_svg (rasterise offline instead) and bevy_vox_scene.
- hexasphere, encase and naga as direct dependencies (they come through Bevy).
- meshopt and basis-universal in the client (C++), inline-spirv, cubecl.

## Finding that changed a design
`DynamicWorld` lives in `bevy_world_serialization`, which pulls in 29 Bevy and wgpu crates (`bevy_camera`, `bevy_mesh`, `bevy_image`, `bevy_window`, `wgpu-types`). `world-format.md` now uses our own `RegionEntities` postcard blob with a version per component, and `vox-format` depends on `bevy_reflect` alone (see `architecture/world-format-design`).

## For the world format and a pack
- The pack is our own manifest plus content-addressed blobs.
- Region entity blobs.
- Definitions, resolved when the pack is built: PropDef, creature defs, sound cues, particle effects, materials with inheritance, curves, and a character pack's rig state table.
- Noise parameters and a generator version.
- Navmesh tiles per region.
- Textures as PNG or zstd KTX2.
- Meshes as packed arrays under zstd.

## Constraints that shaped it
- The browser client is WebGPU-only.
- No threads in wasm, so seedling's threaded web backend is out for now.
- Voices may be placed in space but never changed (no effects on the voice bus).
- The server has no Bevy.

Related: `technologies/bevy`, `decisions/bevy-0-19-1-pin`, `architecture/world-format-design`, `architecture/prop-pipeline-design`.
