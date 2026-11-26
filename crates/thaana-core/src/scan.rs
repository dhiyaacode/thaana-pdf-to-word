//! Scan clean-up and Thaana-aware line finding (port of renderer/segment.js, columns.js and
//! `prepare()` in renderer/renderer.js).
//!
//! Tesseract's own page layout splits fili (vowel signs) into separate "lines", so the app finds
//! text lines itself and sends each one to the recogniser. The pure-arithmetic parts (threshold,
//! line finding, column blocks, ruled-line removal) give exactly the JavaScript results; the steps
//! the browser did with canvas operations (blur, resizing, rotation) are done here natively.

use crate::text::js_round;
use serde::Serialize;

/// A grey image, 0 (black) .. 255 (white), one f32 per pixel like the JavaScript Float32Array.
#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub px: Vec<f32>,
}

impl Gray {
    /// From RGBA bytes (as canvas getImageData gives them), laid on white like the canvas was.
    pub fn from_rgba(rgba: &[u8], w: usize, h: usize) -> Gray {
        let px = rgba
            .chunks_exact(4)
            .take(w * h)
            .map(|p| {
                let a = p[3] as f32 / 255.0;
                let on_white = |c: u8| c as f32 * a + 255.0 * (1.0 - a);
                0.299 * on_white(p[0]) + 0.587 * on_white(p[1]) + 0.114 * on_white(p[2])
            })
            .collect();
        Gray { w, h, px }
    }

    fn at(&self, x: isize, y: isize) -> f32 {
        if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize { 255.0 } else { self.px[y as usize * self.w + x as usize] }
    }

    /// Smooth resize: area averaging when shrinking, bilinear when enlarging.
    pub fn resize(&self, nw: usize, nh: usize) -> Gray {
        let (nw, nh) = (nw.max(1), nh.max(1));
        if nw == self.w && nh == self.h {
            return self.clone();
        }
        let mut out = vec![0f32; nw * nh];
        let (sx, sy) = (self.w as f64 / nw as f64, self.h as f64 / nh as f64);
        if sx >= 1.0 && sy >= 1.0 {
            for y in 0..nh {
                let (y0, y1) = (y as f64 * sy, (y + 1) as f64 * sy);
                for x in 0..nw {
                    let (x0, x1) = (x as f64 * sx, (x + 1) as f64 * sx);
                    let (mut sum, mut area) = (0f64, 0f64);
                    for yy in y0.floor() as usize..(y1.ceil() as usize).min(self.h) {
                        let fy = (y1.min(yy as f64 + 1.0) - y0.max(yy as f64)).max(0.0);
                        for xx in x0.floor() as usize..(x1.ceil() as usize).min(self.w) {
                            let f = fy * (x1.min(xx as f64 + 1.0) - x0.max(xx as f64)).max(0.0);
                            sum += f * self.px[yy * self.w + xx] as f64;
                            area += f;
                        }
                    }
                    out[y * nw + x] = if area > 0.0 { (sum / area) as f32 } else { 255.0 };
                }
            }
        } else {
            for y in 0..nh {
                let fy = ((y as f64 + 0.5) * sy - 0.5).clamp(0.0, (self.h - 1) as f64);
                for x in 0..nw {
                    let fx = ((x as f64 + 0.5) * sx - 0.5).clamp(0.0, (self.w - 1) as f64);
                    out[y * nw + x] = self.bilinear(fx, fy, false);
                }
            }
        }
        Gray { w: nw, h: nh, px: out }
    }

    fn bilinear(&self, fx: f64, fy: f64, white_outside: bool) -> f32 {
        let (x0, y0) = (fx.floor() as isize, fy.floor() as isize);
        let (ax, ay) = ((fx - x0 as f64) as f32, (fy - y0 as f64) as f32);
        let get = |x: isize, y: isize| {
            if white_outside { self.at(x, y) } else { self.px[(y.clamp(0, self.h as isize - 1) as usize) * self.w + x.clamp(0, self.w as isize - 1) as usize] }
        };
        let top = get(x0, y0) * (1.0 - ax) + get(x0 + 1, y0) * ax;
        let bot = get(x0, y0 + 1) * (1.0 - ax) + get(x0 + 1, y0 + 1) * ax;
        top * (1.0 - ay) + bot * ay
    }

    /// Quarter turns (the page's rotate button).
    pub fn rotate90(&self, deg: i32) -> Gray {
        let (w, h) = (self.w, self.h);
        match deg.rem_euclid(360) {
            90 => Gray { w: h, h: w, px: (0..w * h).map(|i| { let (x, y) = (i % h, i / h); self.px[(h - 1 - x) * w + y] }).collect() },
            180 => Gray { w, h, px: self.px.iter().rev().copied().collect() },
            270 => Gray { w: h, h: w, px: (0..w * h).map(|i| { let (x, y) = (i % h, i / h); self.px[x * w + (w - 1 - y)] }).collect() },
            _ => self.clone(),
        }
    }

