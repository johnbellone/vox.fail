// The close-up review sheet of an avatar: its own poses (with their props), two shared clips and
// eight face animations, rendered by three.js from assets/models/<id>.glb in headless Chromium.
//
//     cd tools/review && npm ci            # once (three, playwright; CI also: npx playwright install chromium)
//     node tools/review/render_sheet.mjs capybara            # writes docs/avatar/capybara-poses.png
//     node tools/review/render_sheet.mjs capybara out.png
//
// Each own clip is shown a little past its middle, where the pose is held. Faces are shown close
// up with the neutral body. No GPU needed: Chromium draws with SwiftShader.
import { createRequire } from 'node:module';
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '../..');
const require = createRequire(import.meta.url);
const { chromium } = require('playwright');

const id = process.argv[2];
if (!id || !/^[a-z][a-z0-9_]{1,31}$/.test(id)) {
  console.error('usage: node tools/review/render_sheet.mjs <avatar id> [out.png]');
  process.exit(2);
}
const model = path.join(root, 'assets/models', `${id}.glb`);
const out = path.resolve(process.argv[3] ?? path.join(root, 'docs/avatar', `${id}-poses.png`));

// the avatar's own clips (after the 124 shared ones), with their lengths
const glb = fs.readFileSync(model);
const json = JSON.parse(glb.subarray(20, 20 + glb.readUInt32LE(12)).toString());
const SHARED = 124;
const own = (json.animations ?? []).slice(SHARED).map(a => a.name);
if (!own.length) {
  console.error(`${id} has no clips of its own (after the ${SHARED} shared ones)`);
  process.exit(1);
}
const lengths = Object.fromEntries((json.animations ?? []).map(a => [a.name,
  Math.max(...a.samplers.map(s => json.accessors[s.input].max?.[0] ?? 0))]));

const pose = { dist: 4.4, look: 0.78, h: 0.9 };
const face = { yaw: 15, dist: 2.3, look: 1.12, h: 1.0 };
const shots = [
  { clip: 'Idle', t: 0.3, ...pose },
  ...own.map(c => ({ clip: c, t: +(lengths[c] * 0.55).toFixed(2), ...pose })),
  { clip: 'Walk', t: 0.3, label: 'Walk (shared)', ...pose },
  { clip: 'Victory', t: 0.8, label: 'Victory (shared)', ...pose },
  ...[['Grin', 0.5], ['Shocked', 0.5], ['Angry', 0.5], ['Sleepy', 0.8], ['Laugh', 0.4], ['Crying', 0.6], ['Proud', 0.6], ['Talk', 0.15]]
    .map(([f, t]) => ({ clip: `Face: ${f}`, t, label: f === 'Talk' ? 'Face: Talk (lip sync)' : undefined, ...face })),
];

// serve the page, three.js and the model from one origin
const types = { '.html': 'text/html', '.js': 'text/javascript', '.glb': 'model/gltf-binary' };
const srv = http.createServer((q, r) => {
  const url = decodeURIComponent(q.url.split('?')[0]);
  const file = url === '/model.glb' ? model : path.join(here, path.normalize(url));
  if (!file.startsWith(here) && file !== model) { r.writeHead(403); return r.end(); }
  fs.readFile(file, (e, b) => {
    if (e) { r.writeHead(404); return r.end(); }
    r.writeHead(200, { 'Content-Type': types[path.extname(file)] ?? 'application/octet-stream' });
    r.end(b);
  });
}).listen(0, '127.0.0.1');
await new Promise(ok => srv.once('listening', ok));
const port = srv.address().port;

const browser = await chromium.launch({ args: ['--use-gl=angle', '--use-angle=swiftshader', '--enable-unsafe-swiftshader'] });
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 2400 } });
  await page.goto(`http://127.0.0.1:${port}/sheet.html?cols=4&model=/model.glb&shots=${encodeURIComponent(JSON.stringify(shots))}`);
  await page.waitForFunction(() => document.title === 'done' || document.title.startsWith('error'), null, { timeout: 180000 });
  const title = await page.title();
  if (title !== 'done') throw new Error(title);
  fs.mkdirSync(path.dirname(out), { recursive: true });
  await (await page.$('#sheet')).screenshot({ path: out });
  console.log(`${path.relative(root, out)}: ${own.length} own clips (${own.join(', ')}), ${shots.length} pictures`);
} finally {
  await browser.close();
  srv.close();
}
