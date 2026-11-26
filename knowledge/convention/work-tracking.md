---
type: Process
title: "Work Tracking: GitHub Issues, beads and okf"
description: "Which tracker holds what: GitHub Issues for users, beads for agent tasks, okf for durable project knowledge."
tags: [process, workflow, beads, github, okf]
generated: { by: agent/mcp, at: "2026-11-26T21:54:51Z" }
status: stable
---

# Work Tracking

| Tool | For | Holds |
| --- | --- | --- |
| **GitHub Issues** (johnbellone/vox.fail) | People: users and contributors | Bugs, feature requests, behaviour changes, questions. The public record of *what* and *why*. |
| **beads** (`bd`) | Agents | Implementation tasks, dependencies, follow-ups found during work, hand-off notes. The *how*. Data lives in `.beads/`, committed with the code. |
| **okf** (`knowledge/`) | Agents and humans | Durable knowledge: decisions, architecture, status. Survives context resets. |

## Flow
1. A person opens a GitHub issue (say `#42`).
2. An agent creates beads for the work and puts `gh-42` in each bead (`--external-ref`). Big work becomes an epic with child tasks and dependencies.
3. Agents choose work with `bd ready`, claim it, do it, close it with a reason. New findings become new beads linked to the current one, not TODO comments.
4. When every bead for the issue is closed, the pull request says `Fixes #42`.
5. Any decision or discovery that a future agent needs goes into okf (`okf_create` / `okf_update`).

Who does what (haiku writers, sonnet reviewers, the lead) is in [Decision: Haiku Writes, Sonnet Reviews, Beads Plan the Work](../decisions/haiku-writes-sonnet-reviews.md).

## Rules
- Users never need to touch beads. Agents do not open GitHub issues for internal task breakdowns.
- A bug an agent finds that affects users goes to GitHub Issues, never beads only.
- Before proposing architecture or a new dependency: `okf_search`. Before editing a path: `okf_search(for_path=...)`.
- Before finishing code changes: `cargo test --release --workspace`.
- Finish by running `okf_validate(strict=true, drift=true)` with 0 errors and 0 warnings.

## State on 2026-11-26
beads is installed (`npm install -g @beads/bd`, v1.3.4) and initialised in this clone with prefix `vox` (`bd init --non-interactive --prefix vox --skip-agents --skip-hooks`); usage metrics are off. The `.beads/` Dolt database is ignored by git; its config files are tracked.

The rules above are mirrored in `AGENTS.md` (sections Beads workflow and Models and subagents) and the README section "How we track work".

# Related Concepts
- [Developer and Agent Tooling: okf, beads, pi, MCP](../technologies/dev-and-agent-tooling.md): the tools involved
