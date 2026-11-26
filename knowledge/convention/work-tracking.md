---
type: Process
title: "Work Tracking: GitHub Issues, beads and okf"
description: "Which tracker holds what: GitHub Issues for users, beads for agent tasks, okf for durable project knowledge."
tags: [process, workflow, beads, github, okf]
generated: { by: agent/mcp, at: "2026-11-26T15:57:58Z" }
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
2. An agent creates beads for the work and puts `gh-42` in each title or description. Big work becomes an epic with child tasks and dependencies.
3. Agents choose work with `bd ready`, claim it, do it, close it with a reason. New findings become new beads linked to the current one, not TODO comments.
4. When every bead for the issue is closed, the pull request says `Fixes #42`.
5. Any decision or discovery that a future agent needs goes into okf (`okf_create` / `okf_update`).

## Rules
- Users never need to touch beads. Agents do not open GitHub issues for internal task breakdowns.
- A bug an agent finds that affects users goes to GitHub Issues, never beads only.
- Before proposing architecture or a new dependency: `okf_search`. Before editing a path: `okf_search(for_path=...)`.
- Before finishing code changes: `cargo test --release --workspace`.
- Finish by running `okf_validate(strict=true, drift=true)` with 0 errors and 0 warnings.

## State on 2026-11-26
okf is set up (CLI v0.9.0, MCP server in `.mcp.json`). **beads is not installed yet**: no `bd` binary and no `.beads/` directory. The commands in README.md were written from memory and are unchecked against the installed version.

The rules above are mirrored in `AGENTS.md` and the README section "How we track work".

# Related Concepts
- [Developer and Agent Tooling: okf, beads, pi, MCP](../technologies/dev-and-agent-tooling.md): the tools involved
