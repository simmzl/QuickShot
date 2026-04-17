use fontdue::{Font as FontdueFont, FontSettings};
use std::collections::HashMap;

const FONT_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Rasterized glyph cache keyed by `(char, px_size_tenths)` so callers using
/// integer pt sizes (e.g. 12.0, 14.0) reuse the same bitmap.
pub struct Font {
    inner: Option<FontdueFont>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Font {
    /// Loads the embedded font. On failure returns a Font whose `render_text`
    /// is a silent no-op — the UI still works, labels just don't paint.
    pub fn embedded() -> Self {
        let inner = FontdueFont::from_bytes(FONT_BYTES, FontSettings::default()).ok();
        Self {
            inner,
            cache: HashMap::new(),
        }
    }

    fn rasterize(
        &mut self,
        ch: char,
        px_size: f32,
    ) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.inner.as_ref()?;
        let key = (ch, (px_size * 10.0) as u32);
        if !self.cache.contains_key(&key) {
            let (metrics, bitmap) = font.rasterize(ch, px_size);
            self.cache.insert(key, (metrics, bitmap));
        }
        self.cache.get(&key)
    }

    /// Draw `text` into the softbuffer `buf` (0x00RRGGBB per pixel) at pen
    /// position (x, y) = baseline *top-left* in pixels. `color_rgb` is the
    /// foreground color; alpha is taken from the glyph coverage and blended
    /// over whatever is already in `buf`.
    pub fn render_text(
        &mut self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        x: i32,
        y: i32,
        text: &str,
        px_size: f32,
        color_rgb: u32,
    ) {
        if self.inner.is_none() {
            return;
        }
        let (fr, fg, fb) = (
            ((color_rgb >> 16) & 0xFF) as u32,
            ((color_rgb >> 8) & 0xFF) as u32,
            (color_rgb & 0xFF) as u32,
        );
        let mut pen_x = x as f32;
        for ch in text.chars() {
            let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else {
                continue;
            };
            let gx = pen_x.round() as i32 + metrics.xmin;
            // Fontdue's ymin is measured from the glyph's baseline upwards;
            // to place a "top-left" pen we shift the glyph down by the ascent
            // of the requested size. Approximate the ascent as px_size * 0.8.
            let ascent = (px_size * 0.8) as i32;
            let gy = y + ascent - metrics.height as i32 - metrics.ymin;
            blit_glyph(buf, w, h, gx, gy, &metrics, &bitmap, (fr, fg, fb));
            pen_x += metrics.advance_width;
        }
    }
}

fn blit_glyph(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    color: (u32, u32, u32),
) {
    let (fr, fg, fb) = color;
    for gy in 0..metrics.height as i32 {
        for gx in 0..metrics.width as i32 {
            let alpha = bitmap[(gy * metrics.width as i32 + gx) as usize] as u32;
            if alpha == 0 {
                continue;
            }
            let px = x + gx;
            let py = y + gy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                continue;
            }
            let idx = (py as u32 * w + px as u32) as usize;
            let bg = buf[idx];
            let br = (bg >> 16) & 0xFF;
            let bgc = (bg >> 8) & 0xFF;
            let bb = bg & 0xFF;
            let r = (fr * alpha + br * (255 - alpha)) / 255;
            let g = (fg * alpha + bgc * (255 - alpha)) / 255;
            let b = (fb * alpha + bb * (255 - alpha)) / 255;
            buf[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_loads_and_renders_nonzero_pixels() {
        let mut font = Font::embedded();
        assert!(font.inner.is_some(), "embedded font failed to load");
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(
            buf.iter().any(|&p| p != 0),
            "rendering 'Hi' produced no non-zero pixels"
        );
    }

    #[test]
    fn missing_font_is_silent_noop() {
        let mut font = Font {
            inner: None,
            cache: HashMap::new(),
        };
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(buf.iter().all(|&p| p == 0));
    }
}
