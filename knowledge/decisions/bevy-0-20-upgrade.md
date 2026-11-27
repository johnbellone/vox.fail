---
type: Decision
title: "Decision: Upgrade to Bevy 0.20.1, and Stay There"
description: "On the owner's say-so (2026-11-27) the workspace moved from Bevy 0.19.1 to 0.20.1; further upgrades again need the owner's approval."
tags: [decision, bevy, dependencies, upgrade, wesl]
generated: { by: agent/mcp, at: "2026-11-27T04:40:31Z" }
status: stable
---

# Decision: Upgrade to Bevy 0.20.1, and Stay There

**Decision (the owner, 2026-11-27: "Do the upgrade")**: the workspace is on Bevy **0.20.1**, up from 0.19.1. The rule is unchanged in kind: stay on this version, and upgrade again only with the owner's say-so. This supersedes `decisions/bevy-0-19-1-pin`.

**What the upgrade took** (measured on a trial branch first):
- `bevy = "0.20.1"`; wgpu and naga 29 to 30; naga_oil is gone.
- The audio stack is unchanged (rodio 0.22, cpal 0.17.3), so the avatar app's `cpal = "0.17"` still matches.
- **Shaders to WESL**: all 9 `assets/shaders/*.wgsl` became `.wesl`:
  - `#import bevy_pbr::x` becomes `import bevy_pbr::render::x;`;
  - `#ifdef X` becomes `@if(X)`, with variables hoisted where an old `#ifdef` spanned scopes;
  - `#{MATERIAL_BIND_GROUP}` becomes `constants::MATERIAL_BIND_GROUP`;
  - the UI vertex output import moved to `bevy_ui_render`;
  - `alpha_discard` now takes the material flags and alpha cutoff;
  - the water vertex shader decompresses `Vertex` as Bevy's mesh shader does.
- **Menus**: the old `Button`/`Interaction` are deprecated. Hover now comes from picking's `Hovered`, and mouse choices from a `PointerClick` observer. Keyboard and controller still use Confirm on the focused item, so nothing fires twice.
- **Tonemapping**: `Tonemapping::None` became a full passthrough, so our five uses are now `Tonemapping::Linear`, which keeps the old look.
- Every `TextFont` already set its size, so 0.20's new default font size changes nothing.

**Verified**:
- The workspace and the browser client (wasm32) type-check.
- 290 tests pass.
- Clippy shows the same 94 warnings as before; the upgrade adds none.
- The worlds (sea, void, station, orbit sky) screenshot the same as the 0.19.1 build: mean difference under 1/255, with only animated details differing.
- The avatar app passes all 12 visual tests against references rendered by the 0.19.1 build on the same machine, the retro filter included.

**Consequences**: six crates from the ecosystem analysis have no 0.20 release yet: big_space, bevy_firework, bevy_svg, bevy_feronia, transform-gizmo-bevy and bevy_animation_graph. None was adopted, so nothing is lost, but adopting them waits on their 0.20 releases. New 0.20 features are now available (UI inline images, Feathers widgets, mesh shaders on desktop, BSN), none used yet.

See `technologies/bevy`, `technologies/bevy-ecosystem`, `convention/hard-constraints`.

# Related Concepts
- [Decision (superseded): Stay on Bevy 0.19.1](bevy-0-19-1-pin.md): The rule this supersedes.
- [Bevy Ecosystem Crates: Use, Copy, Watch or Skip](../technologies/bevy-ecosystem.md): The analysis that preceded the upgrade, and the crates still waiting on 0.20 releases.
