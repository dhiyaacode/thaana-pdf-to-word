// Conservative repair of Thaana words from PDFs: fixes misplaced / stray vowel signs (fili)
// only when the result is a known word. Unknown words are left exactly as they are.
const FILI_RE = /[ަ-ް]/g;
const strip = (w) => w.replace(FILI_RE, '');
const counts = (w) => { const m = {}; for (const c of w.match(FILI_RE) || []) m[c] = (m[c] || 0) + 1; return m; };

export class Repairer {
  constructor(wordList) {
    this.known = new Set(); this.skel = new Map();
    for (const w of wordList) {
      this.known.add(w);
      const k = strip(w); if (!k) continue;
      if (!this.skel.has(k)) this.skel.set(k, []);
      this.skel.get(k).push(w);          // list is ordered by frequency
    }
  }
  fixWord(w) {
    w = w.replace(/^[ަ-ް]+/, '');            // vowel sign with no letter before it
    if (!w || this.known.has(w)) return w;
    const cands = this.skel.get(strip(w)); if (!cands) return w;
    const a = counts(w);
    for (const x of cands) {
      const b = counts(x); let miss = 0, extra = 0;
      for (const k in b) miss += Math.max(0, b[k] - (a[k] || 0));
      for (const k in a) extra += Math.max(0, a[k] - (b[k] || 0));
      if ((miss === 0 || extra === 0) && miss + extra <= 2) return x;
    }
    return w;
  }
  // join a word split by a stray space ("ކަ ންތައް"); tidy spacing around punctuation
  fixText(t) {
    const TW = /^[ހ-ޱ]+$/;
    return t.split('\n').map((line) => {
      const parts = line.split(/(\s+)/); const out = [];
      for (let i = 0; i < parts.length; i++) {
        const p = parts[i];
        if (TW.test(p) && i + 2 < parts.length && parts[i + 1] === ' ' && TW.test(parts[i + 2])) {
          const a = this.fixWord(p), b = this.fixWord(parts[i + 2]);
          if (!(this.known.has(a) && this.known.has(b)) && this.known.has(a + b)) { out.push(a + b); i += 2; continue; }
        }
        out.push(TW.test(p) ? this.fixWord(p) : p);
      }
      return out.join('').replace(/\s+([،؟!:؛])/g, '$1').replace(/([،؟؛])(?=[^\s\d])/g, '$1 ');
    }).join('\n');
  }
}