    /// Small-angle rotation about the centre (like canvas rotate(deg)), white outside the page.
    pub fn rotate(&self, deg: f64) -> Gray {
        let (s, c) = (deg.to_radians().sin(), deg.to_radians().cos());
        let (cx, cy) = (self.w as f64 / 2.0, self.h as f64 / 2.0);
        let (w, h) = (self.w, self.h);
        let mut px = vec![255f32; w * h];
        for y in 0..h {
            let dy = y as f64 + 0.5 - cy;
            // source position of this row's first pixel; it moves by (c, -s) per pixel
            let (mut sx, mut sy) = (cx + c * (0.5 - cx) + s * dy - 0.5, cy - s * (0.5 - cx) + c * dy - 0.5);
            for x in 0..w {
                let (x0, y0) = (sx.floor(), sy.floor());
                px[y * w + x] = if x0 >= 0.0 && y0 >= 0.0 && (x0 as usize) + 1 < w && (y0 as usize) + 1 < h {
                    let (xi, yi, ax, ay) = (x0 as usize, y0 as usize, (sx - x0) as f32, (sy - y0) as f32);
                    let i = yi * w + xi;
                    let top = self.px[i] * (1.0 - ax) + self.px[i + 1] * ax;
                    let bot = self.px[i + w] * (1.0 - ax) + self.px[i + w + 1] * ax;
                    top * (1.0 - ay) + bot * ay
                } else {
                    self.bilinear(sx, sy, true)
                };
                sx += c;
                sy -= s;
            }
        }
        Gray { w, h, px }
    }

    /// 8-bit greyscale PNG.
    pub fn png(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut buf, self.w as u32, self.h as u32);
            enc.set_color(png::ColorType::Grayscale);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::Fast);
            let mut wr = enc.write_header().expect("png header");
            let data: Vec<u8> = self.px.iter().map(|&v| v.round().clamp(0.0, 255.0) as u8).collect();
            wr.write_image_data(&data).expect("png data");
        }
        buf
    }

    pub fn crop(&self, b: &LineBox) -> Gray {
        let (cw, ch) = (b.r - b.l, b.b - b.t);
        let mut px = Vec::with_capacity(cw * ch);
        for y in 0..ch {
            px.extend_from_slice(&self.px[(b.t + y) * self.w + b.l..(b.t + y) * self.w + b.l + cw]);
        }
        Gray { w: cw, h: ch, px }
    }
}

/// Gaussian-like blur (the CSS blur the browser version used). For a large blur the image is
/// reduced first: nothing finer than the blur survives it anyway, and it is much faster.
fn blur(g: &Gray, sigma: f64) -> Vec<f32> {
    let f = (sigma / 4.0).floor().max(1.0) as usize;
    if f == 1 {
        return box_blur3(g, sigma);
    }
    let small = g.resize(g.w.div_ceil(f), g.h.div_ceil(f));
    let b = Gray { w: small.w, h: small.h, px: box_blur3(&small, sigma / f as f64) };
    b.resize(g.w, g.h).px
}

/// Box blur used three times: close to a Gaussian.
fn box_blur3(g: &Gray, sigma: f64) -> Vec<f32> {
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    let (w, h) = (g.w, g.h);
    let mut a: Vec<f32> = g.px.clone();
    let mut b = vec![0f32; w * h];
    let pass = |src: &[f32], dst: &mut [f32], len: usize, count: usize, idx: &dyn Fn(usize, usize) -> usize| {
        for line in 0..count {
            let mut sum = 0f64;
            let get = |i: isize| src[idx(line, i.clamp(0, len as isize - 1) as usize)] as f64;
            for i in -(r as isize)..=(r as isize) {
                sum += get(i);
            }
            for i in 0..len {
                dst[idx(line, i)] = (sum / (2 * r + 1) as f64) as f32;
                sum += get(i as isize + r as isize + 1) - get(i as isize - r as isize);
            }
        }
    };
    for _ in 0..3 {
        pass(&a, &mut b, w, h, &|line, i| line * w + i);
        pass(&b, &mut a, h, w, &|line, i| i * w + line);
    }
    a
}

/// Evens out shadows / yellowed paper, then stretches contrast (segment.js normalize()).
pub fn normalize(g: &mut Gray, enhance: bool) {
    if enhance {
        let bg = blur(g, js_round(g.w.max(g.h) as f64 / 60.0));
        for (v, b) in g.px.iter_mut().zip(bg) {
            *v = (*v / b.max(40.0) * 245.0).min(255.0);
        }
    }
    let n = g.px.len();
    let mut hist = [0u32; 256];
    for &v in &g.px {
        hist[(v as i32).clamp(0, 255) as usize] += 1;
    }
    let (mut lo, mut hi, mut acc) = (0usize, 255usize, 0f64);
    for v in 0..256 {
        acc += hist[v] as f64;
        if acc > n as f64 * 0.01 {
            lo = v;
            break;
        }
    }
    acc = 0.0;
    for v in (0..256).rev() {
        acc += hist[v] as f64;
        if acc > n as f64 * 0.01 {
            hi = v;
            break;
        }
    }
    let span = 30f32.max(hi as f32 - lo as f32);
    for v in g.px.iter_mut() {
        *v = ((*v - lo as f32) / span * 255.0).clamp(0.0, 255.0);
    }
}

