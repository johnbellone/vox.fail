---
type: Decision
title: "Decision: vox.fail's Cloudflare Side Is Scripted, With a Tunnel Managed in Cloudflare"
description: "Superseded 2026-11-27: the tunnel \"vox\" is deleted; vox.fail runs on Cloudflare Containers behind the edge Worker. The setup script still sets the zone settings, cache and www rules."
tags: [decision, deployment, cloudflare, tunnel, dns]
generated: { by: agent/mcp, at: "2026-11-27T17:27:02Z" }
status: deprecated
---

## Decision

On 2026-11-27 vox.fail's Cloudflare side was set up from nothing with `deploy/cloudflare-setup.sh`, which uses the `cf` command line and is safe to run again:

- **Zone settings:** Always Use HTTPS on; WebSockets on; Rocket Loader off and Email Address Obfuscation off (both rewrite the pages, which their content policy blocks).
- **Rules:**
  - a cache rule makes `/pkg/`, `/assets/` and `/data/` eligible for caching, respecting the origin's headers;
  - a redirect rule sends `www.vox.fail` to `vox.fail` with a 301, keeping path and query. `www` has a proxied `AAAA 100::` record only so the rule sees requests.
- **Tunnel `vox`, managed in Cloudflare** (`config_src: cloudflare`):
  - `^/w/` goes to vox-host on `127.0.0.1:7790` (every world, by id or old-port alias);
  - `^/api/world/` is refused with a 404;
  - everything else goes to vox-directory on `127.0.0.1:7700`.
- **DNS:** `vox.fail` is a proxied CNAME to the tunnel. The earlier placeholder `AAAA vox.fail 100::` was removed.
- **The server** needs only `cloudflared service install <token>`, with the token from `cf tunnels token get <id>`. Until it connects, the tunnel answers 530 and the edge Worker shows the offline page ([[decisions/edge-worker-offline-page]]).

## Why

A tunnel managed in Cloudflare keeps the routes beside the rest of the zone's setup, in one script in the repo, and leaves the server with a single credential to install. Routing all worlds through vox-host (one port) means a new world needs no new tunnel rule. The directory unit already points `--instances-at` at vox-host.

## Trade-offs

`deploy/cloudflared/config.yml` (a locally managed tunnel, one port per world) remains as the alternative. Only one of the two can serve vox.fail. The server must run `vox-host.service`, not the `vox-world@` units.

See [[architecture/deployment]] and [[decisions/cloudflare-tunnel-hosting]].
