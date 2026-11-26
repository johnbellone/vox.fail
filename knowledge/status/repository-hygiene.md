---
type: Status
title: Repository and Disk Hygiene Findings
description: "Untracked runtime files, a 141 GB worktree build directory, an unpushed branch, and other housekeeping items found during analysis."
tags: [status, hygiene, git, disk]
generated: { by: agent/mcp, at: "2026-11-26T17:38:54Z" }
status: stable
stale_after: 2026-12-26
---

# Repository and Disk Hygiene

| Finding | Detail | Suggested action |
| --- | --- | --- |
| **141 GB `.claude/`** | The worktree `.claude/worktrees/project-build-daed3d` has a 140 GB `target/` plus `node_modules` and `*.db`. Its commits are now on `origin/main`. `.claude/` is untracked and not ignored. Main's own `target/` is 13 GB (old flat layout builds are stale). | Remove the worktree (`git worktree remove`) or `cargo clean` in it; add `.claude/` to `.git/info/exclude`. |
| **Stale local branches** | `claude/project-build-daed3d` (merged into main), `vox` (old main) and a local `okf-flat-layout-backup` (my backup of the first version of this work). | Delete when no longer needed. |
| **Runtime files in repo root** | `avatar_settings.txt`, `hotkeys.txt`, `clips/` (24 MB of MP4/PNG), `questions/` appear untracked after running the app or tests from source; only `gags.txt`, `*.db`, `*.log` are ignored. | Add them to `.gitignore` or set `BLOCK_AVATAR_HOME` to a scratch folder when running. |
| Tests write into the project | Tests create files in the working tree. | Point tests at a temp dir. |
| Secrets | `directory.key*` are ignored; none tracked. | Never commit `/etc/vox/*.env` contents. |
| LFS flag without LFS | Workflows check out with `lfs: true` but there is no `.gitattributes`. | Remove the flag or adopt LFS for 193 GLBs and 110 MB of assets. |
| Large binary history | 984 tracked files; assets 110 MB. | Consider LFS before it grows. |
| No git identity configured | This clone has no `user.name`/`user.email`; commits need one. | `git config` for this repo. |

Counts observed 2026-11-26. Related: `status/restructure-and-follow-on-work`.

# Related Concepts
- [Restructure and Follow-on Work (the former branch, now on main)](restructure-and-follow-on-work.md): where the large worktree came from
