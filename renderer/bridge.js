// The bridge between the interface and the Rust side of the app: it defines `window.ocr`.
// Everything runs in Rust (src-tauri/src/main.rs, crates/thaana-core): opening files and drawing
// pages, the PDF text reader, scan clean-up, finishing the text, dialogs, clipboard, Word export
// and labels. The interface refers to pages by number. Only recognising the text lines of scans
// (OCR) runs here in the web view, with Tesseract and the models in vendor/lang.
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

  let onProgress = () => {};
  const b64ToBlob = (b64) => new Blob([Uint8Array.from(atob(b64), (c) => c.charCodeAt(0))], { type: 'image/png' });

  // A dropped or pasted file goes to Rust as the raw request body. Its name can contain Thaana,
  // which a request header cannot carry as it is, so the header is percent-encoded.
  const header = (o) => ({ headers: { 'x-options': encodeURIComponent(JSON.stringify(o)) } });

  window.ocr = {
    // Tesseract's raw text for each line image (base64 PNG); Rust tidies it in finishScan.
    async recognizeLines(lines, withEnglish) {
      const sch = await getScheduler(withEnglish);
      let done = 0;
      return Promise.all(lines.map(async (b64) => {
        const { data } = await sch.addJob('recognize', b64ToBlob(b64));
        onProgress(++done / lines.length);
        return data.text;
      }));
    },
    // Files: { name, pages: [{ id, kind: 'text'|'damaged'|'scan'|'blank', text, thumb, readable }], error }
    openFiles: (opts) => invoke('open_files', { opts }),
    openDocument: (bytes, name, opts) => invoke('open_document', bytes, header({ name, ...opts })),
    forgetPages: (ids) => invoke('pages_forget', { ids }),
    // A page cut into text lines for the reader: { lines, boxes, pitch, skew, blk }
    prepareScan: (id, { rotation, enhance, columns }) => invoke('scan_prepare', { id, rotation, enhance, columns }),
    // The finished text of a page from what the reader recognised on each line
    finishScan: (f) => invoke('ocr_finish', { f }),
    prepareLabel: async (id, rotation) => {
      const [lines, pitch] = await invoke('scan_prepare_label', { id, rotation });
      return { lines, pitch };
    },
    copy: (text) => invoke('copy_text', { text }),
    saveDocx: (opts) => invoke('save_docx', { opts }),
    saveTxt: (opts) => invoke('save_txt', { opts }),
    labelSave: (o) => invoke('label_save', { o }),
    labelUndo: (id) => invoke('label_undo', { id }),
    labelShow: () => invoke('label_show'),
    labelCount: () => invoke('label_count'),
    info: async () => ({ model: await getModel(), version: await invoke('app_version') }),
    onProgress: (cb) => { onProgress = cb; },
  };
})();
