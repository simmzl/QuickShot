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

    fn rasterize(
        &mut self,
        ch: char,
        px_size: f32,
    ) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let key = (ch, (px_size * 10.0) as u32);

        // Cached path (also catches "previously not found" if we cached an
        // empty bitmap; render paths skip empty bitmaps explicitly).
        if self.cache.contains_key(&key) {
            return self.cache.get(&key);
        }

        // Try primary first.
        if let Some(font) = self.primary.as_ref() {
            let (m, b) = font.rasterize(ch, px_size);
            if !b.is_empty() {
                self.cache.insert(key, (m, b));
                return self.cache.get(&key);
            }
        }

        // Primary missing this glyph (or no primary loaded) — try CJK fallback.
        if let Some(font) = self.cjk_fallback.as_ref() {
            let (m, b) = font.rasterize(ch, px_size);
            if !b.is_empty() {
                self.cache.insert(key, (m, b));
                return self.cache.get(&key);
            }
        }

        // Both fonts missing the glyph — cache an empty result so we don't
        // re-rasterize every frame. fontdue 0.9 doesn't derive Default for
        // Metrics / OutlineBounds, so build a zero one manually.
        let empty = fontdue::Metrics {
            xmin: 0,
            ymin: 0,
            width: 0,
            height: 0,
            advance_width: 0.0,
            advance_height: 0.0,
            bounds: fontdue::OutlineBounds {
                xmin: 0.0,
                ymin: 0.0,
                width: 0.0,
                height: 0.0,
            },
        };
        self.cache.insert(key, (empty, Vec::new()));
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
}
