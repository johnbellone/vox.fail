# Design: vox.fail on Cloudflare Containers

Status: **proposed** (2026-11-27). Nothing below is built yet.

Related: [deploy/README.md](../../deploy/README.md) (today's hosting: one server behind a tunnel),
[apps/edge/README.md](../../apps/edge/README.md) (the Worker in front of vox.fail),
[world-format.md](world-format.md) (world snapshots), `deploy/docker/Dockerfile` (the image).

## Summary

vox.fail moves off a self-run server onto Cloudflare. One **container**, made from the existing
image, runs the directory and every world (vox-host). It sits behind the existing **Worker**
(`apps/edge`) as a **Durable Object**. It sleeps when nobody is using it, and wakes on the next
sign-in or world visit. The data stays in **SQLite**, as today. **Litestream** streams every
change to an **R2** bucket and restores the databases when the container starts. The Worker
serves the pages and game files itself, so browsing vox.fail never wakes the container.

The owner's choices (2026-11-27):
- SQLite with Litestream to R2, not a rewrite for D1;
- sleep when idle, not always on;
- the **basic** instance (¼ vCPU, 1 GiB, 4 GB disk), raised later if needed;
- the dev server and its tunnel are **retired**.

## What we have today (checked in code)

- **Directory** (`bins/vox-directory`): axum, synchronous `rusqlite`, about 24 tables in
  `/var/lib/vox/directory.db` (accounts, characters, wallets and ledger, plots, structures).
  Its signing key is two files beside the database, `directory.key` and `directory.key.pub`,
  made on first start. It also serves the site: the pages (`ServeFile`), and `/assets`,
  `/data` and `/pkg` (`ServeDir`) (`bins/vox-directory/src/state.rs`).
- **Worlds** (`bins/vox-host`): one process, every world on `127.0.0.1:7790`.
  - Each world is a SQLite file `/var/lib/vox/<id>.db` (tables `meta`, `chunks`, `edits`) with
    `vox-format` snapshots beside it (`<db>.snapshots/`, or `--no-snapshots`).
  - SIGTERM saves every world and stops (`bins/vox-host/src/main.rs`).
  - Dungeon instances keep nothing on disk. Atlas tiles are databases made on first visit
    under `<data>/atlas-<name>/`; vox.fail doesn't run an atlas.
- **Secrets** (`deploy/directory.env`): `VOX_DISCORD_CLIENT_ID`, `VOX_DISCORD_CLIENT_SECRET`,
  `VOX_STEAM_KEY` and `VOX_WORLD_SECRET`.
- **Cloudflare today:**
  - the `vox-edge` Worker on `vox.fail/*`, which shows the offline page when the origin is down;
  - the tunnel `vox`, with no server connected;
  - zone rules set by `deploy/cloudflare-setup.sh`.
- **Releases:** a `v*` tag builds the image and a bundle (`release.yml`), which a timer on the
  server installs (`deploy/vox-update.sh`). No server ever ran it, so there is no
  production data to move.

## The plan

```
browser ──► Worker vox-edge (vox.fail/*)
             ├─ pages, /assets, /data, /pkg, /_offline ──► the Worker's static files
             └─ /api/*, /auth/*, /w/* ──► Durable Object "vox" ──► container (basic)
                                                                   ├─ start.sh
                                                                   ├─ litestream ⇄ R2 bucket vox-data
                                                                   ├─ vox-directory :7700
                                                                   └─ vox-host      :7790
```

**The Worker.**
- Its static files grow from the offline page to the whole site: `apps/site/dist` (pages and
  `pkg/`), plus `assets/` (110 MB, 330 files, none near the 25 MiB per-file limit) and `data/`
  (3 MB).
- The Worker has to do what the directory did for these files (`bins/vox-directory/src/routes.rs`):
  - serve the clean page URLs (`/create` → `create.html`, and so on);
  - set the security headers: `nosniff`, `DENY`, `same-origin` referrer, and the content policy;
  - set the cache headers: `no-cache` for pages, `public, max-age=300` for `/pkg/`, `/assets/`
    and `/data/`, and `no-store` for `/api/` and `/auth/`;
  - serve `/pkg/` precompressed (brotli or gzip), as the directory did.
- `/api/world/*` and `/api/host/*` are answered 404 **before** the container route, as the
  tunnel does today. They are for the servers on localhost only (world reports, travel,
  objectives, structure checks, dungeon instances).
- Then `/api/`, `/auth/` and `/w/` go to the Durable Object. `/w/` carries the worlds'
  sockets (`/w/<id>/ws`) and their status pages (`/w/<id>/`).
- The Durable Object passes `cf-connecting-ip` on to the container. With `--behind-proxy`,
  the directory counts sign-in attempts by it (`visitor()` in `state.rs`). Without it, every
  visitor would share one rate-limit bucket.
- If the container can't start in time, the offline page and the JSON 503 answer as they do
  now.

**The Durable Object and the container.**
- One named Durable Object, `vox`, with the `durable_object` scheduling policy. It starts the
  container on the first request and proxies HTTP and WebSockets to ports 7700 and 7790.
- It stops the container (SIGTERM) after **15 minutes** with no requests and no open world
  socket.
- Because the Durable Object decides when to switch images, a new release stops the old
  container before starting the new one. Two containers never write the same databases.

**Processes and flags.** The image's command today starts only the directory, with none of
production's flags (`deploy/docker/Dockerfile`). `deploy/docker/start.sh` becomes the
entrypoint, with `tini` still PID 1 and the unprivileged user (uid 10001) that owns
`/var/lib/vox`. It runs, in order:

1. Write the signing key from `VOX_DIRECTORY_KEY`, as the exact file text (64 hex digits and
   a newline, `bins/vox-directory/src/util.rs`), and its `.pub` beside it.
2. `litestream restore -if-replica-exists` each database: `directory.db` and each world's
   `<id>.db`.
3. `litestream replicate`, with a supervisor as its child process. The supervisor:
   1. starts the directory with `vox-directory.service`'s flags: `--secure`,
      `--behind-proxy`, `--public-url https://vox.fail`, `--steam`, the `--world` list and
      `--instances-at http://127.0.0.1:7790`. Without `--secure` and `--public-url`, cookies
      lose `Secure` and the Steam and Discord return addresses are wrong;
   2. waits until the directory answers. vox-host reads `directory.key.pub` at start
      (`--directory-key`);
   3. starts vox-host with `vox-host.service`'s flags (`--directory-url
      http://127.0.0.1:7700`, `--idle-stop 10`, `--keep void`, `--instances`, the worlds)
      plus `--no-snapshots`;
   4. stops the container if either process exits.

Both processes get `VOX_WORLD_SECRET`. The directory also gets the Discord and Steam secrets.

**Storage.**
- The replica lives in an R2 bucket, `vox-data`, under `litestream/`. Litestream reaches it
  over R2's S3 API with an R2 token limited to that bucket.
- Litestream keeps **7 days** of history, matching the world format's snapshot week.
  `litestream restore -timestamp` can take a database back to any moment in that week.
- Inside the container, worlds run with `--no-snapshots`: the replica replaces the snapshot
  folder, which would vanish on every restart. Keeping `vox-format` snapshots in R2 (an R2
  `BlobStore`) is later work.

**Secrets.**
- Worker secrets, passed to the container as environment variables:
  - the four in `directory.env`;
  - `VOX_DIRECTORY_KEY` (the signing key file's text, made once by the setup script);
  - `LITESTREAM_ACCESS_KEY_ID` and `LITESTREAM_SECRET_ACCESS_KEY`.
- None are in the repository or the image.

**Releases.**
- A `v*` tag builds the image as now. The release job then pushes it to Cloudflare's
  registry and deploys the Worker with the new image reference.
- Worker-only changes on `main` deploy as now (`edge.yml`), with the image reference
  unchanged.
- The CI token gains Containers and Durable Objects rights. The bucket and its token are made
  once by the setup script, not by CI.

**Retiring the server path.**
- Delete the tunnel `vox`. `vox.fail` goes back to a proxied placeholder record, since the
  Worker answers everything.
- Remove the server-pull deploy (`vox-update.sh` with its service and timer, `deploy.env`,
  the bundle in `release.yml`), and the tunnel steps in `cloudflare-setup.sh` and
  `deploy/README.md`.
- The systemd units, the Docker image and the Helm chart stay, for anyone who hosts Vox
  themselves.

## Costs and limits

- **Money.** The $5 Workers Paid plan includes 25 GiB-hours of memory, 375 vCPU-minutes and
  200 GB-hours of disk a month. A basic instance awake all month would use about 730 GiB-hours.
  It sleeps when idle, so a quiet month stays near $5. Every 100 hours awake beyond the
  included amount costs about $1. R2 costs little at this size.
- **Data loss.** A planned stop or sleep loses nothing: the worlds save, then Litestream
  ships the last changes. A hard kill loses about a second of writes, Litestream's sync
  interval.
- **Cold start.** Cloudflare quotes 1–3 s for a container. Restoring the databases adds a
  little: they are small today. The phase 7 bead measures it.
- **Memory.** 1 GiB has to hold the directory and every running world. vox-host's
  `--idle-stop 10` stops worlds nobody is in. This is to be measured; the next size up is
  standard-1 (4 GiB).
- **Atlas tiles** are made on demand, so Litestream wouldn't know their databases. Running an
  atlas on vox.fail needs Litestream's directory replication, or a list made at start. Not
  needed today.

## To check while building

- Whether an open WebSocket through the Durable Object counts as activity. If not, the
  Durable Object counts open world sockets itself before stopping the container.
- That `cf-connecting-ip` reaches the container through the Durable Object. If it doesn't,
  the Worker sets it from the request.
- How long Cloudflare waits after SIGTERM. With continuous replication, only the
  last second is at risk either way.
- That Litestream (0.3.x) restores from and replicates to R2 with a bucket-scoped token.

## Phases (beads)

Each phase is one bead for a writer, with a review bead after each group. The commands are the
acceptance checks.

1. **Litestream in the image.**
   - Change: `deploy/docker/Dockerfile` (add litestream, pinned with a checksum like oxen),
     new `deploy/docker/start.sh`, new `deploy/docker/litestream.yml`.
   - Accept: `docker build` works. `tools/dev/litestream_probe.sh` runs the image against a
     test prefix in R2: write, kill, start again, and the data is there.
2. **The signing key from a secret.**
   - Change: `start.sh` writes `directory.key` and `.pub` from `VOX_DIRECTORY_KEY`, and
     refuses to start without it. Its supervisor starts the directory and vox-host with
     production's flags, in that order.
   - Accept: the probe signs in, restarts, and an old token still verifies. A world socket
     connects with a directory token.
3. **The Worker serves the whole site.**
   - Change: `apps/edge/build.ts` copies the site, assets and data. A `_headers` file gives
     the directory's security and cache headers. Clean page URLs map to their files.
   - Accept: `bun test` in `apps/edge` passes, and new tests check the headers on `/`,
     `/pkg/` and `/api/`, and that `/play` serves `play.html`.
4. **The Durable Object and the container.**
   - Change: `apps/edge/src/worker.ts`, new `apps/edge/src/container.ts`,
     `cloudflare.config.ts`.
   - Accept: `bun test` covers the routing: which paths go to the container; `/api/world/`
     and `/api/host/` refused; `cf-connecting-ip` passed on. `cf deploy --dry-run` passes.
5. **Cloudflare setup.**
   - Change: `deploy/cloudflare-setup.sh` makes the R2 bucket and its token, the Worker
     secrets and the directory key, deletes the tunnel, and sets the placeholder record.
   - Accept: run twice; the second run changes nothing.
6. **Releases.**
   - Change: `.github/workflows/release.yml` pushes the image and deploys; the CI token's
     scope grows.
   - Accept: `actionlint`. A test tag deploys, and `/api/me` answers from the container.
7. **Retire the server path, and measure.**
   - Change: remove `deploy/vox-update.*` and `deploy/deploy.env`, and update
     `deploy/README.md`, AGENTS.md and okf. Measure cold start and memory, and set the
     inactivity timeout and instance size from what's measured.
   - Accept: `tools/dev/check_links.sh` (or a grep for removed paths) is clean, and the
     numbers are recorded here.
