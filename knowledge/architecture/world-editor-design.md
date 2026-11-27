---
type: Design
title: "Design (proposed): The World Editor on the Web"
description: "An editor at vox.fail/edit: workshops (persistent worlds you own for building), an edit mode in the same client, whole operations checked by the server with undo, live world settings, and saving as a level or structure."
tags: [design, proposed, editor, web, levels, structures, workshops]
generated: { by: agent/mcp, at: "2026-11-27T21:09:20Z" }
status: draft
---

# Design (proposed): The World Editor on the Web

Source: `docs/design/world-editor-workshops.md` (2026-11-27; it was `docs/design/world-editor.md` until it met the editor that was built at the same time, which kept that name). **Status: proposed; nothing of this design built.** Asked for by the owner after finding there is no level editor (levels are JSON written by hand or by Python builders; in-game building edits one block at a time and is never exported).

**Against what exists**: the live editor of `decisions/world-editor` is built: the `/edit` page, edit mode in `vox-world`, protocol 9 (entity `Put`/`Remove`/`Build`/`Unbuild`), the `vox_core::edit` rules, the `editor_command` bridge, undo of one's own changes, Basic and Advanced rights. This design's names collide with some of that (protocol 9, `vox-core::edit`, the page's bridge); when it is planned, its ops and workshops extend those rather than redefine them.

## Shape
- **Workshops**: persistent worlds an account owns for building, kept with snapshots; owner plus edit or visit grants (the structure rights model). New host work: add a persistent world at run time (instances are in-memory and thrown away) and keep it across restarts; the directory stores workshops and seeds them from blank ground, a shipped level, a snapshot or a structure.
- **Edit mode** in `vox-world` (desktop and browser): free flight, selection box, ghost preview, tools (box solid/hollow/round/walls, replace, copy/cut/paste with rotate and mirror, eyedropper, bounded flood fill).
- **Ops on the server**: `ToServer::Op {Fill, Replace, Paste}`, `Undo`, `Redo`, expanded by shared edit rules (with prediction, checked arithmetic). Editors come from an `editors` list in the directory's world rules (refreshed every 30 s; guests never edit). Caps: 64^3 cells an op, a cell-counted token bucket, paste blobs at most 1 MB; ops touching someone's plot are refused whole; new `Why` values appended. Ops stored compactly in an `ops` table (not a row per cell); undo of your own ops only, putting back old values where the cell still holds the op's new value, for as long as snapshots keep them (a week). Big ops send chunks mid-visit (new client work).
- **Live settings**: only the world's own keys are stored (directory portals and voice_radius stay added at send time); weather goes the existing climate path. Own small gizmo until transform-gizmo-bevy supports Bevy 0.20.
- **The page**: `/edit` in React/Mantine around the wasm canvas, with commands and events across a bridge.
- **Saving**: a workshop is a world (snapshots and a new live restore); Save as level (greedy box JSON with a round-trip test; later a world-format level file); Save as structure.

## Rollout
1 edit ops; 2 edit mode by commands; 3 workshops (directory, host, rules); 4 the page and bridge; 5 live settings and gizmos; 6 saving and live restore; 7 presence for several editors.

## Settled
The owner's answers are `decisions/world-editor-scope`: public worlds change through workshops and review, three workshops an account, visit links later, no touch editing at first, props as a tab in the editor.

Related: `architecture/world-format-design`, `decisions/structure-entitlements`, `architecture/wire-protocol`, `architecture/vox-server`, `decisions/world-editor`.

# Related Concepts
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](../decisions/structure-entitlements.md): Workshop rights follow the structure rights model; the editor saves selections as structures.
- [Design (phase 1 built): The World Format](world-format-design.md): Snapshots, live restore and levels as files that the editor builds on.
- [Decision: The World Editor's Scope (Public Worlds, Workshops, Visits, Touch, Props)](../decisions/world-editor-scope.md): The owner's answers to the design's open questions.
- [Decision: A Live In-Browser World Editor Built on World Entities](../decisions/world-editor.md): The editor already built, which this design extends.
