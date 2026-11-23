// Reads every glyph of a PDF page with its position (from pdf.js operator list),
// then rebuilds Thaana text in the right order. Pure JS, no native parts.
const FILI = (c) => c >= 'ަ' && c <= 'ް';
const IGN = new Set('‎‏‪‫‬‭‮⁦⁧⁨⁩'.split(''));
const mul = (a, b) => [a[0]*b[0]+a[1]*b[2], a[0]*b[1]+a[1]*b[3], a[2]*b[0]+a[3]*b[2], a[2]*b[1]+a[3]*b[3], a[4]*b[0]+a[5]*b[2]+b[4], a[4]*b[1]+a[5]*b[3]+b[5]];

// some PDFs wrap the matrix in an extra array
const mat = (a) => Array.from(a.length === 1 && a[0] && typeof a[0] === 'object' ? a[0] : a);

export async function pageGlyphs(pdfjs, page) {
  const O = pdfjs.OPS, ol = await page.getOperatorList();
  let ctm = [1,0,0,1,0,0], stack = [], tm = [1,0,0,1,0,0], tlm = [1,0,0,1,0,0];
  let fs = 1, fm = [0.001,0,0,0.001,0,0], cs = 0, ws = 0, hs = 1, lead = 0, rise = 0;
  const out = [];
  const show = (glyphs) => {
    for (const g of glyphs) {
      if (typeof g === 'number') { tm = mul([1,0,0,1,-g/1000*fs*hs,0], tm); continue; }
      if (!g) continue;
      const w = (g.width || 0) * fm[0] * fs;
      const t = mul(tm, ctm);
      const c = g.unicode;
      if (c) out.push({ c, x0: t[4], x1: t[4] + w * Math.abs(t[0]), y: t[5], sz: Math.abs(fs * t[3]) || Math.abs(fs) });
      const adv = (w + cs + (g.isSpace ? ws : 0)) * hs;
      tm = mul([1,0,0,1,adv,0], tm);
    }
  };
  for (let i = 0; i < ol.fnArray.length; i++) {
    const f = ol.fnArray[i]; let a = ol.argsArray[i];
    switch (f) {
      case O.save: stack.push([ctm, fs, fm, cs, ws, hs, lead, rise]); break;
      case O.restore: if (stack.length) [ctm, fs, fm, cs, ws, hs, lead, rise] = stack.pop(); break;
      case O.transform: ctm = mul(mat(a), ctm); break;
      case O.beginText: tm = [1,0,0,1,0,0]; tlm = tm; break;
      case O.setFont: fs = a[1]; break;
      case O.setCharSpacing: cs = a[0]; break;
      case O.setWordSpacing: ws = a[0]; break;
      case O.setHScale: hs = a[0] / 100; break;
      case O.setLeading: lead = a[0]; break;
      case O.setTextRise: rise = a[0]; break;
      case O.setTextMatrix: tm = mat(a); tlm = tm; break;
      case O.moveText: tlm = mul([1,0,0,1,a[0],a[1]], tlm); tm = tlm; break;
      case O.setLeadingMoveText: lead = -a[1]; tlm = mul([1,0,0,1,a[0],a[1]], tlm); tm = tlm; break;
      case O.nextLine: tlm = mul([1,0,0,1,0,-lead], tlm); tm = tlm; break;
      case O.showText: show(a[0]); break;
      case O.showSpacedText: show(a[0]); break;
      case O.nextLineShowText: tlm = mul([1,0,0,1,0,-lead], tlm); tm = tlm; show(a[0]); break;
      case O.nextLineSetSpacingShowText: ws = a[0]; cs = a[1]; tlm = mul([1,0,0,1,0,-lead], tlm); tm = tlm; show(a[2]); break;
    }
  }
  return out.reverse();
}

// A Thaana letter carries at most one vowel sign. Extra ones are stray pieces of a neighbour
// (usually an abafili ަ next to ް or ި), so keep the sign that is not abafili.
function cleanFili(f) {
  const u = [...new Set(f)];
  if (u.length > 1 && u.includes('\u07A6')) { const r = u.filter((c) => c !== '\u07A6'); return r.join(''); }
  return u.join('');
}

const ARABIC = /[\u0600-\u06FF\u0750-\u077F\u08A0-\u08FF\uFB50-\uFDFF\uFE70-\uFEFF]/;

