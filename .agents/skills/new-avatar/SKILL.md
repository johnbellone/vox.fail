---
name: new-avatar
description: Make a new avatar (an animal or creature on the shared block rig) for vox.fail and the avatar app, with its own poses, facial animations and props, then render its review sheets and open a pull request in the avatar format for review. Use for "make a <creature> avatar", an avatar request issue, or reviewing an avatar pull request.
compatibility: Python 3 with numpy and Pillow; Node 22 with tools/review's packages (three, playwright); for the in-game sheet a built block_avatar, a display (or xvfb-run) and Vulkan (Mesa's lavapipe works).
---

# New avatar

An avatar is a model on the shared block rig, built by code in `tools/assets/build_animals.py`:
voxel body parts on the rig's joints, a head with the shared face kit (eyes, brows, mouth),
optional props, and a few clips of its own. It then plays all 124 shared clips (100 body, 24 face)
and lip sync. The worked example is the capybara (`capybara()` and its clips in that file).

The whole process, and the pull request format, is in `docs/avatar/avatar-workflow.md`. Read
AGENTS.md's hard constraints first: **original designs only** (no real people, no trademarked
characters, no likenesses), and generated files come from the scripts, never hand edits.

## 1. Design

From the request, decide: an id (lower case, `[a-z][a-z0-9_]*`, the file name), a name, colours, a
one-sentence look, 3 to 5 poses (F9-F12 play the first four), what the face does in each, and 0 to
3 props (each shown only in the poses that use it). Keep it readable at 5 cm voxels.

## 2. Build it in `tools/assets/build_animals.py`

Copy the shape of `capybara()`:

- `standard_body(skin, torso_fn, arm_fn, leg_fn, hand)` keeps the template's proportions, so the
  shared clips (feet planting, hands reaching) still fit. Paint and add to the parts; don't move
  joints.
- The head: `Vox().box(-7, 7, -2, 10, -6, 6, fur)` and a bevel. The face kit sits on the head's
  front plane (z = 6) at a fixed height: **keep a muzzle or snout's top at y ≤ 4** or it covers the
  eyes, and its front at z ≤ 10.
- `mouth=(0, -0.04, 0.20)`: the mouth parts' offset from the face plane, in metres; 0.20 puts
  them on a muzzle whose front is at z = 10 (0.15 for z = 9).
- `tail_at`/`tail_sway`, or `None` for no tail.
- `props`: `dict(name, parent, at, rot, off, vox)`; parent a joint (`Head`, `LowerArm_R`, ...), `at`
  in metres in that joint's space (`GRIP` for a held thing, `(0, 0.5, 0)` on top of a 10-voxel
  head). They're hidden except in the clips that list them.
- Clips: a branch in `clips_for(name)`. Each is `(name, seconds, pose(t), face(t)[, props(t)])`;
  `pose` gives joint angles in degrees (and `_hip` in metres), `face` returns
  `face(eyes, brows, mouth)` from `FACE_PARTS`, `props` the set of prop names shown. Use `env` to
  ease in and out and `osc` for motion. **Every pose changes the face.**
- Add the function to the lists in `main()`.

Build: `python3 tools/assets/build_animals.py <id>`.

## 3. Look at it, and fix it

```bash
cd tools/review && npm ci && cd -        # once
node tools/review/render_sheet.mjs <id>  # docs/avatar/<id>-poses.png
```

Read the picture. Fix and rebuild until: the eyes and mouth show in every face (a shut eye is a
line, not lost under the muzzle), every prop shows in its pose and is held, not buried, nothing
pokes through the body, and the poses read at a glance. Three or four rounds is normal.

## 4. The rest of the pipeline

1. `tools/assets/guess_surfaces.py`: add the id to `ANIMALS`, run it.
2. `tools/assets/build_catalogue.py`: add it to `GROUPS` (`"Animals"`) and an `ABOUT` line.
3. `python3 tools/assets/make_portraits.py`, then `python3 tools/assets/build_catalogue.py`.
4. `python3 tools/assets/build_wardrobe.py` for `data/head_fit.json` (and any new wardrobe items:
   add to `HATS`/`FACE`/`HANDS` and to `bins/block-avatar/src/character/wardrobe.rs`).
5. **Keep only the new avatar's changes.** These scripts regenerate every character's portrait,
   props and fit; on another machine those can come out different. `git status`, then
   `git checkout --` everything that isn't the new avatar's (and re-add just its entries to
   `data/head_fit.json` and `data/surfaces.json`, in place).
6. The in-game sheet (`docs/avatar/<id>-ingame.png`), with a built app:
   `xvfb-run -a -s "-screen 0 1600x1000x24" python3 tools/review/ingame.py <id> --exe target/debug/block_avatar`
   (with `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json` on a machine without a GPU).
7. Docs: a row in `docs/avatar/manual.md`'s animals table (file, look, own clips), and the
   character counts (`grep -rn "characters" docs | grep -E "[0-9]+ characters"`).

## 5. Check, commit, open the pull request

```bash
python3 tools/review/avatar_pr.py check <id>      # must print "<id>: ok"
git add -A && git commit                          # what was made and how it was checked
git push -u origin <branch>
python3 tools/review/avatar_pr.py body <id> --sha "$(git rev-parse HEAD)" [--issue <n>] > /tmp/pr.md
gh pr create --draft --label avatar --title "<Name>: a new avatar" --body-file /tmp/pr.md
```

Label it `avatar`: the review bot (`.github/workflows/avatar-review.yml`) checks the format and
the files, posts its checklist, and when everything passes marks it ready and asks for review.

## Reviewing an avatar pull request

Run `python3 tools/review/avatar_pr.py check <id> --body <the PR's description>`, then look at
both sheets and check, with evidence: an original design (no likeness, no trademark); faces read
in every pose; props appear only in their poses and sit right; the shared clips (Walk, Victory)
still look right; only the avatar's own generated files changed; the docs row and counts. Report
as `vox-reviewer` does: Verdict, Findings (severity, path, problem, evidence, fix), Verified OK.
