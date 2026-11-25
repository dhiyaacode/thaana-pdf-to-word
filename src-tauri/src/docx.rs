//! Minimal Word (.docx) writer: right-to-left, justified paragraphs, and tables for lines that
//! contain tabs (table cells found in the PDF). Produces the same layout as the Electron version.

use std::io::{Cursor, Write};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

/// Escapes text for XML and drops characters XML 1.0 does not allow.
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

struct Writer {
    rpr: String,
}

impl Writer {
    fn para(&self, text: &str, justify: bool) -> String {
        let (jc, after, line) = if justify { ("both", 120, 360) } else { ("start", 40, 300) };
        let run = if text.is_empty() {
            String::new()
        } else {
            format!(r#"<w:r>{}<w:t xml:space="preserve">{}</w:t></w:r>"#, self.rpr, esc(text))
        };
        format!(r#"<w:p><w:pPr><w:bidi/><w:spacing w:after="{after}" w:line="{line}" w:lineRule="auto"/><w:jc w:val="{jc}"/></w:pPr>{run}</w:p>"#)
    }

    fn table(&self, rows: &[Vec<&str>]) -> String {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1);
        let bd = r#"w:val="single" w:sz="4" w:space="0" w:color="999999""#;
        let mut x = format!(
            r#"<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:top {bd}/><w:left {bd}/><w:bottom {bd}/><w:right {bd}/><w:insideH {bd}/><w:insideV {bd}/></w:tblBorders><w:tblLayout w:type="autofit"/></w:tblPr><w:tblGrid>"#
        );
        for _ in 0..cols {
            x.push_str(&format!(r#"<w:gridCol w:w="{}"/>"#, 9000 / cols));
        }
        x.push_str("</w:tblGrid>");
        for r in rows {
            x.push_str("<w:tr>");
            for c in 0..cols {
                x.push_str(&format!(r#"<w:tc><w:tcPr><w:tcW w:w="{}" w:type="pct"/></w:tcPr>"#, 5000 / cols));
                x.push_str(&self.para(r.get(c).copied().unwrap_or(""), false));
                x.push_str("</w:tc>");
            }
            x.push_str("</w:tr>");
        }
        x.push_str("</w:tbl>");
        x
    }
}

/// Builds the .docx file for `text`, in `font` at `size_pt` points.
pub fn build(text: &str, font: &str, size_pt: f64) -> Result<Vec<u8>, String> {
    let sz = (size_pt.clamp(4.0, 200.0) * 2.0).round() as u32; // half-points
    let f = esc(font);
    // element order follows the WordprocessingML schema (rFonts .. sz, szCs .. rtl)
    let rpr = format!(r#"<w:rPr><w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:cs="{f}" w:eastAsia="{f}"/><w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/><w:rtl/></w:rPr>"#);
    let w = Writer { rpr: rpr.clone() };

    let lines: Vec<&str> = text.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut body = String::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].contains('\t') {
            body.push_str(&w.para(lines[i], true));
            i += 1;
            continue;
        }
        let mut rows = Vec::new();
        while i < lines.len() && lines[i].contains('\t') {
            rows.push(lines[i].split('\t').collect::<Vec<_>>());
            i += 1;
        }
        body.push_str(&w.table(&rows));
        body.push_str(&w.para("", false));
    }

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:document {NS}><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#
    );
    let styles = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><w:styles {NS}><w:docDefaults><w:rPrDefault>{rpr}</w:rPrDefault><w:pPrDefault><w:pPr><w:bidi/></w:pPr></w:pPrDefault></w:docDefaults></w:styles>"#
    );
    let parts: [(&str, String); 5] = [
        ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#.into()),
        ("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.into()),
        ("word/_rels/document.xml.rels", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.into()),
        ("word/document.xml", document),
        ("word/styles.xml", styles),
    ];
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, xml) in parts {
        zip.start_file(name, opts).map_err(|e| e.to_string())?;
        zip.write_all(xml.as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn document_xml(bytes: &[u8]) -> String {
        let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        z.by_name("word/document.xml").unwrap().read_to_string(&mut s).unwrap();
        s
    }

    #[test]
    fn paragraphs_and_tables() {
        let text = "ދިވެހި ބަސް & <test>\nނަން\tމާއްދާ\tނޯޓު\nއަޙްމަދު\t10%\nއެންމެ ފަހު";
        let xml = document_xml(&build(text, "Faruma", 14.0).unwrap());
        assert_eq!(xml.matches("<w:tbl>").count(), 1);
        assert_eq!(xml.matches("<w:tr>").count(), 2);
        assert_eq!(xml.matches("<w:tc>").count(), 6, "short rows are padded to the widest row");
        assert!(xml.contains("ދިވެހި ބަސް &amp; &lt;test&gt;"));
        assert!(xml.contains(r#"<w:sz w:val="28"/>"#));
        assert!(xml.contains("<w:bidiVisual/>") && xml.contains("<w:bidi/>") && xml.contains("<w:rtl/>"));
    }

    #[test]
    fn strips_characters_xml_cannot_hold() {
        let xml = document_xml(&build("a\u{0001}b\u{FFFF}c", "Faruma", 12.0).unwrap());
        assert!(xml.contains(">abc<"));
    }
}