export function rebuild(raw, opts = {}) {
  const items = raw.filter(g => !IGN.has(g.c) && !/[\uE000-\uF8FF]/.test(g.c) && !(opts.dropArabic && ARABIC.test(g.c))).flatMap(g => [...g.c].map((ch, k) => ({ ...g, c: ch, f: [], x1: k > 0 && FILI(ch) ? g.x0 : g.x1 })));
  // Some PDFs store the word space as a vowel-sign glyph (a real vowel sign has no width).
  for (const it of items) if (FILI(it.c) && it.x1 - it.x0 > 0.12 * it.sz) it.c = ' ';
  // Fake-bold: some generators draw every glyph 3-4 times with tiny offsets. Keep one copy.
  { const seen = new Map(); const keep = [];
    for (const it of items) {
      const q = Math.max(1, it.sz * 0.3), kx = Math.round(it.x0 / q), ky = Math.round(it.y / q);
      let dup = false;
      for (let a = -1; a <= 1 && !dup; a++) for (let b = -1; b <= 1 && !dup; b++) {
        const l = seen.get(it.c + '|' + (kx + a) + '|' + (ky + b));
        if (l && l.some(o => Math.abs(o.x0 - it.x0) < q && Math.abs(o.y - it.y) < q)) dup = true;
      }
      if (dup) continue;
      const k = it.c + '|' + kx + '|' + ky; (seen.get(k) || seen.set(k, []).get(k)).push(it); keep.push(it);
    }
    items.length = 0; items.push(...keep); }
  const bases = items.filter(i => !FILI(i.c));
  if (!bases.length) return [];
  const med = bases.map(b => b.sz).sort((a, b) => a - b)[bases.length >> 1];
  let last = null; const orphans = [];
  for (const it of items) {
    if (FILI(it.c)) {
      if (last && Math.abs(it.x0 - last.x0) <= 0.6 && Math.abs(it.y - last.y) < med) last.f.push(it.c);
      else orphans.push([it, last]);
    } else last = it;
  }
  for (const [f, prev] of orphans) {
    let tgt = null, bd = 1e9;
    for (const b of bases) {
      if (b.c === ' ' || Math.abs(b.y - f.y) >= med * 0.8 || Math.abs(b.x0 - f.x0) > 1.2) continue;
      const d = Math.abs(b.x0 - f.x0); if (d < bd) { bd = d; tgt = b; }
    }
    (tgt || prev)?.f.push(f.c);
  }
  // lines: sort by y (PDF y grows upward => descending), group
  const idx = new Map(bases.map((b, i) => [b, i]));
  const sorted = [...bases].sort((a, b) => b.y - a.y);
  const lines = [];
  for (const b of sorted) {
    const L = lines[lines.length - 1];
    if (L && Math.abs(b.y - L[L.length - 1].y) < med * 0.7) L.push(b); else lines.push([b]);
  }
  const isL = (c) => /[0-9A-Za-z]/.test(c), isN = (c) => /[\s.,:;\/\-+%]/.test(c);
  const logical = (L) => {
    const a = [...L].sort((p, q) => ((q.x0 + q.x1) / 2 - (p.x0 + p.x1) / 2));   // visual right-to-left
    const out = []; let i = 0;
    while (i < a.length) {
      if (isL(a[i].c)) {                                  // LTR run (digits / Latin): keep left-to-right
        let j = i, end = i;
        while (j < a.length && (isL(a[j].c) || (isN(a[j].c) && j + 1 < a.length && (isL(a[j + 1].c) || isN(a[j + 1].c))))) { if (isL(a[j].c)) end = j; j++; }
        out.push(...a.slice(i, end + 1).reverse()); i = end + 1;
      } else out.push(a[i++]);
    }
    return out;
  };
  // Some PDFs have no space glyphs at all (words are just spread apart): then put spaces back by the gaps.
  const nSpace = bases.filter(b => b.c === ' ').length;
  const noSpaces = nSpace < bases.length * 0.06;
  const joinLine = (L) => {
    const a = logical(L); let out = '';
    for (let i = 0; i < a.length; i++) {
      const b = a[i];
      if (noSpaces && i > 0 && b.c !== ' ' && a[i - 1].c !== ' ' && !isL(b.c) && !isL(a[i - 1].c)) {
        const gap = a[i - 1].x0 - b.x1;                       // right-to-left: previous letter is on the right
        if (gap > 0.17 * med) out += ' ';
      }
      out += b.c + cleanFili(b.f);
    }
    out = out.replace(/ +/g, ' ').trim();
    // Wingdings bullets come out as a lone Latin 'z' next to Thaana text
    if (/[\u0780-\u07B1]/.test(out)) out = out.replace(/(?<![A-Za-z0-9])z(?![A-Za-z0-9])/g, '').replace(/ +/g, ' ').trim();
    // brackets stored mirrored: if the first one met is a closing bracket, swap them all in this line
    const first = out.match(/[()\[\]]/);
    if (first && (first[0] === ')' || first[0] === ']')) out = out.replace(/[()\[\]]/g, (c) => ({ '(': ')', ')': '(', '[': ']', ']': '[' }[c]));
    return out;
  };
  // Table cells: a very wide gap inside a line separates cells (kept apart with a tab).
  const cellGap = 1.8 * med;
  const splitCells = (L) => {
    const a = [...L].sort((p, q) => q.x1 - p.x1); const parts = [[a[0]]];
    for (let i = 1; i < a.length; i++) {
      const gap = parts[parts.length - 1].reduce((m, g) => Math.min(m, g.x0), 1e9) - a[i].x1;
      if (gap > cellGap) parts.push([a[i]]); else parts[parts.length - 1].push(a[i]);
    }
    return parts;
  };
  return lines.map(L => splitCells(L).map(joinLine).filter(Boolean).join('\t').replace(/^(\d+[.)]|[-•–])\t/, '$1 '));   // list numbers are not table cells
}
