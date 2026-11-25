//! Reads every glyph of a PDF page with its position, plus border lines and pictures.
//! It produces the same data the JavaScript reader gets from pdf.js (`pageGlyphs()` in
//! renderer/pdfglyphs.js): character(s), origin, advance width, baseline and size.

use crate::layout::{Glyph, HRule, PageData, VRule};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use std::collections::HashMap;
use std::rc::Rc;

type M = [f64; 6];
const ID: M = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn mul(a: &M, b: &M) -> M {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}
fn apply(m: &M, x: f64, y: f64) -> (f64, f64) {
    (x * m[0] + y * m[2] + m[4], x * m[1] + y * m[3] + m[5])
}
fn translate(x: f64, y: f64) -> M {
    [1.0, 0.0, 0.0, 1.0, x, y]
}

fn num(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(i) => Some(*i as f64),
        // lopdf keeps reals as f32. Going back through the shortest decimal that gives the same f32
        // recovers the number as written in the PDF (e.g. 539.38), which is what pdf.js uses:
        // `r as f64` would be 539.380005 and can tip results that sit right on a threshold.
        Object::Real(r) => format!("{r}").parse().ok(),
        _ => None,
    }
}

struct Pdf<'a> {
    doc: &'a Document,
    fonts: HashMap<ObjectId, Rc<Font>>,
}

impl<'a> Pdf<'a> {
    fn deref(&self, o: &'a Object) -> &'a Object {
        self.doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
    }
    fn dict(&self, o: &'a Object) -> Option<&'a Dictionary> {
        match self.deref(o) {
            Object::Dictionary(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }
    fn get(&self, d: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
        d.get(key).ok().map(|o| self.deref(o))
    }
    fn get_num(&self, d: &'a Dictionary, key: &[u8]) -> Option<f64> {
        self.get(d, key).and_then(num)
    }
    fn get_array(&self, d: &'a Dictionary, key: &[u8]) -> Option<&'a Vec<Object>> {
        match self.get(d, key) {
            Some(Object::Array(a)) => Some(a),
            _ => None,
        }
    }
    fn stream_data(&self, o: &Object) -> Option<Vec<u8>> {
        match self.deref(o) {
            Object::Stream(s) => Some(if s.dict.has(b"Filter") { s.decompressed_content().ok()? } else { s.content.clone() }),
            _ => None,
        }
    }
    fn matrix(&self, o: Option<&Object>) -> Option<M> {
        let a = match o.map(|o| self.deref(o)) {
            Some(Object::Array(a)) if a.len() == 6 => a,
            _ => return None,
        };
        let v: Vec<f64> = a.iter().filter_map(|x| num(self.deref(x))).collect();
        (v.len() == 6).then(|| [v[0], v[1], v[2], v[3], v[4], v[5]])
    }
}

// ---------------------------------------------------------------- fonts
struct Font {
    two_byte: bool,
    codespace: Vec<(usize, u32, u32)>, // (bytes, low, high) from an embedded encoding CMap
    to_unicode: HashMap<u32, String>,
    encoding: Option<Vec<Option<char>>>, // simple fonts: code -> character
    widths: HashMap<u32, f64>,
    default_width: f64,
    matrix: M,
}

impl Font {
    /// Splits a string into (code, byte length, unicode, width).
    fn glyphs(&self, bytes: &[u8]) -> Vec<(u32, usize, Option<String>, f64)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let n = if !self.two_byte {
                1
            } else if self.codespace.is_empty() {
                2
            } else {
                self.codespace_len(&bytes[i..])
            };
            let n = n.min(bytes.len() - i).max(1);
            let code = bytes[i..i + n].iter().fold(0u32, |a, &b| (a << 8) | b as u32);
            let uni = self.to_unicode.get(&code).cloned().or_else(|| {
                self.encoding.as_ref().and_then(|e| e.get(code as usize).copied().flatten()).map(|c| c.to_string())
            });
            out.push((code, n, uni, self.widths.get(&code).copied().unwrap_or(self.default_width)));
            i += n;
        }
        out
    }
    fn codespace_len(&self, b: &[u8]) -> usize {
        for n in 1..=4.min(b.len()) {
            let code = b[..n].iter().fold(0u32, |a, &x| (a << 8) | x as u32);
            if self.codespace.iter().any(|&(len, lo, hi)| len == n && code >= lo && code <= hi) {
                return n;
            }
        }
        2
    }
}

fn hex_bytes(s: &str) -> Vec<u8> {
    let h: Vec<u8> = s.bytes().filter(|b| b.is_ascii_hexdigit()).collect();
    h.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("0"), 16).unwrap_or(0) << if p.len() == 1 { 4 } else { 0 }).collect()
}
fn utf16be(b: &[u8]) -> String {
    let units: Vec<u16> = b.chunks(2).map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 }).collect();
    String::from_utf16_lossy(&units)
}
fn be(b: &[u8]) -> u32 {
    b.iter().fold(0u32, |a, &x| (a << 8) | x as u32)
}

