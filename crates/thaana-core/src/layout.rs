//! Rebuilds Thaana text lines from the glyphs of a PDF page: vowel signs onto their letters,
//! right-to-left order, tables from border lines, Arabic handling. A line-for-line port of
//! `rebuild()` and `findTables()` in renderer/pdfglyphs.js; keep the two in step.

use crate::text::{is_fili, is_thaana, js_round, js_space, js_trim};
use std::collections::HashMap;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Glyph {
    pub c: String,
    pub x0: f64,
    pub x1: f64,
    pub y: f64,
    pub sz: f64,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct HRule {
    pub y: f64,
    pub x0: f64,
    pub x1: f64,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct VRule {
    pub x: f64,
    pub y0: f64,
    pub y1: f64,
}

/// Everything read from one page.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct PageData {
    pub glyphs: Vec<Glyph>,
    pub h_rules: Vec<HRule>,
    pub v_rules: Vec<VRule>,
    pub images: Vec<f64>, // area of each picture drawn
    pub page_area: f64,
}

#[derive(Clone, Copy, Default)]
pub struct Options {
    pub drop_arabic: bool,
}

// ---- character classes (same ranges as the JavaScript regexes) ----
fn ign(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
fn pua(c: char) -> bool {
    ('\u{E000}'..='\u{F8FF}').contains(&c)
}
/// Arabic script, except the comma, semicolon and question mark that Dhivehi uses too.
fn arabic(c: char) -> bool {
    matches!(c, '\u{0600}'..='\u{060B}' | '\u{060D}'..='\u{061A}' | '\u{061C}'..='\u{061E}' | '\u{0620}'..='\u{06FF}'
        | '\u{0750}'..='\u{077F}' | '\u{08A0}'..='\u{08FF}' | '\u{FB50}'..='\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}
/// Arabic vowel marks and tatweel.
fn arabic_mark(c: char) -> bool {
    matches!(c, '\u{0610}'..='\u{061A}' | '\u{0640}' | '\u{064B}'..='\u{065F}' | '\u{0670}' | '\u{06D6}'..='\u{06ED}' | '\u{08D3}'..='\u{08FF}')
}
fn thaana_letter(c: char) -> bool {
    ('\u{0780}'..='\u{07A5}').contains(&c) || c == '\u{07B1}'
}
/// Quran-verse fonts map whole words to codes in the presentation-forms blocks (not readable text).
/// Real ligature words (U+FDF0-FDFD, such as the name of Allah) are kept.
fn verse_glyph(c: char) -> bool {
    matches!(c, '\u{FB50}'..='\u{FDEF}' | '\u{FDFE}' | '\u{FDFF}' | '\u{FE70}'..='\u{FEFF}')
}
fn is_l(c: char) -> bool {
    c.is_ascii_alphanumeric()
}
fn is_n(c: char) -> bool {
    js_space(c) || ".,:;/-+%=*\u{D7}\u{F7}<>^_".contains(c)
}

/// A Thaana letter carries at most one vowel sign. Extra ones are stray pieces of a neighbour
/// (usually an abafili next to another sign), so keep the sign that is not abafili.
fn clean_fili(f: &[char]) -> String {
    let mut u: Vec<char> = Vec::new();
    for &c in f {
        if !u.contains(&c) {
            u.push(c);
        }
    }
    if u.len() > 1 && u.contains(&'\u{07A6}') {
        u.retain(|&c| c != '\u{07A6}');
    }
    u.into_iter().collect()
}

// ---- tables from border lines ----
#[derive(Clone, Debug)]
pub struct Cell {
    pub r: f64,
    pub l: f64,
    pub span: usize,
}
#[derive(Clone, Debug)]
pub struct Row {
    pub top: f64,
    pub bot: f64,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Copy)]
struct Seg {
    pos: f64,
    a: f64,
    b: f64,
}

/// Merges touching pieces of the same border (Word draws one piece per cell edge).
fn merge_rules(list: &[Seg]) -> Vec<Seg> {
    let mut s = list.to_vec();
    s.sort_by(|p, q| (p.pos - q.pos).partial_cmp(&0.0).unwrap().then((p.a - q.a).partial_cmp(&0.0).unwrap()));
    let mut out: Vec<Seg> = Vec::new();
    for r in s {
        if let Some(m) = out.iter_mut().find(|o| (o.pos - r.pos).abs() < 1.5 && r.a <= o.b + 2.0 && r.b >= o.a - 2.0) {
            m.a = m.a.min(r.a);
            m.b = m.b.max(r.b);
            m.pos = (m.pos + r.pos) / 2.0;
        } else {
            out.push(r);
        }
    }
    out
}

fn cluster(vals: &[f64], tol: f64) -> Vec<f64> {
    let mut s = vals.to_vec();
    s.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap());
    let mut out: Vec<Vec<f64>> = Vec::new();
    for v in s {
        match out.last_mut() {
            Some(g) if v - g[g.len() - 1] < tol => g.push(v),
            _ => out.push(vec![v]),
        }
    }
    out.iter().map(|g| g.iter().sum::<f64>() / g.len() as f64).collect()
}

/// Bordered tables: rows and columns from the border lines.
pub fn find_tables(h_rules: &[HRule], v_rules: &[VRule]) -> Vec<Vec<Row>> {
    let h = merge_rules(&h_rules.iter().map(|r| Seg { pos: r.y, a: r.x0, b: r.x1 }).collect::<Vec<_>>());
    let v = merge_rules(&v_rules.iter().map(|r| Seg { pos: r.x, a: r.y0, b: r.y1 }).collect::<Vec<_>>());
    if h.len() < 2 || v.len() < 2 {
        return vec![];
    }
    // h: pos = y, a..b = x0..x1;  v: pos = x, a..b = y0..y1
    let meets = |h: &Seg, v: &Seg| v.pos >= h.a - 2.0 && v.pos <= h.b + 2.0 && h.pos >= v.a - 2.0 && h.pos <= v.b + 2.0;
    let all: Vec<(Seg, bool)> = h.iter().map(|r| (*r, true)).chain(v.iter().map(|r| (*r, false))).collect();
    let mut seen = vec![false; all.len()];
    let mut tables = Vec::new();
    for start in 0..all.len() {
        if seen[start] {
            continue;
        }
        let mut group = vec![start];
        seen[start] = true;
        let mut i = 0;
        while i < group.len() {
            let (g, gh) = all[group[i]];
            for (o, &(r, oh)) in all.iter().enumerate() {
                if !seen[o] && oh != gh && (if gh { meets(&g, &r) } else { meets(&r, &g) }) {
                    seen[o] = true;
                    group.push(o);
                }
            }
            i += 1;
        }
        let hs: Vec<Seg> = group.iter().filter(|&&k| all[k].1).map(|&k| all[k].0).collect();
        let vs: Vec<Seg> = group.iter().filter(|&&k| !all[k].1).map(|&k| all[k].0).collect();
        if hs.len() < 2 || vs.len() < 2 {
            continue;
        }
        let mut ys = cluster(&hs.iter().map(|s| s.pos).collect::<Vec<_>>(), 1.5);
        ys.sort_by(|p, q| (q - p).partial_cmp(&0.0).unwrap()); // top first
        let mut xs = cluster(&vs.iter().map(|s| s.pos).collect::<Vec<_>>(), 1.5);
        xs.sort_by(|p, q| (q - p).partial_cmp(&0.0).unwrap()); // right first
        if ys.len() < 2 || xs.len() < 3 {
            continue; // one column is just a box around text
        }
        let mut rows = Vec::new();
        for i in 0..ys.len() - 1 {
            let (top, bot) = (ys[i], ys[i + 1]);
            let mid = (top + bot) / 2.0;
            if top - bot < 4.0 {
                continue;
            }
            // a column border that does not cross this row means merged cells
            let cross: Vec<f64> = xs
                .iter()
                .enumerate()
                .filter(|&(j, &x)| j == 0 || j == xs.len() - 1 || vs.iter().any(|v| (v.pos - x).abs() < 1.5 && v.a <= mid && v.b >= mid))
                .map(|(_, &x)| x)
                .collect();
            let cells = (0..cross.len().saturating_sub(1))
                .map(|j| {
                    let span = xs.iter().filter(|&&x| x <= cross[j] + 0.5 && x > cross[j + 1] + 0.5).count();
                    Cell { r: cross[j], l: cross[j + 1], span: span.max(1) }
                })
                .collect();
            rows.push(Row { top, bot, cells });
        }
        if !rows.is_empty() {
            tables.push(rows);
        }
    }
    tables
}

// ---- text lines ----
#[derive(Clone)]
struct Item {
    c: char,
    x0: f64,
    x1: f64,
    y: f64,
    sz: f64,
    f: Vec<char>,
}

fn by_desc(a: f64, b: f64) -> std::cmp::Ordering {
    (b - a).partial_cmp(&0.0).unwrap_or(std::cmp::Ordering::Equal)
}

struct Ctx<'a> {
    it: &'a [Item],
    med: f64,
    opts: Options,
}

impl Ctx<'_> {
    /// Visual right-to-left order, keeping runs of digits / Latin / formulas left-to-right.
    fn logical(&self, l: &[usize]) -> Vec<usize> {
        let mut a = l.to_vec();
        a.sort_by(|&p, &q| by_desc((self.it[p].x0 + self.it[p].x1) / 2.0, (self.it[q].x0 + self.it[q].x1) / 2.0));
        let c = |k: usize| self.it[a[k]].c;
        let mut out = Vec::with_capacity(a.len());
        let mut i = 0;
        while i < a.len() {
            if is_l(c(i)) {
                let (mut j, mut end) = (i, i);
                while j < a.len() && (is_l(c(j)) || (is_n(c(j)) && j + 1 < a.len() && (is_l(c(j + 1)) || is_n(c(j + 1))))) {
                    if is_l(c(j)) {
                        end = j;
                    }
                    j += 1;
                }
                out.extend(a[i..=end].iter().rev());
                i = end + 1;
            } else {
                out.push(a[i]);
                i += 1;
            }
        }
        out
    }

    fn zero_space(&self, k: usize) -> bool {
        let b = &self.it[k];
        b.c == ' ' && b.x1 - b.x0 < 0.05 * b.sz
    }

    fn join_line(&self, line: &[usize]) -> String {
        let it = self.it;
        let mut l: Vec<usize> = line.to_vec();
        // Zero-width "spaces" are left-overs of Word's character mapping; ignore them when the line has real ones.
        if l.iter().any(|&k| it[k].c == ' ' && !self.zero_space(k)) {
            l.retain(|&k| !self.zero_space(k));
        }
        // "Ignore Arabic": leave out lines that are mostly Arabic (verses, prayers). Arabic words inside a
        // Dhivehi sentence (such as the name of Allah) are kept, without their vowel marks.
        let mut dropped_arabic = false;
        if self.opts.drop_arabic {
            let ar = l.iter().filter(|&&k| arabic(it[k].c) && !arabic_mark(it[k].c)).count();
            let th = l.iter().filter(|&&k| thaana_letter(it[k].c)).count();
            if ar > 0 {
                dropped_arabic = ar > th;
                l.retain(|&k| !(arabic_mark(it[k].c) || verse_glyph(it[k].c) || (dropped_arabic && arabic(it[k].c))));
            }
            if l.is_empty() {
                return String::new();
            }
        }
        let a = self.logical(&l);
        let no_spaces = (a.iter().filter(|&&k| it[k].c == ' ').count() as f64) < a.len() as f64 * 0.06;
        let mut out = String::new();
        for i in 0..a.len() {
            let b = &it[a[i]];
            if no_spaces && i > 0 {
                let p = &it[a[i - 1]];
                if b.c != ' ' && p.c != ' ' && !is_l(b.c) && !is_l(p.c) && p.x0 - b.x1 > 0.17 * self.med {
                    out.push(' '); // right-to-left: the previous letter is on the right
                }
            }
            out.push(b.c);
            out.push_str(&clean_fili(&b.f));
        }
        let mut out = collapse_spaces(&out);
        // Wingdings bullets come out as a lone Latin 'z' next to Thaana text
        if out.chars().any(is_thaana) {
            out = collapse_spaces(&remove_lone_z(&out));
        }
        // brackets stored mirrored: if the first one met is a closing bracket, swap them all in this line
        if matches!(out.chars().find(|c| "()[]".contains(*c)), Some(')') | Some(']')) {
            out = out.chars().map(|c| match c { '(' => ')', ')' => '(', '[' => ']', ']' => '[', c => c }).collect();
        }
        // a full stop / comma drawn at the right-hand end of a Thaana line belongs at its end
        out = move_leading_stop(&out);
        // a full stop never has a space before it after a Thaana word
        out = no_space_before_stop(&out);
        // what is left of an Arabic line is only punctuation and verse numbers
        if dropped_arabic && !out.chars().any(|c| is_thaana(c) || c.is_ascii_alphabetic()) {
            return String::new();
        }
        out
    }

    fn group_lines(&self, list: &[usize]) -> Vec<Vec<usize>> {
        let mut sorted = list.to_vec();
        sorted.sort_by(|&a, &b| by_desc(self.it[a].y, self.it[b].y)); // PDF y grows upward => descending
        let mut lines: Vec<Vec<usize>> = Vec::new();
        for b in sorted {
            match lines.last_mut() {
                Some(l) if (self.it[b].y - self.it[*l.last().unwrap()].y).abs() < self.med * 0.7 => l.push(b),
                _ => lines.push(vec![b]),
            }
        }
        lines
    }
}

