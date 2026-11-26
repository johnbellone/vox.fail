---
type: Component
title: vox-client and the Web Front End
description: The player client (desktop and WebAssembly) and the plain HTML/JS pages that the directory serves around it.
tags: [component, vox-client, web, wasm, webgpu]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
code_refs: [bins/vox-client/src/, apps/site/, tools/dev/build_web.sh, tools/test/web_e2e.py]
---

# vox-client and the Site

## vox-client (`bins/vox-client/`, 662 lines, 1 test)
`crate-type = ["cdylib","rlib"]`. The same app runs on the desktop (`vox-client --token <launch code> [--world ws://host:7780/ws]`; default is the Void at vox.fail) and in the browser (built for WebAssembly by `tools/dev/build_web.sh`, started by the site's play page). It adds the window, camera, light, FPS counter, portal sweep, **watch mode** (read-only view for the front page's live view) and the player's character with what they wear; the world is `vox-world`. Without a token, `--model models/dog.glb --name Sam` joins a world that allows guests.

## Browser build (`tools/dev/build_web.sh`)
Profile `wasm-release` (opt-level "s", thin LTO, strip). Needs the `wasm32-unknown-unknown` target (or nightly `-Zbuild-std`) and a matching `wasm-bindgen-cli`. Optional `wasm-opt -Os`. Output `apps/site/public/pkg/` (git-ignored), pre-compressed (gzip, Brotli). About **50 MB of WebAssembly, ~11 MB gzipped** (risk 28). Needs WebGPU; no WebGL2 fallback. Rules for code shared with wasm (from `AGENTS.md`): no threads, blocking I/O or `std::time` clocks; release wasm32 builds wrap on integer overflow (32-bit `usize`), so use checked arithmetic on untrusted sizes.

## The site (`apps/site/`)
React 19, Mantine 9, TypeScript 7, built with Bun (`bun run build` -> `dist/`, served by `vox-directory --web`). About 2.6k lines, 1 test file (`bun test`). Structure: `src/entries/` (one per page: index, create, play, inventory, friends), `src/pages/*.html`, `src/features/{character,friends,home,inventory,play}`, `src/components` (Shell, Page, Portrait, LiveWorld, Glyph...), `src/api` (client, hooks, types), `src/game/wasm.ts` (loads the client), `theme.ts`. three.js (^0.187) draws the character preview. Replaced the earlier no-framework `web/` pages (about 1.7k lines of JS).

Cloudflare must keep Rocket Loader and email obfuscation off (they break scripts under the content policy).

## Tests
`tools/test/web_e2e.py` (Playwright, Chromium with WebGPU on SwiftShader under Xvfb): sign up, make a character, enter the Void. Passed once on the earlier site; not run on the React site and not in CI.

# Related Concepts
- [WebAssembly and WebGPU Browser Client](../technologies/webassembly-webgpu.md): how the browser build works
- [Vox Online: Player Journey and Worlds](../project/vox-online.md): the player journey
