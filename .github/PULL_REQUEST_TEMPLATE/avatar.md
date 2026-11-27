<!-- An avatar pull request (docs/avatar/avatar-workflow.md). Easiest: generate this description with
     python3 tools/review/avatar_pr.py body <id> --sha <commit> [--issue <n>]
     and label the pull request `avatar`: the review bot checks it. Images are pinned to a commit. -->
A new avatar, <Name>, on the shared block rig.

<img src="https://github.com/johnbellone/vox.fail/blob/<sha>/assets/portraits/<id>.png?raw=true" width="160" alt="<Name>'s portrait">

**In game:** the avatar app's own renderer, on the diner set, in each of its own poses.

![<Name> in the app](https://github.com/johnbellone/vox.fail/blob/<sha>/docs/avatar/<id>-ingame.png?raw=true)

**Close up:** its own poses, two shared clips and eight face animations.

![<Name>'s poses, props and faces](https://github.com/johnbellone/vox.fail/blob/<sha>/docs/avatar/<id>-poses.png?raw=true)

## <Name>

<one paragraph: the look>

| Pose | Length | Face | Prop |
|---|---|---|---|
| <own clip> | <s> | <face parts> | <prop> |

## How it's made

<the scripts run; anything regenerated and left out>

## Checked

- `python3 tools/review/avatar_pr.py check <id> --body <this description>`: passes.
- <what you looked at; what you couldn't check>
