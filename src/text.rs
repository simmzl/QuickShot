use fontdue::{Font as FontdueFont, FontSettings};
use image::RgbaImage;
use std::collections::HashMap;

const FONT_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Rasterized glyph cache keyed by `(char, px_size_tenths)` so callers using
/// integer pt sizes (e.g. 12.0, 14.0) reuse the same bitmap.
pub struct Font {
    primary: Option<FontdueFont>,
    cjk_fallback: Option<FontdueFont>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Font {
    /// Loads the embedded primary font and a best-effort OS-provided CJK
    /// fallback. On failure of *both*, `render_text` is a silent no-op — the UI
    /// still works, labels just don't paint.
    pub fn embedded() -> Self {
        let primary = FontdueFont::from_bytes(FONT_BYTES, FontSettings::default()).ok();
        let cjk_fallback = load_cjk_fallback();
        Self {
            primary,
            cjk_fallback,
            cache: HashMap::new(),
        }
    }

    /// Return the rendered pixel width of `text` at `px_size`. Walks the
    /// chars and sums `advance_width` from freshly-computed metrics. Returns
    /// 0.0 if both the embedded primary and CJK fallback fonts failed to load.
    ///
    /// The `&mut self` receiver is for API consistency with `rasterize`/
    /// `render_text` — internally this only calls `fontdue::Font::metrics`
    /// (`&self`), so no caching mutation actually happens here.
    pub fn measure_text_width(&mut self, text: &str, px_size: f32) -> f32 {
        if self.primary.is_none() && self.cjk_fallback.is_none() {
            return 0.0;
        }
        let mut w = 0.0_f32;
        for ch in text.chars() {
            // Mirror the font-selection logic in `rasterize`: try primary
            // first if it has the glyph, then CJK fallback. If neither has
            // it, advance by an em-half (matches the empty-bitmap branch in
            // `render_text`).
            let advance = self
                .primary
                .as_ref()
                .and_then(|font| {
                    (font.lookup_glyph_index(ch) != 0)
                        .then(|| font.metrics(ch, px_size).advance_width)
                })
                .or_else(|| {
                    self.cjk_fallback.as_ref().and_then(|font| {
                        (font.lookup_glyph_index(ch) != 0)
                            .then(|| font.metrics(ch, px_size).advance_width)
                    })
                });
            w += advance.unwrap_or(px_size * 0.5);
        }
        w
    }

    fn rasterize(
        &mut self,
        ch: char,
        px_size: f32,
    ) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let key = (ch, (px_size * 10.0) as u32);

        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        // Primary path: only use if the font actually has this glyph.
        // fontdue's rasterize() returns the .notdef glyph (visible tofu box)
        // for missing characters, so checking bitmap.is_empty() is wrong —
        // we need to ask the font directly via lookup_glyph_index.
        if let Some(font) = self.primary.as_ref() {
            if font.lookup_glyph_index(ch) != 0 {
                let (m, b) = font.rasterize(ch, px_size);
                self.cache.insert(key, (m, b));
                return self.cache.get(&key);
            }
        }

        // Fallback path: use only if it has the glyph.
        if let Some(font) = self.cjk_fallback.as_ref() {
            if font.lookup_glyph_index(ch) != 0 {
                let (m, b) = font.rasterize(ch, px_size);
                self.cache.insert(key, (m, b));
                return self.cache.get(&key);
            }
        }

        // Truly missing in both fonts — cache an empty result so render_text
        // can advance pen_x and skip without re-probing every frame.
        self.cache.insert(key, (
            fontdue::Metrics {
                xmin: 0, ymin: 0, width: 0, height: 0,
                advance_width: px_size * 0.5,
                advance_height: 0.0,
                bounds: fontdue::OutlineBounds {
                    xmin: 0.0, ymin: 0.0, width: 0.0, height: 0.0,
                },
            },
            Vec::new(),
        ));
        self.cache.get(&key)
    }

    /// Draw `text` into the softbuffer `buf` (0x00RRGGBB per pixel) at pen
    /// position (x, y) = baseline *top-left* in pixels. `color_rgb` is the
    /// foreground color; alpha is taken from the glyph coverage and blended
    /// over whatever is already in `buf`.
    // All arguments are primitives needed by the hot-path inner loop; grouping
    // them into a struct would add allocation/indirection with no clarity gain.
    #[allow(clippy::too_many_arguments)]
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
        if self.primary.is_none() && self.cjk_fallback.is_none() {
            return;
        }
        let (fr, fg, fb) = (
            (color_rgb >> 16) & 0xFF,
            (color_rgb >> 8) & 0xFF,
            color_rgb & 0xFF,
        );
        let mut pen_x = x as f32;
        for ch in text.chars() {
            let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else {
                continue;
            };
            if bitmap.is_empty() {
                // Glyph missing in both primary and fallback — advance pen by
                // a reasonable em-width so the rest of the text doesn't
                // collapse onto one column.
                pen_x += px_size * 0.5;
                continue;
            }
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

    /// Draw `text` into an RGBA8 image at pen position (x, y) (top-left of
    /// baseline cell). `color` is the foreground RGBA; the glyph alpha is
    /// composited over the existing image (alpha=255 channel forced opaque).
    /// Mirrors `render_text` but for the export path which uses `image::RgbaImage`
    /// rather than the softbuffer ARGB `&mut [u32]`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_text_rgba(
        &mut self,
        img: &mut RgbaImage,
        x: i32,
        y: i32,
        text: &str,
        px_size: f32,
        color: [u8; 4],
    ) {
        if self.primary.is_none() && self.cjk_fallback.is_none() {
            return;
        }
        let mut pen_x = x as f32;
        let (w, h) = (img.width() as i32, img.height() as i32);
        for ch in text.chars() {
            let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else {
                continue;
            };
            if bitmap.is_empty() {
                pen_x += px_size * 0.5;
                continue;
            }
            let gx = pen_x.round() as i32 + metrics.xmin;
            let ascent = (px_size * 0.8) as i32;
            let gy = y + ascent - metrics.height as i32 - metrics.ymin;
            for ggy in 0..metrics.height as i32 {
                for ggx in 0..metrics.width as i32 {
                    let alpha = bitmap[(ggy * metrics.width as i32 + ggx) as usize];
                    if alpha == 0 {
                        continue;
                    }
                    let px = gx + ggx;
                    let py = gy + ggy;
                    if px < 0 || py < 0 || px >= w || py >= h {
                        continue;
                    }
                    let bg = img.get_pixel_mut(px as u32, py as u32);
                    let a = alpha as u16;
                    let inv = (255 - alpha) as u16;
                    bg[0] = ((color[0] as u16 * a + bg[0] as u16 * inv) / 255) as u8;
                    bg[1] = ((color[1] as u16 * a + bg[1] as u16 * inv) / 255) as u8;
                    bg[2] = ((color[2] as u16 * a + bg[2] as u16 * inv) / 255) as u8;
                    // Destination alpha passes through unchanged. Caller is
                    // responsible for ensuring the destination is opaque if
                    // they want an opaque result (export path already does).
                }
            }
            pen_x += metrics.advance_width;
        }
    }
}

