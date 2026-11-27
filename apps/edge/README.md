# vox.fail's edge Worker

A Cloudflare Worker on `vox.fail/*` (`vox-edge`). It serves the site itself, and sends only
what needs the servers on to them (docs/design/cloudflare-hosting.md).

| Request | Answered by |
| --- | --- |
| Pages (`/`, `/play`, …), `/chunks/`, `/static/` | The Worker's files (the built site), with the directory's security headers and `no-cache` |
| `/pkg/` (the browser client) | The Worker's files, in the brotli or gzip copy the browser takes, kept five minutes |
| `/assets/`, `/data/` | The Worker's files, kept five minutes |
| `/api/world/…`, `/api/host/…` | `404`: these are for the servers on localhost only |
| `/api/…`, `/auth/…`, `/w/…` | The servers. When they can't be reached (530, 502, 504, a 52x), `/api/` answers `503 {"error": "vox is offline …"}`, a sign-in page gets the offline page, and a world socket gets Cloudflare's answer |

The directory's own `503` ("busy") passes through. When the site's API client gets the
offline answer, it goes to the offline page (`/_offline/offline.html`, from `src/offline.tsx`):
what Vox is, the worlds' screenshots, and a Try again that goes back to the page you were on
(`src/back.ts`). The page shares its text with the home page
(`apps/site/src/features/home/Pitch.tsx`).

The browser client's `.wasm` is bigger than a Worker's 25 MiB file limit. So `build.ts` leaves
out any file that big when its `.br` and `.gz` copies are there (`tools/dev/build_web.sh` makes
them), and the Worker sends a copy with its `Content-Encoding`.

```bash
(cd ../site && bun run build)   # the site first
bun test                        # the Worker against stand-in files and servers
bun run build                   # dist/: the site, assets/, data/, _offline/
VOX_SITE=… VOX_ASSETS=… VOX_DATA=… VOX_REQUIRE_PKG=1 bun run build   # a release: files from the image
bun run deploy                  # build and deploy with cf (signed in, or CLOUDFLARE_API_TOKEN
                                # and CLOUDFLARE_ACCOUNT_ID)
```

The Worker is set up in `cloudflare.config.ts` (name, route `vox.fail/*`, the `ASSETS` binding)
and `wrangler.config.ts` (the files folder). `cf` runs on Node, not Bun. The route needs
vox.fail's proxied DNS record (`deploy/cloudflare-setup.sh`).

Deploys come from releases, because only a release builds the browser client;
`.github/workflows/edge.yml` checks pull requests and `main`. Requests through a Worker count
against the Workers plan.
