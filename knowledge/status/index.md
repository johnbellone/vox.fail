# Status
* [Current State of main (as of 2026-11-26)](current-state.md) - A dated snapshot of main at 7c4e517: maturity, metrics, what works, what is unverified and what is not started.
* [Restructure and Follow-on Work (the former branch, now on main)](restructure-and-follow-on-work.md) - The 29 commits of 2026-11-25/26 now on main: restructure into crates/bins/apps, controls, site rewrite, world format, hardening and design docs.
* [Test Results on main (2026-11-26 run)](test-results.md) - Measured output of cargo test --workspace --release on main at 7c4e517, per crate, and what was not run.
* [Verification Gaps: What Has Never Been Run](verification-gaps.md) - Everything that is written and unit-tested but never exercised on its real target, with the way to verify each one.
* [Risks and Open Issues (re-checked 2026-11-26)](risks-and-open-issues.md) - The ranked risk list from docs/avatar/PROJECT.md with a fresh verdict per item against main at 7c4e517: still true, fixed by the restructure work, or not re-checked.
* [Roadmap and Next Steps](roadmap.md) - The planned milestones from the project document, updated with what the restructure and follow-on commits already delivered.
* [Documentation Drift: Where the Docs Disagree with the Code](documentation-drift.md) - A list of stale or contradictory statements in docs/PROJECT.md and the READMEs, so agents know which source to trust.
* [Repository and Disk Hygiene Findings](repository-hygiene.md) - Untracked runtime files, a 141 GB worktree build directory, an unpushed branch, and other housekeeping items found during analysis.
