// The bridge between the interface and the Rust side of the app: it defines `window.ocr`.
// Files, dialogs, clipboard, Word export, labels, the PDF text reader and scan clean-up run in Rust
// (src-tauri/src/main.rs, crates/thaana-core). Reading the text lines of scans (OCR) runs here in
// the web view, with Tesseract and the models in vendor/lang.
(() => {
  if (window.ocr || !window.__TAURI__) return;
  const { invoke } = window.__TAURI__.core;
  const base = new URL('vendor/', location.href).href;

  // Tauri answers a missing file with index.html (status 200), and labels .traineddata as
  // text/html too, so look at the bytes: a model never starts with "<".
  const exists = async (url) => {
    try {
      const r = await fetch(url); if (!r.ok) return false;
      const head = new Uint8Array(await r.arrayBuffer()).subarray(0, 64);
      return head.length > 0 && !/^\s*</.test(new TextDecoder().decode(head));
    } catch { return false; }
  };
  let model;   // 'divx' (the app's own model) when present, else 'div' (standard Tesseract), else ''
  const getModel = async () => model ?? (model = (await exists(base + 'lang/divx.traineddata')) ? 'divx'
    : (await exists(base + 'lang/div.traineddata')) ? 'div' : '');

  let tesseract;
  const loadTesseract = () => tesseract || (tesseract = new Promise((resolve, reject) => {
    const s = document.createElement('script');
    s.src = base + 'tesseract/tesseract.min.js';
    s.onload = () => resolve(window.Tesseract);
    s.onerror = () => reject(new Error('Could not load the reader (vendor/tesseract).'));
    document.head.appendChild(s);
  }));

  const N_WORKERS = Math.max(1, Math.min(8, (navigator.hardwareConcurrency || 2) - 1));   // one reader per core, leaving one free
  let scheduler = null, schedulerLangs = '';
  async function getScheduler(withEnglish) {
    const m = await getModel();
    if (!m) throw new Error('No Thaana reading model found (vendor/lang/div.traineddata).');
    const langs = withEnglish ? `${m}+eng` : m;
    if (scheduler && schedulerLangs === langs) return scheduler;
    if (scheduler) await scheduler.terminate();
    const T = await loadTesseract();
    scheduler = T.createScheduler();
    const workers = await Promise.all(Array.from({ length: N_WORKERS }, async () => {
      const w = await T.createWorker(langs, 1, {
        workerPath: base + 'tesseract/worker.min.js',
        corePath: base + 'tesseract-core/',
        langPath: base + 'lang',
        workerBlobURL: false,
        gzip: false,
        cacheMethod: 'none',
      });
      await w.setParameters({ tessedit_pageseg_mode: T.PSM.SINGLE_LINE, preserve_interword_spaces: '0' });   // normal spacing reads Thaana words better (test/ocr-accuracy.mjs)
      return w;
    }));
    workers.forEach((w) => scheduler.addWorker(w));
    schedulerLangs = langs;
    return scheduler;
  }

  // Symbols a Dhivehi page does not contain (degree, currency, copyright signs, bars, backticks...):
  // the Thaana model invents them in the wide gaps of justified text. Kept when the page has English.
  const JUNK = /[\u00A2\u00A3\u00A5-\u00A7\u00A9\u00AB\u00AC\u00AE\u00B0\u00BB\u00BF\u02C6\u201E\u2030\u2039\u203A\u20AC`|~^{}\\]/g;

  // Clean up common OCR noise in Thaana output.
  const tidy = (text, withEnglish) => (withEnglish ? text : text.replace(JUNK, ' '))
    .replace(/[\u200E\u200F\u202A-\u202E]/g, '')        // stray direction marks
    .replace(/[\u064B-\u0652]/g, '')                      // Arabic harakat the model sometimes invents
    .replace(/ {2,}/g, ' ')
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .replace(/([\u0780-\u07B1]) ?,/g, '$1\u060C')         // Latin comma after Thaana -> Thaana comma
    .replace(/([\u0780-\u07B1]) ?\?/g, '$1\u061F')        // Latin ? after Thaana -> Thaana question mark
    .trim();

  let onProgress = () => {};
  const b64ToBlob = (b64) => new Blob([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], { type: 'image/png' });

  window.ocr = {
    async recognizeLines(lines, withEnglish) {
      const sch = await getScheduler(withEnglish);
      let done = 0;
      return Promise.all(lines.map(async (b64) => {
        const { data } = await sch.addJob('recognize', b64ToBlob(b64));
        onProgress(++done / lines.length);
        return { text: tidy(data.text, withEnglish).replace(/\n+/g, ' '), confidence: data.confidence };
      }));
    },
    copy: (text) => invoke('copy_text', { text }),
    saveDocx: (opts) => invoke('save_docx', { opts }),
    saveTxt: (opts) => invoke('save_txt', { opts }),
    openFiles: () => invoke('open_files'),
    // Text of every page of a PDF, read in Rust (crates/thaana-core): [{ kind, text }, ...]
    readPdf: (bytes, opts) => invoke('pdf_read', bytes, { headers: { 'x-options': JSON.stringify(opts) } }),
    // Scan clean-up and line finding in Rust (crates/thaana-core/src/scan.rs), from the page's RGBA pixels
    prepareScan: (rgba, opts) => invoke('scan_prepare', new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), { headers: { 'x-options': JSON.stringify(opts) } }),
    prepareLabel: async (rgba, opts) => {
      const [lines, pitch] = await invoke('scan_prepare_label', new Uint8Array(rgba.buffer, rgba.byteOffset, rgba.byteLength), { headers: { 'x-options': JSON.stringify(opts) } });
      return { lines, pitch };
    },
    readWords: async () => { try { return await (await fetch(base + 'lang/dhivehi-words.txt')).text(); } catch { return ''; } },
    labelSave: (o) => invoke('label_save', { o }),
    labelUndo: (id) => invoke('label_undo', { id }),
    labelShow: () => invoke('label_show'),
    labelCount: () => invoke('label_count'),
    info: async () => ({ model: await getModel(), version: await invoke('app_version') }),
    onProgress: (cb) => { onProgress = cb; },
  };
})();
