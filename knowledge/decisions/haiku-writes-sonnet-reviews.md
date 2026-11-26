---
type: Decision
title: "Decision: Haiku Writes, Sonnet Reviews, Beads Plan the Work"
description: "Agent work is split into haiku-sized beads (one writer, under 100k tokens, named files, acceptance command) and every writer bead is reviewed by sonnet before the lead closes it."
tags: [decision, agents, beads, process]
generated: { by: agent/mcp, at: "2026-11-26T21:54:59Z" }
status: stable
---

# Decision

Agent work in this repository follows one pipeline:

- The lead plans as a beads epic with children. Each child names files to read, files to change and an acceptance command.
- `vox-writer` (haiku, `.pi/agents/vox-writer.md`) implements exactly one bead using the directed task template in AGENTS.md. It stays under about 100k tokens of context and never claims or closes beads, commits, or edits outside its files.
- `vox-reviewer` (sonnet, `.pi/agents/vox-reviewer.md`) is read-only; it checks facts against code and data and returns PASS, PASS WITH FIXES or FAIL. The lead fixes blocker and major findings, then closes the bead.

# Why

Small models are fast and cheap but drift when given vague or large tasks. Narrow beads and an independent stronger reviewer keep accuracy up. In the first run the reviewer found real errors (a chat binding that does nothing, weather commands a player cannot type, a stale controls paragraph) that the writers had copied from sources.

# Consequences

- AGENTS.md has sections Read this first, Models and subagents (with task templates) and Beads workflow.
- beads is installed (`npm install -g @beads/bd`), initialised with prefix `vox`, usage metrics off.
- Change models in `.pi/agents/`, not in prompts.

# Related Concepts
- [Work Tracking: GitHub Issues, beads and okf](../convention/work-tracking.md): which tracker holds what