/// `.replace(/ +/g, ' ').trim()`
fn collapse_spaces(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for c in s.chars() {
        if c == ' ' {
            if !prev_space {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(c);
            prev_space = false;
        }
    }
    js_trim(&out).to_string()
}

/// `.replace(/(?<![A-Za-z0-9])z(?![A-Za-z0-9])/g, '')`
fn remove_lone_z(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let alnum = |k: Option<&char>| k.is_some_and(|c| c.is_ascii_alphanumeric());
    (0..ch.len())
        .filter(|&i| !(ch[i] == 'z' && !alnum(if i > 0 { ch.get(i - 1) } else { None }) && !alnum(ch.get(i + 1))))
        .map(|i| ch[i])
        .collect()
}

/// `.replace(/^([.،؟!])\s*(?=[ހ-ޱ])(.*)$/, '$2$1')`
fn move_leading_stop(s: &str) -> String {
    let mut it = s.chars();
    let Some(first) = it.next() else { return s.to_string() };
    if !matches!(first, '.' | '\u{060C}' | '\u{061F}' | '!') {
        return s.to_string();
    }
    let rest: &str = it.as_str().trim_start_matches(js_space);
    let line_end = |c: char| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}');
    if !rest.chars().next().is_some_and(is_thaana) || rest.chars().any(line_end) {
        return s.to_string();
    }
    format!("{rest}{first}")
}

