// Measures how well and how fast the app reads scans.
// It takes typed PDFs (whose true text is known from the PDF itself), makes the real app read
// their pages as pictures, and scores the result against the true text.
//   npm run bench:ocr -- some-typed.pdf [more.pdf ...] [--columns=off] [--save=out.txt]
// Without files it uses test/fixtures/typed.pdf (small: good as a check, not as a measure).
// Scores: words exactly right, words right when vowel signs are ignored, and characters right,
// each as the share of the true text found in the same order in what the app read.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { readPdf } from './run-pdf.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = path.join(here, '..', 'src-tauri', 'target', 'smoke', 'release', process.platform === 'win32' ? 'thaana-pdf-to-word.exe' : 'thaana-pdf-to-word');
if (!fs.existsSync(exe)) { console.error('Build the test app first: npm run test:tauri'); process.exit(2); }

const args = process.argv.slice(2);
const opt = (name, dflt) => (args.find((a) => a.startsWith(`--${name}=`)) || `--${name}=${dflt}`).split('=')[1];
const columns = opt('columns', 'on') !== 'off', save = opt('save', '');
let files = args.filter((a) => !a.startsWith('--'));
if (!files.length) files = [path.join(here, 'fixtures', 'typed.pdf')].filter(fs.existsSync);
if (!files.length) { console.error('No PDF given and test/fixtures/typed.pdf is missing.'); process.exit(2); }

const T0 = String.fromCodePoint(0x780), T1 = String.fromCodePoint(0x7B1);       // Thaana block
const FILI = new RegExp(`[${String.fromCodePoint(0x7A6)}-${String.fromCodePoint(0x7B0)}]`, 'g');
const words = (t) => t.match(new RegExp(`[${T0}-${T1}]+`, 'g')) || [];

// Length of the longest common subsequence: how much of `a` appears in `b` in the same order.
function lcs(a, b) {
  let prev = new Int32Array(b.length + 1), cur = new Int32Array(b.length + 1);
  for (let i = 1; i <= a.length; i++) {
    const ai = a[i - 1];
    for (let j = 1; j <= b.length; j++) cur[j] = ai === b[j - 1] ? prev[j - 1] + 1 : (prev[j] >= cur[j - 1] ? prev[j] : cur[j - 1]);
    [prev, cur] = [cur, prev];
  }
  return prev[b.length];
}

function runApp(file) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'thaana-bench-'));
  const script = path.join(tmp, 'bench.js');
  fs.writeFileSync(script, `window.__BENCH = ${JSON.stringify({ file: path.resolve(file).replace(/\\/g, '/'), columns })};\n` + fs.readFileSync(path.join(here, 'ocr-accuracy.page.js'), 'utf8'));
  return new Promise((resolve, reject) => {
    const child = spawn(exe, [], { env: { ...process.env, THAANA_SMOKE_SCRIPT: script }, stdio: ['ignore', 'pipe', 'ignore'] });
    let out = '';
    child.stdout.on('data', (d) => { out += d; });
    const timer = setTimeout(() => child.kill(), 900000);
    child.on('exit', () => {
      clearTimeout(timer);
      const line = out.split(/\r?\n/).find((l) => l.startsWith('BENCH '));
      line ? resolve(JSON.parse(line.slice(6))) : reject(new Error('the app gave no result for ' + file));
    });
  });
}

console.log(`Newspaper columns: ${columns ? 'on' : 'off'}`);
console.log('file'.padEnd(28), 'pages', '  time', ' words right', ' ignoring vowel signs', ' characters');
for (const file of files) {
  const truth = (await readPdf(file)).map((p) => p.text).join('\n');
  const g = words(truth);
  if (g.length < 20) { console.log(path.basename(file).padEnd(28), 'has no typed Thaana text to compare with'); continue; }
  const r = await runApp(file);
  if (!r.status.startsWith('Done')) { console.log(path.basename(file).padEnd(28), 'FAILED:', r.status); continue; }
  const o = words(r.text);
  const pct = (n, d) => (100 * n / d).toFixed(1).padStart(5) + '%';
  const strip = (w) => w.replace(FILI, '');
  const chars = (ws) => Array.from(ws.join(''));
  console.log(path.basename(file).padEnd(28), String(r.pages).padStart(5), (r.secs.toFixed(1) + ' s').padStart(7),
    pct(lcs(g, o), g.length).padStart(12), pct(lcs(g.map(strip), o.map(strip)), g.length).padStart(21), pct(lcs(chars(g), chars(o)), chars(g).length).padStart(11),
    `  (${g.length} words; clean-up in ${r.prep})`);
  if (save) fs.writeFileSync(save, r.text);
}
