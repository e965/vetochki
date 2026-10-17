//! CPU compositing for the scene: decode the WebP assets, shrink them once, blend them into a 0RGB frame
//! (softbuffer's pixel format), and draw the score text.

/// Premultiplied RGBA image.
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[u8; 4]>,
}

/// WebP (lossy VP8, alpha in a separate ALPH chunk) → premultiplied RGBA.
pub fn decode_webp(bytes: &[u8]) -> Image {
    let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).expect("embedded WebP");
    let (w, h) = d.dimensions();
    let mut buf = vec![0; d.output_buffer_size().expect("WebP size")];
    d.read_image(&mut buf).expect("embedded WebP");
    let px = if d.has_alpha() {
        let pre = |v: u8, a: u8| ((u16::from(v) * u16::from(a) + 127) / 255) as u8;
        buf.as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| [pre(r, a), pre(g, a), pre(b, a), a])
            .collect()
    } else {
        buf.as_chunks::<3>()
            .0
            .iter()
            .map(|&[r, g, b]| [r, g, b, 255])
            .collect()
    };
    Image {
        w: w as usize,
        h: h as usize,
        px,
    }
}

/// Box-filter shrink (VCL only ever shrinks: `Proportional` never enlarges).
pub fn shrink(src: &Image, w: usize, h: usize) -> Image {
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        let (y0, y1) = (y * src.h / h, ((y + 1) * src.h / h).max(y * src.h / h + 1));
        for x in 0..w {
            let (x0, x1) = (x * src.w / w, ((x + 1) * src.w / w).max(x * src.w / w + 1));
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for p in &src.px[sy * src.w + x0..sy * src.w + x1] {
                    for c in 0..4 {
                        sum[c] += u32::from(p[c]);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            px.push(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Image { w, h, px }
}

/// A pixel rectangle `[x0, x1) × [y0, y1)` inside a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub x0: usize,
    pub y0: usize,
    pub x1: usize,
    pub y1: usize,
}

impl Area {
    pub fn full(w: usize, h: usize) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        }
    }

    /// The part of the rectangle (x, y, w, h) that lies inside `bounds`.
    pub fn clipped(x: i32, y: i32, w: usize, h: usize, bounds: Area) -> Option<Self> {
        let lo = |v: i32, min: usize| (v.max(0) as usize).max(min);
        let hi =
            |v: i32, len: usize, max: usize| ((i64::from(v) + len as i64).max(0) as usize).min(max);
        let a = Self {
            x0: lo(x, bounds.x0),
            y0: lo(y, bounds.y0),
            x1: hi(x, w, bounds.x1),
            y1: hi(y, h, bounds.y1),
        };
        (a.x0 < a.x1 && a.y0 < a.y1).then_some(a)
    }
}

/// A 0RGB frame (`0x00RRGGBB`, softbuffer's format). Every drawing call is limited to `clip`, so
/// a damaged area can be repainted without touching the rest.
pub struct Frame<'a> {
    pub w: usize,
    pub px: &'a mut [u32],
    pub clip: Area,
}

