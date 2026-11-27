// Builds the offline page (src/offline.html) into dist/_offline/, the Worker's static files, with
// the README's screenshots beside it. Everything is under /_offline/ so it never shadows a path
// the directory serves.
//
//   bun run build.ts        then: bunx wrangler deploy

import { cp, mkdir, rm } from "node:fs/promises";
import path from "node:path";

const root = import.meta.dir;
const out = path.join(root, "dist", "_offline");

await rm(path.join(root, "dist"), { recursive: true, force: true });
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
await cp(path.join(root, "../../docs/screenshots"), path.join(out, "shots"), { recursive: true });
console.log(`built the offline page into dist/_offline/`);
