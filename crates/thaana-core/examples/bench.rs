//! Times each scan clean-up step on one page image (8-bit grey PNG):
//!   cargo run --release --example bench -- page.png
use std::time::Instant;
use thaana_core::scan::*;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let dec = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(&path).unwrap()));
    let mut r = dec.read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    let (w0, h0) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = buf[..w0 * h0].iter().flat_map(|&v| [v, v, v, 255]).collect();
    // like the app: the PDF page is drawn at 2600 px wide
    let s = 2600.0 / w0 as f64;
    let g0 = Gray::from_rgba(&rgba, w0, h0).resize((w0 as f64 * s) as usize, (h0 as f64 * s) as usize);
    let (w, h) = (g0.w, g0.h);
    let rgba: Vec<u8> = g0.px.iter().flat_map(|&v| { let b = v as u8; [b, b, b, 255] }).collect();
    let t = Instant::now(); let lap = |name: &str| { println!("{name:>14}: {:>6.0} ms", t.elapsed().as_secs_f64() * 1000.0); };
    let mut g = Gray::from_rgba(&rgba, w, h); lap("gray");
    g = g.resize(w, h).rotate90(0); lap("resize/rotate");
    normalize(&mut g, true); lap("normalize");
    let thr = otsu(&g.px); lap("otsu");
    let skew = find_skew(&g, thr); lap("find_skew");
    let g = rotate_gray(g, skew); lap("rotate_gray");
    let (b, _) = find_blocks(&g.px, g.w, g.h, thr); lap("find_blocks");
    let p = prepare(&rgba, w, h, 0, true, true); lap("prepare (all)");
    println!("{}x{} skew {skew:.1}, {} blocks, {} lines", w, h, b.len(), p.lines.len());
}
