//! Lucide icon glyph rendering. Bundles a single TTF font (lucide-static's
//! `lucide.ttf`) and exposes `render_centered()` which paints a codepoint
//! into a softbuffer at a given origin/box size with alpha-coverage blending.
//!
//! Reuses fontdue (already a dependency for text annotations) so we get
//! antialiased, correctly hinted icons without pulling in tiny-skia/resvg.
//!
//! Codepoint mapping is defined in `overlay/toolbar.rs` next to the toolbar
//! buttons themselves.
use fontdue::{Font as FontdueFont, FontSettings};
use std::collections::HashMap;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

pub struct IconFont {
    inner: Option<FontdueFont>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl IconFont {
    pub fn embedded() -> Self {
        let inner = FontdueFont::from_bytes(FONT_BYTES, FontSettings::default()).ok();
        Self {
            inner,
            cache: HashMap::new(),
        }
    }

    fn rasterize(&mut self, ch: char, px_size: f32) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.inner.as_ref()?;
        let key = (ch, (px_size * 10.0) as u32);
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }
        let (m, b) = font.rasterize(ch, px_size);
        self.cache.insert(key, (m, b));
        self.cache.get(&key)
    }

    /// Paint glyph `ch` centered in a `box_size`×`box_size` square at `origin`.
    /// softbuffer ARGB layout: `0x00RRGGBB`. Alpha is taken from glyph
    /// coverage and blended over the existing pixel.
    pub fn render_centered(
        &mut self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        origin: (i32, i32),
        box_size: i32,
        ch: char,
        color_rgb: u32,
    ) {
        // Lucide glyphs fit their em-box quite snugly; render at ~85% of the
        // button to leave a modest visual padding around the icon.
        let px_size = box_size as f32 * 0.85;
        let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else {
            return;
        };
        if metrics.width == 0 || metrics.height == 0 {
            return;
        }
        // Center the glyph in the box.
        let cx = origin.0 + box_size / 2;
        let cy = origin.1 + box_size / 2;
        let gx = cx - metrics.width as i32 / 2;
        let gy = cy - metrics.height as i32 / 2;

        let fr = (color_rgb >> 16) & 0xFF;
        let fg = (color_rgb >> 8) & 0xFF;
        let fb = color_rgb & 0xFF;

        let row_stride = metrics.width as i32;
        for ggy in 0..metrics.height as i32 {
            for ggx in 0..row_stride {
                let alpha = bitmap[(ggy * row_stride + ggx) as usize] as u32;
                if alpha == 0 {
                    continue;
                }
                let px = gx + ggx;
                let py = gy + ggy;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the font subset: every codepoint referenced in the toolbar must
    /// still produce a non-empty glyph after `pyftsubset`. If someone adds a
    /// new icon without updating the subset's `--unicodes` list, this test
    /// will catch it — `metrics.width` and `metrics.height` are both 0 for
    /// missing glyphs in fontdue.
    #[test]
    fn subset_includes_all_needed_glyphs() {
        let font = IconFont::embedded();
        let codepoints = [
            '\u{E121}', '\u{E493}', '\u{E167}', '\u{E076}',
            '\u{E1F9}', '\u{E198}', '\u{E4FF}', '\u{E2A1}',
            '\u{E2A0}', '\u{E259}', '\u{E14D}',
        ];
        let inner = font.inner.as_ref().expect("font loaded");
        for ch in codepoints {
            let m = inner.metrics(ch, 24.0);
            assert!(
                m.width > 0 || m.height > 0,
                "codepoint {:#x} should have glyph data after subset",
                ch as u32,
            );
        }
    }
}