/// Otsu's threshold (segment.js otsu()).
pub fn otsu(g: &[f32]) -> f32 {
    let mut h = [0f64; 256];
    for &v in g {
        h[(v as i32).clamp(0, 255) as usize] += 1.0;
    }
    let (mut w, mut m, mut mt, tot) = (0f64, 0f64, 0f64, g.len() as f64);
    for (i, &c) in h.iter().enumerate() {
        mt += i as f64 * c;
    }
    let (mut best, mut t) = (0f64, 128usize);
    for i in 0..255 {
        w += h[i];
        m += i as f64 * h[i];
        if w == 0.0 || w == tot {
            continue;
        }
        let (mb, mf) = (m / w, (mt - m) / (tot - w));
        let v = w * (tot - w) * (mb - mf).powi(2);
        if v > best {
            best = v;
            t = i;
        }
    }
    t as f32
}

/// The rotation (-4..+4 degrees) that makes text lines most horizontal (segment.js findSkew()).
pub fn find_skew(g: &Gray, thr: f32) -> f64 {
    let s = 1000.0 / g.w.max(g.h) as f64;
    let bin = Gray { w: g.w, h: g.h, px: g.px.iter().map(|&v| if v < thr { 0.0 } else { 255.0 }).collect() };
    let small = bin.resize((js_round(g.w as f64 * s) as usize).max(1), (js_round(g.h as f64 * s) as usize).max(1));
    // Rotating the picture by a and summing (255 - pixel) along each row is the same as moving each
    // inked pixel to its rotated row: project only the ink (a few % of the pixels).
    let (cx, cy) = (small.w as f64 / 2.0, small.h as f64 / 2.0);
    let ink: Vec<(f64, f64, f64)> = (0..small.px.len())
        .filter(|&i| small.px[i] < 255.0)
        .map(|i| ((i % small.w) as f64 + 0.5 - cx, (i / small.w) as f64 + 0.5 - cy, 255.0 - small.px[i] as f64))
        .collect();
    let mut rows = vec![0f64; small.h + 1];
    let (mut best, mut best_a) = (-1f64, 0f64);
    let mut a = -4f64;
    while a <= 4.001 {
        let (s, c) = (a.to_radians().sin(), a.to_radians().cos());
        rows.iter_mut().for_each(|r| *r = 0.0);
        for &(dx, dy, wt) in &ink {
            let ry = cy + s * dx + c * dy - 0.5; // row of the rotated pixel, shared between two rows
            if ry < 0.0 || ry >= (small.h - 1) as f64 {
                continue;
            }
            let (r0, fr) = (ry.floor() as usize, ry - ry.floor());
            rows[r0] += wt * (1.0 - fr);
            rows[r0 + 1] += wt * fr;
        }
        let v: f64 = rows.iter().map(|r| r * r).sum();
        if v > best {
            best = v;
            best_a = a;
        }
        a += 0.2;
    }
    best_a
}

pub fn rotate_gray(g: Gray, deg: f64) -> Gray {
    if deg.abs() < 0.05 { g } else { g.rotate(deg) }
}

// ---------------------------------------------------------------- line finding
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct LineBox {
    pub l: usize,
    pub t: usize,
    pub r: usize,
    pub b: usize,
}

/// `letter`: the typical letter height in pixels (from find_blocks), 0 if not known. Thaana lines,
/// with their vowel signs above and below, are 2.5 to 9 letter heights apart. Looking only there
/// keeps out the two false answers: the gap between the vowel-sign rows and the letters (under 2.5),
/// and repeats of the page's larger structure (over 9).
fn line_pitch(p: &[f64], letter: f64) -> f64 {
    let n = p.len();
    let mean = p.iter().sum::<f64>() / n as f64;
    let q: Vec<f64> = p.iter().map(|v| v - mean).collect();
    let mut a0: f64 = q.iter().map(|v| v * v).sum();
    if a0 == 0.0 {
        a0 = 1.0;
    }
    let ac = |lag: usize| -> f64 { (0..n.saturating_sub(lag)).map(|i| q[i] * q[i + lag]).sum::<f64>() / a0 };
    if letter > 0.0 {
        let lo = ((2.5 * letter).ceil() as usize).max(8);
        let hi = (n / 2).min((9.0 * letter).floor() as usize);
        if hi > lo {
            let mut acs = vec![0f64; hi + 1];
            let (mut best, mut pp) = (f64::NEG_INFINITY, lo);
            for lag in lo..=hi {
                acs[lag] = ac(lag);
                if acs[lag] > best {
                    best = acs[lag];
                    pp = lag;
                }
            }
            // The strongest repeat can be every 2nd or 3rd line: take the shortest one nearly as strong.
            for k in [3.0, 2.0] {
                let c = js_round(pp as f64 / k) as usize;
                if c < lo + 2 {
                    continue;
                }
                let (mut m, mut at) = (f64::NEG_INFINITY, c);
                for l in c - 2..=c + 2 {
                    if acs[l] > m {
                        m = acs[l];
                        at = l;
                    }
                }
                if m >= 0.8 * best {
                    return at as f64;
                }
            }
            return pp as f64;
        }
    }
    let mut dip = 8usize;
    while (dip as f64) < n as f64 / 2.0 && ac(dip) >= 0.0 {
        dip += 1;
    }
    let hi = (n / 2).min(dip * 6 + 20);
    let (mut best, mut pp) = (f64::NEG_INFINITY, 40usize);
    for lag in dip..hi {
        let v = ac(lag);
        if v > best {
            best = v;
            pp = lag;
        }
    }
    pp as f64
}

