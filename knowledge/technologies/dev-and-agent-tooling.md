---
type: Technology
title: "Developer and Agent Tooling: okf, beads, pi, MCP"
description: The agent-facing tools set up around the repository and where their configuration lives.
tags: [technology, okf, beads, mcp, agents]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Developer and Agent Tooling

| Tool | Role | State on 2026-11-26 |
| --- | --- | --- |
| **okf** CLI v0.9.0 (okf-memory/okf-agent-memory, Go, MIT) | Git-native agent memory in `knowledge/`: BM25 search, validate, create, update, relate, MCP server | Installed at `~/.local/bin/okf`, checksum verified. Implements OKF v0.3 (reads v0.2). |
| **okf MCP server** | Native `okf_*` tools to the agent | `.mcp.json` server `okf-memory` = `okf mcp knowledge`; needs one-time project approval in pi. Seven tools: `okf_search`, `okf_show`, `okf_validate`, `okf_create`, `okf_update`, `okf_relate`, `okf_changes`. |
| **AGENTS.md** | Project rules for agents (AAG micro-syntax); `CLAUDE.md` symlinks to it; skill in `.agents/skills/okf-memory/` | Present |
| **beads** (`bd`) | Agent task tracking, git-backed in `.beads/` | **Not installed**; no `.beads/` |
| **GitHub Issues** | User bugs and features | Repo: johnbellone/vox.fail |
| **pi** coding agent | Runs agents here; permissions extension, codemode, MCP adapter | Global config in `~/.pi/agent/`; codemode enabled via `defaultTools` |
| **Bun** (1.x), TypeScript 7, React 19, Mantine 9 | The site (`apps/site`): `bun install`, `bun run build`, `bun test` | Used on main (not run in this analysis) |
| **Playwright, Xvfb, Mesa lavapipe** | Headless browser and render tests | Used by `web_e2e.py`, `visual_test.py`, CI |

## okf conventions used here
- Concepts in `knowledge/{project,architecture,technologies,decisions,convention,status}/`.
- Agents declare `generated: {by, at}`; `verified:` is human-only and has never been set.
- Governance: `convention/` defaults to *constraint*; others to *context*; concepts bind to code through `code_refs`.
- Validate before finishing: `okf validate knowledge --strict --drift`.

See `convention/work-tracking`.
