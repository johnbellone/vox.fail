---
type: Decision
title: "Decision: Host vox.fail Behind a Cloudflare Tunnel"
description: "Cloudflared carries traffic to localhost services so the server needs no open ports or certificates; worlds answer on /w/<port>/ws."
tags: [decision, deployment, cloudflare]
generated: { by: agent/mcp, at: "2026-11-26T15:54:16Z" }
status: stable
---

# Decision: Cloudflare Tunnel

**Date**: 2026-11-25 (commit `9de8028`; replaced an earlier Caddyfile).

**Decision**: run `cloudflared` on the server; route `/w/<port>/` to each world and everything else to the directory. Nothing is open on the machine and its address stays private.

**Consequences**
- A tunnel cannot rewrite paths, so `vox-server` answers at `/w/<port>/ws` as well as `/ws`. Portals name worlds by port (`":7781"`), which becomes `wss://vox.fail/w/7781/ws` unchanged.
- The directory needs `--behind-proxy` to take the visitor address from `CF-Connecting-IP`.
- Adding a world means a new ingress rule and a restart of cloudflared.
- Several dashboard settings must be right (Rocket Loader off, Bot Fight Mode off for `/w/`, a cache rule). Cloudflare is a single point of dependency.

See `architecture/deployment`.
