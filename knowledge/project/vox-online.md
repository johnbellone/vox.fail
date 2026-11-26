---
type: Fact
title: "Vox Online: Player Journey and Worlds"
description: "How a player gets from the vox.fail web page into a world, what they can do there, and which worlds exist today."
tags: [vox, online, worlds, players]
generated: { by: agent/mcp, at: "2026-11-26T15:49:37Z" }
status: stable
---

# Vox Online: Player Journey

## Flow
1. Open https://vox.fail. Sign in with **Steam** or **Discord**, or make a **name and password** account.
2. Pick a character (38 choices, Blue first) and what to wear: a hat, a face piece, something to hold, seen on a turntable (three.js preview). **Save character**.
3. **Enter the Void** in the browser (needs WebGPU), or copy the **launch code** and run `vox-client --token <code>` on the desktop. A launch code lasts one day.
4. In a world: walk, fly, build and remove blocks. Other players appear as their own characters.

## Controls (client)
Diablo-style camera from above (wheel zooms 4-45 m, Q/E turn in 45 degree steps), left click walks, WASD walks, Space jumps, F flies, right click places a block, Shift+right click removes one, 1-9 or Shift+wheel choose the block, V switches to a behind-the-shoulder camera. Controller supported (not verified on hardware).

## Worlds today
| World | Port | Level file | Look |
| --- | --- | --- | --- |
| The Void | 7780 | `data/levels/void.json` | Flat, bright, ethereal: endless polished floor in white fog, three faint moons, a circle of pale pillars where you arrive, the First Gate (north), the Ember Door (west), monoliths far off. |
| The yard | 7781 | `data/levels/yard.json` | Back yard with a brick house, a cobblestone path, lamp posts, trees, a snowy hill. Earth sky with sun and clouds. |
| The islands | 7782 | `data/levels/islands.json` | About 480 x 480 blocks of floating islands over an abyss in dark fog; 404,808 blocks, 207 fires. Reached through the Ember Door. |

Portals link them by port (`":7781"`). A level can also list fixed camera shots for places (the First Gate has one).

## Persistence
A world's blocks and edit log are stored in SQLite, so what players build is kept. A new database starts from a seed level.

## Not yet in worlds
Voice, name tags above heads, text chat, undo, accounts per world beyond sign-in, server-side movement checks, and showing what other players wear (the hello carries the model only). See `status/risks-and-open-issues`.

Details: `architecture/vox-server`, `architecture/vox-directory`, `architecture/economy-and-social`, `architecture/worlds-catalogue`.
