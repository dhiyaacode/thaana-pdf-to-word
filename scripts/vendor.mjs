// Copies the browser libraries and reading models next to the interface (renderer/vendor/),
// where the app's web view loads them. Run by `npm install` and before builds.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const out = path.join(root, 'renderer', 'vendor');
const nm = (...p) => path.join(root, 'node_modules', ...p);
const copy = (from, toDir, name = path.basename(from)) => {
  fs.mkdirSync(toDir, { recursive: true });
  fs.copyFileSync(from, path.join(toDir, name));
};

fs.rmSync(out, { recursive: true, force: true });
// PDF.js
for (const f of ['pdf.mjs', 'pdf.worker.mjs']) copy(nm('pdfjs-dist', 'build', f), path.join(out, 'pdfjs'));
// Tesseract (browser build) and its WebAssembly cores (LSTM-only variants: the app uses OEM 1)
copy(nm('tesseract.js', 'dist', 'tesseract.min.js'), path.join(out, 'tesseract'));
copy(nm('tesseract.js', 'dist', 'worker.min.js'), path.join(out, 'tesseract'));
for (const f of fs.readdirSync(nm('tesseract.js-core'))) if (/-lstm\.wasm\.js$/.test(f)) copy(nm('tesseract.js-core', f), path.join(out, 'tesseract-core'));
// Reading models and word list (divx is the app's own model, used first when present)
for (const f of fs.readdirSync(path.join(root, 'lang'))) if (/\.(traineddata|txt)$/.test(f)) copy(path.join(root, 'lang', f), path.join(out, 'lang'));

const size = (d) => fs.readdirSync(d, { recursive: true }).reduce((s, f) => { const p = path.join(d, f); return s + (fs.statSync(p).isFile() ? fs.statSync(p).size : 0); }, 0);
console.log(`renderer/vendor ready (${(size(out) / 1e6).toFixed(1)} MB)`);
