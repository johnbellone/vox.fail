# apps/site: the vox.fail pages

The pages people see at vox.fail: sign in, make your character, the inventory, friends and
raids, and playing in the browser. React 19 and Mantine 9, built with Bun. It replaces the
hand-written pages in `web/`; the URLs and the API it talks to are the same.

## Commands

From the repository root (`bun install` once):

```bash
bun install                      # dependencies for every app in apps/
cd apps/site
bun run build                    # build into dist/
bun run dev                      # build, then rebuild on every change
bun run typecheck                # TypeScript, no output
bun test                         # unit tests (*.test.ts)
```

Serve it with the directory, which also answers the API:

```bash
cargo run -p vox-directory -- --listen 127.0.0.1:7700 --web apps/site/dist
```

Then open http://127.0.0.1:7700/. For the play page and the front page's live view, also run
a world (`cargo run --release -p vox-server`, or `tools/dev/play-local.sh` for three) and build the
WebAssembly client with `tools/dev/build_web.sh` (see below).

## Layout

```
build.ts                 Bun.build over src/pages/*.html into dist/, then copies public/
src/
  pages/*.html           one HTML entry per URL: index (/), create, play, inventory, friends
  entries/*.tsx          each page's script: mount(<ThePage />)
  api/
    client.ts            fetch wrapper, errors, helpers (worldUrl, when, num)
    hooks.ts             useLoad, useApi, useSignedIn
    types.ts             what the directory's API returns
  components/            Shell (Mantine provider, the Void), Page (wordmark and nav),
                         Wordmark, Glyph (pixel pictures), Portrait, ConfirmButton, Problem,
                         LiveWorld (the front page's live view)
  game/wasm.ts           loading and starting the WebAssembly client
  features/
    home/                sign in or up (Steam, Discord, name and password); signed-in door
    character/           cast and wardrobe pickers; Three.js turntable (preview.ts)
    inventory/           explorer (model.ts holds the filtering rules, with tests),
                         item detail, feats, ledger
    friends/             friends, groups, raid parties
    play/                starts the WebAssembly client on a full-window canvas
  styles/global.css      fonts and the Void backdrop
  theme.ts               Mantine theme: gate blue, ember for errors, moon greys, Atkinson
public/                  copied into dist/ as is; public/pkg/ is the wasm client (ignored by git)
```

## Notes

- **The WebAssembly client** is loaded at run time from `/pkg/vox_client.js`, not bundled
  (`src/game/wasm.ts`). `tools/dev/build_web.sh` writes it to `public/pkg/`; run it before
  `bun run build`. The play page and the front page both use it.
- **The front page's live view** (`components/LiveWorld.tsx`) starts the client in watch mode
  on the first world the directory lists: no sign-in, no avatar, a camera drifting round the
  spawn. The world server sends watchers everything players see except names, counts them
  apart from players and caps them (`vox-server --max-watchers`, 200 by default). The still
  Void stays instead on browsers without WebGPU, with reduced motion, on screens under 768 px
  and with data saver on, and when the world can't be reached.
- **Content Security Policy.** The directory sends `script-src 'self'` and allows inline styles,
  which is what Mantine needs. Nothing here uses inline scripts, so do not add Mantine's
  `ColorSchemeScript`; the site is light only (`forceColorScheme="light"`).
- **Assets and data** (`/assets/...`, `/data/...`) come from the directory, not from this
  build.
