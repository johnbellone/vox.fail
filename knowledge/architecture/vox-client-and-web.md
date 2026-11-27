---
type: Component
title: vox-client and the Web Front End
description: The player client (desktop and WebAssembly) and the plain HTML/JS pages that the directory serves around it.
tags: [component, vox-client, web, wasm, webgpu]
generated: { by: agent/mcp, at: "2026-11-27T20:45:40Z" }
status: stable
code_refs: [bins/vox-client/src/, apps/site/, tools/dev/build_web.sh, tools/test/web_e2e.py]
---

# vox-client and the Site

## vox-client (`bins/vox-client/`, about 840 lines)
`crate-type = ["cdylib","rlib"]`. The same app runs on the desktop (`vox-client --token <launch code> [--world ws://host:7780/ws]`; default is the Void at vox.fail) and in the browser (built for WebAssembly by `tools/dev/build_web.sh`, started by the site's play page). It adds the window, camera, light, FPS counter, portal sweep, **watch mode** (read-only view for the front page's live view) and the player's character with what they wear; the world is `vox-world`. Without a token, `--model models/dog.glb --name Sam` joins a world that allows guests.

**Editor mode (browser only).** When the start options say `edit`, the client adds `vox_world::editor::EditorPlugin` and the wasm module exports `editor_command(json)`. The page's commands are queued (at most 256 commands and 1 MiB between two frames; over that the page is told `Refused`/`TooFast`), moved into the editor's inbox before the editor runs, and the editor's events are handed to the page's `window.vox_editor(json)` after it; events wait (the oldest dropped past a cap) until the page has installed `vox_editor`, so the page installs it before calling `start`. See `decisions/world-editor` and `architecture/vox-world`.

## Browser build (`tools/dev/build_web.sh`)
Profile `wasm-release` (opt-level "s", thin LTO, strip). Needs the `wasm32-unknown-unknown` target (or nightly `-Zbuild-std`) and a matching `wasm-bindgen-cli`. Optional `wasm-opt -Os`. Output `apps/site/public/pkg/` (git-ignored), pre-compressed (gzip, Brotli). About **50 MB of WebAssembly, ~11 MB gzipped** (risk 28). Needs WebGPU; no WebGL2 fallback. Rules for code shared with wasm (from `AGENTS.md`): no threads, blocking I/O or `std::time` clocks; release wasm32 builds wrap on integer overflow (32-bit `usize`), so use checked arithmetic on untrusted sizes.

## The site (`apps/site/`)
React 19, Mantine 9, TypeScript 7, built with Bun (`bun run build` -> `dist/`, served by `vox-directory --web`), tested with `bun test`. Structure: `src/entries/` (one per page: index, create, play, inventory, friends, help, edit), `src/pages/*.html`, `src/features/{character,editor,friends,home,inventory,play}`, `src/components` (Shell, Page, Portrait, LiveWorld, Glyph...), `src/api` (client, hooks, types), `src/game/wasm.ts` (loads the client; `editorCommand` calls the wasm export), `src/game/editor.ts` (the page's side of the editor bridge), `theme.ts`. three.js (^0.187) draws the character preview. Replaced the earlier no-framework `web/` pages.

**The editor page (`/edit?world=<id>`, `src/features/editor/`).** `EditorPage` signs in as /play does, asks the directory whether the account may use Advanced controls, installs the event handler, starts the client in edit mode on a canvas and lays out around it the tool rail (select, blocks, portal, prop, node), an inspector per kind (portal, prop, node), the entity list, the block palette, the prop catalogue (thumbnails from `data/props`), spawn and objective panels, the world settings panel and the header. `src/game/editor.ts` mirrors the bridge's `Command` and `Event` types and the entity components (`vox-core::entity`); commands are checked strictly, events read leniently; `editor.samples.json` holds one sample of every variant and `editor.test.ts` parses it, and the Rust bridge has a test against the same file, so the two cannot drift silently. A `ref` the page picks comes back on `done` or `refused`.

Cloudflare must keep Rocket Loader and email obfuscation off (they break scripts under the content policy).

## Tests
`tools/test/web_e2e.py` (Playwright, Chromium with WebGPU on SwiftShader under Xvfb): sign up, make a character, enter the Void. Passed once on the earlier site; not run on the React site and not in CI. `tools/test/editor_e2e.py` is the editor's counterpart (an editor places a portal and a second page sees it); it needs a directory with the yard world signed in and the browser client built, and has not been run.

# Related Concepts
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): how the browser build works
- [Vox Online: Player Journey and Worlds](../project/vox-online.md): the player journey
- [Decision: The World Editor](../decisions/world-editor.md): the /edit page and its bridge


## Voice chat (desktop)
`VoicePlugin` (`crates/vox-world/src/voice/plugin.rs`) is added by `vox-client`. Opt-in (off by default, settings in `voice.json`): the mic (cpal, `capture.rs`) feeds a `vox_voice::Packetizer`; push-to-talk (B / D-pad right) or open mic with a silence gate; N switches nearby/party. Received voices are mixed by `playback.rs` with distance falloff and panning from `mix.rs` (positions in blocks, camera yaw). The HUD shows a MIC marker and who is speaking; the pause menu has a Voice screen. Remote and local mouths follow the voice (`lipsync.rs`). **The browser build has no voice**: a stub drops incoming voice and the menu says it is not available. See `decisions/world-voice-chat`.
