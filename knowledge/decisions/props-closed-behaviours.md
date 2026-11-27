---
type: Decision
title: "Decision: Prop Behaviours Are a Closed Vocabulary, Never Code"
description: "A prop can only do what a fixed list of declarative behaviours allows, so any prop is as safe to accept as a block and the server stays authoritative."
tags: [decision, props, security, behaviours, vox-server]
generated: { by: agent/mcp, at: "2026-11-27T03:35:24Z" }
status: stable
---

# Decision: Prop Behaviours Are a Closed Vocabulary, Never Code

**Decision (proposed with the design, 2026-11-26)**: a prop's behaviours are chosen from a small closed set: `light`, `seat`, `door`, `switch`, `readable`, `sound`, `motion` and `marker` (later `container` and `pickup`). Each is declarative data with parameters in checked ranges. A prop has no scripts and no other way to act.

**Why**: props will arrive from recipes written by language models, hand imports and, later, players. Data validated against Rust types cannot run code, reach the network or exhaust a server, which is the same property the world-graph design gives recipes. It also keeps authority where the project already puts it: the server decides state changes (reach, plot rules, rate limits) and clients only draw and play them, so props cannot be a route for cheating.

**Consequences**: a new kind of behaviour is a change to the world server and clients, not a new prop. Visual-only motion (`motion`) is deterministic from the clock and never gameplay. The only player text that reaches others through a prop is `readable`, which needs the same filtering as names and chat (an open question). Puzzles are built by combining behaviours (a `switch` on a channel opening a `door`), which is also how dungeon templates are dressed.

See `architecture/prop-definition` and `architecture/props-in-worlds`.
