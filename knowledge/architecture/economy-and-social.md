---
type: Architecture
title: "Authority, World Currencies, Feats, Plots, Portals and Raids"
description: "The player economy and social graph in the directory: the shared Authority currency, what it buys, world coins, the ledger, friends, groups and raid parties."
tags: [architecture, economy, game-design, directory]
generated: { by: agent/mcp, at: "2026-11-28T03:24:20Z" }
status: stable
code_refs: [bins/vox-directory/src/game.rs, bins/vox-directory/src/people.rs, apps/site/src/features/inventory/, apps/site/src/features/friends/]
---

# Economy and Social Graph

Code: `bins/vox-directory/src/game.rs` and `people.rs`; pages in `apps/site/src/features/inventory/` and `friends/`. Added in commit `ca58e25`.

## Authority (AUTH)
The one currency every world shares.
- **Earned**: 25 on sign-up; 1 per 10 minutes in a world; 1 per **200** blocks placed (it was 50 until blocks became 0.25 m, and a wall of the same size now takes four times the blocks); up to 60 a day in all; 15 for each raid your party wins.
- Earnings come from the per-minute world reports, which may report at most 4,800 blocks placed per player.
- It **cannot be sent**; it is spent on feats.

## Feats (spend AUTH)
| Feat | Cost | Effect |
| --- | --- | --- |
| Claim a plot | 20 | **64 x 64 blocks (16 m)** around where you last stood, at least 12 m (48 blocks) from the spawn, no overlaps, **6 per world**. Only you (or a group you share it with) can build there; others get `Why::Claimed`. Plots claimed before the change were doubled with the blocks, so they cover the same ground (32 x 32 blocks). |
| Open a portal | 30 | A doorway in your plot to another world that has reported. Appears only to players who arrive after it opens. 2 to 4 m wide, 3 to 8 m high. |
| Issue a currency | 100 | The world's **owner** only, one per world, with a fixed supply minted to them. Anyone can then send it to anyone. |
| Start a raid | 10 | A party leader picks a world. |

## Ledger and inventory
- Every movement is a ledger row: minted, spent or sent.
- Items: deeds, portal keys, banners and trophies. The inventory API adds coins and the wardrobe.
- The `/inventory` page filters by kind and world, searches, sorts, and sends coins.

## Social
- **Friends** are mutual: one asks, the other accepts.
- **Groups** (up to 24) are friends who build together. A plot shared with a group lets every member build.
- **Raid parties** (up to 8) go somewhere together.
  - A raid is won when two or more party members appear together in ten world reports from the target world within an hour. Those present get 15 AUTH and a trophy.
  - The leader role passes on when the leader leaves, and the last one out ends the crew; a group's shared plots return to their owners.

## Operators
- `vox-directory --db directory.db admin own-world void "Sam Hill"` makes someone a world's owner.
- `admin grant "Sam Hill" 50` gives AUTH.

## Trust model and risks
- World servers are trusted to report honestly (shared secret).
- Plots stop *edits*, but not other players walking in. There is no per-world secret or limit yet.
- Reports come once a minute, so a raid needs ten of them.

See `status/risks-and-open-issues` (risk 30) and `decisions/authority-not-transferable`.

# Related Concepts
- [Decision: Authority Cannot Be Sent](../decisions/authority-not-transferable.md): rationale for non-transferable authority
- [Decision: Structures Have an Owner and Use, Copy and Edit Rights](../decisions/structure-entitlements.md): Who may use, copy and edit a structure; plots' builders are meant to edit their plot's structure.
- [Quarter-metre blocks](quarter-metre-blocks-decision.md): why plots and authority changed
