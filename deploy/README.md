# Deploying Vox

vox.fail runs entirely on Cloudflare (design: [cloudflare-hosting.md](../docs/design/cloudflare-hosting.md)).
To host Vox on your own machine instead, see [Hosting Vox yourself](#hosting-vox-yourself).

## vox.fail on Cloudflare

```
browser ──► Worker vox-edge (vox.fail/*, apps/edge)
             ├─ pages, /pkg, /assets, /data ──► the Worker's files
             └─ /api, /auth, /w ──► Durable Object "vox" ──► one container (basic)
                                                              ├─ vox-directory :7700
                                                              ├─ vox-host :7790 (every world)
                                                              └─ litestream ⇄ R2 bucket vox-data
```

- **The Worker** serves the site itself, so browsing never wakes the servers.
- **The container** starts on the first sign-in or world visit and sleeps after 15 minutes
  with no requests and no players.
- **Each start** restores the databases from R2 (`deploy/docker/start.sh`).
- **While it runs**, Litestream streams every change back to R2, with a week of history.
- **If it can't start**, the site shows the offline page (`/_offline/`).
- **Portals** in the levels name worlds by port (`":7781"`), so from `wss://vox.fail/w/7780/ws`
  that's `wss://vox.fail/w/7781/ws`. vox-host answers each world at its old port's path too, so
  the levels work unchanged.

### Setting it up (once)

1. **The Workers Paid plan** ($5 a month). Containers need it: dash.cloudflare.com → Workers &
   Pages → Plans.
2. **Cloudflare:** run `deploy/cloudflare-setup.sh` with the `cf` command line signed in
   (`cf auth login`). It's safe to run again. It sets:
   - the zone settings and rules (below);
   - the DNS records;
   - the R2 bucket `vox-data` and its token;
   - the Worker's secrets:
     - `VOX_DIRECTORY_KEY`, the signing key: never change it once set, or everyone is signed out;
     - `VOX_WORLD_SECRET`;
     - Litestream's two R2 keys.
3. **The sign-in sites:** run the script again with their secrets in the environment:
   `VOX_DISCORD_CLIENT_ID=… VOX_DISCORD_CLIENT_SECRET=… VOX_STEAM_KEY=… deploy/cloudflare-setup.sh`.
   - **Discord:** discord.com/developers/applications → New Application → OAuth2, with the
     redirect `https://vox.fail/auth/discord/return`. Only the `identify` scope is asked for.
   - **Steam:** nothing to register. A Steam Web API key (steamcommunity.com/dev/apikey) lets
     new accounts take their Steam name.
4. **GitHub:** the `production` environment holds `CLOUDFLARE_API_TOKEN` and
   `CLOUDFLARE_ACCOUNT_ID`, and only `main` and `v*` tags may use it. The token, "vox-edge CI",
   has Workers Scripts, Workers Containers and Cloudchamber Write on the account, and Workers
   Routes Write and Zone Read on vox.fail.

### Releasing

Push a version tag: `git tag v0.5.0 && git push origin v0.5.0`. `.github/workflows/release.yml`
then:

1. builds the image once (`deploy/docker/Dockerfile`, which includes the browser client);
2. takes the site, assets and data out of it for the Worker;
3. pushes the image to Cloudflare's registry;
4. deploys the Worker, its Durable Object and the container with `wrangler deploy`.

The browser client takes a while to build, so a release goes live in about half an hour. A
running container keeps its image until it next sleeps, so nobody playing is cut off; the next
start uses the new one. Pull requests and `main` only run the Worker's checks (`edge.yml`).

To go back: `bunx wrangler rollback` in `apps/edge` (the Worker and its image reference), or tag
the older commit again.

### Data, backups and restores

- **What's stored:** the databases in R2, under `vox-data/vox/<name>.db/`: `directory.db`
  (accounts, characters, wallets and ledger, plots, structures) and one per world.
- **History:** Litestream keeps a daily snapshot and every change for a week.
- **Point-in-time restore:** with the container asleep, run
  `litestream restore -timestamp <time> -o directory.db s3://vox-data/vox/directory.db?endpoint=https://<account>.r2.cloudflarestorage.com`
  with an R2 token in `LITESTREAM_ACCESS_KEY_ID` and `LITESTREAM_SECRET_ACCESS_KEY`. Copy the
  result into a fresh replica only with care: the next start restores the latest copy from R2.
- **Trying it locally:** `tools/dev/litestream_probe.sh` and `tools/dev/container_probe.sh` run
  the container's start against a test bucket.
- **Rolling back past the world editor:** world snapshots from that release on are zone format 2,
  which holds each zone's entities (portals, props, nodes, settings) as well as its blocks, and
  older binaries can't read them. Before going back to an older image, restore each world with the
  newer binary to a snapshot from before the upgrade (`vox-server --db <world>.db --restore <id>`,
  world stopped): it keeps the world's current entities, since that snapshot predates them. The
  restore is itself a snapshot, so the newer binary can still undo it. This needs a shell where the
  databases are (self-hosting; on Cloudflare, not yet: see below). The directory and the worlds
  are in one image, so they always update together.
- **Editing:** a world's owner and the editors they name edit it live at `/edit?world=<id>`
  (`docs/help/editor.md`). `--editors-all`, which lets guests edit a world with no directory, is
  for local play only (`tools/dev/play-local.sh`): it is refused alongside a directory and must
  never go into `deploy/docker/host.args`.

World owners (who may issue a world's currency) and grants of authority are made with the
directory's `admin` commands against `directory.db`:
`/opt/vox/bin/vox-directory --db /var/lib/vox/directory.db --key /var/lib/vox/directory.key <world flags> admin own-world void "Their Name"`,
or `admin grant "Name" 50`. The world flags are the `--world` lines in
`deploy/docker/directory.args`. **Not yet possible on Cloudflare:** there's no shell into the
container (`wrangler containers ssh` needs SSH set up in `wrangler.jsonc`, which it isn't), and
editing a copy outside would be overwritten by the next restore from R2.

### Zone settings (`deploy/cloudflare-setup.sh` sets these)

- **Always Use HTTPS:** on.
- **WebSockets:** on.
- **Rocket Loader and Email Address Obfuscation:** off. Both rewrite the pages, which their
  content policy then blocks.
- **A cache rule** for `/pkg/`, `/assets/` and `/data/`: eligible for caching, respecting the
  five minutes the Worker sets.
- **A 301 redirect** from `www.vox.fail` to `vox.fail`. `www` has a proxied `AAAA 100::` record
  only so the rule sees requests.
- **`vox.fail` itself** has a proxied `AAAA 100::` record: the Worker answers everything.
- **Bot Fight Mode:** leave it off, or skip it for `/w/`. The desktop client isn't a browser and
  can't answer a challenge.

The desktop client goes to the Void at vox.fail by default: `vox-client --token <launch code>`.
Downloads offered on the start page go in `apps/site/public/downloads/`, or on GitHub Releases.

## Hosting Vox yourself

The same servers run on any Linux machine (Debian 12 / Ubuntu 22.04 or newer). Alternatives:
the container image in `docker/`, or the Helm chart in `helm/` for Kubernetes (see
[helm/README.md](helm/README.md)).

1. **Build:** `cargo build --release -p vox-directory -p vox-host -p vox-client` and
   `tools/dev/build_web.sh` (the browser client), then the pages:
   `bun install && (cd apps/site && bun run build)`.
2. **Install:** copy to `/opt/vox`: `bin/` (the binaries), `apps/site/dist/` as `web/`,
   `assets/` and `data/`. Make a `vox` user and `/var/lib/vox` owned by it.
3. **Secrets:** copy `directory.env` to `/etc/vox/directory.env` (root, mode 600) and fill it in:
   the Discord client ID and secret, optionally a Steam Web API key, and a world secret from
   `openssl rand -hex 32`. Put the same `VOX_WORLD_SECRET=` line in `/etc/vox/worlds.env`
   (mode 600).
4. **Services:** install `vox-directory.service` and `vox-host.service` in `/etc/systemd/system`,
   then `systemctl enable --now vox-directory vox-host`.
   - The directory goes first: it makes the signing key (`/var/lib/vox/directory.key`, and
     `.pub` for the worlds).
   - vox-host runs every world on 127.0.0.1:7790, at `/w/<id>/ws` and at its old port's path.
   - With `--idle-stop 10`, a world starts with its first player and stops ten minutes after the
     last; `--keep void` keeps the Void running.
   - With `--instances` (and the directory's `--instances-at http://127.0.0.1:7790`), it makes
     raid parties' copies of the dungeons.
   - `vox-world@.service` runs one world per unit instead. Its `/etc/vox/<id>.env` holds `PORT`
     and `SEED`.
5. **Expose it** through a Cloudflare Tunnel, with no open ports. `cloudflared/config.yml` is a
   locally managed tunnel's routes, for one world per port:
   - `cloudflared tunnel create vox`, then `cloudflared tunnel route dns vox <your host>`;
   - install the file as `/etc/cloudflared/config.yml`, then `cloudflared service install`.
   - With vox-host, route `^/w/` to `http://127.0.0.1:7790` instead.
   - Keep `/api/world/` and `/api/host/` refused: they're for the servers on localhost.

   After a release, `deploy/purge-cache.sh` clears Cloudflare's cached files.
6. **Back up `/var/lib/vox`:** the directory's accounts, key, wallets and ledger, and every
   world's blocks.

World owners and authority: `sudo -u vox /opt/vox/bin/vox-directory --db /var/lib/vox/directory.db --key /var/lib/vox/directory.key --world "void=The Void=/w/7780/ws" admin own-world void "Their Name"`
(the same `--world` list as the service), or `admin grant "Name" 50`.