fn rgb(c: u32) -> [u32; 3] {
    [(c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff]
}

/// Source-over of one premultiplied pixel onto a 0RGB one.
fn over(d: &mut u32, s: [u8; 4]) {
    if s[3] == 255 {
        *d = u32::from(s[0]) << 16 | u32::from(s[1]) << 8 | u32::from(s[2]);
    } else if s[3] != 0 {
        let inv = 255 - u32::from(s[3]);
        let dc = rgb(*d);
        let mix = |i: usize| u32::from(s[i]) + (dc[i] * inv + 127) / 255;
        *d = mix(0) << 16 | mix(1) << 8 | mix(2);
    }
}

/// Bilinear source position of target pixel `t` when `src` source pixels span `len` target pixels
/// starting at `origin`: the two neighbouring source indices and the second one's weight (of 256).
fn sample_pos(t: usize, origin: i32, len: usize, src: usize) -> (usize, usize, u32) {
    let s = ((t as f64 - f64::from(origin) + 0.5) * src as f64 / len as f64 - 0.5)
        .clamp(0.0, (src - 1) as f64);
    let i = s as usize;
    (i, (i + 1).min(src - 1), ((s - i as f64) * 256.0) as u32)
}

impl<'a> Frame<'a> {
    pub fn new(w: usize, h: usize, px: &'a mut [u32]) -> Self {
        Self {
            w,
            px,
            clip: Area::full(w, h),
        }
    }

    pub fn fill(&mut self, x: i32, y: i32, w: usize, h: usize, color: u32) {
        if let Some(a) = Area::clipped(x, y, w, h, self.clip) {
            for row in a.y0..a.y1 {
                self.px[row * self.w + a.x0..row * self.w + a.x1].fill(color);
            }
        }
    }

    /// Source-over blend of a premultiplied image with its top-left at (x, y).
    pub fn blit(&mut self, img: &Image, x: i32, y: i32) {
        let Some(a) = Area::clipped(x, y, img.w, img.h, self.clip) else {
            return;
        };
        for fy in a.y0..a.y1 {
            let sy = (fy as i64 - i64::from(y)) as usize;
            for fx in a.x0..a.x1 {
                let sx = (fx as i64 - i64::from(x)) as usize;
                over(&mut self.px[fy * self.w + fx], img.px[sy * img.w + sx]);
            }
        }
    }

    /// [`Self::blit`] with the image resized (bilinear) to `w`×`h`. Only the visible pixels are
    /// sampled, so a target much larger than the frame costs no intermediate image.
    pub fn blit_scaled(&mut self, img: &Image, x: i32, y: i32, w: usize, h: usize) {
        if (w, h) == (img.w, img.h) {
            return self.blit(img, x, y);
        }
        let Some(a) = Area::clipped(x, y, w, h, self.clip) else {
            return;
        };
        let lerp = |p: [u8; 4], q: [u8; 4], t: u32| {
            std::array::from_fn(|c| {
                ((u32::from(p[c]) * (256 - t) + u32::from(q[c]) * t + 128) >> 8) as u8
            })
        };
        let cols: Vec<_> = (a.x0..a.x1).map(|fx| sample_pos(fx, x, w, img.w)).collect();
        for fy in a.y0..a.y1 {
            let (y0, y1, ty) = sample_pos(fy, y, h, img.h);
            let (r0, r1) = (&img.px[y0 * img.w..], &img.px[y1 * img.w..]);
            for (fx, &(x0, x1, tx)) in (a.x0..a.x1).zip(&cols) {
                let s = lerp(lerp(r0[x0], r0[x1], tx), lerp(r1[x0], r1[x1], tx), ty);
                over(&mut self.px[fy * self.w + fx], s);
            }
        }
    }

    /// Draw `text` with its baseline at `baseline`.
    pub fn text(&mut self, glyphs: &Glyphs, text: &str, x: i32, baseline: i32, color: u32) {
        let col = rgb(color);
        let mut pen = x as f32;
        for (m, coverage) in text.chars().filter_map(|c| glyphs.get(c)) {
            let (gx, gy) = (
                pen.round() as i32 + m.xmin,
                baseline - m.ymin - m.height as i32,
            );
            if let Some(a) = Area::clipped(gx, gy, m.width, m.height, self.clip) {
                for fy in a.y0..a.y1 {
                    for fx in a.x0..a.x1 {
                        let cov = coverage[(fy as i64 - i64::from(gy)) as usize * m.width
                            + (fx as i64 - i64::from(gx)) as usize];
                        let (a, d) = (u32::from(cov), &mut self.px[fy * self.w + fx]);
                        let dc = rgb(*d);
                        let mix = |i: usize| (col[i] * a + dc[i] * (255 - a) + 127) / 255;
                        *d = mix(0) << 16 | mix(1) << 8 | mix(2);
                    }
                }
            }
            pen += m.advance_width;
        }
    }
}

/// The only glyphs the score box can show, rasterised once at one size. fontdue keeps every
/// glyph of a font parsed (12.7 MB for the system Tahoma's 4 498 glyphs), so the font itself is
/// dropped as soon as these are cached.
pub struct Glyphs {
    pub ascent: f32,
    pub descent: f32,
    glyphs: Vec<(char, fontdue::Metrics, Vec<u8>)>,
}

impl Glyphs {
    pub fn new(font: fontdue::Font, size: f32, chars: &str) -> Self {
        let lm = font.horizontal_line_metrics(size).expect("horizontal font");
        let glyphs = chars
            .chars()
            .map(|c| {
                let (m, coverage) = font.rasterize(c, size);
                (c, m, coverage)
            })
            .collect();
        Self {
            ascent: lm.ascent,
            descent: lm.descent,
            glyphs,
        }
    }

    /// Characters outside the cache are skipped; the box never holds any.
    fn get(&self, c: char) -> Option<(&fontdue::Metrics, &[u8])> {
        self.glyphs
            .iter()
            .find(|g| g.0 == c)
            .map(|g| (&g.1, g.2.as_slice()))
    }

    pub fn width(&self, text: &str) -> i32 {
        text.chars()
            .filter_map(|c| self.get(c))
            .map(|(m, _)| m.advance_width)
            .sum::<f32>()
            .round() as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_decode() {
        let bg = decode_webp(include_bytes!("../assets/background.webp"));
        assert_eq!((bg.w, bg.h), (2048, 1536));
        assert!(
            bg.px.iter().all(|p| p[3] == 255),
            "the background is opaque"
        );
        let leaves = decode_webp(include_bytes!("../assets/leaves.webp"));
        assert_eq!((leaves.w, leaves.h), (800, 348));
        let branch = decode_webp(include_bytes!("../assets/branch.webp"));
        assert_eq!((branch.w, branch.h), (640, 485));
        // the original PNG's corners are fully transparent; WebP keeps alpha losslessly
        for p in [branch.px[0], leaves.px[0]] {
            assert_eq!(p, [0, 0, 0, 0]);
        }
    }
}
