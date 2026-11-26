# Design: repository structure

Status: **steps 1 to 5 done** · 2026-11-26 · steps 6 to 9 (splitting code inside crates) are not

## Problem

The repository is hard to navigate:

- **Two products share one root.** The root `Cargo.toml` is both the workspace and the avatar
  app (`block_avatar`). So a bare `cargo build` builds only the avatar app, not the Vox client
  people expect, and the repository root mixes the avatar's `src/` and `build.rs` with
  workspace files.
- **Crates sit side by side at the top level** (`core/`, `world/`, `client/`, `server/`,
  `directory/`, `relay/`) next to non-code folders (`assets/`, `data/`, `deploy/`, `demo/`,
  `docs/`, `installer/`, `source/`, `testdata/`, `tools/`, `web/`). Nothing says which are
  libraries, which are programs, and which are data.
- **The avatar app is flat**: 41 modules in `src/`, about 26,000 lines, with no grouping.
  `main.rs` is 2,078 lines with a 690-line `main()`; `bevel.rs` is 1,962 lines.
- **`world/src/client.rs` is 2,029 lines** mixing the network link, message handling, chunk
  meshing, controls, physics, camera, HUD and a command parser.
- **One 1,100-line README** serves both products: the avatar manual, the Vox setup and the
  deployment notes.
- **`tools/`** mixes asset builders, dev scripts and test drivers.

## Principles

1. **Three kinds of code, three folders.** Rust libraries in `crates/`, Rust programs in
   `bins/`, web apps (Bun, TypeScript) in `apps/`. A reader can tell at a glance what runs, what
   is shared, and what is the browser front end.
2. **A virtual workspace at the root.** No package at the root, so `cargo build` builds
   everything and the root holds only workspace files.
3. **Split crates for a reason, not by habit**: a crate earns its place if it is shared by two
   programs, has to build without Bevy (fast tests, server use), or must build for wasm. Inside a
   crate, use **module folders**, not more crates.
4. **One folder, one Bevy `Plugin`.** Each feature folder registers its own systems, so `main.rs`
   becomes a list of plugins.
5. **Runtime data stays where programs find it.** `assets/` and `data/` stay at the root, because
   the programs, the directory and the deployment all look for them there.
6. **Move with `git mv`, one step at a time**, so history and blame follow the files and every
   step builds.

## Proposed layout

```
Cargo.toml                 virtual workspace: members, [workspace.package], [workspace.dependencies]
README.md                  short: what this is, quick start, where to read next
AGENTS.md                  orientation for contributors and agents
package.json               Bun workspace: "workspaces": ["apps/*"]; scripts for dev, build, test
bun.lock

bins/
  vox-client/              was client/      the player client (desktop binary + wasm lib)
  vox-server/              was server/      one world server
  vox-directory/           was directory/   accounts, characters, economy, web pages
  vox-relay/               was relay/       show relay for the avatar app
  block-avatar/            was src/, build.rs, installer/   the streaming avatar app

crates/
  vox-core/                was core/        rules, wire protocol, auth, weather; no Bevy
  vox-world/               was world/       a world on screen; shared by vox-client and block-avatar
  vox-link/                new              one WebSocket link for native and wasm (today written
                                            twice, in world/src/client.rs and src/net.rs)
  avatar-voice/            new              pure-Rust voice analysis from the avatar app: lip sync,
                                            pitch and formants, laugh and sing detection; no Bevy
  vox-art/, vox-graph/,    new (later)      world graphs, recipes and generated art; see
  vox-library/                              world-graph-and-art.md (plus bins/vox-forge)

apps/                      web apps, built with Bun
  site/                    was web/         the vox.fail pages: sign in, make your character,
                                            friends, inventory, play in the browser; bun build
                                            writes dist/, which vox-directory serves (--web)
  forge/                   new (later)      review UI for generated art: renders, accept and
                                            reject (talks to bins/vox-forge)

assets/                    models, props, sets, sounds, fonts, shaders (runtime)
data/                      blocks, characters, wardrobe, levels (runtime)
source/                    MagicaVoxel sources
deploy/                    systemd units, Cloudflare Tunnel, deploy README
tools/
  assets/                  build_*.py, make_*.py, repack.py, guess_surfaces.py
  dev/                     play-local.sh, build_web.sh
  test/                    visual_test.py, web_e2e.py, eval_voice.py
testdata/                  real recordings and visual baselines
demo/                      sample speech and recorded questions
docs/
  design/                  design documents like this one
  avatar/                  the avatar manual (moved from README), PROJECT.md, STREAMDECK.md, images
  vox/                     playing, running worlds, the directory, the economy
```

Notes on the choices:

- **Keep crate names** (`vox-core`, `vox-world`, `vox-server` and so on). Only folders move, so
  `use vox_core::...` lines do not change.
