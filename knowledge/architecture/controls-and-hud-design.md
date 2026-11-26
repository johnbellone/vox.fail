---
type: Design
title: "Design (phases 1-4 built): Block-Game Controls, HUD and Menus"
description: "Action-based input with gamepad hints per brand, first-person view, pixel HUD and controller-navigable menus for Vox worlds."
tags: [design, controls, hud, gamepad, branch]
generated: { by: agent/mcp, at: "2026-11-26T17:38:31Z" }
status: draft
stale_after: 2026-12-26
---

# Design: Controls, HUD and Menus

Source: `docs/design/controls-and-hud.md`, status **phases 1-4 built (creative mode)**, 2026-11-26; code in `crates/vox-world/src/{input,hud,menu}` and `client/`.

## Goals
Familiar block-game play: first-person aim with a crosshair, break and place, hotbar of nine, inventory, health. Every action works on **keyboard and mouse and on a gamepad with no setup**; on-screen hints show the button of the device **last touched**, drawn for its brand (Xbox, PlayStation, Nintendo, Steam); every menu navigable by stick or d-pad; controls rebindable per device.

## Input model
**Actions, not keys**: a layer maps devices to actions. Before phase 1 the world client read the keyboard, mouse and gamepads directly (South jumps, North toggles flying, triggers break and place), has a Diablo-style or behind-the-shoulder camera, no first person, no pointer lock in the browser, a 9-slot text hotbar, no menus, unlimited building.

## Phases
| Phase | Content | State |
| --- | --- | --- |
| 1 | Action layer, device detection, glyph sheets and hint bar, first-person view with pointer lock | Built |
| 2 | HUD in `bevy_ui`: crosshair, hotbar with block icons, notices | Built |
| 3 | Menus with directional navigation: pause, controls (rebinding), settings | Built |
| 4 | Inventory as a creative block palette (default mode), quick wheel (hold Tab) | Built |
| 5 | Server-side survival inventories; health and damage | Not built; needs server-side movement checks (now built: `vox-core::moves`) |

"New worlds default to **creative**" is recorded as a design decision (2026-11-25).

## Non-goals for now
Crafting, hunger, combat with creatures, touch controls.

## Open questions
Which damage sources come first; penalty for dying; whether the account inventory (coins, wardrobe) needs in-game actions.

Related: `architecture/vox-world`, `architecture/vox-client-and-web`.
