---
type: Project
title: "vox.fail: Project Overview"
description: "What vox.fail is: a Rust/Bevy voxel project made of the Block Avatar streaming app and the Vox online block world, sharing one codebase."
tags: [project, overview, vox, block-avatar]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
status: stable
---

# vox.fail: Project Overview

vox.fail is one Cargo workspace that holds **two products** built on the same characters, block rules and renderer.

## 1. Block Avatar (desktop streaming app)

A Bevy desktop app for streamers and video makers. A microphone drives a block-style 3D character: lip sync, nods, brows, gestures and reactions all come from the voice signal, with no face camera. Twitch, YouTube and Kick chat can throw things at the avatar, dress it up, vote in polls and queue recorded questions. The output goes to OBS (window capture), to a 1280x720 60 fps virtual webcam (Windows), and to 9:16 shorts and comic strips that save themselves.

Crate: `block_avatar` (`bins/block-avatar/`), with voice analysis in `crates/avatar-voice/`. About 26k lines of Rust in 76 files.

## 2. Vox Online (multiplayer block world)

A persistent shared build space. Players sign in at https://vox.fail (name and password, Steam or Discord), make a character, and walk into worlds in the browser (WebGPU, Rust compiled to WebAssembly) or in the desktop client. They build with blocks, claim plots, open portals between worlds, trade world currencies, make friends, and raid together. The first world is "the Void", with "the yard" and "the islands" reachable by portal.

Crates: `vox-core`, `vox-world`, `vox-format`, `vox-link`, `vox-client`, `vox-server`, `vox-directory`, `vox-relay`; the pages are the React site in `apps/site`.

## How the two connect

- The avatar app can host a live show through `vox-relay` and can join a world with `--world ws://host:7780`.
- Characters, props, sets and the pose format are shared. A player's chosen character in the web client is one of the same 38 rigged GLB models the streaming app uses.
- Both use `vox-world` for drawing a world.

## Who uses it

| Person | Uses |
| --- | --- |
| Streamer | Block Avatar, local control page on 127.0.0.1:7777 |
| Viewer | Twitch/YouTube/Kick chat commands, the public "ask" page (port 7778 behind a tunnel) |
| Player | https://vox.fail, the browser client or `vox-client` |
| Operator | `vox-directory` admin commands, `deploy/`, Cloudflare dashboard |
| Agent / contributor | GitHub Issues, beads, this okf bundle |

## Where to look next

- Architecture of everything: `architecture/system-overview`.
- What is done and what is not: `status/current-state`.
- The stack: `technologies/stack-overview`.
- Rules nobody may break: `convention/hard-constraints`.

# Related Concepts
- [System Overview and Crate Dependencies](../architecture/system-overview.md): architecture of both products and how the parts connect
- [Current State of main (as of 2026-11-26)](../status/current-state.md): where the project stands today
- [Technology Stack Overview](../technologies/stack-overview.md): technologies used
- [Hard Constraints Set by the Owner](../convention/hard-constraints.md): rules nobody may break
- [Repository Layout](repository-layout.md): where things live in the repository
- [Project History and Timeline](history.md): how the project got here
- [Block Avatar: Feature Inventory](block-avatar-features.md): what the streaming app does
- [Vox Online: Player Journey and Worlds](vox-online.md): what the online world does
- [Characters, Props, Sets and Content Licences](characters-and-content.md): the characters, props, sets and licences
- [Work Tracking: GitHub Issues, beads and okf](../convention/work-tracking.md): how work is tracked
