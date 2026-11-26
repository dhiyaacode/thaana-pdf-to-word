//! Thaana PDF to Word, the library: opens PDFs and pictures, gets correctly ordered Dhivehi text
//! (with tables) out of typed PDFs, draws pages, cleans up scans and cuts them into text lines for
//! the reader, and turns what the reader recognised into finished text.

pub mod layout;
pub mod ocr;
pub mod pages;
pub mod pdf;
pub mod repair;
pub mod scan;
pub mod text;

use std::sync::Arc;

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

    /// The word repairer (the same one is used for the text of scans).
    pub fn repairer(&self) -> &Repairer {
        &self.repairer
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

/// Where a page of an opened file comes from, so it can be drawn for reading at any time.
#[derive(Clone)]
pub enum Source {
    Pdf(Arc<pages::PdfPages>, usize),
    Picture(Arc<scan::Gray>),
}

impl Source {
    /// The page as a grey picture, at the size used for reading scans.
    pub fn for_reading(&self) -> Option<scan::Gray> {
        match self {
            Source::Pdf(p, i) => p.draw_for_reading(*i),
            Source::Picture(g) => Some((**g).clone()),
        }
    }
}

/// One page of an opened file.
pub struct Opened {
    pub kind: Kind,
    /// the finished text (only for `Kind::Text`)
    pub text: String,
    /// small JPEG for the page list (None if the page cannot be drawn)
    pub thumb: Option<Vec<u8>>,
    /// None if the page cannot be drawn (then it cannot be read as a picture either)
    pub source: Option<Source>,
}

impl Reader {
    /// Opens a PDF or a picture file (`ext` like ".pdf", ".png"). With `use_text` off, the text
    /// inside PDFs is ignored and every page is read as a picture.
    pub fn open(&self, bytes: Vec<u8>, ext: &str, opts: ReadOptions, use_text: bool) -> Result<Vec<Opened>, String> {
        if !ext.eq_ignore_ascii_case(".pdf") {
            let (rgba, w, h) = pages::decode_picture(&bytes)?;
            let thumb = pages::thumbnail_jpeg(&rgba, w, h);
            let gray = Arc::new(scan::Gray::from_rgba(&rgba, w, h));
            return Ok(vec![Opened { kind: Kind::Scan, text: String::new(), thumb, source: Some(Source::Picture(gray)) }]);
        }
        let bytes = Arc::new(bytes);
        let drawn = pages::PdfPages::open(bytes.clone()).ok().map(Arc::new);
        let read = if use_text { self.read(&bytes, opts) } else { Err(String::new()) };
        let count = match (&drawn, &read) {
            (Some(d), _) => d.count(),
            (None, Ok(r)) => r.len(),
            (None, Err(e)) => return Err(if e.is_empty() { "cannot open this PDF".into() } else { e.clone() }),
        };
        // the text of each page, when it was read and matches the drawn pages
        let texts: Option<Vec<Page>> = read.ok().filter(|r| r.len() == count);
        let thumbs = match &drawn {
            Some(d) => parallel(count, |i| d.thumbnail(i)),
            None => vec![None; count],
        };
        Ok(thumbs
            .into_iter()
            .enumerate()
            .map(|(i, thumb)| {
                let source = drawn.as_ref().map(|d| Source::Pdf(d.clone(), i));
                match &texts {
                    Some(t) => Opened { kind: t[i].kind, text: t[i].text.clone(), thumb, source },
                    // no usable text layer: every page is read as a picture
                    None => Opened { kind: Kind::Scan, text: String::new(), thumb, source },
                }
            })
            .collect())
    }
}

/// Runs `f(0..n)` on all cores, keeping the results in order.
pub fn parallel<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map(|t| t.get()).unwrap_or(2).min(n.max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut out: Vec<(usize, T)> = std::thread::scope(|sc| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                sc.spawn(|| {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if i >= n {
                            break mine;
                        }
                        mine.push((i, f(i)));
                    }
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().expect("worker thread")).collect()
    });
    out.sort_by_key(|(i, _)| *i);
    out.into_iter().map(|(_, t)| t).collect()
}
