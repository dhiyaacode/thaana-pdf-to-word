//! Prints what the reader gets from PDFs, as text or as JSON (test/ocr-accuracy.mjs uses that).
//!   thaana-pdf [--json] [--keep-arabic] [--no-join] file.pdf ...
//!   thaana-pdf --glyphs N file.pdf    (raw glyphs of page N)
//!   thaana-pdf --scan file.pdf|picture [--no-columns]   (how pages would be cut into lines)
use thaana_core::{ReadOptions, Reader};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| args.iter().any(|a| a == f);
    let opts = ReadOptions { drop_arabic: !flag("--keep-arabic"), join: !flag("--no-join") };
    // --glyphs N: print the raw glyphs of page N (1-based) as JSON, for debugging
    if let Some(i) = args.iter().position(|a| a == "--glyphs") {
        let n: usize = args[i + 1].parse().unwrap();
        let file = args.iter().skip(i + 2).find(|a| !a.starts_with("--")).unwrap();
        let pages = thaana_core::pdf::read_pages(&std::fs::read(file).unwrap()).unwrap();
        println!("{}", serde_json::to_string(&pages[n - 1].glyphs).unwrap());
        return;
    }
    // --scan file.pdf|picture: how each page would be cut into text lines for the reader
    if let Some(i) = args.iter().position(|a| a == "--scan") {
        let file = &args[i + 1];
        let ext = std::path::Path::new(file).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
        let pages = Reader::new().open(std::fs::read(file).unwrap(), &ext, ReadOptions::default(), false).unwrap();
        for (n, p) in pages.iter().enumerate() {
            let Some(g) = p.source.as_ref().and_then(|s| s.for_reading()) else { println!("page {}: cannot be drawn", n + 1); continue };
            let t = std::time::Instant::now();
            let prep = thaana_core::scan::prepare(g, 0, true, !flag("--no-columns"));
            println!("page {}: {} lines, spacing {} px, skew {:.1}, {} ({} ms)", n + 1, prep.lines.len(), prep.pitch, prep.skew,
                if prep.blk.is_some() { "read as columns" } else { "one column" }, t.elapsed().as_millis());
        }
        return;
    }
    let reader = Reader::new();
    let mut all = Vec::new();
    for file in args.iter().filter(|a| !a.starts_with("--")) {
        let result = std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| reader.read(&b, opts));
        if flag("--json") {
            all.push(match result {
                Ok(pages) => serde_json::json!({ "file": file, "pages": pages }),
                Err(e) => serde_json::json!({ "file": file, "error": e }),
            });
            continue;
        }
        match result {
            Ok(pages) => {
                for (i, p) in pages.iter().enumerate() {
                    println!("--- {file} page {}: {:?} ({} glyphs, {} images)", i + 1, p.kind, p.glyphs, p.images);
                    for l in p.text.lines() {
                        println!("  | {}", l.replace('\t', " \u{21E5} "));
                    }
                }
            }
            Err(e) => eprintln!("{file}: {e}"),
        }
    }
    if flag("--json") {
        println!("{}", serde_json::to_string(&all).unwrap());
    }
}
