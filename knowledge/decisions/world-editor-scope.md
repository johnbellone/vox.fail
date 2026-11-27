---
type: Decision
title: "Decision: The World Editor's Scope (Public Worlds, Workshops, Visits, Touch, Props)"
description: "Public worlds are changed by exporting from a workshop and review, three free workshops an account, read-only visit links later, desktop-only editing at first, and props as a tool inside the editor."
tags: [decision, editor, workshops, levels, scope]
generated: { by: agent/mcp, at: "2026-11-27T20:58:00Z" }
status: stable
---

# Decision: The World Editor's Scope

Made 2026-11-27: the owner said "use your recommendations" for the open questions in `docs/design/world-editor.md`.

1. **Public worlds aren't edited in place.** Copy one into a workshop, build there, export a level; changes to shipped worlds are reviewed as a pull request against `data/levels/` (or the builder in `tools/assets/` that generates it). Staff replacing a live public world from a workshop, after a snapshot, may come later.
2. **Three workshops an account**, default world size (512 x 128 x 512 blocks), free (no Authority) to begin with; history is the usual week of snapshots; storage isn't counted against accounts until measured.
3. **Read-only visit links: yes, later** (unlisted, revocable), after the editor's seven rollout steps.
4. **No touch editing in the first version**: desktop browsers with mouse and keyboard; touch devices are told so and offered play.
5. **Props get a tool in the editor** (a Props tab beside the blocks), not a page of their own.

**Why:** each keeps the first version small and safe: no live edits to shared worlds without review, bounded storage, and no input model the client doesn't have yet.

Related: `architecture/world-editor-design`.
