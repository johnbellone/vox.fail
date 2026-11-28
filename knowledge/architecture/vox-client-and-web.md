---
type: Component
title: vox-client and the Web Front End
description: The player client (desktop and WebAssembly) and the plain HTML/JS pages that the directory serves around it.
tags: [component, vox-client, web, wasm, webgpu]
generated: { by: agent/mcp, at: "2026-11-28T03:25:04Z" }
status: stable
code_refs: [bins/vox-client/src/, apps/site/, tools/dev/build_web.sh, tools/test/web_e2e.py]
---

# vox-client and the Site

## vox-client (`bins/vox-client/`, about 770 lines)
`crate-type = ["cdylib","rlib"]`. The same app runs on the desktop and in the browser:
- **Desktop**: `vox-client --token <launch code> [--world ws://host:7780/ws] [--detail browser|mobile]`. The default world is the Void at vox.fail. Without a token, `--model models/dog.glb --name Sam` joins a world that allows guests.
- **Browser**: built for WebAssembly by `tools/dev/build_web.sh` and started by the site's play page.

It adds the window, camera, light, FPS counter, portal sweep, **watch mode** (a read-only view for the front page's live view), and the player's character with what they wear. The world itself is `vox-world`.

**Device profile.** `Start.detail` (`desktop`, `browser` or `mobile`) inserts the `vox_world::detail::Detail` resource: full-detail distance, LOD rings, echo radius and meshing budget. The page sends it, and an unknown or missing value means the platform's default. `--detail` tries a profile on the desktop. See `architecture/vox-world`.

**Editor mode (browser only).** When the start options say `edit`, the client adds `vox_world::editor::EditorPlugin`, and the wasm module exports `editor_command(json)`.
- The page's commands are queued: at most 256 commands and 1 MiB between two frames. Over that, the page is told `Refused`/`TooFast`.
- Commands move into the editor's inbox before the editor runs, and its events go to the page's `window.vox_editor(json)` after it.
- Events wait until the page has installed `vox_editor`, the oldest dropped past a cap, so the page installs it before calling `start`.

See `decisions/world-editor` and `architecture/vox-world`.

## Browser build (`tools/dev/build_web.sh`)
- Profile `wasm-release`: opt-level "s", thin LTO, strip.
- Needs the `wasm32-unknown-unknown` target (or nightly `-Zbuild-std`) and a matching `wasm-bindgen-cli`. `wasm-opt -Os` is optional.
- Output goes to `apps/site/public/pkg/` (git-ignored), pre-compressed with gzip and Brotli: about **57 MB of WebAssembly, about 12 MB gzipped, 8.6 MB Brotli** (risk 28).
- Needs WebGPU; there is no WebGL2 fallback. A browser without a GPU adapter can't run it; Claude's built-in browser pane is one.
- Rules for code shared with wasm (from `AGENTS.md`): no threads, blocking I/O or `std::time` clocks. Release wasm32 builds wrap on integer overflow (32-bit `usize`), so use checked arithmetic on untrusted sizes.

## The site (`apps/site/`)
React 19, Mantine 9 and TypeScript 7, built with Bun (`bun run build` -> `dist/`, served by `vox-directory --web`) and tested with `bun test`. three.js (^0.187) draws the character preview. It replaced the earlier no-framework `web/` pages.

Structure:
- `src/entries/`: one per page (index, create, play, inventory, friends, help, edit); the HTML in `src/pages/*.html`.
- `src/features/{character,editor,friends,home,inventory,play}`.
- `src/components`: Shell, Page, Portrait, LiveWorld, Glyph and others.
- `src/api`: client, hooks, types.
- `src/game/wasm.ts`: loads the client and adds `detail` from `deviceDetail()` (`mobile` for a coarse pointer or a browser that says it's mobile, else `browser`). `editorCommand` calls the wasm export.
- `src/game/editor.ts`: the page's side of the editor bridge.
- `theme.ts`.

**The editor page (`/edit?world=<id>`, `src/features/editor/`).**
- `EditorPage` signs in as /play does, asks the directory whether the account may use Advanced controls, installs the event handler, and starts the client in edit mode on a canvas.
- Around the canvas it lays out:
  - the tool rail (select, blocks, portal, prop, node)
  - an inspector per kind (portal, prop, node)
  - the entity list, the block palette, and the prop catalogue (thumbnails from `data/props`)
  - the spawn and objective panels, the world settings panel, and the header
- Basic portal sizes are 8 to 16 blocks wide and 12 to 32 tall (`BASIC_WIDTH`, `BASIC_HEIGHT`, the directory's limits). An objective's radius is at most 512 blocks. Prop positions are shown in blocks at `UNITS_PER_BLOCK` = 5 (5 cm units).
- `src/game/editor.ts` mirrors the bridge's `Command` and `Event` types and the entity components (`vox-core::entity`). Commands are checked strictly, and events read leniently.
- `editor.samples.json` holds one sample of every variant, `editor.test.ts` parses it, and the Rust bridge has a test against the same file, so the two can't drift silently.
- A `ref` the page picks comes back on `done` or `refused`.

Cloudflare must keep Rocket Loader and email obfuscation off; they break scripts under the content policy.

## Tests
- `tools/test/web_e2e.py` (Playwright, Chromium with WebGPU on SwiftShader under Xvfb): sign up, make a character, enter the Void. It passed once on the earlier site; it hasn't been run on the React site, and isn't in CI.
- `tools/test/editor_e2e.py` is the editor's counterpart: an editor places a portal and a second page sees it. It needs a directory with the yard world signed in and the browser client built, and has not been run.
- A browser measurement page (watch mode, `stats()` for frame times and wasm memory) is planned as `tools/test/measure_web` (bead vox-cti.1).

# Related Concepts
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): how the browser build works
- [Vox Online: Player Journey and Worlds](../project/vox-online.md): the player journey
- [Decision: The World Editor](../decisions/world-editor.md): the /edit page and its bridge
- [Chunks, streaming and draw distance](chunk-streaming-design.md): device profiles and draw distance

## Voice chat (desktop)
`VoicePlugin` (`crates/vox-world/src/voice/plugin.rs`) is added by `vox-client`.
- It is opt-in: off by default, with settings in `voice.json`.
- The mic (cpal, `capture.rs`) feeds a `vox_voice::Packetizer`.
- Push-to-talk (B / D-pad right), or open mic with a silence gate. N switches between nearby and party.
- Received voices are mixed by `playback.rs`, with distance falloff and panning from `mix.rs`: positions in blocks, full volume within 1 m, and a 16 m nearby radius by default.
- The HUD shows a MIC marker and who is speaking, and the pause menu has a Voice screen.
- Remote and local mouths follow the voice (`lipsync.rs`).

**The browser build has no voice**: a stub drops incoming voice, and the menu says it isn't available. See `decisions/world-voice-chat`.
