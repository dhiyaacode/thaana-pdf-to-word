// Checks that the pure scan steps in Rust (crates/thaana-core/src/scan.rs) give exactly the
// JavaScript results (renderer/segment.js, columns.js) on the same page images: Otsu threshold,
// line spacing and line boxes, newspaper column blocks, ruled-line removal.
//   node test/scan-parity.mjs [page.png ...]     (needs Python with Pillow; build the Rust CLI first)
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { otsu, findLines, removeRuling } from '../renderer/segment.js';
import { findBlocks } from '../renderer/columns.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = path.join(here, '..', 'crates', 'thaana-core', 'target', 'release', process.platform === 'win32' ? 'thaana-pdf.exe' : 'thaana-pdf');
const fx = path.join(here, 'fixtures');
const images = [...['page.png', 'columns.png', 'line.png'].map((f) => path.join(fx, f)).filter(fs.existsSync), ...process.argv.slice(2)];
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'scan-parity-'));
// compare values, not key order (Rust writes object keys alphabetically)
const canon = (v) => JSON.stringify(v, (k, x) => (x && typeof x === 'object' && !Array.isArray(x) ? Object.fromEntries(Object.entries(x).sort(([a], [b]) => (a < b ? -1 : 1))) : x));
let ok = 0, n = 0;
for (const img of images) for (const size of [1600, 2400]) {
  // the same grey image for both: scaled like prepare() does, 8-bit
  const raw = path.join(tmp, 'g.gray');
  const [w, h] = execFileSync('python', ['-I', '-c', `
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert('L'); s = float(sys.argv[3]) / max(im.size)
im = im.resize((max(1, round(im.width * s)), max(1, round(im.height * s))), Image.LANCZOS)
open(sys.argv[2], 'wb').write(im.tobytes()); print(im.width, im.height)`, img, raw, String(size)]).toString().trim().split(' ').map(Number);
  const g = Float32Array.from(fs.readFileSync(raw));
  const thr = otsu(g);
  const { boxes, pitch } = findLines(g, w, h, thr);
  const fb = findBlocks(g, w, h, thr), letter = fb.letter ?? 0;
  const blocks = fb.map((b) => ({ l: b.l, t: b.t, r: b.r, b: b.b, kind: b.kind, lines: b.lines ?? 0 }));
  const ruled = removeRuling(g, w, h, thr);
  let changed = 0, sum = 0; for (let k = 0; k < g.length; k++) if (ruled[k] !== g[k]) { changed++; sum += k; }
  const guided = findLines(g, w, h, thr, 0, letter);   // spacing search guided by the letter height
  const js = { otsu: thr, pitch, boxes, blocks, letter, pitch2: guided.pitch, boxes2: guided.boxes, ruling: { changed, sum } };
  const rs = JSON.parse(execFileSync(exe, ['--scan-parity', raw, String(w), String(h)]).toString());
  const name = `${path.basename(img)} at ${w}x${h}`;
  const diffs = Object.keys(js).filter((k) => canon(js[k]) !== canon(rs[k]));
  n++;
  if (diffs.length) console.log(`FAIL  ${name}: ${diffs.map((k) => `${k}\n      JS:   ${JSON.stringify(js[k]).slice(0, 300)}\n      Rust: ${JSON.stringify(rs[k]).slice(0, 300)}`).join('\n  ')}`);
  else { ok++; console.log(`PASS  ${name}: threshold ${thr}, ${guided.boxes.length} lines (spacing ${guided.pitch} px = ${(guided.pitch / (letter || 1)).toFixed(1)} letter heights), ${blocks.length} blocks, ${changed} ruled pixels`); }
}
console.log(`\n${ok}/${n} identical in JavaScript and Rust`);
process.exit(ok === n ? 0 : 1);
