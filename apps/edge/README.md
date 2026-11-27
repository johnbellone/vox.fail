# vox.fail's edge worker

A Cloudflare Worker on `vox.fail/*`, in front of the tunnel. While the server is up it changes
nothing: every request, WebSockets included, goes on to the directory and the worlds. When the
server can't be reached (Cloudflare answers 530, 502, 504 or a 52x), it answers instead:

| Request | While the server is offline |
| --- | --- |
| A page (`/`, `/play`, …) | The offline page (`src/offline.tsx`): what Vox is, the worlds' screenshots, "offline right now", status 503, never cached |
| `/api/…` | `503 {"error": "vox is offline right now; try again later"}` |
| Anything else (game files, world sockets) | Cloudflare's own answer |

The directory's own `503` ("busy") passes through untouched. The offline page's files live under
`/_offline/`, which the directory never uses, and come from the Worker, not the server.

```bash
bun test                 # the Worker against a stand-in server
bun run build            # the offline page into dist/_offline/ (with docs/screenshots)
bun run deploy           # build and deploy (CLOUDFLARE_API_TOKEN, CLOUDFLARE_ACCOUNT_ID)
```

The page shares its text with the home page (`apps/site/src/features/home/Pitch.tsx`). To look at
it, build and serve `dist/` (`python3 -m http.server -d dist`), then open `/_offline/offline.html`.

Deploys run from `.github/workflows/edge.yml` on main, through the `production` environment
(set its required reviewers and secrets as that file says). Requests through a Worker count
against the Workers plan (100,000 a day on the free plan; a world's WebSocket is one request).
