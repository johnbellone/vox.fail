---
type: Decision
title: "Decision: Stay on Bevy 0.19.1"
description: The owner requires Bevy to stay at 0.19.1; upgrades need explicit approval.
tags: [decision, bevy, dependencies]
generated: { by: agent/mcp, at: "2026-11-27T03:55:18Z" }
status: stable
governance: constraint
code_refs: [Cargo.toml, crates/vox-world/Cargo.toml, bins/vox-client/Cargo.toml]
---

# Decision: Stay on Bevy 0.19.1

**Status**: in force (owner constraint, listed in `convention/hard-constraints` and `AGENTS.md`).

**Context**: Bevy changes its API on each minor release; the code base is about 31k lines across the app and `vox-world`, with custom shaders, a PostUpdate face/pose layer and many render-graph details that would break.

**Decision**: use Bevy 0.19.1 only; `cpal` stays at 0.17 to match its audio backend.

**Consequences**: no engine upgrades without approval; check the engine version in any new dependency (plugins must support 0.19). `Cargo.toml` says `bevy = "0.19.1"` (caret): consider `=0.19.1` to enforce it. The rationale text beyond the owner's instruction is inferred, not recorded in the project documents.

## Update, 2026-11-26
Bevy 0.20.0 (2026-10-08) and 0.20.1 (2026-10-28) are out. While asking for the ecosystem analysis, the owner said upgrading is **not ruled out**. That opens the question; it does not approve an upgrade, so the pin stays until the owner says to upgrade.

The analysis (`technologies/bevy-ecosystem`, `docs/design/bevy-ecosystem.md`) recommends staying on 0.19.1 for now:
- No crate on the owner's list needs 0.20.
- Upgrading today would lose six of them until they publish 0.20 releases: big_space, bevy_firework, bevy_svg, bevy_feronia, transform-gizmo-bevy, bevy_animation_graph.
- The cost is real: all 9 shaders use naga_oil syntax that 0.20 replaces with WESL; `bevy_pbr` module paths moved; wgpu and naga go 29 to 30; text and font APIs changed.

Plan the upgrade as its own step when the adopted crates have caught up and Bevy itself gives a reason (mesh shaders for desktop chunks, the scene notation BSN, UI widgets), starting with moving the shaders to WESL.

See `technologies/bevy`.

# Related Concepts
- [Bevy Ecosystem Crates: Use, Copy, Watch or Skip](../technologies/bevy-ecosystem.md): The analysis behind staying on 0.19.1 for now.
