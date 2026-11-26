---
type: Decision
title: "Decision: Stay on Bevy 0.19.1"
description: The owner requires Bevy to stay at 0.19.1; upgrades need explicit approval.
tags: [decision, bevy, dependencies]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
governance: constraint
code_refs: [Cargo.toml, crates/vox-world/Cargo.toml, bins/vox-client/Cargo.toml]
---

# Decision: Stay on Bevy 0.19.1

**Status**: in force (owner constraint, listed in `convention/hard-constraints` and `AGENTS.md`).

**Context**: Bevy changes its API on each minor release; the code base is about 31k lines across the app and `vox-world`, with custom shaders, a PostUpdate face/pose layer and many render-graph details that would break.

**Decision**: use Bevy 0.19.1 only; `cpal` stays at 0.17 to match its audio backend.

**Consequences**: no engine upgrades without approval; check the engine version in any new dependency (plugins must support 0.19). `Cargo.toml` says `bevy = "0.19.1"` (caret): consider `=0.19.1` to enforce it. The rationale text beyond the owner's instruction is inferred, not recorded in the project documents.

See `technologies/bevy`.
