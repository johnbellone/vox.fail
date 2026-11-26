// Builds the vox.fail pages into dist/, which vox-directory serves (`--web apps/site/dist`).
//
//   bun run build            once
//   bun run build --watch    again on every change (for working on the pages)
//
// Each page in src/pages is its own HTML entry, so the URLs stay what the directory routes:
// /, /create, /play, /inventory, /friends, /help. Everything in public/ is copied as is, including
// public/pkg, the WebAssembly client that tools/dev/build_web.sh writes. Bundled files (fonts, the
// icon) go to static/, not assets/: the directory serves /assets from the repository's assets/.

import { cp, rm, readdir } from "node:fs/promises";
import { watch } from "node:fs";
import path from "node:path";

const root = import.meta.dir;
const out = path.join(root, "dist");
const pages = path.join(root, "src/pages");

async function build() {
  const started = performance.now();
  const entrypoints = (await readdir(pages)).filter((f) => f.endsWith(".html")).map((f) => path.join(pages, f));
  await rm(out, { recursive: true, force: true });
  const result = await Bun.build({
    entrypoints,
    outdir: out,
    minify: true,
    splitting: true,
    sourcemap: "linked",
    naming: { entry: "[name].[ext]", chunk: "chunks/[name]-[hash].[ext]", asset: "static/[name]-[hash].[ext]" },
    define: { "process.env.NODE_ENV": JSON.stringify("production") },
  });
  if (!result.success) {
    for (const log of result.logs) console.error(log);
    return false;
  }
  await cp(path.join(root, "public"), out, { recursive: true });
  console.log(`built ${entrypoints.length} pages into dist/ in ${Math.round(performance.now() - started)} ms`);
  return true;
}

const ok = await build();
if (process.argv.includes("--watch")) {
  let timer: Timer | undefined;
  watch(path.join(root, "src"), { recursive: true }, () => {
    clearTimeout(timer);
    timer = setTimeout(() => void build(), 100);
  });
  console.log("watching src/ …");
} else if (!ok) {
  process.exit(1);
}
