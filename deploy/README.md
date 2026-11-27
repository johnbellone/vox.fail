# Deploying Vox at vox.fail (Cloudflare)

vox.fail is on Cloudflare. One machine runs the directory and the worlds on localhost, and a
Cloudflare Tunnel (`cloudflared`) carries traffic to them: no open ports, no certificate to
manage, and the machine's address stays private.

| What | Listens on | Public address |
| --- | --- | --- |
| vox-directory (pages, sign-in, characters, the browser client) | 127.0.0.1:7700 | https://vox.fail/ |
| The Void (vox-server) | 127.0.0.1:7780 | wss://vox.fail/w/7780/ws |
| The yard | 127.0.0.1:7781 | wss://vox.fail/w/7781/ws |
| The islands | 127.0.0.1:7782 | wss://vox.fail/w/7782/ws |
| The dark sea | 127.0.0.1:7783 | wss://vox.fail/w/7783/ws |
| Orbit | 127.0.0.1:7784 | wss://vox.fail/w/7784/ws |

On Kubernetes instead: the image in `docker/` and the Helm chart in `helm/` (see
[helm/README.md](helm/README.md)).

Portals in the levels name other worlds by port (`":7781"`); from a world at
`wss://vox.fail/w/7780/ws` that becomes `wss://vox.fail/w/7781/ws`, so the levels work
unchanged locally and here. The tunnel can't rewrite paths, so world servers answer at
`/w/<port>/ws` as well as `/ws`.

## On the server

1. Build: `cargo build --release -p vox-directory -p vox-server -p vox-client` and
   `tools/dev/build_web.sh` (the browser client, into `apps/site/public/pkg`), then the pages:
   `bun install && (cd apps/site && bun run build)` (Bun is needed to build, not to run).
2. Copy to `/opt/vox`: `bin/` (the three binaries), `apps/site/dist/` as `web/`, `assets/`, `data/`. Make a `vox`
   user and `/var/lib/vox` owned by it.
3. Worlds: `/etc/vox/void.env` holds `PORT=7780` and `SEED=data/levels/void.json`;
   `yard.env` `PORT=7781`, `SEED=data/levels/yard.json`; `islands.env` `PORT=7782`,
   `SEED=data/levels/islands.json`; `sea.env` `PORT=7783`, `SEED=data/levels/sea.json`;
   `orbit.env` `PORT=7784`, `SEED=data/levels/orbit.json`.
4. Secrets: copy `directory.env` to `/etc/vox/directory.env` (root, mode 600) and fill it in
   (Discord client ID and secret; optionally a Steam Web API key; a world secret from
   `openssl rand -hex 32`). Put the same `VOX_WORLD_SECRET=` line in `/etc/vox/worlds.env`
   (mode 600) for the world servers: with it they report who's playing (authority, raids)
   and take the plots and portals players make. A world's `--name` (`void`, `yard`,
   `islands`, `sea`, `orbit`) is its id in the directory's `--world "void=The Void=/w/7780/ws"`.
5. Install `vox-directory.service` and `vox-world@.service` in `/etc/systemd/system`, then
   `systemctl enable --now vox-directory`, then
   `systemctl enable --now vox-world@void vox-world@yard vox-world@islands vox-world@sea vox-world@orbit`. The directory
   goes first: it makes the signing key (`/var/lib/vox/directory.key`, and `.pub` for the
   worlds).
6. The tunnel: install `cloudflared`, then
   `cloudflared tunnel login`, `cloudflared tunnel create vox` (copy the credentials file it
   names to `/etc/cloudflared/vox.json`), `cloudflared tunnel route dns vox vox.fail` (remove
   any A/AAAA/CNAME record for vox.fail first), copy `cloudflared/config.yml` to
   `/etc/cloudflared/config.yml`, and `cloudflared service install`.
   (Or make the tunnel in the dashboard, Zero Trust → Networks → Tunnels, with the same four
   public hostnames: `vox.fail` path `^/w/7780/` → `http://127.0.0.1:7780`, and so on, the
   directory last; then `cloudflared service install <token>`.)

## All worlds in one process (vox-host)

Instead of a `vox-world@` unit per world, `vox-host` runs them all on one port
(`vox-host.service`): each world at `/w/<id>/ws`, and at its old port as an alias
(`/w/7781/ws` is the yard), so levels and clients don't change. It reads and writes the same
`/var/lib/vox/<id>.db` files, so switching is:

1. `systemctl disable --now vox-world@void vox-world@yard vox-world@islands vox-world@sea vox-world@orbit`
2. Install `vox-host.service` in `/etc/systemd/system`, then `systemctl enable --now vox-host`.
3. In the tunnel, replace the per-port rules with one: `vox.fail` path `^/w/` →
   `http://127.0.0.1:7790`, before the directory's rule.

Adding a world is then a `--world <id>` in the unit, not a new unit and tunnel rule. With
`--idle-stop 10`, a world starts when the first player (or watcher) comes and is saved and
stopped after ten minutes with nobody there; `--keep void` keeps the Void always running. A
world's status page (`/w/<id>/`) says `"running": false` while it's stopped, without starting it. Design:
`docs/design/world-network.md`.

## In the Cloudflare dashboard (vox.fail)

- SSL/TLS: Always Use HTTPS on.
- Network: WebSockets on (the default).
- Speed → Optimization: Rocket Loader off. It rewrites the pages' scripts, which the pages'
  content policy then blocks. Scrape Shield → Email Address Obfuscation off, for the same
  reason.
- Security: leave Bot Fight Mode off, or skip it for `/w/` with a WAF skip rule. The desktop
  client isn't a browser and can't answer a challenge.
- Caching → Cache Rules: one rule, "URI Path starts with `/pkg/` or `/assets/` or `/data/`"
  → Eligible for cache, edge TTL "use cache-control header if present". Cloudflare doesn't
  cache `.wasm`, `.glb` or `.json` by default, and the browser client is about 11 MB
  compressed. The directory marks these five minutes; the API, sign-in and pages are never
  kept. After a release, run `deploy/purge-cache.sh` (zone ID and an API token with Cache
  Purge).
- Rules → Redirect Rules: `www.vox.fail/*` → `https://vox.fail/${1}`, 301 (and a proxied
  DNS record for www, say `AAAA www 100::`, so the rule sees the requests).

## The sign-in sites

- Discord: discord.com/developers/applications → New Application → OAuth2. Add the redirect
  `https://vox.fail/auth/discord/return`. The client ID and secret go in
  `/etc/vox/directory.env`. Only the `identify` scope is asked for: Discord tells us your id
  and name, nothing else.
- Steam: nothing to register. Steam sends people back to `https://vox.fail/auth/steam/return`
  and the directory checks the answer with Steam. A Steam Web API key
  (steamcommunity.com/dev/apikey) lets new accounts take their Steam name.

## Desktop client and backups

The desktop client goes to the Void at vox.fail by default: `vox-client --token <launch code>`.
Offer downloads on the start page with `--download "Windows=https://…"` on the directory.
GitHub Releases is a good home for them; files under `apps/site/public/downloads/` (copied into the build) are served too.

Back up `/var/lib/vox`: the directory's accounts, key, wallets and ledger, and every world's
blocks.

World owners (who may issue a world's currency) are set by the operators:
`sudo -u vox /opt/vox/bin/vox-directory --db /var/lib/vox/directory.db --key /var/lib/vox/directory.key --world "void=The Void=/w/7780/ws" admin own-world void "Their Name"`
(the same `--world` list as the service). `admin grant "Name" 50` gives authority.
