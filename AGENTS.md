# AGENTS.md

Orientation for anyone (human or AI agent) working in this repository. Read this first; the
[README](README.md) is the user-facing manual for the avatar app and the Vox worlds.

## What this is

Two products share one Cargo workspace:

1. **Block Avatar** (`block_avatar`, in `bins/block-avatar/`): a Bevy 0.19 desktop app that
   drives block-style characters from a microphone, for streaming through OBS. Lip sync,
   reactions, a local HTTP control page, viewer questions, Twitch chat, optional
   Windows-only webcam output.
2. **Vox** (every other crate): a shared voxel build space. You sign in on a web page, make a
   character, and walk into worlds (the Void, the yard, the islands, the dark sea) that you can build in
   together. It runs on the desktop (`vox-client`) and in the browser (the same client built
   for WebAssembly). Live at https://vox.fail.

## Layout

| Path | Crate | Role |
| --- | --- | --- |
| `crates/vox-core/` | `vox-core` | Engine-free rules shared by everyone: blocks and chunks (`world`), voxel raycast (`ray`), walking physics (`body`), level files (`level`), the wire protocol (`wire`), sign-in tokens (`auth`), client-side prediction (`predict`), weather simulation (`weather`). **No Bevy here.** |
| `crates/vox-world/` | `vox-world` | A world on screen, shared by the app and the client: block meshing and materials (`blocks`), server link, chunks, controls, camera (`client`), input actions, bindings, controller brands and button hints (`input/`), the pixel HUD and quick wheel (`hud`), menus with controller navigation (`menu`), sky and fog (`outdoors`), portals (`portals`), avatar pose streaming (`pose`, `proto`), file access that also works in the browser (`files`). |
| `crates/vox-format/` | `vox-format` | The world format: regions (16 × 16 chunk columns) as checked, zstd-compressed blobs named by their BLAKE3 hash (`region`), a blob store and snapshots (manifests of region hashes, shared blobs, kept a week; `store`). Decodes in wasm too (`ruzstd`); encoding is native. |
| `crates/vox-link/` | `vox-link` | A WebSocket kept open on its own thread, reconnecting with backoff; a `Dial` says where and how (world servers, the relay). Desktop only. |
| `crates/avatar-voice/` | `avatar-voice` | Block Avatar's voice analysis, pure Rust: lip sync (`lipsync`), pitch and formants (`voice`), laugh and sing detection (`detect`), WAV files (`wav`, one hardened reader). Its tests run on real recordings in `testdata/real`. |
| `bins/vox-client/` | `vox-client` | The player client: a thin Bevy app (`lib.rs`) around `vox-world`. Builds natively and to wasm32. Also the FPS counter, the portal sweep and watch mode (the front page's live view). |
| `bins/vox-server/` | `vox-server` | One world: a single thread owns the blocks, players and edit order; WebSocket on `/ws` for players and watchers; SQLite persistence, with `vox-format` snapshots beside it (`<db>.snapshots/`) and restore (`snapshot`). |
| `bins/vox-directory/` | `vox-directory` | Accounts (Steam, Discord, name and password), characters, authority and world currencies, plots, portals, raids, friends and groups, token signing; serves the site and the game's files. |
| `bins/vox-relay/` | `vox-relay` | Lists live shows and relays traffic for the avatar app's network shows. |
| `bins/block-avatar/` | `block_avatar` | The avatar app (and its Windows installer). It re-exports blocks, weather and the world client from `vox-world`; its own `bevel`, `lighting` and `particles` are different systems, not copies. |
| `apps/site/` | (Bun) | The vox.fail pages: React 19, Mantine 9, built with Bun into `dist/`. `public/pkg/` is the wasm client (generated, ignored). See its README. |
| `data/` | | Game data: `blocks.json`, `characters.json`, `wardrobe.json`, `levels/*.json` (void, yard, islands, sea), clip manifests. |
| `assets/` | | Models, props, sets, sounds, fonts, shaders. |
| `deploy/` | | systemd units and the Cloudflare Tunnel config for vox.fail. See `deploy/README.md`. |
| `tools/assets/` | | Python asset builders (models, sets, wardrobe, block tiles, levels). |
| `tools/dev/` | | `play-local.sh` (run everything locally), `build_web.sh` (the wasm client), `oxen_probe.sh` (the snapshot store against Oxen). |
| `tools/test/` | | `visual_test.py`, `web_e2e.py`, `eval_voice.py`. |
| `docs/` | | `vox/` (worlds), `avatar/` (the avatar manual), `design/` (design documents). |
| `source/vox/` | | MagicaVoxel sources for the characters and props. |

Dependency direction: `vox-core` <- `vox-world` <- `vox-client` and `block_avatar`; `vox-server`
and `vox-directory` depend on `vox-core` only. Keep it that way: rules go in `vox-core`,
rendering in `vox-world`. Rust libraries live in `crates/`, Rust programs in `bins/`, Bun web
apps in `apps/`.

## Commands

The root is a virtual workspace, so `cargo build` builds every crate; `-p <name>` builds one.
Shared dependency versions live once in the root `Cargo.toml` (`[workspace.dependencies]`);
members say `name.workspace = true`.

```bash
cargo build --workspace              # everything (first build compiles Bevy: ~5 min)
cargo check --workspace              # fast type check
cargo clippy --workspace             # lints (the tree has existing warnings; do not add new ones)
cargo test --workspace               # tests; the avatar app's tests use testdata/real/

tools/dev/play-local.sh                  # local Void + yard + islands + sea, then the client joins as a guest
tools/dev/play-local.sh --name Sam --model models/dog.glb

cargo run --release -p vox-server -- --listen 127.0.0.1:7780 --db void.db     # one world
cargo run --release -p vox-server -- --db void.db --list-snapshots             # (world closed) its snapshots
cargo run --release -p vox-server -- --db void.db --restore-at 1795700000      # back to a moment; undoable
cargo run --release -p vox-client -- --world ws://127.0.0.1:7780/ws --name Sam
cargo run --release -p vox-directory -- --listen 0.0.0.0:7700                 # accounts and pages
tools/dev/build_web.sh               # browser client into apps/site/public/pkg (wasm32 target + wasm-bindgen-cli)
bun install && (cd apps/site && bun run build)   # the pages into apps/site/dist
```

`tools/dev/play-local.sh` refuses to start if ports 7780 to 7783 are taken; clear a leftover with
`pkill vox-server`. Worlds save to `void.db`, `yard.db`, `islands.db`, `sea.db` in the repo root (git
ignores `*.db`).

## How the pieces talk

- **Client <-> world:** WebSocket, `vox_core::wire` messages (postcard). The server is
  authoritative: the client predicts its own edits (`vox-core::predict`) and the server confirms or
  refuses them with the same rules (`vox-core::world`).
- **Sign-in:** the directory signs a token (Ed25519) naming you and your character. World
  servers started with `--directory-key` verify it offline. Guests are allowed only when a
  world is started without a directory key. The client sends the token only to the host it
  started on, never to a portal destination on another host.
- **Portals:** a level lists portals with a destination `url` (`":7781"` means the same host,
  that port; `"/w/7781/ws"` a path). Walking through calls `WorldClient::travel`, which
  reconnects to the destination. If it cannot be reached the client returns to the old world.
- **Worlds -> directory:** each world reports who is present once a minute (authority, raids,
  plots) over localhost with `VOX_WORLD_SECRET`. Those endpoints (`/api/world/*`) must not be
  exposed publicly; the tunnel config refuses them.
- **Production:** one machine runs the directory and worlds on localhost behind a Cloudflare
  Tunnel at vox.fail. Details and the order to start things: `deploy/README.md`.

## Conventions

- Rust 2024 edition, Bevy 0.19. Positions in `vox-core` are in **blocks**; the app scales by
  the palette's block size (0.5 m) for metres.
- The client crate also compiles to **wasm32**. Do not use threads, blocking I/O, `std::time`
  clocks, or unchecked arithmetic on untrusted sizes in code shared with `vox-world` or
  `vox-core`: release builds on wasm32 wrap on overflow (`usize` is 32 bits).
- **Everything from the network is untrusted:** decode with checked arithmetic, reject
  non-finite floats, cap sizes. Everything the server accepts from a client is validated on the
  server (`vox-core::world` holds the rules).
- Mutex locks recover from poisoning (`lock().unwrap_or_else(|e| e.into_inner())`) so one
  panicking worker does not take the whole app down. Keep using that form.
- The avatar app's local control page and settings file take text from a browser: strip control
  characters before writing to the settings file or to a network protocol (see `save_keys`
  and `Chat::connect`).
- SQL is always parameterised. No string-built queries.
- Log with `crate::note!` / `crate::problem!` (see `bins/block-avatar/src/applog.rs`, `crates/vox-world/src/files.rs`), not
  bare `println!`.
- Comments explain why, and every module starts with a `//!` doc saying what it is for.

## Design documents

Proposals live in `docs/design/`. Read the relevant one before large changes:

- [world-scaling.md](docs/design/world-scaling.md): 60 km worlds; regions, generated terrain,
  streaming by view radius, simulation owners, region hashes; browser and desktop budgets.
- [repo-structure.md](docs/design/repo-structure.md): moving to `bins/`, `crates/` and `apps/`, folders
  and plugins inside the avatar app and `vox-world`, and the step-by-step migration.
- [world-graph-and-art.md](docs/design/world-graph-and-art.md): levels as graphs of nodes
  refined in passes by a language model; recipes (typed building operations) interpreted by
  Rust into voxels in the project's style, native and wasm; the `vox-art`, `vox-graph`,
  `vox-library` crates and the `vox-forge` tool.
- [world-format.md](docs/design/world-format.md): regions as compressed blobs (blocks in
  run-length encoding + zstd, entities as Bevy `DynamicWorld` in RON or postcard),
  content-addressed snapshots, levels as files, the `vox-format` crate.
- [controls-and-hud.md](docs/design/controls-and-hud.md): block-game controls on an action
  layer, gamepad out of the box with brand-matched button hints, HUD (hotbar, health), menus
  with controller navigation, server-side inventory and health.

## Known gaps (not yet done)

From the code review; none are regressions, but know they exist before relying on them:

- The world server checks moves (`vox-core::moves`: a distance budget at flying speed with room
  for lag, fast falls allowed, the spawn always reachable) and sends `Correct` back when a move
  is impossible; `--trust-moves` turns this off for test and automation worlds (the control
  page's `world:goto` teleports). Still taken on trust: the first position after joining,
  because a portal's arrival point is set by the world you came from.
- Sign-in hardening is in: Steam answers must be plain (no repeated `openid.*` keys) and signed
  over every field we read; password hashing runs at most four at a time; the relay limits shows
  and wrong codes per address (and wrong codes per show) and drops text from guests and viewers.
  Not yet: a world's sign-in token is valid on any world (no audience in the token).
- The avatar app's systems are plugins by pipeline (`app/plugins.rs`: `Setup`, `Frame`, `Motion`,
  `Network`, `WorldGlue`), not by folder: several chains run systems from different folders in
  a set order. Resources are built from the settings in `app/configure.rs`; `main()` is about
  50 lines. Check schedule changes with `tools/test/visual_test.py` (render references with the
  old build, `--update`, then compare the new one).
- Remeshing copies voxels chunk by chunk and runs within a per-frame budget (8 ms desktop,
  5 ms browser), nearest first. World faces use a packed 20-byte vertex (`blocks::ATTRIBUTE_BLOCK_UV`
  and `ATTRIBUTE_BLOCK_FACE`, unpacked by `assets/shaders/block_vertex.wgsl`); neighbouring
  solid faces with the same tile and even light are merged (`mesh_cells(.., merge: true)`), and
  `assets/shaders/block.wgsl` repeats the tile across them, sampling every texture with the
  unwrapped coordinates' derivatives (no seams). Anything drawn with `BlockMaterial` must use
  `into_packed_mesh`; chunk meshes live only on the GPU (`bounds()` gives their `Aabb`). Still:
  a one-block edit rebuilds its whole chunk, and weather and fog baking run on the main thread.
- Duplicated helpers: several xorshift generators, value noise in both
  `vox-core::weather` and `vox-world::outdoors`, and the child-spawning for opaque, glass, water
  and flame meshes.
- The Linux webcam output does not exist: the `W` virtual camera is Windows-only (it writes
  OBS's DirectShow shared-memory queue). On Linux use OBS window capture and OBS's own
  virtual camera.

## Working here

- Make changes small and keep `cargo check --workspace` clean; run `cargo clippy` on what you
  touch.
- Do not edit generated or binary assets by hand (`assets/models`, `apps/site/public/pkg`, `apps/site/dist`); use the scripts
  in `tools/`.
- Do not commit `*.db`, `directory.key*`, or anything under `/etc/vox` equivalents. Secrets
  (`VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_SECRET`, `VOX_STEAM_KEY`) live in environment files
  on the server, never in the repo.

## Task tracking and agent memory

- **GitHub Issues** are for people: user-facing bugs, feature requests and questions. They are the
  public record of what should change and why. Never track a user-facing bug in beads only.
- **beads** (`bd`) is for agents: implementation tasks, dependencies and follow-ups. Put the
  GitHub issue in each bead (`gh-<n>`); work from `bd ready`; close with a reason. (beads may not
  be installed yet.)
- **okf** (`knowledge/`) is durable project knowledge: architecture, technologies, decisions and
  status. Search it before large changes; record decisions in it when you finish.
- Hard constraints: no real-person likenesses or trademarked characters; no synthetic voices
  (test and demo audio are real recordings); the medic's cross is green; no poses of assaulting
  police; sound effects are real CC0 recordings; stay on Bevy 0.19.1. See
  `knowledge/convention/hard-constraints.md`.
- Before finishing code changes run `cargo test --workspace`, and
  `okf validate knowledge --strict --drift` when you touched `knowledge/`.

<!-- BEGIN OKF AGENT MEMORY -->
## 1. Behavioral Invariants & Constraints (RFC 2119)
- MUST execute `okf_search(query=keywords, limit=3)` before proposing architecture, new dependencies, or substantial changes.
- NEVER scan `knowledge/` via `list_dir`, `grep_search`, `find`, or raw file readers.
- NEVER forge human verification (`verified:` is human-only; declare `generated: { by: "<actor>", at: "<iso-time>" }`).
- PREFER native `okf_*` MCP tools OVER CLI fallback commands.

## 2. Guard Clauses & Scope Governance
- ON edit(@path/):
    IF first_visit(@path/) => okf_search(for_path=@path/)
    IF governance == "hold" => STOP("Subsystem frozen by governance. Request explicit human confirmation.")
    IF governance == "constraint" => MUST adhere to all listed invariants
    IF governance == "context" => proceed with awareness
- ON user_query(architecture | requirements | conventions | domain_facts):
    okf_search(query=keywords, limit=3) => evaluate summary description
    IF relevant => okf_show(concept_id) ONLY on demand
    IF updating_existing_concept => PREFER okf_update OVER okf_create

## 3. Completion Pipeline (Sequential Assertion Gates)
1. IF decisions_made => MUST okf_create(category/*, type="decision", title=..., desc=...)
2. IF requirements_discovered => MUST okf_update(concept_id)
3. IF concepts_mutated => MUST sync(knowledge/log.md, knowledge/index.md)
4. ASSERT(okf_validate(strict=true, drift=true) == {errors: 0, warnings: 0}, ELSE=fix_before_exit)
<!-- END OKF AGENT MEMORY -->
