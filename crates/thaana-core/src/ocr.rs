//! Turning what the reader (Tesseract) recognised on each text line into finished text: removing
//! its usual noise, joining lines into paragraphs (and newspaper columns into blocks), and the
//! dictionary word repair.

use crate::repair::Repairer;
use crate::scan::LineBox;
use crate::text::{is_thaana, js_trim};

/// Symbols a Dhivehi page does not contain (degree, currency and copyright signs, bars, backticks…):
/// the Thaana model invents them in the wide gaps of justified text.
fn junk(c: char) -> bool {
    matches!(c, '\u{A2}' | '\u{A3}' | '\u{A5}'..='\u{A7}' | '\u{A9}' | '\u{AB}' | '\u{AC}' | '\u{AE}' | '\u{B0}' | '\u{BB}' | '\u{BF}'
        | '\u{2C6}' | '\u{201E}' | '\u{2030}' | '\u{2039}' | '\u{203A}' | '\u{20AC}' | '`' | '|' | '~' | '^' | '{' | '}' | '\\')
}

/// Cleans up one line as the reader returned it. Junk symbols are kept when the page also has
/// English (`with_english`).
pub fn tidy(text: &str, with_english: bool) -> String {
    let mut c: Vec<char> = text
        .chars()
        .map(|ch| if !with_english && junk(ch) { ' ' } else { ch })
        // stray direction marks, and Arabic vowel marks the model sometimes invents
        .filter(|&ch| !matches!(ch, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{064B}'..='\u{0652}'))
        .collect();
    // runs of spaces -> one space; spaces before a line break go
    let mut out: Vec<char> = Vec::with_capacity(c.len());
    for ch in c.drain(..) {
        if ch == ' ' && out.last() == Some(&' ') {
            continue;
        }
        if ch == '\n' {
            while matches!(out.last(), Some(' ') | Some('\t')) {
                out.pop();
            }
        }
        out.push(ch);
    }
    // a Latin comma or question mark after Thaana is the Thaana one (with any space before it gone)
    let mut fixed: Vec<char> = Vec::with_capacity(out.len());
    let mut i = 0;
    while i < out.len() {
        let ch = out[i];
        fixed.push(ch);
        if is_thaana(ch) {
            let j = if out.get(i + 1) == Some(&' ') { i + 2 } else { i + 1 };
            match out.get(j) {
                Some(',') => {
                    fixed.push('\u{060C}');
                    i = j + 1;
                    continue;
                }
                Some('?') => {
                    fixed.push('\u{061F}');
                    i = j + 1;
                    continue;
                }
                _ => {}
            }
        }
        i += 1;
    }
    let s: String = fixed.into_iter().collect();
    // the reader works on single lines: any line breaks inside become spaces
    let mut one = String::with_capacity(s.len());
    for part in js_trim(&s).split('\n').filter(|p| !p.is_empty()) {
        if !one.is_empty() {
            one.push(' ');
        }
        one.push_str(part);
    }
    one
}

/// Where paragraphs end: a bigger gap than usual, the previous line stopping well short of the
/// left margin, or this line indented from the right.
pub fn paragraph_breaks(boxes: &[LineBox], pitch: f64) -> Vec<bool> {
    if boxes.is_empty() {
        return vec![];
    }
    let min_l = boxes.iter().map(|b| b.l).min().unwrap() as f64;
    let max_r = boxes.iter().map(|b| b.r).max().unwrap() as f64;
    let width = if max_r - min_l == 0.0 { 1.0 } else { max_r - min_l };
    (0..boxes.len())
        .map(|i| {
            if i == 0 {
                return false;
            }
            let (b, prev) = (&boxes[i], &boxes[i - 1]);
            let gap = (b.t + b.b) as f64 / 2.0 - (prev.t + prev.b) as f64 / 2.0;
            let prev_short = prev.l as f64 - min_l > 0.12 * width;
            let indented = max_r - b.r as f64 > 0.08 * width;
            gap > 1.45 * pitch || prev_short || indented
        })
        .collect()
}

/// Puts the lines of a page together. `blk` (newspaper mode) gives each line's block: blocks are
/// assembled separately and separated by a blank line.
pub fn assemble(texts: &[String], boxes: &[LineBox], pitch: f64, join: bool, blk: Option<&[usize]>) -> String {
    if let Some(blk) = blk {
        let mut order: Vec<usize> = Vec::new();
        for &b in blk {
            if !order.contains(&b) {
                order.push(b);
            }
        }
        return order
            .iter()
            .map(|&b| {
                let idx: Vec<usize> = (0..texts.len()).filter(|&i| blk[i] == b).collect();
                let t: Vec<String> = idx.iter().map(|&i| texts[i].clone()).collect();
                let bx: Vec<LineBox> = idx.iter().map(|&i| boxes[i]).collect();
                assemble(&t, &bx, pitch, join, None)
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    if !join {
        return texts.iter().filter(|t| !t.is_empty()).cloned().collect::<Vec<_>>().join("\n");
    }
    let breaks = paragraph_breaks(boxes, pitch);
    let mut out = String::new();
    for (i, t) in texts.iter().enumerate() {
        if t.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(if breaks[i] { '\n' } else { ' ' });
        }
        out.push_str(t);
    }
    out
}

/// The finished text of a scanned page from what the reader recognised on each line.
pub fn finish(recognised: &[String], boxes: &[LineBox], pitch: f64, blk: Option<&[usize]>, join: bool, with_english: bool, repairer: &Repairer) -> String {
    let texts: Vec<String> = recognised.iter().map(|t| tidy(t, with_english)).collect();
    // dictionary repair, as for typed PDFs: joins words the reader split and fixes vowel signs,
    // only where the result is a known word
    repairer.fix_text(&assemble(&texts, boxes, pitch, join, blk))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tidies_reader_noise() {
        assert_eq!(tidy("ދިވެހި ° ބަސް | ދ ,ހ\n", false), "ދިވެހި ބަސް ދ\u{060C}ހ");
        assert_eq!(tidy("price \u{20AC}5 | x", true), "price \u{20AC}5 | x");
        assert_eq!(tidy("ހ?\u{200F}", false), "ހ\u{061F}");
    }

    #[test]
    fn joins_lines_into_paragraphs() {
        let b = |l, t| LineBox { l, t, r: 1000, b: t + 40 };
        let boxes = [b(0, 0), b(0, 45), b(600, 90), b(0, 200)];
        let texts: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        // line 3 stops short (ends the paragraph), line 4 comes after a big gap
        assert_eq!(assemble(&texts, &boxes, 45.0, true, None), "a b c\nd");
        assert_eq!(assemble(&texts, &boxes, 45.0, false, None), "a\nb\nc\nd");
        assert_eq!(assemble(&texts, &boxes, 45.0, true, Some(&[1, 1, 0, 0])), "a b\n\nc\nd");
    }
}