/// `.replace(/([ހ-ޱ]) +\.(?= |$)/g, '$1.')`
fn no_space_before_stop(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < ch.len() {
        out.push(ch[i]);
        if is_thaana(ch[i]) {
            let mut j = i + 1;
            while j < ch.len() && ch[j] == ' ' {
                j += 1;
            }
            if j > i + 1 && j < ch.len() && ch[j] == '.' && (j + 1 == ch.len() || ch[j + 1] == ' ') {
                out.push('.');
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `.replace(/^(\d+[.)]|[-•–])\t/, '$1 ')`: list numbers are not table cells.
fn list_number_not_cell(s: &str) -> String {
    let digits = s.chars().take_while(|c| c.is_ascii_digit()).count();
    let head = if digits > 0 && matches!(s[digits..].chars().next(), Some('.') | Some(')')) {
        digits + 1
    } else if matches!(s.chars().next(), Some('-') | Some('\u{2022}') | Some('\u{2013}')) {
        s.chars().next().unwrap().len_utf8()
    } else {
        return s.to_string();
    };
    if s[head..].starts_with('\t') {
        format!("{} {}", &s[..head], &s[head + 1..])
    } else {
        s.to_string()
    }
}

/// The text lines of one page (port of `rebuild()` in renderer/pdfglyphs.js).
pub fn rebuild(page: &PageData, opts: Options) -> Vec<String> {
    // one item per character
    let mut items: Vec<Item> = Vec::new();
    for g in &page.glyphs {
        let single_ign = { let mut cs = g.c.chars(); matches!((cs.next(), cs.next()), (Some(c), None) if ign(c)) };
        if single_ign || g.c.chars().any(pua) {
            continue;
        }
        for (k, ch) in g.c.chars().enumerate() {
            items.push(Item { c: ch, x0: g.x0, x1: if k > 0 && is_fili(ch) { g.x0 } else { g.x1 }, y: g.y, sz: g.sz, f: vec![] });
        }
    }
    // Fake-bold: some generators draw every glyph 3-4 times with tiny offsets. Keep one copy.
    {
        let mut seen: HashMap<(char, i64, i64), Vec<usize>> = HashMap::new();
        let mut keep: Vec<Item> = Vec::new();
        for it in items.into_iter() {
            let q = (it.sz * 0.3).max(1.0);
            let (kx, ky) = (js_round(it.x0 / q) as i64, js_round(it.y / q) as i64);
            let mut dup = false;
            'outer: for a in -1..=1 {
                for b in -1..=1 {
                    if let Some(l) = seen.get(&(it.c, kx + a, ky + b)) {
                        if l.iter().any(|&o| (keep[o].x0 - it.x0).abs() < q && (keep[o].y - it.y).abs() < q) {
                            dup = true;
                            break 'outer;
                        }
                    }
                }
            }
            if dup {
                continue;
            }
            seen.entry((it.c, kx, ky)).or_default().push(keep.len());
            keep.push(it);
        }
        items = keep;
    }
    let mut letters: Vec<usize> = (0..items.len()).filter(|&k| !is_fili(items[k].c)).collect();
    if letters.is_empty() {
        return vec![];
    }
    let med = {
        let mut s: Vec<f64> = letters.iter().map(|&k| items[k].sz).collect();
        s.sort_by(|a, b| (a - b).partial_cmp(&0.0).unwrap());
        s[s.len() >> 1]
    };
    // Attach each vowel sign to its letter. Distances scale with the text size (0.6 pt at 12 pt).
    let mut last: Option<usize> = None;
    let (mut orphans, mut wide) = (Vec::new(), Vec::new());
    for k in 0..items.len() {
        let it = &items[k];
        if is_fili(it.c) {
            // A vowel sign drawn with its own advance width: Word does this for the last letter of a
            // word in justified text, so it is normally a real vowel sign (see the JavaScript notes).
            if it.x1 - it.x0 > 0.12 * it.sz {
                wide.push((k, last));
                continue;
            }
            match last {
                Some(p) if (it.x0 - items[p].x0).abs() <= 0.05 * it.sz && (it.y - items[p].y).abs() < med => {
                    let c = it.c;
                    items[p].f.push(c);
                }
                _ => orphans.push((k, last)),
            }
        } else if !(it.c == ' ' && it.x1 - it.x0 < 0.05 * it.sz) {
            last = Some(k); // Word puts a zero-width "space" between a letter and its last vowel sign
        }
    }
    for (f, prev) in orphans {
        let fi = items[f].clone();
        let mut tgt = None;
        let mut bd = 1e9;
        for &b in &letters {
            let bi = &items[b];
            if bi.c == ' ' || (bi.y - fi.y).abs() >= med * 0.8 || (bi.x0 - fi.x0).abs() > 0.1 * fi.sz {
                continue;
            }
            let d = (bi.x0 - fi.x0).abs();
            if d < bd {
                bd = d;
                tgt = Some(b);
            }
        }
        if let Some(t) = tgt.or(prev) {
            items[t].f.push(fi.c);
        }
    }
    for (f, prev) in wide {
        let fi = items[f].clone();
        let attach = prev.is_some_and(|p| {
            let pi = &items[p];
            pi.c != ' ' && pi.f.is_empty() && (fi.y - pi.y).abs() < med * 0.8 && fi.x0 >= pi.x0 - 0.15 * fi.sz && fi.x0 <= pi.x1 + 0.05 * fi.sz
        });
        if attach {
            items[prev.unwrap()].f.push(fi.c);
        } else {
            items[f].c = ' ';
            items[f].x1 = fi.x0 + 0.25 * fi.sz;
            letters.push(f);
        }
    }
    let bases = letters;
    let cx = Ctx { it: &items, med, opts };

    // Bordered tables: put each letter in its cell; lines that wrap inside a cell are joined.
    let mut out: Vec<(f64, String)> = Vec::new();
    let mut in_table = vec![false; items.len()];
    for rows in find_tables(&page.h_rules, &page.v_rules) {
        for row in rows {
            let mut cells: Vec<Vec<usize>> = vec![vec![]; row.cells.len()];
            for &b in &bases {
                if in_table[b] {
                    continue;
                }
                let bi = &items[b];
                let (x, y) = ((bi.x0 + bi.x1) / 2.0, bi.y + 0.3 * bi.sz); // middle of the letter, above the baseline
                if y >= row.top || y <= row.bot {
                    continue;
                }
                if let Some(k) = row.cells.iter().position(|c| x < c.r && x > c.l) {
                    cells[k].push(b);
                    in_table[b] = true;
                }
            }
            if !cells.iter().any(|c| c.iter().any(|&b| items[b].c != ' ')) {
                continue;
            }
            let mut texts = Vec::new();
            for (k, c) in row.cells.iter().enumerate() {
                let t: Vec<String> = cx.group_lines(&cells[k]).iter().map(|l| cx.join_line(l)).filter(|t| !t.is_empty()).collect();
                texts.push(t.join(" "));
                for _ in 1..c.span {
                    texts.push(String::new()); // merged cell: keep the columns lined up
                }
            }
            out.push((row.top, texts.iter().map(|t| t.replace('\t', " ")).collect::<Vec<_>>().join("\t")));
        }
    }
    // Everything else; a very wide gap inside a line still separates cells of tables without borders.
    let cell_gap = 1.8 * med;
    let split_cells = |l: &[usize]| -> Vec<Vec<usize>> {
        let mut a = l.to_vec();
        a.sort_by(|&p, &q| by_desc(items[p].x1, items[q].x1));
        let mut parts: Vec<Vec<usize>> = vec![vec![a[0]]];
        for &g in &a[1..] {
            let lp = parts.last().unwrap();
            let gap = lp.iter().fold(1e9_f64, |m, &k| m.min(items[k].x0)) - items[g].x1;
            if gap > cell_gap { parts.push(vec![g]) } else { parts.last_mut().unwrap().push(g) }
        }
        parts
    };
    struct TRow {
        entry: usize,
        y: f64,
        texts: Vec<String>,
        cols: Vec<(f64, f64)>, // (r, l)
    }
    let finish = |r: &TRow, out: &mut Vec<(f64, String)>| out[r.entry].1 = list_number_not_cell(&r.texts.join("\t"));
    let mut row: Option<TRow> = None;
    let rest: Vec<usize> = bases.iter().copied().filter(|&b| !in_table[b]).collect();
    for l in cx.group_lines(&rest) {
        let y = items[l[0]].y;
        let real: Vec<usize> = l.iter().copied().filter(|&b| items[b].c != ' ').collect();
        // A line right under a table row with nothing in the row's first (right-hand) column is the
        // rest of cells that wrapped onto a second line: add it to those cells instead.
        if let Some(r) = row.as_mut() {
            if !real.is_empty() && r.y - y < 2.5 * med {
                let bounds: Vec<f64> = (1..r.cols.len()).map(|k| (r.cols[k - 1].1 + r.cols[k].0) / 2.0).collect();
                let col = |b: usize| {
                    let x = (items[b].x0 + items[b].x1) / 2.0;
                    bounds.iter().position(|&v| x > v).unwrap_or(bounds.len())
                };
                let (last_l, first_r) = (r.cols[r.cols.len() - 1].1, r.cols[0].0);
                let inside = real.iter().all(|&b| items[b].x1 > last_l - 2.0 * med && items[b].x0 < first_r + 2.0 * med);
                if inside && !real.iter().any(|&b| col(b) == 0) {
                    let mut groups: Vec<Vec<usize>> = vec![vec![]; r.cols.len()];
                    for &b in &l {
                        groups[col(b)].push(b);
                    }
                    for (k, g) in groups.iter().enumerate() {
                        let t = if g.is_empty() { String::new() } else { cx.join_line(g) };
                        if !t.is_empty() {
                            r.texts[k] = if r.texts[k].is_empty() { t } else { format!("{} {}", r.texts[k], t) };
                        }
                    }
                    r.y = y;
                    continue;
                }
            }
        }
        if let Some(r) = row.take() {
            finish(&r, &mut out);
        }
        let parts = split_cells(&l);
        let texts: Vec<String> = parts.iter().map(|p| cx.join_line(p)).collect();
        out.push((y, String::new()));
        let entry = out.len() - 1;
        let worded = texts.iter().filter(|t| t.chars().any(|c| is_thaana(c) || c.is_ascii_alphanumeric())).count();
        if worded >= 2 {
            let keep: Vec<usize> = (0..texts.len()).filter(|&k| !texts[k].is_empty()).collect();
            row = Some(TRow {
                entry,
                y,
                texts: keep.iter().map(|&k| texts[k].clone()).collect(),
                cols: keep
                    .iter()
                    .map(|&k| {
                        let p = &parts[k];
                        (p.iter().map(|&g| items[g].x1).fold(f64::NEG_INFINITY, f64::max), p.iter().map(|&g| items[g].x0).fold(f64::INFINITY, f64::min))
                    })
                    .collect(),
            });
        } else {
            out[entry].1 = texts.concat();
        }
    }
    if let Some(r) = row.take() {
        finish(&r, &mut out);
    }
    out.sort_by(|a, b| by_desc(a.0, b.0));
    out.into_iter().map(|(_, t)| t).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn javascript_regex_equivalents() {
        assert_eq!(collapse_spaces("  a   b "), "a b");
        assert_eq!(remove_lone_z("z ދ az z"), " ދ az ");
        assert_eq!(move_leading_stop(". ދިވެހި"), "ދިވެހި.");
        assert_eq!(move_leading_stop(".a"), ".a");
        assert_eq!(no_space_before_stop("ދެވެ . ދ ދ  ."), "ދެވެ. ދ ދ.");
        assert_eq!(list_number_not_cell("1.\tހ\tދ"), "1. ހ\tދ");
        assert_eq!(list_number_not_cell("\u{2022}\tހ"), "\u{2022} ހ");
        assert_eq!(clean_fili(&['\u{07A6}', '\u{07A8}', '\u{07A8}']), "\u{07A8}");
    }
}
