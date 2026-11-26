//! Prints what the Rust reader gets from PDFs, as JSON (used by test/parity.mjs), or as text.
//!   thaana-pdf [--json] [--keep-arabic] [--no-join] file.pdf ...
//!   thaana-pdf --glyphs N file.pdf    (raw glyphs of page N)
//!   thaana-pdf --scan-parity image.gray W H
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
    // --scan-parity image.gray W H: the pure scan steps on a raw 8-bit grey image (test/scan-parity.mjs)
    if let Some(i) = args.iter().position(|a| a == "--scan-parity") {
        use thaana_core::scan;
        let (w, h): (usize, usize) = (args[i + 2].parse().unwrap(), args[i + 3].parse().unwrap());
        let g: Vec<f32> = std::fs::read(&args[i + 1]).unwrap().iter().map(|&b| b as f32).collect();
        let thr = scan::otsu(&g);
        let (boxes, pitch) = scan::find_lines(&g, w, h, thr, 0.0);
        let (blocks, letter) = scan::find_blocks(&g, w, h, thr);
        let ruled = scan::remove_ruling(&g, w, h, thr);
        let changed: Vec<usize> = (0..g.len()).filter(|&k| ruled[k] != g[k]).collect();
        println!("{}", serde_json::json!({ "otsu": thr, "pitch": pitch, "boxes": boxes, "blocks": blocks, "letter": letter,
            "ruling": { "changed": changed.len(), "sum": changed.iter().map(|&k| k as u64).sum::<u64>() } }));
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
