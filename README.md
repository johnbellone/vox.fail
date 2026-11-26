# Vox

A world built from blocks, shared with everyone in it: sign in, make your character, walk into
the Void and build with friends. Live at https://vox.fail. This repository also holds **Block
Avatar**, a mic-driven avatar app for streams, built from the same characters and blocks.

## Quick start: walk around in Vox

```bash
tools/dev/play-local.sh                                    # the Void, the yard and the islands, then the client
tools/dev/play-local.sh --name Sam --model models/dog.glb  # your name and character
```

That builds in release mode (the first build compiles Bevy and takes several minutes), starts
three local worlds with guests allowed and opens the client in the Void. Click to look round,
WASD to walk, left click breaks, right click places, E for every block, hold Tab for the quick
wheel, Esc for the menu; a controller works too, with its own button hints. Walk into a doorway
to cross to another world. If a port is taken by a leftover server, `pkill vox-server`.

## What's here

| Folder | What |
| --- | --- |
| `crates/vox-core` | The rules every part shares, with no engine: blocks and chunks, edits, raycasts, walking physics, levels, the wire protocol, sign-in tokens, weather |
| `crates/vox-world` | A world on screen (Bevy): meshing, sky, portals, the server link, input actions, HUD, menus |
| `bins/vox-client` | The player client, on the desktop and (as WebAssembly) in the browser |
| `bins/vox-server` | One world server |
| `bins/vox-directory` | Accounts, characters, the economy, the web pages' API |
| `bins/vox-relay` | Relays network shows for the avatar app |
| `bins/block-avatar` | The streaming avatar app |
| `apps/site` | The vox.fail pages (React, Mantine, Bun) |
| `assets/`, `data/` | Models, props, sets, sounds, fonts; blocks, characters, wardrobe, levels |
| `tools/assets`, `tools/dev`, `tools/test` | Asset builders; dev scripts (local play, the browser build); test drivers |
| `deploy/` | systemd units and the Cloudflare Tunnel for vox.fail |
| `docs/` | `vox/` (playing and running worlds), `avatar/` (the avatar app's manual), `design/` (design documents) |

## Building

```bash
cargo build --workspace          # every Rust crate (a bare `cargo build` does too)
cargo test --workspace
bun install                      # the web apps' dependencies
(cd apps/site && bun run build)  # the pages, into apps/site/dist
tools/dev/build_web.sh           # the browser client, into apps/site/public/pkg
```

On Linux, install the audio and input headers first: `sudo apt install libasound2-dev libudev-dev`
(Debian and Ubuntu) or `sudo pacman -S alsa-lib systemd-libs` (Arch).

## Read next

- [docs/vox](docs/vox/README.md): running worlds, the directory, signing in, authority, plots,
  portals, raids.
- [docs/avatar/manual.md](docs/avatar/manual.md): Block Avatar, the streaming app.
- [deploy/README.md](deploy/README.md): how vox.fail is deployed.
- [docs/design](docs/design): world scaling, the repository layout, world graphs and generated
  art, controls and HUD.
- [AGENTS.md](AGENTS.md): for contributors and AI agents: layout, commands, conventions.
