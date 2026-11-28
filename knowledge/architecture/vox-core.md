---
type: Component
title: "vox-core: Shared Rules, Wire Format and Tokens"
description: "The engine-free crate that holds block chunks, edit checks, ray casts, body movement, levels, weather, the world wire format and token signing."
tags: [component, vox-core, rust, blocks]
generated: { by: agent/mcp, at: "2026-11-28T03:24:20Z" }
status: stable
code_refs: [crates/vox-core/src/]
---

# vox-core: Shared Rules, Wire Format, Moves and Tokens

Path `crates/vox-core/`, about 6,800 lines in 17 files. **No Bevy.** Depends on `serde`, `serde_json`, `postcard`, `ed25519-dalek` and `base64`. Used by the server, the directory, the clients and the avatar app.

**Scale.** A block is `BLOCK_M` = **0.25 m** in every world (`lib.rs`; see `architecture/quarter-metre-blocks-decision`). `block_size(blocks_json)` refuses a palette that says otherwise. Sizes that are really metres are written as `in_blocks(m)` or `whole_blocks(m)`, so they keep their size if the block size changes again.

| Module | What it holds |
| --- | --- |
| `world.rs` | Blocks in **32 x 32 x 32 chunks** (8 m), id 0 = air, a version per chunk, `Cell = [i32;3]`, and `Bounds` (default 1,024 × 256 × 1,024 blocks; a level may give its own). Also the checks an edit must pass. `Why`: `Outside`, `NoSuchBlock`, `NoChange`, `TooFar`, `TooFast`, `Claimed`, and `Blocked` (a placed prop stands there; last, because the wire numbers the variants). |
| `ray.rs` | Voxel raycast (Amanatides and Woo). |
| `body.rs` | Moving a box through blocks: solids, step-ups, falling, ceilings. The player's size is in metres here (`PLAYER_HALF`, `PLAYER_HEIGHT` 1.8 m, `EYES` 1.6 m, `STEP_UP` 0.525 m). Props add their own cells beside the blocks (`Occupancy`, `with_props`). A prop's footprint is listed in **half-metre cells**, relative to the cell holding its pivot; each cell covers `PROP_CELL_BLOCKS` (2) × 2 blocks, for `height * 2` layers. `prop_footprint_cells`, `prop_base_cells` and `prop_behind_cell` give the same cells on the server and in the client's prediction. |
| `moves.rs` | **The server's check on reported positions.** Each player has a distance budget, refilled at the fastest a player can go (11 m/s sideways, 9 up, falls to 70) and capped at about two seconds for lag. The spawn is always reachable. Speeds are m/s divided by the block size. |
| `level.rs` | Level JSON: boxes (solid, hollow with `thick`, round) and single blocks, an optional `bounds`, and the `"world"` object (sky, time, weather, camera, hotbar, portals). **`apply`** puts blocks straight into any target in file order, clipped to a region, after checking every name; `fill` writes into a `World` that way. Nothing builds a map of every block. `load` (a map) is kept for tests. |
| `rescale.rs` | Converting a world saved at half-metre blocks: each block into a 2³ cube (`chunk`, one stored chunk at a time), entities (`component`) and settings (`settings`) moved with the blocks. Used once by the server and on restoring old snapshots. |
| `predict.rs` | Server-confirmed blocks plus the player's pending edits; a refused edit disappears. |
| `entity.rs` | **Entities**: the things placed in a world beside its blocks. `Component` is `Portal`, `Spawn`, `Objective`, `Settings`, `Prop` or `Node`; variant order is part of the format, so new ones go last. `Prop {prop: [u8;16] (the definition's hash), at, yaw (0..24, 15 degree steps), variant, state (0..=7), channel, text (<= 140), owner}`: `at` is in 5 cm units, `PROP_UNITS_PER_BLOCK` = 5, and `prop_cell` floors it to a block. `Node {kind, detail (0..=4), at, size (each side 1..=64), parent, recipe (JSON, <= 16 KiB), seed}`; `MAX_NODE_SEED` is 2^53 - 1, so a JavaScript page can send a seed back unchanged. `Component::validate` is the gate for anything from the network (finite numbers, ranges, size caps, no control characters); `tidy` repairs authored input and never replaces it. Portal sides are `portal::MIN_SIDE..=MAX_SIDE` (0.5 to 32 m), defaulting to 2 × 3 m. `EntityWhy` is the refusal reason (see `architecture/wire-protocol`). |
| `edit.rs` | **Who may change an entity, and whether the change is allowed**: pure functions `check_put` and `check_remove` over a `Ctx` the caller fills (role `Builder` or `Editor`, account, feet, bounds, plots, zone entity counts, `ground`). Caps: 4,000 entities a zone, 2 portals and 64 props a plot, reach 8 m (32 blocks). Prop rules go through closures in `Ctx`: `known_prop`; `prop_cells` (every cell the prop covers, checked against the bounds and a builder's plot); `prop_footprint_free` (no solid block or other prop under it, leaving out the prop being changed); and `prop_supported`. Support is `prop_stands` over the prop's `Surface`s (`Support`: `Floor`, `Wall`, `Ceiling`, `Table`, `Water`; `support_cells` says which cells to check). A floating prop is refused `NoSupport` and an occupied footprint `Blocked`; a change that keeps the placement (text or state only) skips both checks. The server, the directory and the tests run the same checks. |
| `terrain.rs`, `relief.rs` | An atlas's generated ground (tiles of 1,024 blocks, 256 m) and the dips in natural ground (relief version 2: the same ground at quarter-metre blocks). |
| `wire.rs` | `ToServer`/`ToClient`, postcard, `PROTOCOL = 10`. See `architecture/wire-protocol`. |
| `auth.rs` | Ed25519 tokens. See `architecture/identity-and-tokens`. |
| `weather.rs` | Atmosphere simulation. See `architecture/weather-simulation`. |

## Why it exists
So the server, the directory and the clients run the *same* code for edit validation, prediction, movement checks and token verification. See `decisions/core-without-bevy`. It is also the home of the deterministic terrain generator (`terrain`, `architecture/world-scaling-design`).

## Rules for contributors (AGENTS.md)
Everything from the network is untrusted: use checked arithmetic, reject non-finite floats and cap sizes. Code shared with wasm avoids threads and `std::time`.

# Related Concepts
- [World Wire Protocol (PROTOCOL = 10)](wire-protocol.md): its wire messages
- [Weather Simulation](weather-simulation.md): its weather model
- [Identity, Sessions and Signed Launch Tokens](identity-and-tokens.md): its token code
- [Decision: Keep vox-core Free of Bevy](../decisions/core-without-bevy.md): why it has no engine
- [Decision: The World Editor](../decisions/world-editor.md): the entity and edit rules it serves
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): the block size and how saved worlds were converted
