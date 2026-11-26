// The interface: shows the pages and the text, and runs the steps in order. The work is done in
// Rust (opening files, drawing pages, reading PDF text, preparing scans, finishing their text) and
// by the reader in bridge.js; see window.ocr there. Pages are referred to by their number.

const $ = (id) => document.getElementById(id);
const pagesEl = $('pages'), textEl = $('text'), statusEl = $('status'), bar = $('bar');
const btn = { open: $('btnOpen'), read: $('btnRead'), copy: $('btnCopy'), docx: $('btnDocx'), txt: $('btnTxt'), clear: $('btnClear'), label: $('btnLabel') };

let pages = [];          // { id, name, kind, text, rotation, readable, fromPdf, el }
let busy = false;
let baseName = 'thaana-text';

function setStatus(msg, warn = false) { statusEl.textContent = msg; statusEl.classList.toggle('warn', warn); }
function refreshButtons() {
  const hasText = textEl.value.trim().length > 0;
  btn.read.disabled = busy || pages.length === 0;
  btn.open.disabled = busy;
  btn.label.disabled = busy || !pages.some((p) => !p.fromPdf && p.readable);
  btn.copy.disabled = btn.docx.disabled = btn.txt.disabled = busy || !hasText;
  $('emptyHint').style.display = pages.length ? 'none' : '';
  const words = textEl.value.trim() ? textEl.value.trim().split(/\s+/).length : 0;
  $('counts').textContent = words ? `${words} words` : '';
}
function refreshPdfText() {
  const t = pages.map((p) => p.text).filter(Boolean).join('\n\n');
  if (t) textEl.value = t;
}
const plural = (n, w) => `${n} ${w}${n > 1 ? 's' : ''}`;
const openOptions = () => ({ dropArabic: $('optNoArabic').checked, join: $('optJoin').checked, useText: $('optPdfText').checked });

/* ---------- pages ---------- */
// Adds the pages of one opened file. Returns true when the status line already says what to do.
function addFile({ name, pages: list, error }) {
  if (error) { setStatus(`Couldn't open ${name}: ${error}`, true); return true; }
  baseName = name.replace(/\.[^.]+$/, '');
  if (!/\.pdf$/i.test(name)) { list.forEach((p) => addPage(name, p)); return false; }
  const count = (k) => list.filter((p) => p.kind === k).length;
  const fromText = count('text'), inc = count('damaged'), scans = count('scan'), blank = count('blank');
  // A PDF with no Thaana text at all (e.g. made with an old non-Unicode font) is read as pictures.
  list.forEach((p, i) => addPage(`${name} · p${i + 1}`, p, fromText === 0 && p.kind === 'blank'));
  refreshPdfText();
  if (fromText) {
    setStatus(`${name}: text taken straight from the PDF (${plural(fromText, 'page')}${blank ? `, ${blank} blank` : ''}) — no reading needed. Copy or save as Word.`
      + (inc ? ` ${plural(inc, 'page')} had damaged text inside the PDF — click “Read text” to read ${inc > 1 ? 'them' : 'it'} as a picture.` : '')
      + (scans ? ` ${plural(scans, 'page')} ${scans > 1 ? 'are scans' : 'is a scan'} — click “Read text” to read ${scans > 1 ? 'them' : 'it'}.` : ''));
    return true;
  }
  if (inc) {
    setStatus(`${name}: the text inside this PDF is damaged (missing vowel signs), so it will be read as a picture. Click “Read text”.`, true);
    return true;
  }
  return false;
}

function addPage(name, info, readAnyway = false) {
  const page = { id: info.id, name, kind: info.kind, text: info.kind === 'text' ? info.text : '', rotation: 0, readable: info.readable };
  // Pages that need no reading: typed text, and blank pages (unless the whole PDF had no text).
  page.fromPdf = info.kind === 'text' || (info.kind === 'blank' && !readAnyway);
  const el = document.createElement('div');
  el.className = page.fromPdf ? 'page done' : 'page';
  el.innerHTML = `<img alt=""><div class="row"><span class="name"></span><span class="tools">
    <button data-a="rot" title="Rotate 90°">⟳</button><button data-a="del" title="Remove">✕</button></span></div>`;
  el.querySelector('.name').textContent = name;
  const img = el.querySelector('img');
  if (info.thumb) img.src = info.thumb; else img.alt = 'This page cannot be shown';
  el.querySelector('[data-a=rot]').onclick = () => {
    page.rotation = (page.rotation + 90) % 360;
    img.style.transform = page.rotation ? `rotate(${page.rotation}deg)${page.rotation % 180 ? ' scale(0.75)' : ''}` : '';
  };
  el.querySelector('[data-a=del]').onclick = () => {
    pages = pages.filter((p) => p !== page); el.remove(); window.ocr.forgetPages([page.id]); refreshButtons();
  };
  page.el = el; pagesEl.appendChild(el); pages.push(page);
  refreshButtons();
  return page;
}

