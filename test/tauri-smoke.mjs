// End-to-end test of the app: starts the real app built with test hooks, runs
// test/tauri-smoke.page.js inside its window, then checks the Word file it saved.
//   npm run test:app [-- extra.pdf ...]
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.join(here, '..');
const exe = path.join(root, 'src-tauri', 'target', 'smoke', 'release', process.platform === 'win32' ? 'thaana-pdf-to-word.exe' : 'thaana-pdf-to-word');
if (!fs.existsSync(exe)) { console.error('Build it first: npm run test:app'); process.exit(2); }

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'thaana-tauri-'));
const saveTo = path.join(tmp, 'typed.docx');
const script = path.join(tmp, 'smoke.js');
const config = {
  fixtures: path.join(here, 'fixtures').replace(/\\/g, '/'),
  spec: JSON.parse(fs.readFileSync(path.join(here, 'fixtures.json'), 'utf8')),
  extra: process.argv.slice(2).map((f) => path.resolve(f).replace(/\\/g, '/')),
};
fs.writeFileSync(script, `window.__SMOKE = ${JSON.stringify(config)};\n` + fs.readFileSync(path.join(here, 'tauri-smoke.page.js'), 'utf8'));

const child = spawn(exe, [], { env: { ...process.env, THAANA_SMOKE_SCRIPT: script, THAANA_SAVE_TO: saveTo }, stdio: ['ignore', 'pipe', 'inherit'] });
let out = '';
child.stdout.on('data', (d) => { out += d; process.stdout.write(d); });
const timer = setTimeout(() => { console.error('timed out'); child.kill(); }, 600000);
const code = await new Promise((r) => child.on('exit', r));
clearTimeout(timer);

// The Word file written by Rust: one 3x4 table with the wrapped cell intact
function unzipEntry(buf, name) {
  let e = buf.length - 22; while (buf.readUInt32LE(e) !== 0x06054b50) e--;
  let p = buf.readUInt32LE(e + 16);
  for (let i = 0, n = buf.readUInt16LE(e + 10); i < n; i++) {
    const method = buf.readUInt16LE(p + 10), size = buf.readUInt32LE(p + 20), nl = buf.readUInt16LE(p + 28);
    const xl = buf.readUInt16LE(p + 30), cl = buf.readUInt16LE(p + 32), off = buf.readUInt32LE(p + 42);
    if (buf.toString('utf8', p + 46, p + 46 + nl) === name) {
      const start = off + 30 + buf.readUInt16LE(off + 26) + buf.readUInt16LE(off + 28), data = buf.subarray(start, start + size);
      return (method === 8 ? zlib.inflateRawSync(data) : data).toString('utf8');
    }
    p += 46 + nl + xl + cl;
  }
  throw new Error(name + ' not in zip');
}
let docxOk = false;
if (fs.existsSync(saveTo)) {
  const xml = unzipEntry(fs.readFileSync(saveTo), 'word/document.xml');
  const cells = [...xml.matchAll(/<w:tc>[\s\S]*?<\/w:tc>/g)].map((m) => [...m[0].matchAll(/<w:t[^>]*>([^<]*)<\/w:t>/g)].map((x) => x[1]).join(''));
  docxOk = (xml.match(/<w:tbl>/g) || []).length === 1 && (xml.match(/<w:tr>/g) || []).length === 3 && cells.length === 12 && cells.includes(config.spec.table[1][3]);
}
console.log(`${docxOk ? 'PASS' : 'FAIL'}  Word file from Rust: one table, 3 rows x 4 cells, wrapped cell intact`);
const ok = code === 0 && docxOk && /RESULT (\d+)\/\1/.test(out);
console.log(ok ? '\nApp: all checks passed' : `\nApp: FAILED (exit code ${code})`);
process.exit(ok ? 0 : 1);
