---
type: Requirement
title: Hard Constraints Set by the Owner
description: "Non-negotiable content, platform and licensing rules; breaking any one of them is a defect."
tags: [constraints, content, legal, governance]
generated: { by: agent/mcp, at: "2026-11-27T04:40:31Z" }
status: stable
governance: constraint
code_refs: [Cargo.toml, assets/, source/vox/, testdata/, demo/, data/characters.json, tools/]
---

# Hard Constraints

These come from the project owner (see `docs/avatar/PROJECT.md` and `AGENTS.md`). An agent MUST treat a breach of any item as a defect and stop to ask.

1. **No likenesses of real people and no trademarked characters.** All characters are original. Requests for named politicians and for Mr. Peanut were declined. Nothing in the cast is based on a real person.
2. **No synthetic voices.** Test and demo audio is real, unmodified human recordings. Viewers are heard in their own recorded voice, never changed or synthesised.
3. **The medic uses a green cross.** The red cross emblem on white is legally protected.
4. **No combat poses that depict assaulting police.** Police animations are defensive only.
5. **Stay on Bevy 0.20.1.** Never upgrade without the owner's say-so. The owner approved the move from 0.19.1 to 0.20.1 on 2026-11-27. See `decisions/bevy-0-20-upgrade`.
6. **Sound effects are real recordings (CC0)**, only trimmed and levelled.

## Related backlog constraint
A "January 6 Capitol world" is on the backlog and not started. If it is ever built, the same rules apply (invented characters only, defensive police animations), and the owner wants Twitch and YouTube rules on streaming real violent events settled *before* any modelling starts. See `status/roadmap`.

## Why governance is "constraint"
Edits to characters, props, sounds, test audio or dependencies must follow these rules.

# Related Concepts
- [Decision: Upgrade to Bevy 0.20.1, and Stay There](../decisions/bevy-0-20-upgrade.md): constraint 5 in detail
- [Characters, Props, Sets and Content Licences](../project/characters-and-content.md): content the rules apply to
