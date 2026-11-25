// Checks that the Rust PDF reader (crates/thaana-core) gives exactly the same result as the
// JavaScript one (renderer/pdfglyphs.js) on every page: same kind, same lines, same text.
//   node test/parity.mjs [extra.pdf ...]      (build first: cargo build --release in crates/thaana-core)
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { readPdf } from './run-pdf.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const exe = path.join(here, '..', 'crates', 'thaana-core', 'target', 'release', process.platform === 'win32' ? 'thaana-pdf.exe' : 'thaana-pdf');
const fixtures = fs.existsSync(path.join(here, 'fixtures')) ? fs.readdirSync(path.join(here, 'fixtures')).filter((f) => f.endsWith('.pdf')).map((f) => path.join(here, 'fixtures', f)) : [];
const files = [...fixtures, ...process.argv.slice(2)];
if (!files.length) { console.log('no PDFs to compare (run test/make-fixtures.* first)'); process.exit(0); }

const kindOf = (k) => k.split(' ')[0].replace('damaged', 'damaged');   // JS: "text", "damaged text (…)", "scan (…)", "blank"
let pages = 0, same = 0;
for (const [label, opts, flags] of [['Ignore Arabic on', {}, []], ['Ignore Arabic off', { dropArabic: false }, ['--keep-arabic']]]) {
  const rust = JSON.parse(execFileSync(exe, ['--json', ...flags, ...files], { maxBuffer: 1 << 28 }).toString('utf8'));
  for (const [i, file] of files.entries()) {
    const name = `${path.basename(file)} (${label})`;
    if (rust[i].error) { console.log(`FAIL  ${name}: Rust could not read it: ${rust[i].error}`); pages++; continue; }
    const js = await readPdf(file, opts);
    if (js.length !== rust[i].pages.length) { console.log(`FAIL  ${name}: ${js.length} pages in JS, ${rust[i].pages.length} in Rust`); pages++; continue; }
    js.forEach((j, p) => {
      const r = rust[i].pages[p];
      pages++;
      const jl = j.raw.split('\n'), rl = r.raw;
      const problems = [];
      if (kindOf(j.kind) !== r.kind) problems.push(`kind: JS ${kindOf(j.kind)} / Rust ${r.kind}`);
      if (j.raw !== rl.join('\n')) {
        const k = jl.findIndex((l, n) => l !== rl[n]);
        problems.push(`line ${k + 1} differs:\n      JS:   ${JSON.stringify(jl[k])}\n      Rust: ${JSON.stringify(rl[k])}`);
      }
      if (j.text !== r.text) problems.push('final text differs');
      if (problems.length) console.log(`FAIL  ${name} page ${p + 1}: ${problems.join('; ')}`);
      else same++;
    });
  }
}
console.log(`\n${same}/${pages} pages identical in JavaScript and Rust`);
process.exit(same === pages ? 0 : 1);
