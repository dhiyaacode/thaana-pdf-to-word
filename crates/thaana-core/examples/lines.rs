//! Saves the line images a page is cut into (for checking line finding by eye).
//!   cargo run --release --example lines -- file.pdf|picture page out_dir
use thaana_core::{scan, ReadOptions, Reader};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ext = std::path::Path::new(&a[1]).extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let pages = Reader::new().open(std::fs::read(&a[1]).unwrap(), &ext, ReadOptions::default(), false).unwrap();
    let g = pages[a[2].parse::<usize>().unwrap() - 1].source.as_ref().unwrap().for_reading().unwrap();
    println!("page image {}x{}", g.w, g.h);
    let p = scan::prepare(g, 0, true, true);
    std::fs::create_dir_all(&a[3]).unwrap();
    for (i, (png, b)) in p.lines.iter().zip(&p.boxes).enumerate() {
        std::fs::write(format!("{}/{:03}.png", a[3], i), png).unwrap();
        println!("line {i}: top {} bottom {} height {}", b.t, b.b, b.b - b.t);
    }
}
