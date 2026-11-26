---
type: Decision
title: "Decision: Structures Have an Owner and Use, Copy and Edit Rights"
description: "A structure has one owner who grants use, copy or edit to named accounts or opens it to everyone to use or copy; world servers check with the directory before placing one."
tags: [decision, structures, entitlements, directory, game-design]
generated: { by: agent/mcp, at: "2026-11-26T22:11:34Z" }
status: stable
---

# Decision: Structures Have an Owner and Use, Copy and Edit Rights

**Decision**: a structure (a versioned blueprint) has an owner and rights for others: use (place it), copy (make a structure of their own from it) and edit (save new versions). Each right includes the ones before it. Only the owner grants, revokes, opens it to everyone and deletes. "Open to everyone" is use or copy for signed-in accounts, never edit; guests have no rights. The owner chose "owner plus grants" over owner-only and over authority-priced use.

**Where**: the rules are `vox_core::entitle` (pure, tested); the records and API are `bins/vox-directory/src/structures.rs`; a world server asks `GET /api/world/structures/{id}?account=&version=` (world secret) before placing one and checks on every placement, so revoking takes effect at once.

**Details**: versions are immutable and the head is the newest; only editors may place or see older versions. A copy is a new structure owned by the copier with `derived_from` recording the source. Someone without a right is told the structure does not exist (404). Limits: 256 grants a structure, 200 structures an account, 500 versions each.

**Not built yet**: placing structures in worlds and plot-derived structures (design phase 6); plot builders are meant to get edit while they remain builders.

See `architecture/world-format-design` and `architecture/economy-and-social`.
