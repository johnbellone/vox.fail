---
type: Decision
title: "Wheels Replace the Hotbar (Block Wheel on LB, Emote Wheel on RB)"
description: "The owner's 2026-11-28 decision: no on-screen hotbar; a held block wheel (LB/Tab) with All-blocks and Props sectors, and a held emote wheel (RB/G) with eight per-device emotes and target snapping."
tags: [decision, controls, hud, gamepad, emotes, props]
generated: { by: agent/mcp, at: "2026-11-28T05:13:53Z" }
status: stable
---

## Decision (owner, 2026-11-28)

- **No hotbar on screen.** What is in your hand shows small, bottom right. Keys 1-9 and the mouse wheel still pick the world's quick blocks (the `hotbar` world setting).
- **Block wheel**: hold the left bumper (Tab). Quick blocks on the top arc; two bottom sectors, *All* and *Props*, open a second wheel (16 per page, pages by left stick or mouse wheel). A block from All goes into the current quick slot; a prop goes in your hand and Place sends `ToServer::Put` (request ids from `0x8000_0000`, so refusals are told from the editor's).
- **Emote wheel**: hold the right bumper (G). Eight emotes chosen on the menu's Emotes screen, kept on this device (`emotes.json` / localStorage `vox.emotes`). Right stick picks; left stick snaps the nearest player, prop or doorway within 16 m. On release you face it and the clip plays once; walking stops it. Others see it through the pose stream, with no new message.
- **Migration**: saved controls carry a layout version; older files move the bumpers to the wheels once, and voice channel takes the freed d-pad left.
- **Default emotes** are neutral clips only (hard constraints).

## Where

`crates/vox-world/src/wheel.rs`, `emote.rs`, `hud.rs`, `client/controls.rs`, `input/bindings.rs`, `menu.rs`, `bins/vox-client/src/lib.rs`. The full design is in `docs/design/controls-and-hud.md`, section "Wheels instead of a hotbar". Related: [[architecture/controls-and-hud-design]].
