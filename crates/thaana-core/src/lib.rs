//! Thaana PDF reader: gets correctly ordered Dhivehi text (with tables) out of typed PDFs.
//! A Rust port of renderer/pdfglyphs.js, repair.js and pdftext.js that gives the same output;
//! test/parity.mjs checks that on every test PDF.

pub mod layout;
pub mod pdf;
pub mod repair;
pub mod text;

use layout::Options;
use repair::Repairer;
use serde::Serialize;

/// The Dhivehi word list used to repair vowel signs (most frequent words first).
pub const WORDS: &str = include_str!("../../../lang/dhivehi-words.txt");

#[derive(Clone, Copy)]
pub struct ReadOptions {
    /// "Ignore Arabic text in PDFs"
    pub drop_arabic: bool,
    /// "Join lines into paragraphs"
    pub join: bool,
}

impl Default for ReadOptions {
    fn default() -> Self {
        ReadOptions { drop_arabic: true, join: true }
    }
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Thaana text taken straight from the PDF
    Text,
    /// the PDF's text has lost most vowel signs: read the page as a picture instead
    Damaged,
    /// no text, but a page-sized picture: a scan, to be read with "Read text"
    Scan,
    /// nothing to read
    Blank,
}

#[derive(Serialize, Debug, Clone)]
pub struct Page {
    pub kind: Kind,
    /// the finished text (only for `Kind::Text`)
    pub text: String,
    /// the rebuilt lines before word repair and paragraph joining
    pub raw: Vec<String>,
    pub glyphs: usize,
    pub images: usize,
}

pub struct Reader {
    repairer: Repairer,
}

impl Default for Reader {
    fn default() -> Self {
        Self::new()
    }
}

impl Reader {
    pub fn new() -> Self {
        Reader { repairer: Repairer::new(WORDS) }
    }

    /// Reads every page of a PDF, deciding for each what it is (same rules as the app).
    pub fn read(&self, pdf_bytes: &[u8], opts: ReadOptions) -> Result<Vec<Page>, String> {
        let pages = pdf::read_pages(pdf_bytes)?;
        Ok(pages
            .iter()
            .map(|p| {
                let raw = layout::rebuild(p, Options { drop_arabic: opts.drop_arabic });
                let joined = raw.join("\n");
                let (kind, text) = if text::has_thaana(&joined) && !text::text_looks_complete(&joined) {
                    (Kind::Damaged, String::new())
                } else if text::has_thaana(&joined) {
                    let fixed = self.repairer.fix_text(&joined);
                    (Kind::Text, text::join_pdf_lines(&fixed.split('\n').collect::<Vec<_>>(), opts.join))
                } else if text::looks_scanned(&p.images, p.page_area) {
                    (Kind::Scan, String::new())
                } else {
                    (Kind::Blank, String::new())
                };
                Page { kind, text, raw, glyphs: p.glyphs.len(), images: p.images.len() }
            })
            .collect())
    }
}
