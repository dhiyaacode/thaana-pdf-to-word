//! Checks on the test PDFs and pictures made by test/make-fixtures.ps1 and test/make-fixtures.py
//! (Windows, with Word and Edge). Each test is skipped, with a note, when its file is missing.
//!   cargo test --manifest-path crates/thaana-core/Cargo.toml

use std::path::PathBuf;
use thaana_core::{pages, scan, Kind, ReadOptions, Reader};

fn fixture(name: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures").join(name);
    match std::fs::read(&p) {
        Ok(b) => Some(b),
        Err(_) => {
            eprintln!("skipped: {} is missing (run test/make-fixtures.*)", p.display());
            None
        }
    }
}

struct Spec {
    paragraphs: Vec<String>,
    table: Vec<Vec<String>>,
    after: String,
}

fn spec() -> Spec {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let strs = |a: &serde_json::Value| a.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect::<Vec<_>>();
    Spec { paragraphs: strs(&v["paragraphs"]), table: v["table"].as_array().unwrap().iter().map(strs).collect(), after: v["after"].as_str().unwrap().into() }
}

#[test]
fn typed_pdfs_every_word_and_table_row() {
    let reader = Reader::new();
    let spec = spec();
    for name in ["typed.pdf", "typed-noborders.pdf"] {
        let Some(bytes) = fixture(name) else { continue };
        let pages = reader.read(&bytes, ReadOptions::default()).unwrap();
        assert_eq!(pages[0].kind, Kind::Text, "{name}");
        let text = &pages[0].text;
        let lines: Vec<&str> = text.split('\n').collect();
        for p in &spec.paragraphs {
            assert!(text.contains(p.as_str()), "{name}: missing {p}\n--- got:\n{text}");
        }
        assert!(lines.contains(&spec.after.as_str()), "{name}: missing {}", spec.after);
        for row in &spec.table {
            assert!(lines.contains(&row.join("\t").as_str()), "{name}: missing row {row:?}\n--- got:\n{text}");
        }
    }
}

#[test]
fn mixed_pdf_typed_page_then_scan() {
    let Some(bytes) = fixture("mixed.pdf") else { return };
    let pages = Reader::new().read(&bytes, ReadOptions::default()).unwrap();
    assert_eq!(pages.iter().map(|p| p.kind).collect::<Vec<_>>(), vec![Kind::Text, Kind::Scan]);
}

#[test]
fn form_xobjects_land_in_the_right_place() {
    let Some(bytes) = fixture("form.pdf") else { return };
    let spec = spec();
    let pages = Reader::new().read(&bytes, ReadOptions::default()).unwrap();
    let raw = &pages[0].raw;
    // the two half-size copies sit side by side: each table row twice, each list line twice
    for row in &spec.table {
        assert_eq!(raw.iter().filter(|l| **l == row.join("\t")).count(), 2, "{raw:#?}");
    }
    assert!(raw.contains(&format!("{0}\t{0}", spec.paragraphs[1])), "{raw:#?}");
}

#[test]
fn opening_draws_thumbnails_and_keeps_pages_readable() {
    let Some(bytes) = fixture("mixed.pdf") else { return };
    let pages = Reader::new().open(bytes, ".pdf", ReadOptions::default(), true).unwrap();
    assert_eq!(pages.len(), 2);
    for p in &pages {
        let jpg = p.thumb.as_ref().expect("thumbnail");
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
    }
    // the scanned page can be drawn for reading at the reader's size
    let g = pages[1].source.as_ref().unwrap().for_reading().unwrap();
    assert!((2400..=3600).contains(&g.w.max(g.h)), "{}x{}", g.w, g.h);
    assert!(g.px.iter().any(|&v| v < 100.0), "the scanned page should have ink");
    // the scan inside the PDF is test/fixtures/page.png: the same 12 lines must be found
    // (needs smooth enlargement of the 150 dpi picture)
    assert_eq!(scan::prepare(g, 0, true, true).lines.len(), 12);
}

#[test]
fn draws_a_typed_page() {
    let Some(bytes) = fixture("typed.pdf") else { return };
    let pdf = pages::PdfPages::open(std::sync::Arc::new(bytes)).unwrap();
    let (rgba, w, h) = pdf.draw(0, 1.0).unwrap();
    assert_eq!(rgba.len(), w * h * 4);
    let dark = rgba.chunks(4).filter(|p| p[0] < 100).count();
    assert!(dark > 1000, "the page should show text and table borders ({dark} dark pixels)");
}

fn read_picture(name: &str) -> Option<scan::Prepared> {
    let bytes = fixture(name)?;
    let pages = Reader::new().open(bytes, ".png", ReadOptions::default(), true).unwrap();
    Some(scan::prepare(pages[0].source.as_ref().unwrap().for_reading().unwrap(), 0, true, true))
}

#[test]
fn scan_one_column_page_finds_each_line() {
    // test/fixtures/page.png: 12 lines of text in one column
    let Some(p) = read_picture("page.png") else { return };
    assert_eq!(p.lines.len(), 12);
    assert!(p.blk.is_none(), "a one-column page is not read as newspaper columns");
}

#[test]
fn scan_newspaper_page_reads_columns() {
    // test/fixtures/columns.png: a heading and two columns of 15 and 14 lines
    let Some(p) = read_picture("columns.png") else { return };
    assert!(p.blk.is_some(), "two columns side by side should be read column by column");
    assert!((28..=32).contains(&p.lines.len()), "{} lines", p.lines.len());
}