/// Tokens of a CMap: <hex>, [ ... ] arrays and plain words.
fn cmap_tokens(s: &str) -> Vec<String> {
    let mut t = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'<' => {
                let j = s[i..].find('>').map(|k| i + k).unwrap_or(b.len() - 1);
                t.push(s[i..=j].to_string());
                i = j + 1;
            }
            b'[' | b']' => {
                t.push((b[i] as char).to_string());
                i += 1;
            }
            b'%' => {
                while i < b.len() && b[i] != b'\n' && b[i] != b'\r' {
                    i += 1;
                }
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let j = (i..b.len()).find(|&k| b[k].is_ascii_whitespace() || matches!(b[k], b'<' | b'[' | b']')).unwrap_or(b.len());
                t.push(s[i..j].to_string());
                i = j.max(i + 1);
            }
        }
    }
    t
}

/// Parses a ToUnicode CMap (bfchar / bfrange) and its codespace ranges.
fn parse_cmap(data: &[u8]) -> (HashMap<u32, String>, Vec<(usize, u32, u32)>) {
    let s = String::from_utf8_lossy(data);
    let t = cmap_tokens(&s);
    let (mut map, mut space) = (HashMap::new(), Vec::new());
    let is_hex = |x: &str| x.starts_with('<');
    let mut i = 0;
    while i < t.len() {
        match t[i].as_str() {
            "begincodespacerange" => {
                i += 1;
                while i + 1 < t.len() && is_hex(&t[i]) {
                    let (lo, hi) = (hex_bytes(&t[i]), hex_bytes(&t[i + 1]));
                    space.push((lo.len(), be(&lo), be(&hi)));
                    i += 2;
                }
            }
            "beginbfchar" => {
                i += 1;
                while i + 1 < t.len() && is_hex(&t[i]) {
                    map.insert(be(&hex_bytes(&t[i])), utf16be(&hex_bytes(&t[i + 1])));
                    i += 2;
                }
            }
            "beginbfrange" => {
                i += 1;
                while i + 2 < t.len() && is_hex(&t[i]) {
                    let (lo, hi) = (be(&hex_bytes(&t[i])), be(&hex_bytes(&t[i + 1])));
                    if t[i + 2] == "[" {
                        let mut k = i + 3;
                        let mut code = lo;
                        while k < t.len() && t[k] != "]" {
                            if code <= hi {
                                map.insert(code, utf16be(&hex_bytes(&t[k])));
                            }
                            code += 1;
                            k += 1;
                        }
                        i = k + 1;
                    } else {
                        let dst = hex_bytes(&t[i + 2]);
                        if hi >= lo && hi - lo < 65536 {
                            for (n, code) in (lo..=hi).enumerate() {
                                // the last UTF-16 unit counts up across the range
                                let mut units: Vec<u16> = dst.chunks(2).map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 }).collect();
                                if let Some(last) = units.last_mut() {
                                    *last = last.wrapping_add(n as u16);
                                }
                                map.insert(code, String::from_utf16_lossy(&units));
                            }
                        }
                        i += 3;
                    }
                }
            }
            _ => i += 1,
        }
    }
    (map, space)
}

