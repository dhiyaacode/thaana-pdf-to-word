//! Small text helpers. Some follow JavaScript's rules exactly (what counts as whitespace, how
//! Math.round rounds, string length in UTF-16 units): the reader was first written in JavaScript
//! and these keep its results unchanged.

/// JavaScript's Math.round: halves round up (towards +infinity).
pub fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// JavaScript's `\s` (and the characters `String.prototype.trim` removes).
pub fn js_space(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'..='\u{200A}'
        | '\u{2028}' | '\u{2029}' | '\u{202F}' | '\u{205F}' | '\u{3000}' | '\u{FEFF}')
}

pub fn js_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}

/// String length as JavaScript counts it (UTF-16 code units).
pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

/// Any Thaana character, letter or vowel sign (U+0780-U+07B1).
pub fn is_thaana(c: char) -> bool {
    ('\u{0780}'..='\u{07B1}').contains(&c)
}

/// A Thaana vowel sign (fili, U+07A6-U+07B0).
pub fn is_fili(c: char) -> bool {
    ('\u{07A6}'..='\u{07B0}').contains(&c)
}

/// A PDF line that runs nearly the full width continues into the next one; a short line ends a
/// paragraph. Table rows (cells separated by tabs) are never joined to anything.
pub fn join_pdf_lines(lines: &[&str], join: bool) -> String {
    let lines: Vec<&str> = lines.iter().copied().filter(|l| !js_trim(l).is_empty()).collect();
    if !join {
        return lines.join("\n");
    }
    let is_row = |l: &str| l.contains('\t');
    let max = lines.iter().filter(|l| !is_row(l)).map(|l| utf16_len(l)).max().unwrap_or(0).max(1) as f64;
    let starts_list = |l: &str| {
        let t = l.trim_start_matches(js_space);
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        digits > 0 && matches!(t[digits..].chars().next(), Some('.') | Some(')'))
    };
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        out.push_str(l);
        if i + 1 == lines.len() {
            break;
        }
        let next = lines[i + 1];
        let brk = is_row(l) || is_row(next) || (utf16_len(l) as f64) < max * 0.72 || starts_list(next);
        out.push(if brk { '\n' } else { ' ' });
    }
    out
}

/// Some PDFs lose most vowel signs in their text layer. Real Dhivehi has about one per letter;
/// if far fewer come out, the text can't be trusted and the page is read as a picture instead.
pub fn text_looks_complete(s: &str) -> bool {
    let base = s.chars().filter(|&c| ('\u{0780}'..='\u{07A5}').contains(&c) || c == '\u{07B1}').count();
    let fili = s.chars().filter(|&c| is_fili(c)).count();
    base < 100 || fili as f64 / base as f64 >= 0.95
}

pub fn has_thaana(s: &str) -> bool {
    s.chars().filter(|&c| is_thaana(c)).count() >= 15
}

/// A page with no usable text that shows a big picture is a scan: it still has to be read.
pub fn looks_scanned(image_areas: &[f64], page_area: f64) -> bool {
    image_areas.iter().any(|&a| page_area == 0.0 || a > 0.15 * page_area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_never_joins_table_rows() {
        let long = "ހ".repeat(50);
        let lines = [long.as_str(), "a\tb", long.as_str(), long.as_str()];
        assert_eq!(join_pdf_lines(&lines, true), format!("{long}\na\tb\n{long} {long}"));
    }

    #[test]
    fn looks_complete() {
        assert!(text_looks_complete(&"ދިވެހި".repeat(40)));
        assert!(!text_looks_complete(&"ދވހ".repeat(40)));
    }

    #[test]
    fn js_rounding() {
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(2.5), 3.0);
    }
}
