# AGENTS.md

Orientation for anyone (human or AI agent) working in this repository. Read this first; the
[README](README.md) is the user-facing manual for the avatar app and the Vox worlds.

## What this is

Two products share one Cargo workspace:

1. **Block Avatar** (`block_avatar`, the root crate in `src/`): a Bevy 0.19 desktop app that
   drives block-style characters from a microphone, for streaming through OBS. Lip sync,
   reactions, a local HTTP control page, viewer questions, Twitch chat, optional
   Windows-only webcam output.
2. **Vox** (every other crate): a shared voxel build space. You sign in on a web page, make a
   character, and walk into worlds (the Void, the yard, the islands) that you can build in
   together. It runs on the desktop (`vox-client`) and in the browser (the same client built
   for WebAssembly). Live at https://vox.fail.

## Layout

| Path | Crate | Role |
| --- | --- | --- |
| `core/` | `vox-core` | Engine-free rules shared by everyone: blocks and chunks (`world`), voxel raycast (`ray`), walking physics (`body`), level files (`level`), the wire protocol (`wire`), sign-in tokens (`auth`), client-side prediction (`predict`), weather simulation (`weather`). **No Bevy here.** |
| `world/` | `vox-world` | A world on screen, shared by the app and the client: block meshing and materials (`blocks`), server link, chunks, controls, camera, HUD (`client`), sky and fog (`outdoors`), portals (`portals`), avatar pose streaming (`pose`, `proto`), file access that also works in the browser (`files`). |
| `client/` | `vox-client` | The player client: a thin Bevy app (`lib.rs`) around `vox-world`. Builds natively and to wasm32. Also has the FPS counter and the portal transition. |
| `server/` | `vox-server` | One world: a single thread owns the blocks, players and edit order; WebSocket on `/ws`; SQLite persistence. |
| `directory/` | `vox-directory` | Accounts (Steam, Discord, name and password), characters, authority and world currencies, plots, portals, raids, friends and groups, token signing, the web pages. |
| `relay/` | `vox-relay` | Lists live shows and relays traffic for the avatar app's network shows. |
| `src/` | `block_avatar` | The avatar app. It re-exports blocks, weather and the world client from `vox-world`; its own `bevel`, `lighting` and `particles` are different systems, not copies. |
| `web/` | | The browser client's HTML and JS; `web/pkg/` is generated and ignored. |
| `data/` | | Game data: `blocks.json`, `characters.json`, `wardrobe.json`, `levels/*.json` (void, yard, islands), clip manifests. |
| `assets/` | | Models, props, sets, sounds, fonts, shaders. |
| `deploy/` | | systemd units and the Cloudflare Tunnel config for vox.fail. See `deploy/README.md`. |
| `tools/` | | Python asset builders, `build_web.sh` (wasm build), `play-local.sh` (run everything locally). |
| `source/vox/` | | MagicaVoxel sources for the characters and props. |

Dependency direction: `core` <- `world` <- `client` and the root app; `server` and `directory`
depend on `core` only. Keep it that way: rules go in `core`, rendering in `world`.

## Commands

A bare `cargo build` builds only the root package (`block_avatar`). Name the crate or use
`--workspace`, or you will not rebuild what you changed.

```bash
cargo build --workspace              # everything (first build compiles Bevy: ~5 min)
cargo check --workspace              # fast type check
cargo clippy --workspace             # lints (the tree has existing warnings; do not add new ones)
cargo test --workspace               # tests; the avatar app's tests use testdata/real/

tools/play-local.sh                  # local Void + yard + islands, then the client joins as a guest
tools/play-local.sh --name Sam --model models/dog.glb

cargo run --release -p vox-server -- --listen 127.0.0.1:7780 --db void.db     # one world
cargo run --release -p vox-client -- --world ws://127.0.0.1:7780/ws --name Sam
cargo run --release -p vox-directory -- --listen 0.0.0.0:7700                 # accounts and pages
tools/build_web.sh                   # browser client into web/pkg (needs wasm32 target + wasm-bindgen-cli)
```

`tools/play-local.sh` refuses to start if ports 7780 to 7782 are taken; clear a leftover with
`pkill vox-server`. Worlds save to `void.db`, `yard.db`, `islands.db` in the repo root (git
ignores `*.db`).

## How the pieces talk

- **Client <-> world:** WebSocket, `vox_core::wire` messages (postcard). The server is
  authoritative: the client predicts its own edits (`core/predict`) and the server confirms or
  refuses them with the same rules (`core/world`).
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
  clocks, or unchecked arithmetic on untrusted sizes in code shared with `world/` or
  `core/`: release builds on wasm32 wrap on overflow (`usize` is 32 bits).
- **Everything from the network is untrusted:** decode with checked arithmetic, reject
  non-finite floats, cap sizes. Everything the server accepts from a client is validated on the
  server (`core/world` holds the rules).
- Mutex locks recover from poisoning (`lock().unwrap_or_else(|e| e.into_inner())`) so one
  panicking worker does not take the whole app down. Keep using that form.
- The avatar app's local control page and settings file take text from a browser: strip control
  characters before writing to the settings file or to a network protocol (see `save_keys`
  and `Chat::connect`).
- SQL is always parameterised. No string-built queries.
- Log with `crate::note!` / `crate::problem!` (see `src/applog.rs`, `world/src/files.rs`), not
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

## Known gaps (not yet done)

From the code review; none are regressions, but know they exist before relying on them:

- The world server trusts the position a client reports for reach checks and plot claims; there is
  no speed or bounds validation.
- Steam OpenID return validation does not check which fields were signed or reject
  duplicate `openid.*` keys.
- Argon2 hashing has no concurrency cap; the relay's session creation and private codes have
  no per-IP limit.
- `world/src/client.rs` (about 2000 lines) mixes transport, messages, meshing, controls,
  camera, HUD and the command parser; `src/main.rs` has a 690-line `main()`. Splits are
  proposed in the review notes but not done.
- Remeshing reads every voxel through a slow lookup, rebuilds a whole chunk for a one-block
  edit, and meshes all dirty chunks in the frame a world becomes ready; weather and fog baking
  run on the main thread.
- Duplicated helpers: two WAV parsers, several xorshift generators, value noise in both
  `core/weather` and `world/outdoors`, two native WebSocket loops (`world/client` and
  `src/net.rs`), and the child-spawning for opaque, glass, water and flame meshes.
- The Linux webcam output does not exist: the `W` virtual camera is Windows-only (it writes
  OBS's DirectShow shared-memory queue). On Linux use OBS window capture and OBS's own
  virtual camera.

## Working here

- Make changes small and keep `cargo check --workspace` clean; run `cargo clippy` on what you
  touch.
- Do not edit generated or binary assets by hand (`assets/models`, `web/pkg`); use the scripts
  in `tools/`.
- Do not commit `*.db`, `directory.key*`, or anything under `/etc/vox` equivalents. Secrets
  (`VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_SECRET`, `VOX_STEAM_KEY`) live in environment files
  on the server, never in the repo.