/// Windows-1252 (WinAnsiEncoding); used as the base for simple fonts.
fn win_ansi(code: u8) -> Option<char> {
    const HI: [u16; 32] = [
        0x20AC, 0, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0, 0x017D, 0, 0, 0x2018, 0x2019, 0x201C,
        0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0, 0x017E, 0x0178,
    ];
    match code {
        0x20..=0x7E | 0xA0..=0xFF => Some(code as char),
        0x80..=0x9F => char::from_u32(HI[(code - 0x80) as usize] as u32).filter(|&c| c != '\0'),
        _ => None,
    }
}

/// Unicode for a glyph name in an /Encoding /Differences array.
fn glyph_name(name: &[u8]) -> Option<char> {
    let n = std::str::from_utf8(name).ok()?;
    let hex = |s: &str| u32::from_str_radix(s, 16).ok().and_then(char::from_u32);
    if let Some(h) = n.strip_prefix("uni") {
        return h.get(..4).and_then(hex);
    }
    if let Some(h) = n.strip_prefix('u') {
        if (4..=6).contains(&h.len()) {
            if let Some(c) = hex(h) {
                return Some(c);
            }
        }
    }
    if n.len() == 1 && n.as_bytes()[0].is_ascii_alphabetic() {
        return n.chars().next();
    }
    const NAMES: &[(&str, char)] = &[
        ("space", ' '), ("exclam", '!'), ("quotedbl", '"'), ("numbersign", '#'), ("dollar", '$'), ("percent", '%'), ("ampersand", '&'),
        ("quotesingle", '\''), ("parenleft", '('), ("parenright", ')'), ("asterisk", '*'), ("plus", '+'), ("comma", ','), ("hyphen", '-'),
        ("period", '.'), ("slash", '/'), ("zero", '0'), ("one", '1'), ("two", '2'), ("three", '3'), ("four", '4'), ("five", '5'), ("six", '6'),
        ("seven", '7'), ("eight", '8'), ("nine", '9'), ("colon", ':'), ("semicolon", ';'), ("less", '<'), ("equal", '='), ("greater", '>'),
        ("question", '?'), ("at", '@'), ("bracketleft", '['), ("backslash", '\\'), ("bracketright", ']'), ("asciicircum", '^'),
        ("underscore", '_'), ("grave", '`'), ("braceleft", '{'), ("bar", '|'), ("braceright", '}'), ("asciitilde", '~'), ("bullet", '\u{2022}'),
        ("endash", '\u{2013}'), ("emdash", '\u{2014}'), ("quoteleft", '\u{2018}'), ("quoteright", '\u{2019}'), ("quotedblleft", '\u{201C}'),
        ("quotedblright", '\u{201D}'), ("ellipsis", '\u{2026}'), ("degree", '\u{B0}'), ("multiply", '\u{D7}'), ("divide", '\u{F7}'),
        ("nbspace", '\u{A0}'), ("minus", '\u{2212}'),
    ];
    NAMES.iter().find(|(k, _)| *k == n).map(|&(_, c)| c)
}

impl<'a> Pdf<'a> {
    fn font(&mut self, obj: &'a Object) -> Rc<Font> {
        let id = match obj {
            Object::Reference(id) => Some(*id),
            _ => None,
        };
        if let Some(f) = id.and_then(|id| self.fonts.get(&id)) {
            return f.clone();
        }
        let f = Rc::new(self.load_font(obj));
        if let Some(id) = id {
            self.fonts.insert(id, f.clone());
        }
        f
    }