- **`apps/` is only for Bun web apps.** `web/` becomes `apps/site/`, a Bun project in
  TypeScript. Its pages stay as they are at first (plain HTML and ES modules) and gain a build
  step: `bun build` bundles and fingerprints the scripts into `apps/site/dist/`. The directory
  already takes the folder to serve with `--web`, so it runs with `--web apps/site/dist` and
  needs no code change. The vendored Three.js in `web/vendor/` becomes an npm dependency.
- **The wasm client is a build input to the site.** `tools/dev/build_web.sh` writes the
  wasm-bindgen output to `apps/site/public/pkg/` (ignored by git), and the site's build copies it
  into `dist/`. The Rust side stays in `bins/vox-client/`.
- **Rust and Bun are separate workspaces in one repository**: `Cargo.toml` lists `bins/*` and
  `crates/*`; `package.json` lists `apps/*`. Neither tool looks inside the other's folders.
- `installer/` moves into `bins/block-avatar/`, because it only packages that app.
- No `vox-util` grab-bag crate. Small shared helpers (hex, random tokens, unix time,
  text cleaning) go into a `util` module in `vox-core`, which every Vox crate already
  depends on.

## Inside the avatar app (`bins/block-avatar/src/`)

Group the 41 flat modules by what they do. Each folder exposes one `Plugin`.

| Folder | Modules today | What it is |
| --- | --- | --- |
| `app/` | `main.rs` (split), `startup`, `applog`, `sim` | Entry point, plugin list, settings file, logging, the simulation clock |
| `audio/` | `audio`, `netvoice`, `voicefx` | Microphone capture and voice playback (analysis moves to `avatar-voice`) |
| `character/` | `face`, `facecap`, `wardrobe`, `jiggle`, `puppet`, `bevel/` | The character: expressions, face capture, dress-up, secondary motion, bevels |
| `stage/` | `camera`, `lighting`, `stage`, `sets`, `show`, `particles`, `fx`, `bubbles`, `crowd` | What the viewer sees around the character |
| `audience/` | `chat/`, `streamchat`, `events`, `poll`, `ask/`, `questions/`, `guest`, `cohost` | Chat, Twitch moments, polls, viewer questions, guests |
| `control/` | `commands`, `control`, `hotkeys` | The command set and every way to send commands |
| `output/` | `webcam`, `clips`, `cards`, `snapshot` | Webcam, shorts and comic strips, visual test snapshots |
| `net/` | `net` | Shows over the relay |
| `world/` | (glue only) | Joining a Vox world from the app, through `vox-world` |

Into `crates/avatar-voice/`: `lipsync`, `voice`, `detect`, plus one WAV module (today there are two
parsers, in `lipsync.rs` and `questions.rs`). These are already Bevy-free, so their tests stop
compiling Bevy. `tests_real.rs` moves with them as an integration test.

Large files split inside their folder:

- `main.rs` (2,078 lines): `app/mod.rs` with the plugin list, `app/settings.rs` (load, save,
  the sanitising `save_keys`), `app/hud.rs`, `app/state.rs` (`publish_state`). `main()` shrinks
  to building the `App` and adding plugins.
- `bevel.rs` (1,962): `character/bevel/{atlas,mesh,apply}.rs`.
- `questions.rs` (1,220): `audience/questions/{queue,store,transcribe}.rs`.
- `ask.rs` (1,158): `audience/ask/{server,login,session}.rs`, with its HTML, JS and CSS moved to
  files loaded with `include_str!`.
- `chat.rs` (1,076): `audience/chat/{irc,ratelimit,throws}.rs`.
- `guest.rs` (1,013): `audience/guest/{playback,update}.rs`.
- `control.rs` (691): page markup moved to a file with `include_str!`; the hand-written HTTP
  server replaced by `tiny_http`, which `ask` already uses.

## Inside `vox-world` (`crates/vox-world/src/`)

Split `client.rs` (2,029 lines) by concern; `WorldClient` (about 100 fields) splits into a few
resources so Bevy change detection works.

```
lib.rs          plugin list and the shared types (WorldCamera, LocalAvatar)
net/            link use (via vox-link), Hello/Welcome, receive and dispatch, travel and portals' reconnect
chunks/         chunk store, remesh queue with per-frame budget, chunk spawn (one Meshes::spawn
                for opaque, glass, water and flame, today written three times)
blocks/         block definitions, atlas, meshing, materials (today blocks.rs)
player/         controls, walking physics, building
camera.rs       camera modes
hud.rs          crosshair, HUD text (rebuilt only when something changes)
commands.rs     the control-page command parser
outdoors/       sky, fog, rain, floor (today outdoors.rs)
portals.rs      unchanged
avatar/         pose and proto (today pose.rs, proto.rs)
files.rs        unchanged
```

This also prepares for the world-scaling work (`docs/design/world-scaling.md`), which adds
regions, streaming and far terrain mostly under `net/` and `chunks/`.