async function openDropped(file) {
  setStatus(`Opening ${file.name}…`);
  return addFile(await window.ocr.openDocument(new Uint8Array(await file.arrayBuffer()), file.name, openOptions()));
}

/* ---------- reading scans ---------- */
let pageIndex = 0, pageCount = 1;
window.ocr.onProgress((p) => { if (busy) bar.style.width = `${((pageIndex + p) / pageCount) * 100}%`; });

async function readAll() {
  if (!pages.length) return;
  busy = true; refreshButtons();
  const english = $('optEnglish').checked, join = $('optJoin').checked;
  const prepOptions = { enhance: $('optEnhance').checked, columns: !!($('optColumns') && $('optColumns').checked) };
  const prepare = (p) => window.ocr.prepareScan(p.id, { rotation: p.rotation, ...prepOptions });
  pageCount = pages.length;
  const out = [];
  const t0 = performance.now();
  // The next page is prepared in Rust while the reader works on this one.
  const ahead = new Map();
  const prepareAhead = (from) => {
    const i = pages.findIndex((p, k) => k > from && !p.fromPdf && p.readable);
    if (i < 0 || ahead.has(i)) return;
    const job = prepare(pages[i]); job.catch(() => {});   // its error is reported when it is used
    ahead.set(i, job);
  };
  try {
    for (pageIndex = 0; pageIndex < pages.length; pageIndex++) {
      const page = pages[pageIndex];
      if (page.fromPdf) { out.push(page.text); textEl.value = out.filter(Boolean).join('\n\n'); continue; }
      if (!page.readable) { page.el.className = 'page'; out.push(''); continue; }
      page.el.className = 'page working';
      setStatus(`Finding text lines on page ${pageIndex + 1} of ${pages.length}…`);
      const prep = await (ahead.get(pageIndex) || prepare(page));
      ahead.delete(pageIndex);
      prepareAhead(pageIndex);
      if (!prep.lines.length) { page.el.className = 'page'; out.push(''); continue; }
      setStatus(pageIndex === 0 && !window.__warm ? 'Starting the reader (first time takes a few seconds)…' : `Reading page ${pageIndex + 1} of ${pages.length} (${prep.lines.length} lines)…`);
      const recognised = await window.ocr.recognizeLines(prep.lines, english);
      window.__warm = true;
      page.text = await window.ocr.finishScan({ lines: recognised, boxes: prep.boxes, pitch: prep.pitch, blk: prep.blk ?? null, join, withEnglish: english });
      page.el.className = 'page done';
      out.push(page.text);
      textEl.value = out.filter(Boolean).join('\n\n');
    }
    const secs = ((performance.now() - t0) / 1000).toFixed(0);
    setStatus(`Done — ${pages.length} page${pages.length > 1 ? 's' : ''} in ${secs}s. Check the fili before saving.`);
  } catch (e) {
    console.error(e);
    setStatus('Something went wrong while reading: ' + (e.message || e), true);
  } finally {
    busy = false; bar.style.width = '0'; refreshButtons();
  }
}

/* ---------- buttons ---------- */
btn.open.onclick = async () => {
  setStatus('Opening…');
  const files = await window.ocr.openFiles(openOptions());
  let told = false;
  for (const f of files) told = addFile(f) || told;
  if (!files.length) setStatus('');
  else if (!told && !pages.every((p) => p.fromPdf)) setStatus(`${pages.length} page(s) ready. Click “Read text”.`);
  refreshButtons();
};
btn.read.onclick = readAll;
btn.copy.onclick = async () => { await window.ocr.copy(textEl.value); setStatus('Copied. Paste into Word with ⌘V / Ctrl+V.'); };
btn.docx.onclick = async () => {
  const p = await window.ocr.saveDocx({ text: textEl.value, fontName: $('optFont').value, fontSize: +$('optSize').value || 14, suggestedName: baseName });
  if (p) setStatus('Saved: ' + p);
};
btn.txt.onclick = async () => { const p = await window.ocr.saveTxt({ text: textEl.value, suggestedName: baseName }); if (p) setStatus('Saved: ' + p); };

