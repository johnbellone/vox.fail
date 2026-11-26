---
type: Decision
title: "Decision: Player Help Is Markdown in docs/help, Rendered by the Site"
description: "The player help lives once, as markdown in docs/help/, and the site's /help page bundles those files at build time instead of copying the text."
tags: [decision, docs, site, help]
generated: { by: agent/mcp, at: "2026-11-26T21:54:59Z" }
status: stable
---

# Decision

Player help is nine short markdown pages in `docs/help/` (index, getting-started, controls, building, worlds, accounts, avatar, troubleshooting, faq). The site page `/help` (apps/site, `features/help`) imports them with Bun's text loader and renders them with react-markdown and remark-gfm. Raw HTML is not rendered; only http, https and mailto links survive; links between help pages become hashes (#controls); other relative links point at the file on GitHub. The directory serves `/help` (`bins/vox-directory/src/state.rs`).

# Why

One source means the repository and the website cannot drift apart. Pages stay one screen each so one haiku writer can own one page.

# Notes

- Controls come from `crates/vox-world/src/input/bindings.rs`; the code wins over older prose. Chat is bound but does nothing yet, so it is not documented. Weather and `world:` commands work only from the Block Avatar control page.
- `tools/dev/check_links.sh` checks relative links in README, AGENTS.md, docs/help and docs/vox/README.md.

# Related Concepts
- [Decision: Haiku Writes, Sonnet Reviews, Beads Plan the Work](haiku-writes-sonnet-reviews.md): how the pages were produced
