// Runs inside the Tauri app window (see test/tauri-smoke.mjs). window.__SMOKE is set by the runner.
(async () => {
  const { invoke } = window.__TAURI__.core;
  const { fixtures, spec, extra } = window.__SMOKE;
  const log = (line) => invoke('smoke_log', { line });
  const results = [];
  const check = async (name, ok, info = '') => { results.push(!!ok); await log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : '  ' + String(info).slice(0, 400)}`); };
  const $ = (id) => document.getElementById(id);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const until = async (cond, ms = 180000) => { const t = Date.now(); while (Date.now() - t < ms) { if (await cond()) return true; await sleep(250); } return false; };
  const status = () => $('status').textContent, text = () => $('text').value;
  const drop = async (file) => {
    const name = file.split(/[\\/]/).pop();
    const bytes = Uint8Array.from(atob(await invoke('smoke_read', { path: file })), (c) => c.charCodeAt(0));
    const dt = new DataTransfer(); dt.items.add(new File([bytes], name));
    window.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, cancelable: true }));
    await until(() => !status().startsWith('Opening') && (status().includes(name) || status().includes('ready')));
  };
  const errors = [];
  window.addEventListener('error', (e) => errors.push(e.message));
  window.addEventListener('unhandledrejection', (e) => errors.push(String(e.reason)));

  try {
    const info = await window.ocr.info();
    await check('bridge: window.ocr from Rust, reading model found', info.model && info.version, JSON.stringify(info));

    await drop(fixtures + '/typed.pdf');
    let t = text();
    await check('typed.pdf: paragraph text with all vowel signs', t.includes(spec.paragraphs[0]), t.slice(0, 300));
    await check('typed.pdf: table rows kept as rows', spec.table.every((r) => t.split('\n').includes(r.join('\t'))), t);

    $('btnDocx').click();
    await check('Save as Word (Rust): saved', await until(() => status().startsWith('Saved'), 20000), status());

    $('btnClear').click();
    await drop(fixtures + '/mixed.pdf');
    await check('mixed.pdf: status says page 2 is a scan', /is a scan/.test(status()), status());
    $('btnRead').click();
    const done = await until(() => /^Done|went wrong/.test(status()), 300000);
    t = text();
    const ocr = t.split('\n\n').slice(1).join('\n');
    await check('Read text (Tesseract in the web view): finished', done && status().startsWith('Done'), status());
    await check('Read text: scanned page read into Thaana', (ocr.match(/[\u0780-\u07B1]/g) || []).length > 200, ocr.slice(0, 300));

    for (const f of extra) {
      $('btnClear').click();
      await drop(f);
      await check(`${f.split(/[\\/]/).pop()}: text taken from the PDF`, text().length > 100, status());
    }
    await check('no errors in the page', errors.length === 0, errors.join(' | '));
  } catch (e) {
    await check('smoke run', false, e && e.stack || e);
  }
  await log(`RESULT ${results.filter(Boolean).length}/${results.length}`);
  await invoke('smoke_exit', { code: results.every(Boolean) ? 0 : 1 });
})();
