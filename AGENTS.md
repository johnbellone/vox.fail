# AGENTS.md

Orientation for anyone (human or AI agent) working in this repository. Read this first; the
[README](README.md) is the user-facing manual for the avatar app and the Vox worlds.

## Read this first (30 seconds)

1. **Find your work:** writers: your task message is your bead. Leads: `bd ready` lists unblocked
   beads; `bd show <id>` gives one bead's files and acceptance check. Do that bead and nothing
   else. No bead and no task message: ask. Writers never claim or close beads; the lead closes
   a bead after its review passes.
2. **Know your role:** haiku writes (code and prose, one bead, under 100k tokens of context);
   sonnet reviews. See [Models and subagents](#models-and-subagents).
3. **Stay in your files.** Touch only the files the bead names. Need another? Stop and ask.
4. **Never invent facts.** Key bindings, ports, flags and numbers come from code or data you
   read, not from memory.
5. **Prove it:** run the bead's acceptance command, report the real result.
6. **Hard stops:** (the full list is the "Hard constraints" bullet under "Task tracking and agent memory") no secrets, `*.db` or `directory.key*` in git; no edits to generated assets
   (`assets/models`, `apps/site/public/pkg`, `apps/site/dist`); no commits or pushes unless asked;
   do not read `knowledge/` with file tools (use `okf_*`, see the end of this file).

Quick commands: `cargo check --workspace` (types), `cargo test --workspace` (tests),
`cd apps/site && bun run build` (pages), `tools/dev/check_links.sh` (markdown links, once it
exists), `bd ready` / `bd close <id> --reason "..."` (work).

## What this is

Two products share one Cargo workspace:

1. **Block Avatar** (`block_avatar`, in `bins/block-avatar/`): a Bevy 0.20 desktop app that
   drives block-style characters from a microphone, for streaming through OBS. Lip sync,
   reactions, a local HTTP control page, viewer questions, Twitch chat, optional
   Windows-only webcam output.
2. **Vox** (every other crate): a shared voxel build space. You sign in on a web page, make a
   character, and walk into worlds (the Void, the yard, the islands, the dark sea, orbit) that you can build in
   together. It runs on the desktop (`vox-client`) and in the browser (the same client built
   for WebAssembly). Live at https://vox.fail.

## Layout

| Path | Crate | Role |
| --- | --- | --- |
| `crates/vox-core/` | `vox-core` | Engine-free rules shared by everyone: blocks and chunks (`world`), voxel raycast (`ray`), walking physics (`body`), level files (`level`), the wire protocol (`wire`), sign-in tokens (`auth`), client-side prediction (`predict`), weather simulation (`weather`), who may use, copy and edit a structure (`entitle`). **No Bevy here.** |
| `crates/vox-world/` | `vox-world` | A world on screen, shared by the app and the client: block meshing and materials (`blocks`), server link, chunks, controls, camera (`client`), input actions, bindings, controller brands and button hints (`input/`), the pixel HUD and quick wheel (`hud`), menus with controller navigation (`menu`), sky and fog (`outdoors`), portals (`portals`), avatar pose streaming (`pose`, `proto`), file access that also works in the browser (`files`). |
| `crates/vox-format/` | `vox-format` | The world format: regions (16 × 16 chunk columns) as checked, zstd-compressed blobs named by their BLAKE3 hash (`region`), a blob store and snapshots (manifests of region hashes, shared blobs, kept a week; `store`), in a folder or an Oxen repository with a commit per snapshot and history older than the week squashed (`oxen`, runs the `oxen` command line). Decodes in wasm too (`ruzstd`); encoding is native. |
| `crates/vox-link/` | `vox-link` | A WebSocket kept open on its own thread, reconnecting with backoff; a `Dial` says where and how (world servers, the relay). Desktop only. |
| `crates/avatar-voice/` | `avatar-voice` | Block Avatar's voice analysis, pure Rust: lip sync (`lipsync`), pitch and formants (`voice`), laugh and sing detection (`detect`), WAV files (`wav`, one hardened reader). Its tests run on real recordings in `testdata/real`. |
| `bins/vox-client/` | `vox-client` | The player client: a thin Bevy app (`lib.rs`) around `vox-world`. Builds natively and to wasm32. Also the FPS counter, the portal sweep and watch mode (the front page's live view). |
| `bins/vox-server/` | `vox-server` | One world: a single thread owns the blocks, players and edit order; WebSocket on `/ws` for players and watchers; SQLite persistence, with `vox-format` snapshots beside it (`<db>.snapshots/`) and restore (`snapshot`). |
| `bins/vox-host/` | `vox-host` | Many worlds in one process on one port, each a `vox-server` world routed by path (`/w/<id>/ws`, plus an alias such as its old port); `GET /` lists them. With `--idle-stop <minutes>` worlds start on their first connection and stop when unused (`--keep <id>` always runs). With `--instances` it makes raid parties' copies of dungeons (`data/levels/dungeons/*.json`) when the directory asks (`POST /api/host/instance`, world secret), thrown away when empty or done. Takes the same world flags (`vox_server::flags`). |
| `bins/vox-directory/` | `vox-directory` | Accounts (Steam, Discord, name and password), characters, authority and world currencies, plots, portals, raids, friends and groups, structures and their entitlements (`structures`), token signing; serves the site and the game's files. |
| `bins/vox-relay/` | `vox-relay` | Lists live shows and relays traffic for the avatar app's network shows. |
| `bins/block-avatar/` | `block_avatar` | The avatar app (and its Windows installer). It re-exports blocks, weather and the world client from `vox-world`; its own `bevel`, `lighting` and `particles` are different systems, not copies. |
| `apps/site/` | (Bun) | The vox.fail pages: React 19, Mantine 9, built with Bun into `dist/`. `public/pkg/` is the wasm client (generated, ignored). See its README. |
| `data/` | | Game data: `blocks.json`, `characters.json`, `wardrobe.json`, `levels/*.json` (void, yard, islands, sea, orbit), clip manifests. |
| `assets/` | | Models, props, sets, sounds, fonts, shaders. |
| `deploy/` | | systemd units and the Cloudflare Tunnel config for vox.fail (`deploy/README.md`); the container image (`docker/Dockerfile`) and a Helm chart for Kubernetes (`helm/vox`, see `helm/README.md`). |
| `tools/assets/` | | Python asset builders (models, sets, wardrobe, block tiles, levels). |
| `tools/dev/` | | `play-local.sh` (run everything locally), `build_web.sh` (the wasm client), `oxen_probe.sh` (the snapshot store against Oxen). |
| `tools/test/` | | `visual_test.py`, `web_e2e.py`, `eval_voice.py`. |
| `docs/` | | `help/` (player help, also shown on the site; being added, bead `vox-dg5.4` onward), `vox/` (worlds), `avatar/` (the avatar manual), `design/` (design documents). |
| `.pi/agents/` | | Project subagents: `vox-writer` (haiku) and `vox-reviewer` (sonnet). See [Models and subagents](#models-and-subagents). |
| `.beads/` | | The beads task database (`bd`). Never edit by hand. |
| `knowledge/` | | okf project memory. Use the `okf_*` tools, not file tools. |
| `source/vox/` | | MagicaVoxel sources for the characters and props. |

Dependency direction: `vox-core` <- `vox-world` <- `vox-client` and `block_avatar`; `vox-server`
and `vox-directory` depend on `vox-core` only. Keep it that way: rules go in `vox-core`,
rendering in `vox-world`. Rust libraries live in `crates/`, Rust programs in `bins/`, Bun web
apps in `apps/`.

## Commands

The root is a virtual workspace, so `cargo build` builds every crate; `-p <name>` builds one.
Shared dependency versions live once in the root `Cargo.toml` (`[workspace.dependencies]`);
members say `name.workspace = true`.

```bash
cargo build --workspace              # everything (first build compiles Bevy: ~5 min)
cargo check --workspace              # fast type check
cargo clippy --workspace             # lints (the tree has existing warnings; do not add new ones)
cargo test --workspace               # tests; the avatar app's tests use testdata/real/

tools/dev/play-local.sh                  # local Void + yard + islands + sea + orbit, then the client joins as a guest
tools/dev/play-local.sh --name Sam --model models/dog.glb

cargo run --release -p vox-server -- --listen 127.0.0.1:7780 --db void.db     # one world
cargo run --release -p vox-server -- --db void.db --oxen                       # snapshots as Oxen commits (oxen on the PATH, or --oxen <path>)
cargo run --release -p vox-server -- --db void.db --list-snapshots             # (world closed) its snapshots
cargo run --release -p vox-server -- --db void.db --restore-at 1795700000      # back to a moment; undoable
cargo run --release -p vox-host -- --listen 127.0.0.1:7790 --world void@7780 --world yard@7781   # several worlds, one port
cargo run --release -p vox-client -- --world ws://127.0.0.1:7780/ws --name Sam
cargo run --release -p vox-directory -- --listen 0.0.0.0:7700                 # accounts and pages
tools/dev/build_web.sh               # browser client into apps/site/public/pkg (wasm32 target + wasm-bindgen-cli)
bun install && (cd apps/site && bun run build)   # the pages into apps/site/dist
```

`tools/dev/play-local.sh` refuses to start if ports 7780 to 7784 are taken; clear a leftover with
`pkill vox-server`. Worlds save to `void.db`, `yard.db`, `islands.db`, `sea.db`, `orbit.db` in the repo root (git
ignores `*.db`).

## How the pieces talk

- **Client <-> world:** WebSocket, `vox_core::wire` messages (postcard). The server is
  authoritative: the client predicts its own edits (`vox-core::predict`) and the server confirms or
  refuses them with the same rules (`vox-core::world`).
- **Sign-in:** the directory signs a token (Ed25519) naming you and your character. World
  servers started with `--directory-key` verify it offline. Guests are allowed only when a
  world is started without a directory key. The client sends the token only to the host it
  started on, never to a portal destination on another host; going through a portal it sends a
  **ticket** instead (`vox-core::auth::Ticket`: one world, once, for a minute, with the arrival
  point).
- **Portals:** a level lists portals with a destination `url` (`":7781"` means the same host,
  that port; `"/w/7781/ws"` a path). Walking into one, the client sends `Through`; the world
  checks it's in the doorway (`vox-core::portal`), works out the arrival point, asks the
  directory for a ticket (`POST /api/world/travel`) and answers `Travel { url, ticket, arrive }`.
  The client then reconnects (`WorldClient::travel`) and says hello with the ticket; the world
  there admits each ticket once and starts the player at its arrival point. Without a directory
  (local play) the answer has no ticket. If the way can't be reached the client returns to the
  old world. A portal with `"url": "dungeon:<name>"` leads to the traveller's raid party's own
  instance of that dungeon: the directory (`--instances-at`) has the host make one if the party
  hasn't one open (at most three new ones an hour), and gives tickets for it only to the party.
- **Worlds -> directory:** each world reports who is present once a minute (authority, raids,
  plots) over localhost with `VOX_WORLD_SECRET`. Those endpoints (`/api/world/*`) must not be
  exposed publicly; the tunnel config refuses them.
- **Production:** one machine runs the directory and worlds on localhost behind a Cloudflare
  Tunnel at vox.fail. Details and the order to start things: `deploy/README.md`.

## Conventions

- Rust 2024 edition, Bevy 0.20 (0.20.1). Shaders are WESL (`assets/shaders/*.wesl`: `import …;`,
  `@if(…)`, `constants::MATERIAL_BIND_GROUP`), as Bevy 0.20 requires. Positions in `vox-core` are in **blocks**; the app scales by
  the palette's block size (0.5 m) for metres.
- The client crate also compiles to **wasm32**. Do not use threads, blocking I/O, `std::time`
  clocks, or unchecked arithmetic on untrusted sizes in code shared with `vox-world` or
  `vox-core`: release builds on wasm32 wrap on overflow (`usize` is 32 bits).
- **Everything from the network is untrusted:** decode with checked arithmetic, reject
  non-finite floats, cap sizes. Everything the server accepts from a client is validated on the
  server (`vox-core::world` holds the rules).
- Mutex locks recover from poisoning (`lock().unwrap_or_else(|e| e.into_inner())`) so one
  panicking worker does not take the whole app down. Keep using that form.
- The avatar app's local control page and settings file take text from a browser: strip control
  characters before writing to the settings file or to a network protocol (see `save_keys`
  and `Chat::connect`).
- SQL is always parameterised. No string-built queries.
- Log with `crate::note!` / `crate::problem!` (see `bins/block-avatar/src/applog.rs`, `crates/vox-world/src/files.rs`), not
  bare `println!`.
- Comments explain why, and every module starts with a `//!` doc saying what it is for.

## Models and subagents

Plans are written by the lead (a person or a larger model); work is split into beads small
enough for a haiku-class model; sonnet-class models review. The roles are pinned in
`.pi/agents/` (`vox-writer`, `vox-reviewer`); change a model there, not in prompts.

| Role | Model | Does | Must not |
| --- | --- | --- | --- |
| Lead | any | Plans, writes beads, picks files, launches subagents, decides, runs `okf_*`, closes beads | Hand a writer a vague task |
| `vox-writer` | haiku | Implements **one bead**: code or prose, in the named files | Explore the repo, touch other files, decide design, commit |
| `vox-reviewer` | sonnet | Reads the diff or prose, checks facts against code and data, reports `PASS` / `PASS WITH FIXES` / `FAIL` | Edit files, run shell commands |

Rules that keep haiku inside its limits:

- **One bead, one writer, one cwd.** Never run two writers on the same files at once.
- **Under 100k tokens of context.** A bead names at most about eight files to read and a few to
  change. Split a bead that needs more. Point at line ranges for big files; never at `assets/`,
  `target/`, `*.db`, `apps/site/dist` or `apps/site/public/pkg`.
- **Prose beads carry their sources.** List the files each claim must come from. Pages stay
  short (about one screen per topic) so one writer can finish one.
- **Every bead ends with a command.** Something that prints pass or fail: `cargo check -p <crate>`,
  `cargo test -p <crate> <name>`, `bun run build`, a `grep`, a link check.
- **Every writer bead gets a reviewer.** Sonnet reviews before the bead closes; fix `blocker`
  and `major` findings (a new writer run, or the lead) and re-review until `PASS`.
- **Escalate, do not guess.** A writer that needs a decision stops and reports it.

Launch (Pi subagents): `subagent({ agent: "vox-writer", task: <template below> })`, then
`subagent({ agent: "vox-reviewer", task: <review template> })`. Parallel writers are fine when
their `Files to change` do not overlap.

### Directed task template

Give a writer exactly this, filled in. Short and concrete beats long and general.

```text
Bead: vox-xxx.n  <title>
Goal: <one sentence, the outcome>
Files to read: <paths, with line ranges for big files>
Files to change: <paths; "new: path" for a new file>
Do:
  1. <concrete step>
  2. <concrete step>
Do not: <scope limits, e.g. "do not touch other docs", "no new dependencies">
Acceptance: <command to run> -> <expected result>
Report: what changed, the acceptance output, and anything you could not verify.
```

Review template for `vox-reviewer`:

```text
Bead: vox-xxx.n  <title>
Acceptance criteria: <copied from the bead>
Files changed: <paths>
Check against: <code and data files the claims come from>
Focus: <facts | correctness | security | conventions | lost content | links>
Report: verdict and findings with path:line evidence.
```

## Beads workflow

Beads (`bd`, prefix `vox`) are how work is planned and run. Do not keep plans only in chat.

```bash
bd ready                              # unblocked work
bd show vox-abc.2                     # one bead: description, acceptance, dependencies
bd update vox-abc.2 --claim           # take it (marks in progress)
bd close vox-abc.2 --reason "what was done and how it was checked"
bd create "Title" --parent vox-abc --silent -p 2 \
  -d "Files and steps" --acceptance "command -> result" --deps "vox-abc.1"
bd list                               # everything, as a tree
```

- **Plan as an epic with children.** One bead per haiku-sized piece of work, plus one review
  bead after each group of writer beads (dependency: the writers it reviews).
- **A bead says:** the files (read and change), the steps, and the acceptance command. If you
  could not hand it to `vox-writer` as the template above, it is too vague or too big. Split it.
- **Dependencies are real.** Use `--deps` so `bd ready` shows only what can start now.
- **User-facing bug or request?** It also needs a GitHub issue; put `gh-<n>` in `--external-ref`.
- **Claim before you start, close with a reason** that names the check you ran. Do not close a
  writer bead before its review passes.
- **Found something else?** Create a new bead (`--deps "discovered-from:<id>"`); do not widen the
  current one.
- `bd metrics off` keeps usage reporting off (already set in this clone).

## Design documents

Proposals live in `docs/design/`. Read the relevant one before large changes:

- [world-scaling.md](docs/design/world-scaling.md): 60 km worlds; regions, generated terrain,
  streaming by view radius, simulation owners, region hashes; browser and desktop budgets.
- [repo-structure.md](docs/design/repo-structure.md): moving to `bins/`, `crates/` and `apps/`, folders
  and plugins inside the avatar app and `vox-world`, and the step-by-step migration.
- [world-graph-and-art.md](docs/design/world-graph-and-art.md): levels as graphs of nodes
  refined in passes by a language model; recipes (typed building operations) interpreted by
  Rust into voxels in the project's style, native and wasm; the `vox-art`, `vox-graph`,
  `vox-library` crates and the `vox-forge` tool.
- [world-format.md](docs/design/world-format.md): regions as compressed blobs (blocks in
  run-length encoding + zstd, entities as Bevy `DynamicWorld` in RON or postcard),
  content-addressed snapshots, levels as files, the `vox-format` crate.
- [world-network.md](docs/design/world-network.md): many worlds; worlds by id with many per
  host, a world registry, server-driven portal travel with tickets, dungeon instances for raid
  parties, an atlas of tiled worlds; the rollout in phases.
- [bevy-ecosystem.md](docs/design/bevy-ecosystem.md): an analysis of 42 Bevy ecosystem crates
  (use directly, copy the pattern, watch or skip), the Bevy 0.20 upgrade question, and what belongs
  in the world format and a pack.
- [props.md](docs/design/props.md): props (small authored objects with behaviours) as recipes or
  imported MagicaVoxel files, the checks and review pipeline, placing them in worlds, and a
  seventy-prop catalogue with a twelve-prop first wave.
- [controls-and-hud.md](docs/design/controls-and-hud.md): block-game controls on an action
  layer, gamepad out of the box with brand-matched button hints, HUD (hotbar, health), menus
  with controller navigation, server-side inventory and health.

## Known gaps (not yet done)

From the code review; none are regressions, but know they exist before relying on them:

- The world server checks moves (`vox-core::moves`: a distance budget at flying speed with room
  for lag, fast falls allowed, the spawn always reachable) and sends `Correct` back when a move
  is impossible; `--trust-moves` turns this off for test and automation worlds (the control
  page's `world:goto` teleports). A signed-in player starts at their ticket's arrival point,
  or at the spawn; guests (who get no tickets) and worlds without sign-in still take the first
  position after joining on trust.
- Sign-in hardening is in: Steam answers must be plain (no repeated `openid.*` keys) and signed
  over every field we read; password hashing runs at most four at a time; the relay limits shows
  and wrong codes per address (and wrong codes per show) and drops text from guests and viewers.
  Not yet: the day-long sign-in token is valid on any world (no audience in it); portal trips
  use tickets, which are good for one world only, once.
- The avatar app's systems are plugins by pipeline (`app/plugins.rs`: `Setup`, `Frame`, `Motion`,
  `Network`, `WorldGlue`), not by folder: several chains run systems from different folders in
  a set order. Resources are built from the settings in `app/configure.rs`; `main()` is about
  50 lines. Check schedule changes with `tools/test/visual_test.py` (render references with the
  old build, `--update`, then compare the new one).
- Remeshing copies voxels chunk by chunk and runs within a per-frame budget (8 ms desktop,
  5 ms browser), nearest first. World faces use a packed 20-byte vertex (`blocks::ATTRIBUTE_BLOCK_UV`
  and `ATTRIBUTE_BLOCK_FACE`, unpacked by `assets/shaders/block_vertex.wesl`); neighbouring
  solid faces with the same tile and even light are merged (`mesh_cells(.., merge: true)`), and
  `assets/shaders/block.wesl` repeats the tile across them, sampling every texture with the
  unwrapped coordinates' derivatives (no seams). Anything drawn with `BlockMaterial` must use
  `into_packed_mesh`; chunk meshes live only on the GPU (`bounds()` gives their `Aabb`). Still:
  a one-block edit rebuilds its whole chunk, and weather and fog baking run on the main thread.
- Duplicated helpers: several xorshift generators, value noise in both
  `vox-core::weather` and `vox-world::outdoors`, and the child-spawning for opaque, glass, water
  and flame meshes.
- The Linux webcam output does not exist: the `W` virtual camera is Windows-only (it writes
  OBS's DirectShow shared-memory queue). On Linux use OBS window capture and OBS's own
  virtual camera.

## Working here

- Make changes small and keep `cargo check --workspace` clean; run `cargo clippy` on what you
  touch.
- Do not edit generated or binary assets by hand (`assets/models`, `apps/site/public/pkg`, `apps/site/dist`); use the scripts
  in `tools/`.
- Do not commit `*.db`, `directory.key*`, or anything under `/etc/vox` equivalents. Secrets
  (`VOX_WORLD_SECRET`, `VOX_DISCORD_CLIENT_SECRET`, `VOX_STEAM_KEY`) live in environment files
  on the server, never in the repo.

## Task tracking and agent memory

- **GitHub Issues** are for people: user-facing bugs, feature requests and questions. They are the
  public record of what should change and why. Never track a user-facing bug in beads only.
- **beads** (`bd`) is for agents: implementation tasks, dependencies and follow-ups. Put the
  GitHub issue in each bead (`gh-<n>`, `--external-ref`); work from `bd ready`; close with a
  reason. The workflow is in [Beads workflow](#beads-workflow). Install: `npm install -g @beads/bd`;
  set up once per clone with `bd init --non-interactive --prefix vox --skip-agents --skip-hooks`
  (this file is the agent instructions; do not let `bd init` rewrite it).
- **okf** (`knowledge/`) is durable project knowledge: architecture, technologies, decisions and
  status. Search it before large changes; record decisions in it when you finish.
- Hard constraints: no real-person likenesses or trademarked characters; no synthetic voices
  (test and demo audio are real recordings); the medic's cross is green; no poses of assaulting
  police; sound effects are real CC0 recordings; stay on Bevy 0.20.1 (upgraded from 0.19.1 on the owner's say-so, 2026-11-27). See
  `knowledge/convention/hard-constraints.md`.
- Before finishing code changes run `cargo test --workspace`, and
  `okf validate knowledge --strict --drift` when you touched `knowledge/`.
- Done means: acceptance command passed, review passed, bead closed with a reason, decisions
  recorded in okf (lead only).

<!-- BEGIN OKF AGENT MEMORY -->
## 1. Behavioral Invariants & Constraints (RFC 2119)
- MUST execute `okf_search(query=keywords, limit=3)` before proposing architecture, new dependencies, or substantial changes.
- NEVER scan `knowledge/` via `list_dir`, `grep_search`, `find`, or raw file readers.
- NEVER forge human verification (`verified:` is human-only; declare `generated: { by: "<actor>", at: "<iso-time>" }`).
- PREFER native `okf_*` MCP tools OVER CLI fallback commands.

## 2. Guard Clauses & Scope Governance
- ON edit(@path/):
    IF first_visit(@path/) => okf_search(for_path=@path/)
    IF governance == "hold" => STOP("Subsystem frozen by governance. Request explicit human confirmation.")
    IF governance == "constraint" => MUST adhere to all listed invariants
    IF governance == "context" => proceed with awareness
- ON user_query(architecture | requirements | conventions | domain_facts):
    okf_search(query=keywords, limit=3) => evaluate summary description
    IF relevant => okf_show(concept_id) ONLY on demand
    IF updating_existing_concept => PREFER okf_update OVER okf_create

## 3. Completion Pipeline (Sequential Assertion Gates)
1. IF decisions_made => MUST okf_create(category/*, type="decision", title=..., desc=...)
2. IF requirements_discovered => MUST okf_update(concept_id)
3. IF concepts_mutated => MUST sync(knowledge/log.md, knowledge/index.md)
4. ASSERT(okf_validate(strict=true, drift=true) == {errors: 0, warnings: 0}, ELSE=fix_before_exit)
<!-- END OKF AGENT MEMORY -->
