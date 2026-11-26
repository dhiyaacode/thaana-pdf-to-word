//! Conservative repair of Thaana words: fixes misplaced or
//! stray vowel signs (fili) only when the result is a known word. Unknown words are left exactly
//! as they are.

use crate::text::{is_fili, is_thaana, js_space};
use std::collections::{HashMap, HashSet};

fn strip(w: &str) -> String {
    w.chars().filter(|&c| !is_fili(c)).collect()
}

fn counts(w: &str) -> HashMap<char, i32> {
    let mut m = HashMap::new();
    for c in w.chars().filter(|&c| is_fili(c)) {
        *m.entry(c).or_insert(0) += 1;
    }
    m
}

pub struct Repairer {
    known: HashSet<String>,
    skel: HashMap<String, Vec<String>>, // skeleton (no vowel signs) -> words, most frequent first
}

impl Repairer {
    /// `words`: the word list, one word per line, most frequent first.
    pub fn new(words: &str) -> Self {
        let mut known = HashSet::new();
        let mut skel: HashMap<String, Vec<String>> = HashMap::new();
        for w in words.split('\n').map(|l| l.trim_end_matches('\r')).filter(|l| !l.is_empty()) {
            known.insert(w.to_string());
            let k = strip(w);
            if !k.is_empty() {
                skel.entry(k).or_default().push(w.to_string());
            }
        }
        Repairer { known, skel }
    }

    pub fn fix_word(&self, w: &str) -> String {
        let w: String = w.trim_start_matches(is_fili).to_string(); // vowel sign with no letter before it
        if w.is_empty() || self.known.contains(&w) {
            return w;
        }
        let Some(cands) = self.skel.get(&strip(&w)) else { return w };
        let a = counts(&w);
        for x in cands {
            let b = counts(x);
            let miss: i32 = b.iter().map(|(k, v)| (v - a.get(k).copied().unwrap_or(0)).max(0)).sum();
            let extra: i32 = a.iter().map(|(k, v)| (v - b.get(k).copied().unwrap_or(0)).max(0)).sum();
            if (miss == 0 || extra == 0) && miss + extra <= 2 {
                return x.clone();
            }
        }
        w
    }

    /// Joins a word split by a stray space ("ކަ ންތައް") and tidies spacing around punctuation.
    pub fn fix_text(&self, t: &str) -> String {
        let tw = |p: &str| !p.is_empty() && p.chars().all(is_thaana);
        t.split('\n')
            .map(|line| {
                let parts = split_keep_spaces(line);
                let mut out = String::new();
                let mut i = 0;
                while i < parts.len() {
                    let p = parts[i];
                    if tw(p) && i + 2 < parts.len() && parts[i + 1] == " " && tw(parts[i + 2]) {
                        let (a, b) = (self.fix_word(p), self.fix_word(parts[i + 2]));
                        let ab = format!("{a}{b}");
                        if !(self.known.contains(&a) && self.known.contains(&b)) && self.known.contains(&ab) {
                            out.push_str(&ab);
                            i += 3;
                            continue;
                        }
                    }
                    if tw(p) { out.push_str(&self.fix_word(p)) } else { out.push_str(p) }
                    i += 1;
                }
                tidy_punctuation(&out)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// `line.split(/(\s+)/)` in JavaScript: words and the whitespace between them, alternating,
/// starting and ending with a (possibly empty) word.
fn split_keep_spaces(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_space = false;
    for (i, c) in line.char_indices() {
        let sp = js_space(c);
        if sp != in_space {
            parts.push(&line[start..i]);
            start = i;
            in_space = sp;
        }
    }
    parts.push(&line[start..]);
    if in_space {
        parts.push("");
    }
    parts
}

/// `.replace(/\s+([،؟!:؛])/g, '$1').replace(/([،؟؛])(?=[^\s\d])/g, '$1 ')`
fn tidy_punctuation(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut a: Vec<char> = Vec::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if js_space(chars[i]) {
            let mut j = i;
            while j < chars.len() && js_space(chars[j]) {
                j += 1;
            }
            if j < chars.len() && matches!(chars[j], '\u{060C}' | '\u{061F}' | '!' | ':' | '\u{061B}') {
                i = j; // drop the whitespace before the punctuation
                continue;
            }
            a.extend_from_slice(&chars[i..j]);
            i = j;
            continue;
        }
        a.push(chars[i]);
        i += 1;
    }
    let mut out = String::with_capacity(s.len() + 8);
    for (k, &c) in a.iter().enumerate() {
        out.push(c);
        if matches!(c, '\u{060C}' | '\u{061F}' | '\u{061B}') {
            if let Some(&n) = a.get(k + 1) {
                if !js_space(n) && !n.is_ascii_digit() {
                    out.push(' ');
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_like_javascript() {
        assert_eq!(split_keep_spaces(" a b"), vec!["", " ", "a", " ", "b"]);
        assert_eq!(split_keep_spaces("a "), vec!["a", " ", ""]);
        assert_eq!(split_keep_spaces("ab"), vec!["ab"]);
    }

    #[test]
    fn punctuation() {
        assert_eq!(tidy_punctuation("ހ ، ދ"), "ހ، ދ");
        assert_eq!(tidy_punctuation("ހ،ދ"), "ހ، ދ");
        assert_eq!(tidy_punctuation("ހ،1"), "ހ،1");
    }

    #[test]
    fn repairs_known_words_only() {
        let r = Repairer::new("ސުވާލު\nކަންތައް\n");
        assert_eq!(r.fix_word("ސުވާލ"), "ސުވާލު");
        assert_eq!(r.fix_word("ޒޒޒ"), "ޒޒޒ");
        assert_eq!(r.fix_text("ކަ ންތައް"), "ކަންތައް");
    }
}
