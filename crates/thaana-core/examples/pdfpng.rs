//! Draws the pages of a PDF to PNG files (for checking by eye).   cargo run --release --example pdfpng -- file.pdf out_dir
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let pdf = thaana_core::pages::PdfPages::open(std::sync::Arc::new(std::fs::read(&a[1]).unwrap())).unwrap();
    std::fs::create_dir_all(&a[2]).unwrap();
    for i in 0..pdf.count() {
        let (w, _) = pdf.size(i).unwrap();
        let (rgba, w, h) = pdf.draw(i, 900.0 / w).unwrap();
        let g = thaana_core::scan::Gray::from_rgba(&rgba, w, h);
        std::fs::write(format!("{}/page{}.png", a[2], i + 1), g.png()).unwrap();
    }
}
