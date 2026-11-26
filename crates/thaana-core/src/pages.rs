//! Opening documents and drawing their pages: PDFs with hayro (a PDF renderer in pure Rust),
//! pictures (PNG, JPEG, TIFF, BMP, WebP, GIF) with the `image` crate. This replaces PDF.js and
//! the browser's image decoding that the interface used before.

use crate::scan::Gray;
use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};
use std::sync::Arc;

/// Width at which a PDF page is drawn for reading (like the browser version: 2600 px, at most
/// 4 times its size in points). `prepare` then brings it into the size range the reader likes.
const READ_WIDTH: f32 = 2600.0;
/// Long side of the page pictures shown in the page list.
pub const THUMB_SIDE: u32 = 420;

/// An opened PDF, ready to draw any page. Can be shared between threads.
pub struct PdfPages {
    pdf: Pdf,
}

impl PdfPages {
    pub fn open(bytes: Arc<Vec<u8>>) -> Result<Self, String> {
        let pdf = Pdf::new(bytes.to_vec()).map_err(|e| format!("cannot draw this PDF: {e:?}"))?;
        Ok(PdfPages { pdf })
    }

    pub fn count(&self) -> usize {
        self.pdf.pages().len()
    }

    /// Size of a page in points (after its /Rotate).
    pub fn size(&self, index: usize) -> Option<(f32, f32)> {
        self.pdf.pages().get(index).map(|p| p.render_dimensions())
    }

    /// Draws a page on white at `scale` (1.0 = 72 dpi). Returns RGBA bytes, width, height.
    pub fn draw(&self, index: usize, scale: f32) -> Option<(Vec<u8>, usize, usize)> {
        let pages = self.pdf.pages();
        let page = pages.get(index)?;
        let cache = RenderCache::new();
        // Smooth image scaling (as the browser did): scanned pages are pictures stored at 150-300 dpi
        // and drawn larger; blocky enlargement makes thin strokes and short lines harder to find.
        let settings = RenderSettings { force_image_interpolation: true, ..RenderSettings::default() };
        let px = render(page, &cache, &InterpreterSettings::default(), &settings, &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
        let (w, h) = (px.width() as usize, px.height() as usize);
        // drawn on opaque white, so the premultiplied pixels are plain RGBA
        Some((px.data_as_u8_slice().to_vec(), w, h))
    }

    /// The page as a grey picture at the size used for reading scans.
    pub fn draw_for_reading(&self, index: usize) -> Option<Gray> {
        let (w, h) = self.size(index)?;
        let mut scale = (READ_WIDTH / w).min(4.0);
        // draw straight at the size prepare() would scale it to, instead of resizing twice
        scale *= crate::scan::reading_scale((w * scale).max(h * scale) as f64) as f32;
        let (rgba, w, h) = self.draw(index, scale)?;
        Some(Gray::from_rgba(&rgba, w, h))
    }

    /// A small JPEG of the page for the page list.
    pub fn thumbnail(&self, index: usize) -> Option<Vec<u8>> {
        let (w, h) = self.size(index)?;
        let (rgba, w, h) = self.draw(index, THUMB_SIDE as f32 / w.max(h))?;
        thumbnail_jpeg(&rgba, w, h)
    }
}

/// A picture file decoded to RGBA (any orientation tag is not applied; use the rotate button).
pub fn decode_picture(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
    let img = image::load_from_memory(bytes).map_err(|e| format!("cannot open this picture: {e}"))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    Ok((rgba.into_raw(), w, h))
}

/// A small JPEG (long side THUMB_SIDE) of an RGBA picture, laid on white.
pub fn thumbnail_jpeg(rgba: &[u8], w: usize, h: usize) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec())?;
    let s = THUMB_SIDE as f32 / w.max(h) as f32;
    let small = if s < 1.0 { image::imageops::thumbnail(&img, ((w as f32 * s) as u32).max(1), ((h as f32 * s) as u32).max(1)) } else { img };
    let rgb: image::RgbImage = image::ImageBuffer::from_fn(small.width(), small.height(), |x, y| {
        let p = small.get_pixel(x, y);
        let a = p[3] as u32;
        image::Rgb([0, 1, 2].map(|c| ((p[c] as u32 * a + 255 * (255 - a)) / 255) as u8))
    });
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 82).encode_image(&rgb).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_png_and_makes_a_thumbnail() {
        let g = Gray { w: 900, h: 600, px: vec![128.0; 900 * 600] };
        let (rgba, w, h) = decode_picture(&g.png()).unwrap();
        assert_eq!((w, h), (900, 600));
        assert_eq!(&rgba[..4], &[128, 128, 128, 255]);
        let jpg = thumbnail_jpeg(&rgba, w, h).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
    }
}
