// Builds the Worker's static files into dist/ (docs/design/cloudflare-hosting.md):
//   dist/              the site (apps/site/dist: the pages, chunks/, static/, pkg/)
//   dist/assets/       the game's assets      dist/data/   the game's data
//   dist/_offline/     the offline page (src/offline.html) with the README's screenshots
//
//   bun run build.ts                  the site from apps/site/dist (build it first)
//   VOX_SITE=<dir> VOX_ASSETS=<dir> VOX_DATA=<dir> bun run build.ts   files from elsewhere (a
//                                     release takes them from the image's /opt/vox)
//   VOX_REQUIRE_PKG=1                 fail without the browser client (releases)
//
// A Worker's file may be at most 25 MiB. The browser client's .wasm is bigger, so a big file is
// left out when its .br and .gz copies are there (the Worker sends those); otherwise it's an error.

import { cp, mkdir, readdir, rm, stat } from "node:fs/promises";
import path from "node:path";

const root = import.meta.dir;
const repo = path.join(root, "../..");
const dist = path.join(root, "dist");
const LIMIT = 25 * 1024 * 1024;

const site = process.env.VOX_SITE ?? path.join(repo, "apps/site/dist");
const assets = process.env.VOX_ASSETS ?? path.join(repo, "assets");
const data = process.env.VOX_DATA ?? path.join(repo, "data");

const exists = (p: string) => stat(p).then(() => true, () => false);

/** Copy a folder, leaving out what's too big (when its compressed copies stand in for it). */
async function copyTree(from: string, to: string): Promise<{ files: number; skipped: string[] }> {
  let files = 0;
  const skipped: string[] = [];
  await cp(from, to, {
    recursive: true,
    filter: async (src) => {
      const s = await stat(src);
      if (s.isDirectory()) return true;
      if (s.size <= LIMIT) {
        files++;
        return true;
      }
      if ((await exists(src + ".br")) && (await exists(src + ".gz"))) {
        skipped.push(path.relative(from, src));
        return false;
      }
      throw new Error(`${src} is ${(s.size / 1048576).toFixed(1)} MiB, over a Worker's 25 MiB, with no .br and .gz copies`);
    },
  });
  return { files, skipped };
}

await rm(dist, { recursive: true, force: true });
await mkdir(dist, { recursive: true });

// the site
if (!(await exists(path.join(site, "index.html")))) {
  console.error(`no site in ${site}: build it first (cd apps/site && bun run build)`);
  process.exit(1);
}
const hasPkg = await exists(path.join(site, "pkg", "vox_client.js"));
if (!hasPkg && process.env.VOX_REQUIRE_PKG) {
  console.error(`no browser client in ${site}/pkg (tools/dev/build_web.sh)`);
  process.exit(1);
}
const s = await copyTree(site, dist);
const a = await copyTree(assets, path.join(dist, "assets"));
const d = await copyTree(data, path.join(dist, "data"));

// the offline page, under /_offline/ so it never shadows a page of the site
const out = path.join(dist, "_offline");
const result = await Bun.build({
  entrypoints: [path.join(root, "src/offline.html")],
  outdir: out,
  publicPath: "/_offline/",
  minify: true,
  naming: { entry: "[name].[ext]", chunk: "chunks/[name]-[hash].[ext]", asset: "static/[name]-[hash].[ext]" },
  define: { "process.env.NODE_ENV": JSON.stringify("production") },
});
if (!result.success) {
  for (const log of result.logs) console.error(log);
  process.exit(1);
}
await mkdir(path.join(out, "shots"), { recursive: true });
await cp(path.join(repo, "docs/screenshots"), path.join(out, "shots"), { recursive: true });

const count = async (dir: string): Promise<number> => {
  let n = 0;
  for (const e of await readdir(dir, { withFileTypes: true, recursive: true })) if (e.isFile()) n++;
  return n;
};
console.log(
  `built dist/: the site (${s.files} files${hasPkg ? "" : ", no browser client"}), assets (${a.files}), data (${d.files}), ` +
    `the offline page; ${await count(dist)} files in all`,
);
const skipped = [...s.skipped, ...a.skipped, ...d.skipped];
if (skipped.length) console.log(`left out (served from their .br / .gz copies): ${skipped.join(", ")}`);
