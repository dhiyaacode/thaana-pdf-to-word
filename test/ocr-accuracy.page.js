// Runs inside the app window for test/ocr-accuracy.mjs (window.__BENCH is set by the runner):
// reads a PDF as pictures even though it has text, and reports the text and the time.
(async () => {
  const { invoke } = window.__TAURI__.core;
  const { file, columns } = window.__BENCH;
  const $ = (id) => document.getElementById(id);
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const status = () => $('status').textContent;
  let result;
  try {
    $('optPdfText').checked = false;          // do not use the PDF's own text
    $('optColumns').checked = columns;
    const bytes = Uint8Array.from(atob(await invoke('smoke_read', { path: file })), (c) => c.charCodeAt(0));
    const dt = new DataTransfer(); dt.items.add(new File([bytes], file.split('/').pop()));
    window.dispatchEvent(new DragEvent('drop', { dataTransfer: dt, cancelable: true }));
    while (!/page\(s\) ready|Couldn't/.test(status())) await sleep(100);
    await sleep(500);
    const pages = document.querySelectorAll('#pages .page').length;
    const t0 = performance.now();
    $('btnRead').click();
    while (!/^Done|went wrong/.test(status())) await sleep(50);
    result = { secs: (performance.now() - t0) / 1000, pages, prep: document.body.dataset.scanPrep || 'js', status: status(), text: $('text').value };
  } catch (e) {
    result = { secs: 0, pages: 0, prep: '', status: 'error: ' + (e && e.message || e), text: '' };
  }
  await invoke('smoke_log', { line: 'BENCH ' + JSON.stringify(result) });
  invoke('smoke_exit', { code: 0 });
})();
