---
description: Review an avatar pull request against the avatar format and the new-avatar skill's checklist
argument-hint: "<pull request number>"
---
Review avatar pull request #$1 for vox.fail. Read-only: don't edit files or push.

1. `gh pr view $1 --json body,files,headRefName` and check out its head. The avatar's id is the
   `assets/models/<id>.glb` it adds.
2. `gh pr view $1 --json body -q .body > /tmp/body.md`, then
   `python3 tools/review/avatar_pr.py check <id> --body /tmp/body.md`. Every problem it prints is a
   finding.
3. Look at `docs/avatar/<id>-poses.png` and `docs/avatar/<id>-ingame.png` (read the images), and
   go through the "Reviewing an avatar pull request" checklist in `.agents/skills/new-avatar/SKILL.md`.
4. `git diff --stat origin/main...HEAD`: only the avatar's own generated files, the builder, its data
   entries and docs should change.

Report in the `vox-reviewer` shape: Verdict (PASS | PASS WITH FIXES | FAIL), Findings (most severe
first: severity, path, problem, evidence, suggested fix), Verified OK.