/// Text line boxes in reading order (top to bottom), plus the line pitch (segment.js findLines()).
/// `pitch_hint`: line spacing measured elsewhere (a block of one or two lines cannot show its own).
/// `letter`: typical letter height, to keep the measured spacing in a sensible range (see line_pitch).
pub fn find_lines(g: &[f32], w: usize, h: usize, thr: f32, pitch_hint: f64, letter: f64) -> (Vec<LineBox>, f64) {
    let p: Vec<f64> = (0..h).map(|y| g[y * w..(y + 1) * w].iter().filter(|&&v| v < thr).count() as f64).collect();
    let pp = if pitch_hint != 0.0 { pitch_hint } else { line_pitch(&p, letter) };
    let k = (js_round(pp * 0.45) as usize).max(3);
    let win: Vec<f64> = (0..k).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (if k > 1 { k - 1 } else { 1 }) as f64).cos()).collect();
    let mut ws: f64 = win.iter().sum();
    if ws == 0.0 {
        ws = 1.0;
    }
    let ps: Vec<f64> = (0..h)
        .map(|y| {
            let mut s = 0f64;
            for (i, wv) in win.iter().enumerate() {
                let yy = y as isize + i as isize - (k >> 1) as isize;
                if yy >= 0 && (yy as usize) < h {
                    s += p[yy as usize] * wv;
                }
            }
            s / ws
        })
        .collect();
    let max = ps.iter().fold(0f64, |a, &v| a.max(v));
    let mut cand: Vec<usize> = (1..h.saturating_sub(1)).filter(|&y| ps[y] >= ps[y - 1] && ps[y] > ps[y + 1] && ps[y] > 0.12 * max).collect();
    cand.sort_by(|&a, &b| ps[b].partial_cmp(&ps[a]).unwrap());
    let mut peaks: Vec<usize> = Vec::new();
    for y in cand {
        if peaks.iter().all(|&q| (y as f64 - q as f64).abs() >= 0.6 * pp) {
            peaks.push(y);
        }
    }
    peaks.sort();
    let cuts: Vec<usize> = (0..peaks.len().saturating_sub(1))
        .map(|i| {
            let mut m = peaks[i];
            for y in peaks[i]..peaks[i + 1] {
                if ps[y] < ps[m] {
                    m = y;
                }
            }
            m
        })
        .collect();
    let mut boxes = Vec::new();
    for (i, &c) in peaks.iter().enumerate() {
        let t = if i > 0 { cuts[i - 1] } else { js_round(c as f64 - 0.6 * pp).max(0.0) as usize };
        let b = if i < cuts.len() { cuts[i] } else { (js_round(c as f64 + 0.6 * pp) as usize).min(h) };
        let (mut l, mut r, mut ink) = (w as isize, -1isize, 0f64);
        for x in 0..w {
            let col = (t..b).filter(|&y| g[y * w + x] < thr).count();
            if col > 1 {
                if (x as isize) < l {
                    l = x as isize;
                }
                r = x as isize;
            }
            ink += col as f64;
        }
        if r < 0 || ink < 0.02 * pp * pp {
            continue;
        }
        boxes.push(LineBox { l: (l - 20).max(0) as usize, t, r: ((r + 20) as usize).min(w), b });
    }
    (boxes, pp)
}

/// Crops one line and resizes it so the line spacing is ~45 px, the text size the model reads best.
pub const TARGET_PITCH: f64 = 45.0;
pub fn crop_to_png(g: &Gray, b: &LineBox, pitch: f64) -> Vec<u8> {
    let c = g.crop(b);
    let s = if pitch != 0.0 { TARGET_PITCH / pitch } else { 1.0 };
    if (s - 1.0).abs() < 0.03 {
        return c.png();
    }
    c.resize((js_round(c.w as f64 * s) as usize).max(1), (js_round(c.h as f64 * s) as usize).max(1)).png()
}

// ---------------------------------------------------------------- column blocks (columns.js)
fn runs(flags: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut s: Option<usize> = None;
    for (i, &f) in flags.iter().enumerate() {
        if f != 0 {
            s.get_or_insert(i);
        } else if let Some(st) = s.take() {
            out.push((st, i));
        }
    }
    if let Some(st) = s {
        out.push((st, flags.len()));
    }
    out
}