/* ---------- handwriting labelling ---------- */
const L = { queue: [], i: 0, saved: 0, last: null };
async function startLabeling() {
  $('labeler').hidden = false; $('lprog').textContent = 'finding lines…';
  L.queue = [];
  for (const page of pages.filter((p) => !p.fromPdf && p.readable)) {
    try {
      const r = await window.ocr.prepareLabel(page.id, page.rotation);
      r.lines.forEach((l) => L.queue.push({ pngOrig: l.orig, show: 'data:image/png;base64,' + l.show, pitch: Math.round(r.pitch), page: page.name }));
    } catch (e) { console.error(e); }
  }
  L.i = 0; L.last = null; L.saved = await window.ocr.labelCount(); showLine();
}
function showLine() {
  const it = L.queue[L.i];
  if (!it) { $('lprog').textContent = `all lines done · ${L.saved} saved in total`; $('limg').removeAttribute('src'); $('ltext').value = ''; return; }
  $('limg').src = it.show; $('ltext').value = ''; $('ltext').focus();
  $('lprog').textContent = `line ${L.i + 1} of ${L.queue.length} · ${L.saved} saved in total`;
}
async function saveLine() {
  const it = L.queue[L.i]; const text = $('ltext').value.trim(); if (!it || !text) return;
  const id = Date.now().toString(36) + Math.random().toString(36).slice(2, 6);
  await window.ocr.labelSave({ id, png: it.pngOrig, text, meta: `pitch=${it.pitch};src=${it.page}` });
  L.last = id; L.saved++; L.i++; showLine();
}
btn.label.onclick = startLabeling;
$('lsave').onclick = saveLine;
$('ltext').addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); saveLine(); } });
$('lskip').onclick = () => { L.i++; showLine(); };
$('lundo').onclick = async () => { if (L.last) { await window.ocr.labelUndo(L.last); L.last = null; L.saved--; L.i = Math.max(0, L.i - 1); showLine(); } };
$('lfolder').onclick = () => window.ocr.labelShow();
$('lclose').onclick = () => { $('labeler').hidden = true; };

btn.clear.onclick = () => {
  window.ocr.forgetPages(pages.map((p) => p.id));
  pages = []; pagesEl.querySelectorAll('.page').forEach((e) => e.remove()); textEl.value = ''; setStatus('Cleared.'); refreshButtons();
};
textEl.addEventListener('input', refreshButtons);

/* ---------- drag & drop, paste ---------- */
const exts = ['.pdf', '.png', '.jpg', '.jpeg', '.tif', '.tiff', '.bmp', '.webp', '.gif'];
window.addEventListener('dragover', (e) => { e.preventDefault(); document.body.classList.add('dragging'); });
window.addEventListener('dragleave', (e) => { if (!e.relatedTarget) document.body.classList.remove('dragging'); });
window.addEventListener('drop', async (e) => {
  e.preventDefault(); document.body.classList.remove('dragging');
  if (busy) return;
  let told = false;
  for (const file of e.dataTransfer.files) {
    const ext = '.' + file.name.split('.').pop().toLowerCase();
    if (!exts.includes(ext)) { setStatus(`Skipped ${file.name} — not an image or PDF.`, true); told = true; continue; }
    told = (await openDropped(file)) || told;
  }
  if (!told && !pages.every((p) => p.fromPdf)) setStatus(`${pages.length} page(s) ready. Click “Read text”.`);
  refreshButtons();
});

// Paste a screenshot straight from the clipboard (⌘V / Ctrl+V) when the text box isn't focused.
window.addEventListener('paste', async (e) => {
  if (document.activeElement === textEl || busy) return;
  for (const item of e.clipboardData.items) {
    if (!item.type.startsWith('image/')) continue;
    const blob = item.getAsFile();
    const ext = item.type === 'image/jpeg' ? 'jpg' : item.type.split('/')[1] || 'png';
    addFile(await window.ocr.openDocument(new Uint8Array(await blob.arrayBuffer()), `Pasted image.${ext}`, openOptions()));
    setStatus('Image pasted. Click “Read text”.');
  }
});

window.ocr.info().then((i) => {
  document.title = `Thaana PDF to Word ${i.version}`;
  if (!i.model) setStatus('Reading scans is not available: the Thaana reading model (lang/div.traineddata) is missing. Typed PDFs still work.', true);
});
refreshButtons();
