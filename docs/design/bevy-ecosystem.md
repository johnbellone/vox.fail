# Analysis: Bevy ecosystem crates for vox.fail

Status: **analysis** · 2026-11-26, **updated 2026-11-27: the workspace is now on Bevy 0.20.1** (the
owner approved the upgrade after a trial; see *The upgrade question*) · checked against crates.io, docs.rs and each project's
repository on this date, and against this repository's `Cargo.lock` and code. Nothing here is
adopted yet; the verdicts are recommendations.

Related: [world-format.md](world-format.md) (entities, packs), [props.md](props.md) (props,
behaviours, VOXP), [world-graph-and-art.md](world-graph-and-art.md),
[world-scaling.md](world-scaling.md), [world-network.md](world-network.md),
[controls-and-hud.md](controls-and-hud.md).

## Summary

Forty-two crates and tools, in six areas: AI and scripting; audio, animation and curves; cameras,
physics, large worlds and navigation; materials, effects, outlines and vector graphics; saving,
editor tools, UI, file formats and the ECS; GPU plumbing, geometry, noise and compression.

- **Stay on Bevy 0.19.1 for now; plan the upgrade to 0.20 for later.** Bevy 0.20.0 came out on
  2026-10-08 and 0.20.1 on 2026-10-28. No crate on the list *needs* 0.20: every one we would use
  has a 0.19 release. Upgrading today would *lose* six of them (big_space, bevy_firework,
  bevy_svg, bevy_feronia, transform-gizmo-bevy and bevy_animation_graph have no 0.20 release yet)
  and costs a shader migration (below). Upgrading is the owner's call; the owner has said it is
  not ruled out.
- **Use directly** (eleven, on 0.19.1): bevy_seedling (audio), bevy_mod_outline,
  bevy_vector_shapes, bevy_firework (if its look fits), bevy_hanabi (avatar app only),
  bevy_lookup_curve (optional), bevy_egui and transform-gizmo-bevy (editor builds only),
  big_space (when worlds outgrow a few kilometres), `bevy_reflect` alone (in `vox-format`) and
  bytemuck (already in the tree). The core of rerecast, which has no Bevy, comes later, in the
  server.
- **Copy the pattern** (twelve): big-brain (utility AI in `vox-core`), bevy_proto (template
  inheritance), bevy-console (one typed command table), bevy_animation_graph (a state table on
  Bevy's own `AnimationGraph`), tnua (movement feel in `vox-core::body`), moonshine_save (save
  marked entities, model apart from view), `DynamicWorld` (our own entity blob, see below),
  bevy_materialize (material inheritance), bevy_feronia (wind sway), bevy_enoki (effect files),
  voronator and noise (our own deterministic noise and cells), dot_vox (our own hardened reader,
  with dot_vox as the test oracle).
- **Skip**: scripting in the game (bevy_mod_scripting, Rune, mlua; Rhai only for trusted tools,
  if ever), bevy_kira_audio, bevy_easings, the three camera crates, Avian and avian_pickup for
  players, bevy_svg, bevy_vox_scene, hexasphere/encase/naga/inline-spirv as direct dependencies,
  meshopt and basis-universal in the client (C++), cubecl.
- **A correction to world-format.md**: `DynamicWorld` lives in `bevy_world_serialization`, which
  in our lockfile pulls in 29 Bevy and wgpu crates (`bevy_camera`, `bevy_mesh`, `bevy_image`,
  `bevy_window`, `wgpu-types` among them). So `vox-format` cannot use it and stay light, and the
  server cannot use it and stay free of Bevy. The fix: our own small entity blob (below).

## The upgrade question

**Done, 2026-11-27.** The owner said to upgrade; a trial branch measured it first, and it was
smaller than the table below feared:

- **Compile**: one line broke (the menus' `Button`). Everything else built.
- **Menus**: moved off the deprecated `Interaction` to picking's `Hovered` and a click observer.
- **Shaders**: the 9 became WESL; `alpha_discard` gained arguments; the water vertex shader
  decompresses `Vertex`.
- **Tonemapping**: `Tonemapping::None` became `Linear` to keep the look.

The worlds screenshot the same as on 0.19.1, the avatar app passes all 12 visual tests against
0.19.1 references, there are 290 tests and the same clippy warnings as before, and the browser
client type-checks. The six crates below without 0.20 releases weren't adopted, so nothing was
lost; adopting them now waits on their releases. The original analysis follows.

| | Stay on 0.19.1 | Upgrade to 0.20.1 now |
| --- | --- | --- |
| Crates on this list we would use | all have a 0.19 release | big_space (only an rc), bevy_firework, bevy_svg, bevy_feronia (already git-only for 0.19), transform-gizmo-bevy, bevy_animation_graph: no 0.20 release yet |
| Crates gained | none needed | newer releases of seedling, outline, vector_shapes, hanabi, egui, lookup_curve, moonshine_save, bevy_vox_scene (all have 0.19 releases too) |
| Cost | none | all 9 shaders in `assets/shaders/` use naga_oil's `#import`, `#ifdef` and `#{MATERIAL_BIND_GROUP}`, which 0.20 replaces with WESL (`import …;`, `@if`, `constants::…`); `bevy_pbr` module paths moved (`bevy_pbr::forward_io` to `bevy_pbr::render::forward_io` and so on); wgpu and naga 29 to 30; text and fonts (`TextFont` defaults, `FontSource`), pointer events, weak ordering of built-in schedules, several mesh and reflection renames |
| What 0.20 brings | | mesh shaders (voxels named as a use; **not on the web**), the scene notation (BSN) stabilised, better texture compression, a pan-orbit camera, UI widgets, Solari path tracing |

Where it touches us: Bevy is used in `vox-world` (17 files, about 9,000 lines), the avatar app (31
files, about 24,000 lines) and the client. The server, the directory, `vox-core` and
`vox-format` don't use Bevy and are untouched.

**Recommendation**: upgrade as its own planned step when (a) the crates we adopt have 0.20
releases (watch big_space, bevy_firework, transform-gizmo-bevy), and (b) there is a reason in
Bevy itself (mesh shaders for desktop chunks, BSN for the world format's entities, the UI
widgets). Do the shader move to WESL first, on its own, with the visual tests as the check.

## Verdicts

Bevy column: the release that supports 0.19 / the newest Bevy the crate supports.
"Not Bevy" means the crate is engine-independent.

### AI, scripting, console, data-driven entities

| Crate | Bevy (0.19 / newest) | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| big-brain | none / 0.17 (git) | Apache-2.0 | likely | **Copy the pattern** | Stale; scorers are Bevy systems, so it can't run in the Bevy-free server, where creatures are decided. Write a small utility scorer in `vox-core`: inputs, scores, the best action, with hysteresis, seeded per instance |
| bevy_mod_scripting | 0.21.x / 0.20 | MIT/Apache | **no** | **Skip** | Not wasm, Bevy-only, and broad reflective access to the world: against props.md's "behaviours are data, never code" |
| Rhai | not Bevy | MIT/Apache | yes | **Watch** | The best language if one is ever needed, and then only for trusted tools (`vox-forge` macros), with limits on and `unchecked` off |
| Rune | not Bevy | MIT/Apache | partial | **Skip** | Pre-1.0, fewer safety limits than Rhai |
| mlua | not Bevy | MIT | awkward (C) | **Skip** | C dependency; sandbox for Luau only |
| bevy_console | 0.18 / 0.20 | MIT | maybe | **Copy the pattern** | Adds egui and clap. Merge the avatar app's and the client's commands into one typed command table with help, parsed the same from HTTP, chat and a debug overlay |
| bevy_proto | none / 0.11 | MIT/Apache | | **Copy the pattern** | Dead since 2023, and keyed on Rust type paths. Copy template inheritance for PropDef variants and creature defs, resolved when the pack is built |

### Audio, animation, curves

| Crate | Bevy | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| bevy_seedling (Firewheel) | 0.8 / 0.20 | MIT/Apache | yes (cpal); the better web backend needs threaded wasm | **Use directly** | The audio standard for worlds and the avatar app: buses (props, ambience, voice), sampler pools, spatial audio, custom nodes (an ADPCM voice node from `vox-voice`). Replaces `bevy_audio` in one change. Measure browser latency first |
| bevy_kira_audio | 0.26 / 0.20 | MIT/Apache | yes | **Skip** | Weak spatial audio; seedling covers more |
| bevy_animation_graph | 0.11 / 0.19 (0.20 on git) | MIT/Apache | ? | **Copy the pattern** | One maintainer, assets may break between versions. Keep Bevy's own `AnimationGraph`; add a RON state table and `AnimationMask` for wave-while-walking |
| bevy_lookup_curve | 0.12 / 0.20 | MIT/Apache | likely | **Use directly** (optional) | Hand-made curves as RON assets: door overshoot, portal sweep timing, gusts. Implements Bevy's `Curve<f32>` |
| bevy_easings | 0.19 / 0.20 | MIT/Apache | likely | **Skip** | Duplicates Bevy's own `EaseFunction` and `EasingCurve`, and runs on frame time; prop motion must be sampled from the world clock |

The no-synthetic-voices rule applies to audio: a voice may be placed in the world (gain,
panning, distance) but never changed (no pitch, formant, reverb or distortion on the voice
bus). Effects go on the ambience and prop buses only.

### Cameras, physics, large worlds, navigation

| Crate | Bevy | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| bevy_third_person_camera | none / 0.18 | MIT/Apache | ? | **Skip** | Our cameras already have more (Diablo, behind, first person, forced shots, zones). Add wall avoidance ourselves with `vox-core::ray` |
| bevy_pancam | 0.21.1 / 0.20 | MIT/Apache | likely | **Watch** | Only if we add an orthographic map |
| bevy_flycam | 0.19 / 0.19 | **ISC** | ? | **Skip** | We fly already; a debug camera is about 50 lines |
| bevy-tnua (+ Avian 0.7) | 0.32 & 0.12.x / 0.20 | MIT/Apache | likely | **Copy the pattern** | Player physics stays engine-free in `vox-core` (the server has no Bevy; prediction needs the same rules on both sides). Copy the feel: coyote time, jump buffering, variable jump height, smooth step-up, crouch at edges; `moves` allows the same |
| Avian 3D | 0.7 / 0.20 | MIT/Apache | likely | **Skip** (players), **Watch** (cosmetic) | Only ever for client-side debris; not now |
| big_space | 0.13 / 0.19 (0.20 only an rc) | MIT/Apache | likely | **Use directly** (later) | Floating origin for 60 km worlds, client only. Make a big_space cell one of our zones, and add an engine-free zone-key-plus-offset position to `vox-core` that the server shares |
| avian_pickup | 0.6 / 0.20 | MIT/Apache | untested | **Skip** | Carrying is a server rule; a throw's arc is cosmetic |
| rerecast | not Bevy (`no_std`, glam 0.33) | MIT/Apache | likely | **Watch**, then use the core in the server | Navmeshes for NPCs, built in the server or a tool. Start with A\* on cells (exact with our collision); rerecast for open areas. Its glam 0.33 differs from Bevy 0.19's 0.32.1, which is fine in the Bevy-free server |

### Materials, effects, outlines, vector graphics

The browser client is WebGPU-only (`bins/vox-client/Cargo.toml` enables `webgpu`), and
`vox-world` already treats compute as optional on browsers that lack it.

| Crate | Bevy | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| bevy_materialize | 0.11 / 0.20 | MIT/Apache | likely | **Copy the pattern** | One material per block would break the atlas `BlockMaterial`. Copy inheritance and typed properties into the pack's material schema |
| bevy_feronia | git only / 0.19 | MIT/Apache | risky (compute culling) | **Copy the pattern** | Wind sway weighted by height, one wind uniform from `vox-core::weather`, for pines, reeds, kelp, banners and grass |
| bevy_firework | 0.10 / 0.19 | MIT/Apache | yes (WebGL too) | **Use directly** (if the look fits) | The CPU particle layer for worlds: rain, snow, spray, embers, sparks. Else grow the avatar app's own particles into a shared module |
| bevy_enoki | 0.7 / 0.20 | **MIT** | yes | **Copy the pattern** | 2D only; copy its RON effect files with hot reload as the format for our particle descriptors |
| bevy_hanabi | 0.19 / 0.20 | MIT/Apache | WebGPU only (compute) | **Use directly** (avatar app); **Watch** (worlds) | GPU particles for the stream's confetti and fireworks; in worlds only behind a compute check with a CPU fallback |
| bevy_mod_outline | 0.13.1 / 0.20.1 | MIT/Apache | WebGPU yes | **Use directly** | Prop and avatar outlines (hover, selection, stream readability). Blocks keep their own cube highlight (the packed vertex has no normals for extrusion) |
| bevy_svg | 0.19 / 0.19 | MIT/Apache | ? | **Skip** | The HUD is pixel art; rasterise SVG button hints to atlases at build time |
| bevy_vector_shapes | 0.13.1 / 0.20 | MIT/Apache | yes | **Use directly** | The quick wheel, cooldown and health arcs, portal rings, plot borders; draw through its low-resolution canvas with nearest sampling to keep the pixel look |

### Saving, editor tools, UI, formats, the ECS

| Item | Bevy | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| moonshine_save | 0.7 / 0.20 | MIT | likely | **Copy the pattern** | Saves whole worlds; we need zone blobs, migrations and a limited registry. Copy: a marker for what is saved, the model apart from what is drawn, drawn parts rebuilt on load |
| transform-gizmo-bevy | 0.11 / 0.19 | MIT/Apache | likely | **Use directly** (editor feature) | Move, rotate, scale for placing props and structures in an editor build; snaps to 0.5 m and 5 cm |
| bevy_egui | 0.42 / 0.20 | MIT | yes | **Use directly** (dev and editor features) | Inspectors, budget readouts, a snapshot browser, editor panels. Players keep the pixel HUD and menus |
| dot_vox | not Bevy | MIT | yes | **Copy the pattern** (tools only) | Props' Lane B reads files from players: write our own hardened `.vox` reader (sizes, a voxel cap, no recursion), fuzzed and tested on the 143 existing files; dot_vox as the test oracle and in `vox-forge` |
| bevy_vox_scene (`vox_bake`) | 0.22 / 0.20 | MIT | ? | **Skip** (read it) | Its PBR atlases conflict with the palette rule; read its meshing; watch `vox_bake` |
| glTF, OBJ, FBX and other loaders | | | | **Skip** | Bevy's glTF loader serves the avatar app; the rest aren't needed |
| `bevy_ecs` / `bevy_reflect` alone | 0.19.1 | MIT/Apache | yes | **Use `bevy_reflect` alone** in `vox-format` | For the component types, stable names (`#[reflect(type_path = "vox::Portal")]`) and a limited registry. Not `bevy_world_serialization`. The server stays without an ECS for now |

### GPU plumbing, geometry, noise, compression

Bevy 0.19.1 already brings wgpu and naga 29.0.4, encase 0.12.1, bytemuck 1.25.2 and hexasphere
18; Bevy 0.20 moves to wgpu and naga 30 and hexasphere 19.

| Crate | In the tree via Bevy | Licence | wasm | Verdict | Where, and why |
| --- | --- | --- | --- | --- | --- |
| hexasphere | 18.0.0 | MIT/Apache | yes | **Skip** (use Bevy's `Sphere::mesh().ico()`) | The orbit planet is better analytic in the sky shader |
| meshopt | no | MIT/Apache | **C++** | **Watch** (native tools) | Only for far-zone and prop LOD in `vox-forge`; zstd over packed vertices already does most of it |
| voronator | no | MIT | yes | **Copy the pattern** | A global triangulation doesn't stream. Use jittered-grid cells from an integer hash (a zone needs only its 3 × 3 cells) |
| noise | no | MIT/Apache | yes | **Skip; write our own** | Dormant; old `rand`; output could change with versions; trigonometry may differ between native and wasm. One `vox-core::noise` (integer hash, value, gradient, fBm, cells), basic arithmetic only, golden tests on both targets; replaces the two copies in `weather.rs` and `outdoors.rs` |
| cubecl | no | MIT/Apache | WebGPU only | **Skip** | Needs wgpu 30 (Bevy 0.20); GPU results differ by vendor so they can't decide anything the server needs |
| bytemuck | 1.25.2 | Zlib/MIT/Apache | yes | **Use directly** | The packed vertex, zone and VOXP blobs without hand shifts; the same copy as Bevy's |
| encase | 0.12.1 | MIT-0 | yes | **Through Bevy only** | `ShaderType` derive; a direct 0.13 would be a second, incompatible copy |
| inline-spirv | no | MIT/Apache | C++ or old naga | **Skip** | We write WGSL; SPIR-V is no use in the browser |
| naga | 29.0.4 | MIT/Apache | yes | **Skip** as a direct dependency | If a shader-validation test is ever added, pin it to Bevy's exact version, as a dev-dependency |
| basis-universal | no (optional in Bevy) | MIT/Apache | **C++**, dormant since 2023 | **Skip** | Ship tiles as PNG or zstd-supercompressed KTX2 (Bevy reads it in pure Rust); block compression would spoil the pixel tiles |

## What belongs in the world format and a pack

Gathered from all six areas. A **pack** is our own manifest and content-addressed blobs, the same
scheme as snapshots, not a glTF or Bevy scene; `.vox` and `.glb` are only ways in and out.

- **The manifest** (`pack.json` or postcard): format and palette versions, the level graph, region
  hashes, structure and prop blob hashes, the component registry version, the generator version,
  dependencies on other packs.
- **Zone entities**: our own postcard blob, `ZoneEntities { entities: [{ id, components:
  [(stable_name, version, bytes)] }] }`, with entity links as local ids inside the blob and a
  version per component so migrations run before anything reaches an ECS (instead of
  `DynamicWorld`). The world server and `vox-format` stay free of Bevy's render crates; the
  client turns the blob into Bevy entities in `vox-world`.
- **Saved versus drawn**: only marked components are saved; what is drawn (meshes, particles,
  lights) is rebuilt from them on load (moonshine_save's model and view).
- **Definitions** (validated, closed vocabularies, resolved at build time so a server never chases
  templates): PropDef (with template inheritance), creature defs (stats, behaviours such as
  wander, chase, flee, guard; score curves), sound cues (recording, bus, loop or on use, gain,
  range), particle effects (emitter shape, rate, lifetime, colour and size curves, a voxel
  shape, wind coupling), materials (roughness, metallic, glow, `inherits`, typed properties),
  curves (knots, as bevy_lookup_curve's RON), and a character pack's rig state table (states,
  clips, masks, crossfades, footstep events).
- **Generation**: noise parameters (seed, octaves, lacunarity and gain as rationals, cell size and
  jitter, a biome table) and a `noise_version`, so zones not yet generated stay reproducible.
- **Navigation**: navmesh tiles per zone as hashed blobs, built from the blocks and the prop
  occupancy layer, rebuilt only where a zone's hash changed; dungeon templates can ship them.
- **Textures**: block tiles as PNG or zstd-supercompressed KTX2 with precomputed mipmaps, named by
  hash.
- **Meshes**: packed vertex and index arrays under zstd; no C++ codec on the wire.

## Next steps, in order

1. Fix the `Entities` section of `world-format.md`: `bevy_reflect` alone and our own entity blob
   (done alongside this analysis).
2. One deterministic `vox-core::noise` with golden tests on native and wasm; remove the two
   copies.
3. Movement feel in `vox-core::body`, `moves` and `predict` (coyote time, jump buffer, variable
   jump, smooth step-up).
4. bevy_mod_outline and bevy_vector_shapes for prop hover and the quick wheel.
5. Audio on bevy_seedling: buses, the prop `sound` behaviour and ambience; voice placed but never
   changed. Measure browser latency before relying on it.
6. Particles: bevy_firework (or our own CPU module) for weather and fire; the effect descriptor in
   the pack.
7. A dev and editor build feature with bevy_egui and transform-gizmo-bevy.
8. Later: big_space for big worlds; utility AI and cell A\* (then rerecast) for creatures.
9. The Bevy 0.20 upgrade as its own step, starting with the shaders to WESL, when the crates
   above have caught up.

## Not verified

- wasm builds of most crates (only some say so in their documentation), and browser audio
  latency.
- Whether Firewheel's cpal matches the avatar app's `cpal` 0.17.
- meshopt and basis-universal on `wasm32-unknown-unknown`; whether cubecl can share Bevy's wgpu
  device.
- The exact 0.20 module paths for every `bevy_pbr` import our shaders use, and whether any
  naga_oil syntax still works under 0.20.
- Whether the two third-person camera repositories are the same project.

## Sources

Checked on 2026-11-26: the crates.io API (`https://crates.io/api/v1/crates/<name>` and each
version's dependencies) for every crate above, and for `bevy`, `bevy_world_serialization`,
`bevy_camera`, `wgpu`, `bevy_shader` and `bevy_curve`; each crate's repository and commit
history (GitHub, or Codeberg for big-brain); docs.rs for `bevy_ecs` 0.19.1 and its features;
[the Bevy 0.20 release notes](https://bevy.org/news/bevy-0-20/) and
[the 0.19 to 0.20 migration guide](https://bevy.org/learn/migration-guides/0-19-to-0-20/);
[arewegameyet's 3D format loaders](https://arewegameyet.rs/ecosystem/3dformatloaders/); the Rhai
book's wasm and safety pages; the Rune book's sandboxing page; hanabi's `docs/wasm.md`; and, in
this repository, `Cargo.lock`, `Cargo.toml`, `bins/vox-client/Cargo.toml`, `assets/shaders/`,
`crates/vox-core/src/weather.rs`, `crates/vox-world/src/outdoors.rs` and the avatar app's sound
code.