fn dilate(ink: &[u8], w: usize, h: usize, kx: i64, ky: i64) -> Vec<u8> {
    let (rx, ry) = (kx >> 1, ky >> 1);
    let mut tmp = vec![0u8; w * h];
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        let o = y * w;
        let mut last: i64 = -1_000_000_000;
        let mut pos = vec![-1_000_000_000i64; w];
        for x in 0..w {
            if ink[o + x] != 0 {
                last = x as i64;
            }
            pos[x] = last;
        }
        let mut next: i64 = 1_000_000_000;
        for x in (0..w).rev() {
            if ink[o + x] != 0 {
                next = x as i64;
            }
            if x as i64 - pos[x] <= rx || next - x as i64 <= rx {
                tmp[o + x] = 1;
            }
        }
    }
    for x in 0..w {
        let mut last: i64 = -1_000_000_000;
        let mut pos = vec![0i64; h];
        for y in 0..h {
            if tmp[y * w + x] != 0 {
                last = y as i64;
            }
            pos[y] = last;
        }
        let mut next: i64 = 1_000_000_000;
        for y in (0..h).rev() {
            if tmp[y * w + x] != 0 {
                next = y as i64;
            }
            if y as i64 - pos[y] <= ry || next - y as i64 <= ry {
                out[y * w + x] = 1;
            }
        }
    }
    out
}

/// Connected components (8-neighbour) of the ink: boxes [x, y, w, h] of those with >= 6 pixels.
fn components(ink: &[u8], w: usize, h: usize) -> Vec<[usize; 4]> {
    let mut seen = vec![false; w * h];
    let mut stack: Vec<usize> = Vec::new();
    let mut res = Vec::new();
    for i in 0..w * h {
        if ink[i] == 0 || seen[i] {
            continue;
        }
        let (mut n, mut x0, mut y0, mut x1, mut y1) = (0usize, w, h, 0usize, 0usize);
        stack.push(i);
        seen[i] = true;
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            n += 1;
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
            for dy in -1isize..=1 {
                let yy = y as isize + dy;
                if yy < 0 || yy >= h as isize {
                    continue;
                }
                for dx in -1isize..=1 {
                    let xx = x as isize + dx;
                    if xx < 0 || xx >= w as isize {
                        continue;
                    }
                    let q = yy as usize * w + xx as usize;
                    if ink[q] != 0 && !seen[q] {
                        seen[q] = true;
                        stack.push(q);
                    }
                }
            }
        }
        if n >= 6 {
            res.push([x0, y0, x1 - x0 + 1, y1 - y0 + 1]);
        }
    }
    res
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Block {
    pub l: usize,
    pub t: usize,
    pub r: usize,
    pub b: usize,
    pub kind: &'static str,
    pub lines: usize,
}

