// Prints docs/manual.html to docs/Thaana-PDF-to-Word-User-Manual.pdf with Microsoft Edge
// (or Chrome) in headless mode.   node scripts/manual.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath, pathToFileURL } from 'node:url';

const root = path.join(path.dirname(fileURLToPath(import.meta.url)), '..');
const src = path.join(root, 'docs', 'manual.html');
const out = path.join(root, 'docs', 'Thaana-PDF-to-Word-User-Manual.pdf');
const browsers = [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Microsoft/Edge/Application/msedge.exe',
  'C:/Program Files/Google/Chrome/Application/chrome.exe',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
];
const browser = browsers.find((b) => fs.existsSync(b));
if (!browser) { console.error('Microsoft Edge or Google Chrome is needed to print the manual.'); process.exit(1); }
const profile = fs.mkdtempSync(path.join(os.tmpdir(), 'manual-'));
execFileSync(browser, ['--headless', '--disable-gpu', `--user-data-dir=${profile}`, '--no-pdf-header-footer', `--print-to-pdf=${out}`, pathToFileURL(src).href], { stdio: 'ignore', timeout: 60000 });
fs.rmSync(profile, { recursive: true, force: true });
console.log(`docs/Thaana-PDF-to-Word-User-Manual.pdf (${Math.round(fs.statSync(out).size / 1024)} KB)`);