## Inside the other crates

- **`vox-core`**: fine as a crate. Add `util.rs` for the shared helpers above, and later
  `terrain/` for the generator and `region.rs` for region keys and hashes.
- **`vox-directory`**: split `lib.rs` into `db.rs` (schema and migrations), `session.rs`
  (cookies, sign-in, rate limits) and `routes.rs`; `game.rs`, `people.rs`, `social.rs` stay.
- **`vox-server`**: split `lib.rs` into `connection.rs` (WebSocket task, per-connection limits),
  `world.rs` (the world thread) and `store.rs` (SQLite).
- **`vox-client`**: keeps the FPS counter and the portal sweep, moved into `hud.rs` and
  `transition.rs`; `dress` reuses the fitting code from the avatar's wardrobe (move it to
  `vox-world` so both use one copy).

## Workspace manifest

```toml
[workspace]
resolver = "3"
members = ["bins/*", "crates/*"]
default-members = ["bins/*", "crates/*"]

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
bevy = "0.19.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
postcard = { version = "1", features = ["use-std"] }
tungstenite = { version = "0.27", default-features = false }
# ...each shared dependency once; members write `bevy.workspace = true`
```

One version of each shared dependency, declared once, so the crates cannot drift apart.
`[profile.*]` sections (including `wasm-release`) stay in the root manifest.

## What has to change besides moving files

| Place | Change |
| --- | --- |
| `bins/block-avatar/build.rs` | Icon path becomes `../../assets/icon.ico` (or the icon moves into the app) |
| `asset_base()` in the avatar app, `find_root()` in `vox-client` | Already search the working directory and above the executable; check they still find `assets/` from `target/` |
| `tools/dev/build_web.sh`, `tools/dev/play-local.sh` | `cd` to the repo root instead of `$(dirname $0)/..`; `build_web.sh` writes to `apps/site/public/pkg/` |
| `vox-directory` | Run with `--web apps/site/dist` (the flag exists; the default `web/` path changes to match) |
| `tools/test/web_e2e.py` | Point at the built site; later move it into `apps/site` as a Bun test |
| `.github/workflows/*.yml` | `cargo build -p block_avatar` (it is no longer the root package), and new paths |
| `bins/block-avatar/installer/*.nsi` | Paths to the binary and assets |
| `deploy/README.md` | Build with `bun install && bun run build` in `apps/site`, then copy `apps/site/dist/` to `/opt/vox/web/`; Rust builds with `-p` names as now. The server needs Bun only to build, not to run |
| `README.md`, `AGENTS.md` | New layout and commands |
| `.gitignore` | `node_modules/`, `apps/*/dist/`, `apps/site/public/pkg/` (replaces `web/pkg/`) |

## Migration plan

Each step is one commit that builds and passes `cargo test --workspace` (and, from step 4,
`bun run build` and `bun test`). Use `git mv` for every move.

1. **Workspace dependencies.** Add `[workspace.dependencies]` and switch members to
   `workspace = true`. No moves.
2. **Virtual root.** Move `src/`, `build.rs` and `installer/` to `bins/block-avatar/`, and
   make the root manifest workspace-only. Fix `build.rs`, workflows and the installer.
3. **Move crates.** `core`, `world` to `crates/`; `client`, `server`, `directory`, `relay` to
   `bins/`. Update path dependencies and scripts.
4. **Bun site.** Add the root `package.json` (Bun workspaces), move `web/` to `apps/site/`,
   add `bun build` to `dist/`, Three.js from npm, and point the directory and the deployment at
   `dist/`. Pages unchanged; TypeScript comes file by file afterwards.
5. **Split tools and docs.** `tools/{assets,dev,test}`, `docs/{avatar,vox,design}`, and cut the
   README down to an overview that links to the manuals.
6. **Avatar app folders.** Group modules into folders with plugins, no behaviour change.
7. **`avatar-voice` crate**, with one WAV module.
8. **Split `vox-world`'s `client.rs`** and extract `vox-link`.
9. **Split the remaining large files** (`main.rs`, `bevel.rs`, `questions.rs`, `ask.rs`,
   `chat.rs`, `guest.rs`, directory and server `lib.rs`).

Steps 1 to 5 are mechanical and low risk. Steps 6 to 9 move code between files and should each
be reviewed on their own. Open branches will conflict with the moves; land or rebase them
before step 2.

## Open questions

- Product naming: the avatar app's window is now titled "Vox". Is the avatar app part of
  Vox (rename it `vox-avatar`), or a separate product that stays `block-avatar`?
- Should the site move to a framework later (server-rendered pages, components), or stay plain
  pages with a Bun build? The layout works for either.
- Should `relay` stay in this repository? It serves only the avatar app's network shows.
- Should the avatar manual live in `docs/avatar/` or stay in the README for people who
  download the Windows installer?
