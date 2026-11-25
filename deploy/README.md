# Deploying Vox at vox.fail

One machine runs everything behind Caddy (which gets the TLS certificate):

| What | Listens on | Public address |
| --- | --- | --- |
| vox-directory (pages, accounts, characters, the browser client) | 127.0.0.1:7700 | https://vox.fail/ |
| The Void (vox-server) | 127.0.0.1:7780 | wss://vox.fail/w/7780/ws |
| The yard | 127.0.0.1:7781 | wss://vox.fail/w/7781/ws |
| The islands | 127.0.0.1:7782 | wss://vox.fail/w/7782/ws |

Portals in the levels name other worlds by port (`":7781"`); behind this proxy that becomes
`wss://vox.fail/w/7781/ws`, so the levels work unchanged locally and here.

## Steps

1. Build: `cargo build --release -p vox-directory -p vox-server -p vox-client` and
   `tools/build_web.sh` (the browser client, into `web/pkg`).
2. Copy to `/opt/vox`: `bin/` (the three binaries), `web/`, `assets/`, `data/`. Make a `vox`
   user and `/var/lib/vox` owned by it.
3. `/etc/vox/void.env`: `PORT=7780` and `SEED=data/levels/void.json`; `yard.env`:
   `PORT=7781`, `SEED=data/levels/yard.json`; `islands.env`: `PORT=7782`,
   `SEED=data/levels/islands.json`.
4. Install `vox-directory.service` and `vox-world@.service` in `/etc/systemd/system`, then
   `systemctl enable --now vox-directory vox-world@void vox-world@yard vox-world@islands`.
   Start the directory first: it makes the signing key the worlds check sign-ins with.
5. Point vox.fail (and www) at the machine, put `Caddyfile` in `/etc/caddy/`, reload Caddy.

The desktop client goes to the Void at vox.fail by default: `vox-client --token <launch code>`.
Offer it on the start page with `--download "Windows=https://vox.fail/downloads/vox-client-windows.zip"`
on the directory (files under `web/downloads/` are served as they are).

Back up `/var/lib/vox`: the directory's accounts and key, and every world's blocks.
