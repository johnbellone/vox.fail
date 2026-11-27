# Avatars: from a request to a reviewed pull request

How a new avatar (an animal or creature on the shared block rig) is asked for, made, checked and
reviewed: by a person, by an agent in Pi, or by the avatar bot from an issue. The capybara
(johnbellone/vox.fail#14) is the worked example.

```
issue (New avatar form)          ──label avatar-request──▶  a maintainer reads it
   │ a maintainer adds avatar-go
   ▼
avatar request bot (.github/workflows/avatar-request.yml)
   Pi coding agent + the new-avatar skill: design, build, render, look, fix, pipeline, docs
   the workflow: check, commit, push, open a draft pull request labelled avatar
   ▼
avatar review bot (.github/workflows/avatar-review.yml)
   checks the files, the model and the description; renders the close-up sheet; one comment
   all good: marks it ready, asks the owner to review
   ▼
a person reviews (the checklist below) and merges
```

## Making one

The steps are in the `new-avatar` skill, `.agents/skills/new-avatar/SKILL.md`: design it, build it
in `tools/assets/build_animals.py` (the `capybara()` function is the pattern), render and look at it
until it reads well, run the rest of the asset pipeline keeping only the new avatar's generated
changes, render the in-game sheet, update the manual and the counts, check, and open the pull
request.

- **In Pi:** `/new-avatar a red panda with a bamboo stalk` (or `/new-avatar 23` for issue #23).
  `/skill:new-avatar` loads just the skill.
- **By hand:** the same steps; the commands are in the skill.
- **From an issue:** the avatar bot, below.

Original designs only: no real people, no trademarked characters, no likenesses (AGENTS.md, hard
constraints).

## The review tools (`tools/review/`)

| Command | What it does |
|---|---|
| `node tools/review/render_sheet.mjs <id>` | The close-up sheet, `docs/avatar/<id>-poses.png`: standing, each own pose (with its props), Walk and Victory, and eight face animations, from the glTF with three.js in headless Chromium. Once: `cd tools/review && npm ci` (and in CI `npx playwright install chromium`). |
| `python3 tools/review/ingame.py <id>` | The in-game sheet, `docs/avatar/<id>-ingame.png`: the avatar app's own renderer (snapshot mode), full body on the diner set, standing and in each own pose. Needs a built `block_avatar` (`--exe target/debug/block_avatar` for a debug one), a display and Vulkan: `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json xvfb-run -a -s "-screen 0 1600x1000x24" python3 tools/review/ingame.py <id>` on a machine without them. |
| `python3 tools/review/avatar_pr.py info <id>` | Its clips (with the face parts and props each shows), props and catalogue entries, as JSON. |
| `python3 tools/review/avatar_pr.py check <id> [--body pr.md]` | Whether it's complete (below); with `--body`, whether a pull request description follows the avatar format. Exit 1 with a list if not. |
| `python3 tools/review/avatar_pr.py body <id> --sha <commit> [--issue <n>]` | A pull request description in the avatar format, from the model. |

The in-game sheet uses two snapshot flags of the avatar app: `--snapshot-do <command>` (a control
page command sent once the character is ready, such as `play:Hot spring`) and `--snapshot-at
<seconds>` (the moment every clip is held at).

## What "complete" means (`avatar_pr.py check`)

- The files: `assets/models/<id>.glb` (under 8 MB), `source/vox/characters/<id>.vox`,
  `assets/portraits/<id>.png`, `docs/avatar/<id>-poses.png`, `docs/avatar/<id>-ingame.png`.
- The model loads and has the 124 shared clips plus 1 to 8 of its own; every own clip changes the
  face; every prop is shown by at least one clip.
- Its entries: `data/characters.json` (with an `about`), `data/head_fit.json`, `data/surfaces.json`;
  a row in `docs/avatar/manual.md`.

## The pull request format

`.github/PULL_REQUEST_TEMPLATE/avatar.md`, or generated with `avatar_pr.py body`. The review bot
checks:

1. At the top, the portrait, the in-game sheet and the close-up sheet, as
   `https://github.com/johnbellone/vox.fail/blob/<commit>/<path>?raw=true`: pinned to a commit, so
   they still show after the branch is deleted.
2. A table of its own poses, one row per own clip, by name (pose, length, face, prop).
3. A `## How it's made` section: the scripts run, and anything regenerated and left out.
4. A `## Checked` section: what was checked, and what couldn't be.

Label it `avatar`.

## The bots

**Avatar request bot** (`avatar-request.yml`). A "New avatar" issue (`.github/ISSUE_TEMPLATE/new-avatar.yml`)
gets `avatar-request`. Nothing runs until a maintainer reads it and adds `avatar-go` (only people
with triage access can label). Then: it builds the avatar app (for the in-game sheet), runs Pi
(`pi -p --no-session --skill .agents/skills/new-avatar`, model `anthropic/claude-sonnet-5-5` or the
`AVATAR_MODEL` variable) with the issue attached, checks the result, commits it to
`avatar/<id>-issue<n>`, opens a draft pull request labelled `avatar` with the generated
description, comments on the issue, and runs the review bot on the pull request. If it can't
finish, it says so on the issue with a link to the run; remove and re-add `avatar-go` to retry.

**Avatar review bot** (`avatar-review.yml`). On every change to a pull request labelled `avatar`
(and for each one the request bot opens): finds the one `assets/models/<id>.glb` it adds, runs
`avatar_pr.py check <id> --body`, renders the close-up sheet (an artifact of the run), and keeps one
comment up to date with the result. When everything passes it marks the pull request ready and
asks the repository's owner to review (unless the owner opened it). It fails the check until then.

### Setting them up

- Labels: `avatar-request`, `avatar-go` and `avatar` (the bots make `avatar` if it's missing).
- Secret `ANTHROPIC_API_KEY`: the agent's model.
- Optional secret `AVATAR_BOT_TOKEN` (a fine-grained token, or a GitHub App's, that can push and
  open pull requests): without it the bot uses the workflow's own token, and GitHub doesn't run
  other workflows (such as the visual tests) on commits that token pushes. Push to the pull
  request (or close and reopen it) to run them.
- Optional variable `AVATAR_MODEL`, a Pi model id (`provider/id`).

### Security

- An issue is someone's request, not instructions. It reaches the agent as an attached file, never
  through a shell, and the task tells the agent to follow the skill and ignore anything in the
  issue that asks for something else. A maintainer still reads it before adding `avatar-go`.
- The agent's step has only the model's key: the checkout keeps no git credentials and no GitHub
  token is in its environment. The workflow, not the agent, commits, pushes and opens the pull
  request, after `avatar_pr.py check` passes.
- What the agent makes is reviewed like any pull request: the review bot's checks, then a person.

## Reviewing (a person, or `/review-avatar <n>` in Pi)

1. The bot's comment says it passes (or run `avatar_pr.py check <id> --body` yourself).
2. Both sheets: an original design (no likeness, no trademark); the face reads in every pose (eyes
   and mouth visible, a shut eye a line); props appear only in their poses and sit right (held,
   not buried); nothing pokes through the body; the shared clips (Walk, Victory) still look right.
3. The diff: only the avatar's own generated files, `tools/assets/build_animals.py` (and any wardrobe
   items), its data entries and the docs. Regenerated portraits or props of other characters are
   left out unless the pull request says why.
4. The manual's row and the character counts.