/// Column / block analysis for newspapers and magazines (XY-cut), right-to-left reading order.
/// Returns the text and heading blocks in reading order (columns.js findBlocks()).
/// Also returns the typical letter height in pixels (0 when the page has too little ink to tell).
pub fn find_blocks(g: &[f32], w: usize, h: usize, thr: f32) -> (Vec<Block>, f64) {
    let mut raw: Vec<u8> = g.iter().map(|&v| (v < thr) as u8).collect();
    // Screenshots of a page on a dark background: ignore everything outside the white page.
    {
        let (mut cw, mut rw) = (vec![0f32; w], vec![0f32; h]);
        for y in 0..h {
            for x in 0..w {
                if g[y * w + x] > 235.0 {
                    cw[x] += 1.0;
                    rw[y] += 1.0;
                }
            }
        }
        let (mut x0, mut x1, mut y0, mut y1) = (-1isize, -1isize, -1isize, -1isize);
        for x in 0..w {
            if cw[x] / h as f32 > 0.35 {
                if x0 < 0 {
                    x0 = x as isize;
                }
                x1 = x as isize;
            }
        }
        for y in 0..h {
            if rw[y] / w as f32 > 0.35 {
                if y0 < 0 {
                    y0 = y as isize;
                }
                y1 = y as isize;
            }
        }
        let (fw, fh) = (w as f64, h as f64);
        if x0 >= 0 && y0 >= 0 && (((x1 - x0) as f64) < 0.9 * fw || ((y1 - y0) as f64) < 0.9 * fh) && ((x1 - x0) as f64) > 0.2 * fw && ((y1 - y0) as f64) > 0.2 * fh {
            for y in 0..h {
                for x in 0..w {
                    let (xi, yi) = (x as isize, y as isize);
                    if xi < x0 || xi > x1 || yi < y0 || yi > y1 {
                        raw[y * w + x] = 0;
                    }
                }
            }
        }
    }
    let cc = components(&raw, w, h);
    if cc.len() < 20 {
        return (vec![Block { l: 0, t: 0, r: w, b: h, kind: "text", lines: 0 }], 0.0);
    }
    // about half of all Thaana ink blobs are vowel signs, so take the upper quarter: a letter
    let medh = |x0: usize, y0: usize, x1: usize, y1: usize| -> f64 {
        let mut hs: Vec<usize> = cc.iter().filter(|c| c[0] >= x0 && c[0] + c[2] <= x1 && c[1] >= y0 && c[1] + c[3] <= y1).map(|c| c[3]).collect();
        if hs.is_empty() {
            return 15.0;
        }
        hs.sort();
        hs[(hs.len() as f64 * 0.75).floor() as usize] as f64
    };
    // work at the size where a typical letter is ~15 px, so thresholds match the prototype
    let letter = medh(0, 0, w, h);
    let k = (letter / 15.0).max(1.0);
    let kx = (js_round(9.0 * k) as i64).max(3) | 1;
    let ky = (js_round(3.0 * k) as i64).max(1) | 1;
    let ink = dilate(&raw, w, h, kx, ky);

    let mut leaves: Vec<[usize; 4]> = Vec::new();
    fn cut(ink: &[u8], w: usize, k: f64, medh: &dyn Fn(usize, usize, usize, usize) -> f64, leaves: &mut Vec<[usize; 4]>, x0: usize, y0: usize, x1: usize, y1: usize) {
        // trim to ink
        let (mut rx0, mut rx1, mut ry0, mut ry1, mut cnt) = (x1, x0, y1, y0, 0f64);
        for y in y0..y1 {
            for x in x0..x1 {
                if ink[y * w + x] != 0 {
                    cnt += 1.0;
                    rx0 = rx0.min(x);
                    rx1 = rx1.max(x);
                    ry0 = ry0.min(y);
                    ry1 = ry1.max(y);
                }
            }
        }
        if cnt < 40.0 * k * k {
            return;
        }
        let (x0, x1, y0, y1) = (rx0, rx1 + 1, ry0, ry1 + 1);
        let (ww, hh) = (x1 - x0, y1 - y0);
        let row_e: Vec<u8> = (0..hh).map(|y| (!(0..ww).any(|x| ink[(y0 + y) * w + x0 + x] != 0)) as u8).collect();
        let col_e: Vec<u8> = (0..ww).map(|x| (!(0..hh).any(|y| ink[(y0 + y) * w + x0 + x] != 0)) as u8).collect();
        let m = medh(x0, y0, x1, y1);
        let (gv, gh) = ((14.0 * k).max(0.9 * m), (12.0 * k).max(m));
        let hg: Vec<(usize, usize)> = runs(&row_e).into_iter().filter(|r| (r.1 - r.0) as f64 >= gh && r.0 > 0 && r.1 < hh).collect();
        // A column gap only counts where the region is several text lines tall. In one line of text
        // (about 2.5 letter heights with its vowel signs) the same gap is just the space between
        // two words of justified text, and cutting there would send single words to the reader.
        let vg: Vec<(usize, usize)> = if hh as f64 >= 5.0 * m {
            runs(&col_e).into_iter().filter(|r| (r.1 - r.0) as f64 >= gv && r.0 > 0 && r.1 < ww).collect()
        } else {
            vec![]
        };
        if hg.is_empty() && vg.is_empty() {
            leaves.push([x0, y0, x1, y1]);
            return;
        }
        let big = |a: &[(usize, usize)]| a.iter().fold(None, |b: Option<(usize, usize)>, &r| match b { Some(b) if r.1 - r.0 <= b.1 - b.0 => Some(b), _ => Some(r) });
        let (bh, bv) = (big(&hg), big(&vg));
        match (bh, bv) {
            (Some(bh), bv) if bv.is_none_or(|bv| (bh.1 - bh.0) as f64 >= (bv.1 - bv.0) as f64 * 0.6) => {
                cut(ink, w, k, medh, leaves, x0, y0, x1, y0 + bh.0);
                cut(ink, w, k, medh, leaves, x0, y0 + bh.1, x1, y1);
            }
            (_, Some(bv)) => {
                cut(ink, w, k, medh, leaves, x0 + bv.1, y0, x1, y1); // right column first
                cut(ink, w, k, medh, leaves, x0, y0, x0 + bv.0, y1);
            }
            _ => {}
        }
    }
    cut(&ink, w, k, &medh, &mut leaves, 0, 0, w, h);

    // classify leaves
    let blocks: Vec<Block> = leaves
        .iter()
        .map(|&[x0, y0, x1, y1]| {
            let (bw, bh) = (x1 - x0, y1 - y0);
            let (mut mid, mut n) = (0usize, 0usize);
            for y in (y0..y1).step_by(2) {
                for x in (x0..x1).step_by(2) {
                    let v = g[y * w + x];
                    n += 1;
                    if v > 60.0 && v < 200.0 {
                        mid += 1;
                    }
                }
            }
            let mid_frac = if n > 0 { mid as f64 / n as f64 } else { 0.0 };
            // count text lines roughly: rows with ink
            let prof: Vec<u8> = (0..bh)
                .map(|y| {
                    let c = (x0..x1).step_by(2).filter(|&x| raw[(y0 + y) * w + x] != 0).count() as f64;
                    (c > 1f64.max(0.01 * bw as f64 / 2.0)) as u8
                })
                .collect();
            let lines = runs(&prof).iter().filter(|r| (r.1 - r.0) as f64 > 3.0 * k).count();
            let mut kind = if mid_frac > 0.35 { "image" } else if lines >= 2 { "text" } else { "head" };
            if (bw as f64) < 60.0 * k && (bh as f64) < 60.0 * k {
                kind = "tiny";
            }
            Block { l: x0, t: y0, r: x1, b: y1, kind, lines }
        })
        .collect();
    // reading order: bands top-down, right-to-left within a band
    let mut rest = blocks;
    let mut out = Vec::new();
    while !rest.is_empty() {
        let top = rest.iter().fold(rest[0], |a, &b| if b.t < a.t { b } else { a });
        let (mut band, others): (Vec<Block>, Vec<Block>) = rest
            .into_iter()
            .partition(|r| (r.t as f64) < top.b as f64 - 0.5 * (top.b as f64 - top.t as f64) || (r.t as f64) < top.t as f64 + 40.0 * k);
        band.sort_by(|a, b| b.r.cmp(&a.r));
        out.extend(band);
        rest = others;
    }
    (out.into_iter().filter(|b| b.kind == "text" || b.kind == "head").collect(), letter)
}

