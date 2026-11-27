---
type: Decision
title: "Decision (superseded): Stay on Bevy 0.19.1"
description: Superseded on 2026-11-27 by the owner-approved upgrade to Bevy 0.20.1; kept for history.
tags: [decision, bevy, dependencies]
generated: { by: agent/mcp, at: "2026-11-27T04:40:12Z" }
status: deprecated
governance: constraint
code_refs: [Cargo.toml, crates/vox-world/Cargo.toml, bins/vox-client/Cargo.toml]
---

# Decision (superseded): Stay on Bevy 0.19.1

**Status**: superseded on 2026-11-27 by `decisions/bevy-0-20-upgrade`. The owner said "Do the upgrade", and the workspace moved to Bevy 0.20.1. The same kind of rule applies to the new version.

**History**: the owner required Bevy 0.19.1. Bevy changes its API on each minor release, and the code base (about 31k lines across the app and `vox-world`, with custom shaders, a PostUpdate face/pose layer and render-graph details) would break. `cpal` was kept at 0.17 to match Bevy's audio backend, and it still is.

On 2026-11-26 the owner said an upgrade was not ruled out, and asked for an ecosystem analysis (`technologies/bevy-ecosystem`). The analysis first advised waiting. A trial upgrade then showed the work was small (WESL shaders, one menu change, tonemapping names), and the owner approved it.