    fn load_font(&self, obj: &'a Object) -> Font {
        let empty = Dictionary::new();
        let d = self.dict(obj).unwrap_or(&empty);
        let subtype = self.get(d, b"Subtype").and_then(|o| o.as_name().ok()).unwrap_or(b"");
        let (to_unicode, _) = self.get(d, b"ToUnicode").and_then(|o| self.stream_data(o)).map(|s| parse_cmap(&s)).unwrap_or_default();
        let mut f = Font {
            two_byte: subtype == b"Type0",
            codespace: vec![],
            to_unicode,
            encoding: None,
            widths: HashMap::new(),
            default_width: 0.0,
            matrix: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0],
        };
        if f.two_byte {
            // embedded encoding CMap: its codespace says how many bytes each code takes
            if let Some(data) = self.get(d, b"Encoding").and_then(|o| self.stream_data(o)) {
                f.codespace = parse_cmap(&data).1;
            }
            let desc = self.get_array(d, b"DescendantFonts").and_then(|a| a.first()).and_then(|o| self.dict(o));
            if let Some(desc) = desc {
                f.default_width = self.get_num(desc, b"DW").unwrap_or(1000.0);
                if let Some(w) = self.get_array(desc, b"W") {
                    let mut i = 0;
                    while i < w.len() {
                        let Some(first) = num(self.deref(&w[i])) else { break };
                        match w.get(i + 1).map(|o| self.deref(o)) {
                            Some(Object::Array(list)) => {
                                for (k, v) in list.iter().enumerate() {
                                    if let Some(v) = num(self.deref(v)) {
                                        f.widths.insert(first as u32 + k as u32, v);
                                    }
                                }
                                i += 2;
                            }
                            Some(o) => {
                                let (last, v) = (num(o), w.get(i + 2).and_then(|x| num(self.deref(x))));
                                if let (Some(last), Some(v)) = (last, v) {
                                    for c in first as u32..=(last as u32).min(first as u32 + 65535) {
                                        f.widths.insert(c, v);
                                    }
                                }
                                i += 3;
                            }
                            None => break,
                        }
                    }
                }
            }
        } else {
            if subtype == b"Type3" {
                if let Some(m) = self.matrix(d.get(b"FontMatrix").ok()) {
                    f.matrix = m;
                }
            }
            let first = self.get_num(d, b"FirstChar").unwrap_or(0.0) as u32;
            if let Some(ws) = self.get_array(d, b"Widths") {
                for (k, v) in ws.iter().enumerate() {
                    if let Some(v) = num(self.deref(v)) {
                        f.widths.insert(first + k as u32, v);
                    }
                }
            }
            if let Some(fd) = self.get(d, b"FontDescriptor").and_then(|o| self.dict(o)) {
                f.default_width = self.get_num(fd, b"MissingWidth").unwrap_or(0.0);
            }
            let mut enc: Vec<Option<char>> = (0..=255u8).map(win_ansi).collect();
            let diffs = match self.get(d, b"Encoding") {
                Some(Object::Dictionary(e)) => self.get_array(e, b"Differences"),
                _ => None,
            };
            if let Some(diffs) = diffs {
                let mut code = 0usize;
                for o in diffs {
                    match self.deref(o) {
                        Object::Integer(c) => code = *c as usize,
                        Object::Name(n) => {
                            if code < 256 {
                                enc[code] = glyph_name(n);
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
            f.encoding = Some(enc);
        }
        f
    }
}

// ---------------------------------------------------------------- page interpreter
#[derive(Clone)]
struct State {
    ctm: M,
    fs: f64,
    fm: M,
    cs: f64,
    ws: f64,
    hs: f64,
    lead: f64,
    rise: f64,
    lw: f64,
    font: Option<Rc<Font>>,
}

#[derive(Clone, Copy)]
enum PathOp {
    Move(f64, f64),
    Line(f64, f64),
    Curve,
    Close,
}

struct Run<'p, 'a> {
    pdf: &'p mut Pdf<'a>,
    st: State,
    stack: Vec<State>,
    tm: M,
    tlm: M,
    path: Vec<PathOp>,
    out: Vec<Glyph>,
    h: Vec<HRule>,
    v: Vec<VRule>,
    images: Vec<f64>,
    depth: usize,
}

impl<'p, 'a> Run<'p, 'a> {
    fn show(&mut self, items: &[Object]) {
        let Some(font) = self.st.font.clone() else { return };
        for it in items {
            match it {
                Object::String(bytes, _) => {
                    for (code, n, uni, width) in font.glyphs(bytes) {
                        let s = &self.st;
                        let w = width * s.fm[0] * s.fs;
                        let t = mul(&self.tm, &s.ctm);
                        if let Some(c) = uni.filter(|c| !c.is_empty()) {
                            let (sx, sy) = (t[0].hypot(t[1]), t[2].hypot(t[3]));
                            let sz = if s.fs.abs() * sy != 0.0 { s.fs.abs() * sy } else { s.fs.abs() };
                            self.out.push(Glyph { c, x0: t[4], x1: t[4] + w * sx, y: t[5], sz });
                        }
                        let is_space = n == 1 && code == 32;
                        let adv = (w + s.cs + if is_space { s.ws } else { 0.0 }) * s.hs;
                        self.tm = mul(&translate(adv, 0.0), &self.tm);
                    }
                }
                o => {
                    if let Some(g) = num(o) {
                        self.tm = mul(&translate(-g / 1000.0 * self.st.fs * self.st.hs, 0.0), &self.tm);
                    }
                }
            }
        }
    }

    fn add_rule(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, thick: f64) {
        let (w, h) = ((x1 - x0).abs(), (y1 - y0).abs());
        if h <= thick && w >= 3.0 {
            self.h.push(HRule { y: (y0 + y1) / 2.0, x0: x0.min(x1), x1: x0.max(x1) });
        } else if w <= thick && h >= 3.0 {
            self.v.push(VRule { x: (x0 + x1) / 2.0, y0: y0.min(y1), y1: y0.max(y1) });
        }
    }

    /// Table borders: Word draws them as thin filled rectangles; other programs stroke lines.
    fn paint(&mut self, fill: bool, stroke: bool) {
        let ctm = self.st.ctm;
        let mut subs: Vec<Vec<(f64, f64)>> = Vec::new();
        let mut cur: Option<usize> = None;
        let mut start = (0.0, 0.0);
        for op in std::mem::take(&mut self.path) {
            match op {
                PathOp::Move(x, y) => {
                    let p = apply(&ctm, x, y);
                    start = p;
                    subs.push(vec![p]);
                    cur = Some(subs.len() - 1);
                }
                PathOp::Line(x, y) => {
                    let p = apply(&ctm, x, y);
                    match cur {
                        Some(k) => subs[k].push(p),
                        None => {
                            subs.push(vec![p]);
                            cur = Some(subs.len() - 1);
                        }
                    }
                }
                PathOp::Curve => cur = None, // curves are never table borders
                PathOp::Close => {
                    if let Some(k) = cur {
                        subs[k].push(start);
                    }
                }
            }
        }
        for s in subs {
            if s.len() < 2 {
                continue;
            }
            if stroke {
                for j in 1..s.len() {
                    self.add_rule(s[j - 1].0, s[j - 1].1, s[j].0, s[j].1, 1.0);
                }
            }
            if fill && s.len() >= 4 {
                let (mut x0, mut y0, mut x1, mut y1) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for &(x, y) in &s {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
                self.add_rule(x0, y0, x1, y1, 3.0);
            }
        }
    }

    fn run(&mut self, content: &[u8], res: &[&'a Dictionary]) {
        let Ok(ops) = Content::decode(content) else { return };
        let lookup = |pdf: &Pdf<'a>, kind: &[u8], name: &[u8]| -> Option<&'a Object> {
            res.iter().find_map(|r| pdf.get(r, kind).and_then(|o| pdf.dict(o)).and_then(|d| d.get(name).ok()))
        };
        for op in &ops.operations {
            let a = &op.operands;
            let n = |k: usize| a.get(k).and_then(num).unwrap_or(0.0);
            match op.operator.as_str() {
                "q" => self.stack.push(self.st.clone()),
                "Q" => {
                    if let Some(s) = self.stack.pop() {
                        self.st = s;
                    }
                }
                "cm" if a.len() == 6 => self.st.ctm = mul(&[n(0), n(1), n(2), n(3), n(4), n(5)], &self.st.ctm),
                "w" => self.st.lw = n(0),
                "BT" => {
                    self.tm = ID;
                    self.tlm = ID;
                }
                "Tf" => {
                    let font_obj = a.first().and_then(|o| o.as_name().ok()).and_then(|name| lookup(self.pdf, b"Font", name));
                    self.st.font = font_obj.map(|o| self.pdf.font(o));
                    self.st.fm = self.st.font.as_ref().map(|f| f.matrix).unwrap_or([0.001, 0.0, 0.0, 0.001, 0.0, 0.0]);
                    self.st.fs = n(1);
                }
                "Tc" => self.st.cs = n(0),
                "Tw" => self.st.ws = n(0),
                "Tz" => self.st.hs = n(0) / 100.0,
                "TL" => self.st.lead = n(0),
                "Ts" => self.st.rise = n(0),
                "Tm" if a.len() == 6 => {
                    self.tm = [n(0), n(1), n(2), n(3), n(4), n(5)];
                    self.tlm = self.tm;
                }
                "Td" => {
                    self.tlm = mul(&translate(n(0), n(1)), &self.tlm);
                    self.tm = self.tlm;
                }
                "TD" => {
                    self.st.lead = -n(1);
                    self.tlm = mul(&translate(n(0), n(1)), &self.tlm);
                    self.tm = self.tlm;
                }
                "T*" => {
                    self.tlm = mul(&translate(0.0, -self.st.lead), &self.tlm);
                    self.tm = self.tlm;
                }
                "Tj" => self.show(a),
                "TJ" => {
                    if let Some(Object::Array(items)) = a.first() {
                        let items = items.clone();
                        self.show(&items);
                    }
                }
                "'" => {
                    self.tlm = mul(&translate(0.0, -self.st.lead), &self.tlm);
                    self.tm = self.tlm;
                    self.show(a);
                }
                "\"" => {
                    self.st.ws = n(0);
                    self.st.cs = n(1);
                    self.tlm = mul(&translate(0.0, -self.st.lead), &self.tlm);
                    self.tm = self.tlm;
                    if let Some(s) = a.get(2) {
                        self.show(std::slice::from_ref(s));
                    }
                }
                // paths
                "m" => self.path.push(PathOp::Move(n(0), n(1))),
                "l" => self.path.push(PathOp::Line(n(0), n(1))),
                "c" | "v" | "y" => self.path.push(PathOp::Curve),
                "h" => self.path.push(PathOp::Close),
                "re" => {
                    let (x, y, w, h) = (n(0), n(1), n(2), n(3));
                    self.path.extend([PathOp::Move(x, y), PathOp::Line(x + w, y), PathOp::Line(x + w, y + h), PathOp::Line(x, y + h), PathOp::Close]);
                }
                "S" | "s" => self.paint(false, true),
                "f" | "F" | "f*" => self.paint(true, false),
                "B" | "B*" | "b" | "b*" => self.paint(true, true),
                "n" => self.path.clear(),
                // pictures and form XObjects
                "Do" => {
                    let Some(xobj) = a.first().and_then(|o| o.as_name().ok()).and_then(|name| lookup(self.pdf, b"XObject", name)) else { continue };
                    let Some(d) = self.pdf.dict(xobj) else { continue };
                    match self.pdf.get(d, b"Subtype").and_then(|o| o.as_name().ok()) {
                        Some(b"Image") => {
                            let mask = matches!(self.pdf.get(d, b"ImageMask"), Some(Object::Boolean(true)));
                            if !mask {
                                let m = self.st.ctm;
                                self.images.push((m[0] * m[3] - m[1] * m[2]).abs());
                            }
                        }
                        Some(b"Form") if self.depth < 12 => {
                            let Some(data) = self.pdf.stream_data(xobj) else { continue };
                            // text and lines inside a form XObject are drawn with the form's own matrix
                            self.stack.push(self.st.clone());
                            if let Some(m) = self.pdf.matrix(d.get(b"Matrix").ok()) {
                                self.st.ctm = mul(&m, &self.st.ctm);
                            }
                            let mut inner: Vec<&'a Dictionary> = self.pdf.get(d, b"Resources").and_then(|o| self.pdf.dict(o)).into_iter().collect();
                            inner.extend_from_slice(res);
                            self.depth += 1;
                            let (tm, tlm) = (self.tm, self.tlm);
                            self.run(&data, &inner);
                            (self.tm, self.tlm) = (tm, tlm);
                            self.depth -= 1;
                            if let Some(s) = self.stack.pop() {
                                self.st = s;
                            }
                        }
                        _ => {}
                    }
                }
                "BI" => {
                    let mask = a.first().and_then(|o| o.as_dict().ok()).is_some_and(|d| {
                        matches!(d.get(b"IM").or_else(|_| d.get(b"ImageMask")), Ok(Object::Boolean(true)))
                    });
                    if !mask {
                        let m = self.st.ctm;
                        self.images.push((m[0] * m[3] - m[1] * m[2]).abs());
                    }
                }
                _ => {}
            }
        }
    }
}

/// Every page of the PDF: glyphs, border lines and pictures.
pub fn read_pages(bytes: &[u8]) -> Result<Vec<PageData>, String> {
    let mut doc = Document::load_mem(bytes).map_err(|e| format!("cannot read this PDF: {e}"))?;
    if doc.is_encrypted() {
        doc.decrypt("").map_err(|e| format!("this PDF is password-protected: {e}"))?;
    }
    let mut pdf = Pdf { doc: &doc, fonts: HashMap::new() };
    let mut pages = Vec::new();
    for (_, page_id) in doc.get_pages() {
        let content = doc.get_page_content(page_id);
        let (own, inherited) = doc.get_page_resources(page_id).unwrap_or((None, vec![]));
        let mut res: Vec<&Dictionary> = own.into_iter().collect();
        res.extend(inherited.iter().filter_map(|id| doc.get_dictionary(*id).ok()));
        let mut run = Run {
            pdf: &mut pdf,
            st: State { ctm: ID, fs: 1.0, fm: [0.001, 0.0, 0.0, 0.001, 0.0, 0.0], cs: 0.0, ws: 0.0, hs: 1.0, lead: 0.0, rise: 0.0, lw: 1.0, font: None },
            stack: vec![],
            tm: ID,
            tlm: ID,
            path: vec![],
            out: vec![],
            h: vec![],
            v: vec![],
            images: vec![],
            depth: 0,
        };
        run.run(&content, &res);
        let (mut glyphs, h_rules, v_rules, images) = (run.out, run.h, run.v, run.images);
        glyphs.reverse();
        pages.push(PageData { glyphs, h_rules, v_rules, images, page_area: page_area(&doc, page_id) });
    }
    Ok(pages)
}

/// Area of the visible page (CropBox, else MediaBox; both can be inherited).
fn page_area(doc: &Document, page_id: ObjectId) -> f64 {
    let pdf = Pdf { doc, fonts: HashMap::new() };
    let boxed = |key: &[u8]| -> Option<[f64; 4]> {
        let mut d = doc.get_dictionary(page_id).ok()?;
        for _ in 0..32 {
            if let Some(Object::Array(a)) = d.get(key).ok().map(|o| pdf.deref(o)) {
                let v: Vec<f64> = a.iter().filter_map(|x| num(pdf.deref(x))).collect();
                if v.len() == 4 {
                    return Some([v[0], v[1], v[2], v[3]]);
                }
            }
            d = pdf.dict(d.get(b"Parent").ok()?)?;
        }
        None
    };
    let media = boxed(b"MediaBox").unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let b = match boxed(b"CropBox") {
        Some(c) => [c[0].max(media[0].min(media[2])), c[1].max(media[1].min(media[3])), c[2].min(media[0].max(media[2])), c[3].min(media[1].max(media[3]))],
        None => media,
    };
    ((b[2] - b[0]) * (b[3] - b[1])).abs()
}