/// Best-effort load of a CJK fallback font from well-known OS paths. Keeps the
/// binary small (CJK fonts are 10–60 MB) at the cost of needing the host OS to
/// ship one.
///
/// `.ttc` (TrueType Collection) files contain multiple font faces. The default
/// `collection_index = 0` may not be the CJK face (e.g. STHeiti Light.ttc face
/// 0 has no Chinese glyphs). We probe each face index until we find one that
/// contains the probe character '你' (U+4F60).
fn load_cjk_fallback() -> Option<FontdueFont> {
    #[cfg(target_os = "macos")]
    let candidates: &[&str] = &[
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
    ];
    #[cfg(target_os = "windows")]
    let candidates: &[&str] = &[
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let candidates: &[&str] = &[
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    ];

    // Probe character — if a face contains '你', it's a CJK face.
    let probe = '你';

    for path in candidates {
        let Ok(data) = std::fs::read(path) else { continue };
        // .ttc files contain multiple faces; try up to 32 (typical max).
        // Stop probing this file when from_bytes fails (out of faces).
        for index in 0..32u32 {
            let settings = FontSettings {
                collection_index: index,
                ..FontSettings::default()
            };
            match FontdueFont::from_bytes(data.as_slice(), settings) {
                Ok(font) => {
                    if font.lookup_glyph_index(probe) != 0 {
                        eprintln!(
                            "quickshot: CJK fallback loaded from {path} (face {index})"
                        );
                        return Some(font);
                    }
                    // This face exists but doesn't contain '你' — try next index.
                }
                Err(_) => {
                    // No more faces in this .ttc.
                    break;
                }
            }
        }
    }

    eprintln!("quickshot: no CJK fallback face found; CJK input will render as tofu");
    None
}

// All arguments are primitives for a tight pixel-blending loop; a struct
// wrapper would hurt readability here without any shared type that makes sense.
#[allow(clippy::too_many_arguments)]
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
        assert!(font.primary.is_some(), "embedded primary font failed to load");
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
            primary: None,
            cjk_fallback: None,
            cache: HashMap::new(),
        };
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn cjk_glyph_renders_via_fallback_on_macos() {
        // Skip on non-macOS where the test machine may not have a CJK font installed.
        #[cfg(target_os = "macos")]
        {
            let mut font = Font::embedded();
            // 你 (you) — present in PingFang/STHeiti/Hiragino Sans GB, not in
            // JetBrainsMono.
            let (w, h) = (64u32, 64u32);
            let mut buf = vec![0u32; (w * h) as usize];
            font.render_text(&mut buf, w, h, 2, 2, "你", 24.0, 0x00FFFFFF);
            assert!(
                buf.iter().any(|&p| p != 0),
                "CJK glyph should render via fallback font"
            );
        }
    }

    #[test]
    fn primary_notdef_does_not_starve_fallback() {
        // Regression for: JetBrainsMono's .notdef glyph is a non-empty box,
        // so a naive "if bitmap is empty, try fallback" gate falsely concludes
        // primary handled the char. We must use lookup_glyph_index instead.
        #[cfg(target_os = "macos")]
        {
            let mut font = Font::embedded();
            // Render "h你" — h goes through primary, 你 must go through CJK fallback.
            // At 32px, 'h' (JetBrainsMono) advances ~19px, so 你 starts around
            // x≈22 and ends around x≈52. Split the buffer at the post-'h'
            // pen position so 'h' lands in the left zone and 你 in the right.
            let (w, h) = (128u32, 64u32);
            let mut buf = vec![0u32; (w * h) as usize];
            font.render_text(&mut buf, w, h, 2, 2, "h你", 32.0, 0x00FFFFFF);
            // 'h' ends well before x=20; 你 starts at x≈22. A split at x=20
            // puts each glyph cleanly on its own side.
            let split_x: usize = 20;
            let mut left_painted = 0;
            let mut right_painted = 0;
            for y in 0..h as usize {
                for x in 0..w as usize {
                    if buf[y * w as usize + x] != 0 {
                        if x < split_x { left_painted += 1; } else { right_painted += 1; }
                    }
                }
            }
            assert!(left_painted > 0, "primary 'h' produced no pixels");
            assert!(right_painted > 0, "fallback '你' produced no pixels (CJK fallback starved by primary .notdef)");
        }
    }
}