/// Erases long thin horizontal / vertical ruling (notebook lines, margins) so handwriting can be
/// found (segment.js removeRuling()).
pub fn remove_ruling(g: &[f32], w: usize, h: usize, thr: f32) -> Vec<f32> {
    let ink: Vec<u8> = g.iter().map(|&v| (v < thr) as u8).collect();
    let dil = |src: &[u8], rx: isize, ry: isize| -> Vec<u8> {
        let mut a = vec![0u8; w * h];
        let mut b = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if src[y * w + x] == 0 {
                    continue;
                }
                for d in -rx..=rx {
                    let xx = x as isize + d;
                    if xx >= 0 && xx < w as isize {
                        a[y * w + xx as usize] = 1;
                    }
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                if a[y * w + x] == 0 {
                    continue;
                }
                for d in -ry..=ry {
                    let yy = y as isize + d;
                    if yy >= 0 && yy < h as isize {
                        b[yy as usize * w + x] = 1;
                    }
                }
            }
        }
        b
    };
    let d3 = dil(&ink, 1, 1);
    // vertical erosion by 3 (keeps only thin-but-continuous strokes >=3 tall after dilation)
    let mut e = vec![0u8; w * h];
    for y in 1..h.saturating_sub(1) {
        for x in 0..w {
            e[y * w + x] = d3[(y - 1) * w + x] & d3[y * w + x] & d3[(y + 1) * w + x];
        }
    }
    let ll = (js_round(w as f64 / 7.0) as usize).max(60);
    let mut hr = vec![0u8; w * h];
    for y in 0..h {
        let mut x = 0;
        while x < w {
            if e[y * w + x] == 0 {
                x += 1;
                continue;
            }
            let s = x;
            while x < w && e[y * w + x] != 0 {
                x += 1;
            }
            if x - s >= ll {
                hr[y * w + s..y * w + x].fill(1);
            }
        }
    }
    // vertical rules (margin line)
    let lv = (js_round(h as f64 / 5.0) as usize).max(60);
    let mut vr = vec![0u8; w * h];
    let on = |x: usize, y: usize| d3[y * w + x] & d3[y * w + (x + 1).min(w - 1)] & d3[y * w + x.saturating_sub(1)];
    for x in 0..w {
        let mut y = 0;
        while y < h {
            if on(x, y) == 0 {
                y += 1;
                continue;
            }
            let s = y;
            while y < h && on(x, y) != 0 {
                y += 1;
            }
            if y - s >= lv {
                for k in s..y {
                    vr[k * w + x] = 1;
                }
            }
        }
    }
    let m: Vec<u8> = hr.iter().zip(&vr).map(|(a, b)| a | b).collect();
    let mask = dil(&m, 2, 2);
    g.iter().enumerate().map(|(i, &v)| if mask[i] != 0 && ink[i] != 0 { 255.0 } else { v }).collect()
}

// ---------------------------------------------------------------- whole pages
#[derive(Serialize)]
pub struct Prepared {
    /// one PNG per text line, ready for the recogniser
    #[serde(skip)]
    pub lines: Vec<Vec<u8>>,
    pub boxes: Vec<LineBox>,
    pub pitch: f64,
    pub skew: f64,
    /// newspaper mode: the block each line belongs to
    pub blk: Option<Vec<usize>>,
}

fn page_gray(rgba: &[u8], w: usize, h: usize, rotation: i32, scale: f64) -> Gray {
    let g = Gray::from_rgba(rgba, w, h);
    g.resize(js_round(w as f64 * scale) as usize, js_round(h as f64 * scale) as usize).rotate90(rotation)
}

