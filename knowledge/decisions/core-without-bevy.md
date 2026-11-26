---
type: Decision
title: "Decision: Keep vox-core Free of Bevy"
description: "Shared game rules live in an engine-free crate so servers, clients and future pocket worlds run identical code."
tags: [decision, architecture, vox-core]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: vox-core has no Bevy

**Context**: block rules (edit checks, prediction, raycasts, movement, weather, token verification) must behave identically on the server, in the desktop client and in the browser.

**Decision**: put them in `vox-core` with only serde, postcard, ed25519-dalek and base64 as dependencies. The crate description says: "No Bevy, so the app, the server and (later) pocket worlds all run the same code."

**Consequences**: the server compiles fast and ships small; unit tests run in milliseconds (44 tests in 0.37 s); the client predicts edits with the server's own rules. The cost is a thin separation between simulation (core) and presentation (`vox-world`) that must be maintained.

See `architecture/vox-core`.