/// Cleans up a scanned page and cuts it into text lines (renderer.js prepare()).
pub fn prepare(rgba: &[u8], w: usize, h: usize, rotation: i32, enhance: bool, columns: bool) -> Prepared {
    // Aim for text big enough for the OCR engine: ~2400-3200 px across a page.
    let long = w.max(h) as f64;
    let scale = if long < 2400.0 { (2400.0 / long).min(3.0) } else if long > 3600.0 { 3600.0 / long } else { 1.0 };
    let mut g = page_gray(rgba, w, h, rotation, scale);
    normalize(&mut g, enhance);
    let thr = otsu(&g.px);
    let skew = find_skew(&g, thr);
    let g = rotate_gray(g, skew);
    let (w, h) = (g.w, g.h);
    // Newspapers / magazines: read column by column (right to left), but only when the page really
    // has columns: two tall text blocks side by side, "tall" being 15 letter heights (about four
    // lines) or more. A letterhead, a label beside a paragraph or a single line are not columns:
    // such pages are read as one column, where the whole page shows the line spacing reliably.
    // (find_blocks always runs: it also measures the letter height that line finding needs.)
    let (blocks, letter) = find_blocks(&g.px, w, h, thr);
    let blocks = if columns { blocks } else { vec![] };
    let l = if letter > 0.0 { letter } else { 15.0 };
    let tall = |b: &Block| (b.b - b.t) as f64 >= 15.0 * l;
    let beside = |a: &Block, b: &Block| {
        let overlap = a.b.min(b.b) as f64 - a.t.max(b.t) as f64;
        (a.r <= b.l || b.r <= a.l) && overlap >= 0.5 * (a.b - a.t).min(b.b - b.t) as f64
    };
    let has_columns = blocks.iter().enumerate().any(|(i, a)| tall(a) && blocks.iter().enumerate().any(|(j, b)| i != j && tall(b) && beside(a, b)));
    if has_columns {
        struct Sub {
            bl: Block,
            bi: usize,
            img: Gray,
            own: (Vec<LineBox>, f64),
        }
        let subs: Vec<Sub> = blocks
            .iter()
            .enumerate()
            .filter(|(_, bl)| bl.r - bl.l >= 20 && bl.b - bl.t >= 10)
            .map(|(bi, bl)| {
                let img = g.crop(&LineBox { l: bl.l, t: bl.t, r: bl.r, b: bl.b });
                let own = find_lines(&img.px, img.w, img.h, thr, 0.0, letter);
                Sub { bl: *bl, bi, img, own }
            })
            .collect();
        // A block of one or two lines (a headline, a caption) cannot show its own line spacing: it
        // would cut the vowel-sign rows off as separate lines. It gets the spacing of the columns
        // (the median of the tall blocks).
        let mut good: Vec<f64> = subs.iter().filter(|s| tall(&s.bl)).map(|s| s.own.1).collect();
        good.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let reference = if good.is_empty() { find_lines(&g.px, w, h, thr, 0.0, letter).1 } else { good[good.len() >> 1] };
        let (mut boxes, mut lines, mut blk, mut pitch) = (vec![], vec![], vec![], 0f64);
        for s in &subs {
            let r = if tall(&s.bl) && s.img.h as f64 >= 4.0 * reference { s.own.clone() } else { find_lines(&s.img.px, s.img.w, s.img.h, thr, reference, letter) };
            if pitch == 0.0 {
                pitch = r.1;
            }
            for b in &r.0 {
                lines.push(crop_to_png(&s.img, b, r.1));
                boxes.push(LineBox { l: b.l + s.bl.l, t: b.t + s.bl.t, r: b.r + s.bl.l, b: b.b + s.bl.t });
                blk.push(s.bi);
            }
        }
        return Prepared { lines, boxes, pitch: if pitch != 0.0 { pitch } else { 45.0 }, skew, blk: Some(blk) };
    }
    let (boxes, pitch) = find_lines(&g.px, w, h, thr, 0.0, letter);
    let lines = boxes.iter().map(|b| crop_to_png(&g, b, pitch)).collect();
    Prepared { lines, boxes, pitch, skew, blk: None }
}

/// Lines of a handwritten page for labelling: the original crop and the crop with ruling removed
/// (renderer.js prepareLabel()).
pub fn prepare_label(rgba: &[u8], w: usize, h: usize, rotation: i32) -> (Vec<(Vec<u8>, Vec<u8>)>, f64) {
    let long = w.max(h) as f64;
    let scale = if long > 3000.0 { 3000.0 / long } else if long < 1800.0 { (1800.0 / long).min(2.0) } else { 1.0 };
    let mut g = page_gray(rgba, w, h, rotation, scale);
    normalize(&mut g, true);
    let thr = otsu(&g.px);
    let skew = find_skew(&g, thr);
    let g = rotate_gray(g, skew);
    let gc = Gray { w: g.w, h: g.h, px: remove_ruling(&g.px, g.w, g.h, thr) };
    let (boxes, pitch) = find_lines(&gc.px, gc.w, gc.h, thr, 0.0, 0.0);
    (boxes.iter().map(|b| (g.crop(b).png(), gc.crop(b).png())).collect(), pitch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns() {
        let g = Gray { w: 3, h: 2, px: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0] };
        assert_eq!(g.rotate90(90).px, vec![4.0, 1.0, 5.0, 2.0, 6.0, 3.0]);
        assert_eq!(g.rotate90(270).px, vec![3.0, 6.0, 2.0, 5.0, 1.0, 4.0]);
        assert_eq!(g.rotate90(180).px, vec![6.0, 5.0, 4.0, 3.0, 2.0, 1.0]);
    }

    #[test]
    fn otsu_splits_two_levels() {
        let px: Vec<f32> = (0..100).map(|i| if i < 30 { 20.0 } else { 230.0 }).collect();
        let t = otsu(&px);
        assert!((20.0..230.0).contains(&t));
    }

    #[test]
    fn png_round_trip_size() {
        let g = Gray { w: 4, h: 2, px: vec![0.0; 8] };
        let png = g.png();
        assert_eq!(&png[1..4], b"PNG");
    }
}
