//! Text rendering.
//!
//! Two independent paths:
//!   * **UI labels** (size label, magnifier readout — ASCII only) use the
//!     embedded JetBrainsMono via fontdue. Parsing it costs ~2–3 ms.
//!   * **Annotation text** (user-typed, any script incl. CJK / emoji) is
//!     rasterized by the platform text stack — AppKit on macOS, GDI on
//!     Windows — into a coverage mask. This is the same stack that drives the
//!     native editor (NSTextView / EDIT), so what you type is what gets
//!     exported, and we no longer parse a 20–55 MB CJK `.ttc` with fontdue
//!     (that alone cost ~1.1 s on every overlay open).

use fontdue::{Font as FontdueFont, FontSettings};
use image::RgbaImage;
use std::collections::HashMap;

const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Max cached annotation masks before the cache is flushed. Each entry is a
/// small coverage bitmap; the cap only guards against unbounded growth while a
/// user re-edits text many times in one session.
const MASK_CACHE_CAP: usize = 64;

/// Rendered (possibly multi-line) annotation text as a **premultiplied RGBA**
/// bitmap. Rendering in full color (rather than a 1-channel coverage mask
/// tinted afterwards) keeps color glyphs — emoji — in their own colors.
/// `(0, 0)` is the top-left of the first line's line box.
#[derive(Debug, Clone, Default)]
pub struct TextMask {
    pub w: u32,
    pub h: u32,
    /// Premultiplied RGBA8, row-major, `w * h * 4` bytes.
    pub rgba: Vec<u8>,
}

impl TextMask {
    /// Build from a coverage mask tinted with a solid color (platforms
    /// without color-glyph rendering).
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub fn from_coverage(w: u32, h: u32, alpha: &[u8], color_rgb: u32) -> Self {
        let (r, g, b) = ((color_rgb >> 16) & 0xFF, (color_rgb >> 8) & 0xFF, color_rgb & 0xFF);
        let mut rgba = Vec::with_capacity(alpha.len() * 4);
        for &a in alpha {
            let a32 = a as u32;
            rgba.extend_from_slice(&[
                (r * a32 / 255) as u8,
                (g * a32 / 255) as u8,
                (b * a32 / 255) as u8,
                a,
            ]);
        }
        Self { w, h, rgba }
    }
}

pub struct Font {
    primary: Option<FontdueFont>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
    masks: HashMap<(String, u32, u32), TextMask>,
}

impl Font {
    /// Loads the embedded label font. Cheap (~2–3 ms) — no system font scan.
    pub fn embedded() -> Self {
        let primary = FontdueFont::from_bytes(FONT_BYTES, FontSettings::default()).ok();
        Self {
            primary,
            cache: HashMap::new(),
            masks: HashMap::new(),
        }
    }

    fn ensure_glyph(&mut self, ch: char, px_size: f32) -> (char, u32) {
        let key = (ch, (px_size * 10.0) as u32);
        if self.cache.contains_key(&key) {
            return key;
        }
        let entry = match self.primary.as_ref() {
            Some(font) if font.lookup_glyph_index(ch) != 0 => font.rasterize(ch, px_size),
            _ => (
                fontdue::Metrics {
                    xmin: 0,
                    ymin: 0,
                    width: 0,
                    height: 0,
                    advance_width: px_size * 0.5,
                    advance_height: 0.0,
                    bounds: fontdue::OutlineBounds {
                        xmin: 0.0,
                        ymin: 0.0,
                        width: 0.0,
                        height: 0.0,
                    },
                },
                Vec::new(),
            ),
        };
        self.cache.insert(key, entry);
        key
    }

    /// Draw an ASCII UI label into the softbuffer (0x00RRGGBB) with its
    /// line box top-left at (x, y).
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
        if self.primary.is_none() {
            return;
        }
        let color = ((color_rgb >> 16) & 0xFF, (color_rgb >> 8) & 0xFF, color_rgb & 0xFF);
        let ascent = (px_size * 0.8) as i32;
        let mut pen_x = x as f32;
        for ch in text.chars() {
            let key = self.ensure_glyph(ch, px_size);
            let Some((metrics, bitmap)) = self.cache.get(&key) else { continue };
            if !bitmap.is_empty() {
                let gx = pen_x.round() as i32 + metrics.xmin;
                let gy = y + ascent - metrics.height as i32 - metrics.ymin;
                blit_coverage_buf(
                    buf,
                    w,
                    h,
                    gx,
                    gy,
                    metrics.width as u32,
                    metrics.height as u32,
                    bitmap,
                    color,
                );
            }
            pen_x += metrics.advance_width;
        }
    }

    /// Cap height (height of 'H' above the baseline) in px, for optically
    /// centering labels: digits / capitals sit exactly in [baseline − cap,
    /// baseline].
    pub fn cap_height(&mut self, px_size: f32) -> f32 {
        let key = self.ensure_glyph('H', px_size);
        self.cache
            .get(&key)
            .map(|(m, _)| (m.height as i32 + m.ymin) as f32)
            .unwrap_or(px_size * 0.72)
    }

    /// `y` argument for `render_text` that places the label's cap-height box
    /// vertically centered on `center_y`. Mirrors render_text's baseline
    /// placement (baseline = y + ⌊0.8·px⌋).
    pub fn label_y_centered(&mut self, center_y: f32, px_size: f32) -> i32 {
        let cap = self.cap_height(px_size);
        let ascent = (px_size * 0.8) as i32;
        (center_y + cap / 2.0).round() as i32 - ascent
    }

    /// Advance width of an ASCII UI label in the embedded label font.
    pub fn measure_label(&mut self, text: &str, px_size: f32) -> f32 {
        let mut w = 0.0;
        for ch in text.chars() {
            let key = self.ensure_glyph(ch, px_size);
            if let Some((m, _)) = self.cache.get(&key) {
                w += m.advance_width;
            }
        }
        w
    }

    /// Rasterized annotation text in `color_rgb` (0x00RRGGBB), memoized per
    /// (content, size, color).
    pub fn text_mask(&mut self, content: &str, px_size: f32, color_rgb: u32) -> &TextMask {
        let key = (content.to_owned(), px_size.to_bits(), color_rgb);
        if !self.masks.contains_key(&key) {
            if self.masks.len() >= MASK_CACHE_CAP {
                self.masks.clear();
            }
            let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
            let mask =
                platform::rasterize(&normalized, px_size, color_rgb, self.primary.as_ref());
            self.masks.insert(key.clone(), mask);
        }
        &self.masks[&key]
    }

    /// Size (w, h) of the annotation text as rendered by `text_mask`
    /// (independent of color).
    pub fn measure_annotation(&mut self, content: &str, px_size: f32, color_rgb: u32) -> (u32, u32) {
        let m = self.text_mask(content, px_size, color_rgb);
        (m.w, m.h)
    }
}

/// Blend a coverage bitmap of `bw × bh` into the softbuffer at (x, y).
#[allow(clippy::too_many_arguments)]
fn blit_coverage_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    bw: u32,
    bh: u32,
    cov: &[u8],
    color: (u32, u32, u32),
) {
    let (fr, fg, fb) = color;
    for gy in 0..bh as i32 {
        let py = y + gy;
        if py < 0 || py >= h as i32 {
            continue;
        }
        for gx in 0..bw as i32 {
            let a = cov[(gy * bw as i32 + gx) as usize] as u32;
            if a == 0 {
                continue;
            }
            let px = x + gx;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let idx = (py as u32 * w + px as u32) as usize;
            let bg = buf[idx];
            let inv = 255 - a;
            let r = (fr * a + ((bg >> 16) & 0xFF) * inv) / 255;
            let g = (fg * a + ((bg >> 8) & 0xFF) * inv) / 255;
            let b = (fb * a + (bg & 0xFF) * inv) / 255;
            buf[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

/// Premultiplied source-over of one RGBA source pixel onto (r, g, b).
#[inline(always)]
fn over(src: &[u8], dst: (u32, u32, u32)) -> (u32, u32, u32) {
    let inv = 255 - src[3] as u32;
    (
        (src[0] as u32 + dst.0 * inv / 255).min(255),
        (src[1] as u32 + dst.1 * inv / 255).min(255),
        (src[2] as u32 + dst.2 * inv / 255).min(255),
    )
}

/// Draw a text bitmap into the softbuffer (0x00RRGGBB). `scale` is the
/// window/frame pixel ratio (1.0 in the common case); non-1 scales use
/// nearest sampling.
pub fn blit_mask_buf(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, mask: &TextMask, scale: f32) {
    if mask.w == 0 || mask.h == 0 {
        return;
    }
    let unit = (scale - 1.0).abs() < 1e-3;
    let (dw, dh) = if unit {
        (mask.w, mask.h)
    } else {
        (
            ((mask.w as f32) * scale).round().max(0.0) as u32,
            ((mask.h as f32) * scale).round().max(0.0) as u32,
        )
    };
    for dy in 0..dh as i32 {
        let py = y + dy;
        if py < 0 || py >= h as i32 {
            continue;
        }
        let sy = if unit { dy as u32 } else { ((dy as u64 * mask.h as u64) / dh as u64) as u32 };
        for dx in 0..dw as i32 {
            let px = x + dx;
            if px < 0 || px >= w as i32 {
                continue;
            }
            let sx = if unit { dx as u32 } else { ((dx as u64 * mask.w as u64) / dw as u64) as u32 };
            let si = ((sy * mask.w + sx) * 4) as usize;
            let src = &mask.rgba[si..si + 4];
            if src[3] == 0 {
                continue;
            }
            let idx = (py as u32 * w + px as u32) as usize;
            let bg = buf[idx];
            let (r, g, b) = over(src, ((bg >> 16) & 0xFF, (bg >> 8) & 0xFF, bg & 0xFF));
            buf[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

/// Composite a text bitmap over an RGBA image (export path). Destination
/// alpha is preserved.
pub fn blit_mask_rgba(img: &mut RgbaImage, x: i32, y: i32, mask: &TextMask) {
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    for gy in 0..mask.h as i32 {
        let py = y + gy;
        if py < 0 || py >= ih {
            continue;
        }
        for gx in 0..mask.w as i32 {
            let px = x + gx;
            if px < 0 || px >= iw {
                continue;
            }
            let si = ((gy as u32 * mask.w + gx as u32) * 4) as usize;
            let src = &mask.rgba[si..si + 4];
            if src[3] == 0 {
                continue;
            }
            let bg = img.get_pixel_mut(px as u32, py as u32);
            let (r, g, b) = over(src, (bg[0] as u32, bg[1] as u32, bg[2] as u32));
            bg[0] = r as u8;
            bg[1] = g as u8;
            bg[2] = b as u8;
        }
    }
}

// ------------------------------------------------------------ platform ----

#[cfg(target_os = "macos")]
mod platform {
    use super::TextMask;
    use objc2::rc::autoreleasepool;
    use objc2_core_graphics::{
        kCGColorSpaceSRGB, CGBitmapContextCreate, CGColorSpace, CGContext, CGImageAlphaInfo,
    };
    use objc2::runtime::AnyObject;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{
        NSAttributedStringNSExtendedStringDrawing, NSColor, NSFont, NSFontAttributeName,
        NSForegroundColorAttributeName, NSGraphicsContext, NSStringDrawingOptions,
    };
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};
    use objc2_foundation::{NSAttributedString, NSDictionary, NSString};

    /// Font used for annotation text — shared with the NSTextView editor so
    /// on-screen editing and the exported PNG match.
    pub fn annotation_font(px: f64) -> objc2::rc::Retained<NSFont> {
        NSFont::systemFontOfSize(px)
    }

    pub fn rasterize(
        content: &str,
        px: f32,
        color_rgb: u32,
        _fallback: Option<&fontdue::Font>,
    ) -> TextMask {
        if content.is_empty() {
            return TextMask::default();
        }
        autoreleasepool(|_| unsafe {
            let font = annotation_font(px as f64);
            // Draw in the real annotation color: AppKit renders color glyphs
            // (emoji) with their own palette regardless of foreground color.
            let color = NSColor::colorWithSRGBRed_green_blue_alpha(
                ((color_rgb >> 16) & 0xFF) as f64 / 255.0,
                ((color_rgb >> 8) & 0xFF) as f64 / 255.0,
                (color_rgb & 0xFF) as f64 / 255.0,
                1.0,
            );
            let keys: [&NSString; 2] = [NSFontAttributeName, NSForegroundColorAttributeName];
            let objs: [&AnyObject; 2] = [font.as_ref(), color.as_ref()];
            let attrs = NSDictionary::from_slices(&keys, &objs);
            let astr = NSAttributedString::initWithString_attributes(
                NSAttributedString::alloc(),
                &NSString::from_str(content),
                Some(&attrs),
            );
            let opts = NSStringDrawingOptions::UsesLineFragmentOrigin;
            let bounds =
                astr.boundingRectWithSize_options_context(CGSize::new(1.0e7, 1.0e7), opts, None);
            let w = (bounds.size.width.ceil() as usize).max(1) + 2;
            let h = (bounds.size.height.ceil() as usize).max(1) + 2;

            let mut pixels = vec![0u8; w * h * 4];
            // sRGB so the drawn color matches `NSColor` sRGB components 1:1.
            let cs = CGColorSpace::with_name(Some(kCGColorSpaceSRGB));
            let Some(ctx) = CGBitmapContextCreate(
                pixels.as_mut_ptr().cast(),
                w,
                h,
                8,
                w * 4,
                cs.as_deref(),
                CGImageAlphaInfo::PremultipliedLast.0,
            ) else {
                return TextMask::default();
            };
            // Grayscale AA → a clean coverage mask (no LCD color fringes).
            CGContext::set_should_smooth_fonts(Some(&ctx), false);
            CGContext::set_allows_font_smoothing(Some(&ctx), false);
            // Flip to a top-left origin to match `flipped: true` below.
            CGContext::translate_ctm(Some(&ctx), 0.0, h as f64);
            CGContext::scale_ctm(Some(&ctx), 1.0, -1.0);

            let ns_ctx = NSGraphicsContext::graphicsContextWithCGContext_flipped(&ctx, true);
            let prev = NSGraphicsContext::currentContext();
            NSGraphicsContext::setCurrentContext(Some(&ns_ctx));
            astr.drawWithRect_options_context(
                CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)),
                opts,
                None,
            );
            NSGraphicsContext::setCurrentContext(prev.as_deref());
            drop(ns_ctx);
            CGContext::flush(Some(&ctx));
            drop(ctx);

            // PremultipliedLast RGBA in memory == our TextMask layout.
            TextMask { w: w as u32, h: h as u32, rgba: pixels }
        })
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::TextMask;
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::*;

    pub const FACE: &str = "Microsoft YaHei UI";

    pub fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Annotation font — shared with the EDIT editor so editing and the
    /// exported PNG match.
    pub fn create_font(px: i32) -> HFONT {
        let face = wide(FACE);
        unsafe {
            CreateFontW(
                -px.max(1),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                OUT_DEFAULT_PRECIS as u32,
                CLIP_DEFAULT_PRECIS as u32,
                ANTIALIASED_QUALITY as u32,
                (DEFAULT_PITCH | FF_DONTCARE) as u32,
                face.as_ptr(),
            )
        }
    }

    pub fn rasterize(
        content: &str,
        px: f32,
        color_rgb: u32,
        _fallback: Option<&fontdue::Font>,
    ) -> TextMask {
        if content.is_empty() {
            return TextMask::default();
        }
        let text: Vec<u16> = content.replace('\n', "\r\n").encode_utf16().collect();
        unsafe {
            let hdc = CreateCompatibleDC(std::ptr::null_mut());
            if hdc.is_null() {
                return TextMask::default();
            }
            let font = create_font(px.round() as i32);
            let old_font = SelectObject(hdc, font as HGDIOBJ);
            let flags = DT_NOPREFIX | DT_EXPANDTABS | DT_LEFT | DT_TOP;
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            DrawTextW(hdc, text.as_ptr(), text.len() as i32, &mut rc, flags | DT_CALCRECT);
            let w = (rc.right.max(1) + 2) as i32;
            let h = (rc.bottom.max(1) + 2) as i32;

            let mut bmi: BITMAPINFO = std::mem::zeroed();
            bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
            bmi.bmiHeader.biWidth = w;
            bmi.bmiHeader.biHeight = -h; // top-down
            bmi.bmiHeader.biPlanes = 1;
            bmi.bmiHeader.biBitCount = 32;
            bmi.bmiHeader.biCompression = BI_RGB;
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
            let mut mask = TextMask::default();
            if !dib.is_null() && !bits.is_null() {
                let old_bmp = SelectObject(hdc, dib as HGDIOBJ);
                SetBkMode(hdc, TRANSPARENT as _);
                SetTextColor(hdc, 0x00FF_FFFF);
                let mut draw = RECT { left: 0, top: 0, right: w, bottom: h };
                DrawTextW(hdc, text.as_ptr(), text.len() as i32, &mut draw, flags);
                GdiFlush();
                let px_bytes = std::slice::from_raw_parts(bits as *const u8, (w * h * 4) as usize);
                // GDI DrawText has no color-glyph support; render a white
                // coverage mask and tint it.
                let alpha: Vec<u8> =
                    px_bytes.chunks_exact(4).map(|p| p[0].max(p[1]).max(p[2])).collect();
                mask = TextMask::from_coverage(w as u32, h as u32, &alpha, color_rgb);
                SelectObject(hdc, old_bmp);
                DeleteObject(dib as HGDIOBJ);
            }
            SelectObject(hdc, old_font);
            DeleteObject(font as HGDIOBJ);
            DeleteDC(hdc);
            mask
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use super::TextMask;

    /// Fallback: embedded font only (no CJK).
    pub fn rasterize(content: &str, px: f32, color_rgb: u32, font: Option<&fontdue::Font>) -> TextMask {
        let Some(font) = font else { return TextMask::default() };
        let lines: Vec<&str> = content.split('\n').collect();
        let line_h = (px * 1.2).round() as i32;
        let width = lines
            .iter()
            .map(|l| l.chars().map(|c| font.metrics(c, px).advance_width).sum::<f32>().ceil() as i32)
            .max()
            .unwrap_or(0)
            .max(1)
            + 2;
        let height = line_h * lines.len() as i32 + 2;
        let mut alpha = vec![0u8; (width * height) as usize];
        let ascent = (px * 0.8) as i32;
        for (i, line) in lines.iter().enumerate() {
            let mut pen = 0.0f32;
            for c in line.chars() {
                let (m, bmp) = font.rasterize(c, px);
                let gx = pen.round() as i32 + m.xmin;
                let gy = i as i32 * line_h + ascent - m.height as i32 - m.ymin;
                for y in 0..m.height as i32 {
                    for x in 0..m.width as i32 {
                        let (px_, py) = (gx + x, gy + y);
                        if px_ >= 0 && py >= 0 && px_ < width && py < height {
                            let d = &mut alpha[(py * width + px_) as usize];
                            *d = (*d).max(bmp[(y * m.width as i32 + x) as usize]);
                        }
                    }
                }
                pen += m.advance_width;
            }
        }
        TextMask::from_coverage(width as u32, height as u32, &alpha, color_rgb)
    }
}

#[cfg(target_os = "macos")]
pub use platform::annotation_font;
#[cfg(target_os = "windows")]
pub use platform::{create_font as create_annotation_font, wide};

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
        assert!(buf.iter().any(|&p| p != 0));
    }

    #[test]
    fn missing_font_is_silent_noop() {
        let mut font = Font { primary: None, cache: HashMap::new(), masks: HashMap::new() };
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(buf.iter().all(|&p| p == 0));
    }

    #[test]
    fn empty_annotation_mask_is_empty() {
        let mut font = Font::embedded();
        assert_eq!(font.measure_annotation("", 24.0, 0xFFFFFF), (0, 0));
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn cjk_annotation_mask_has_coverage() {
        let mut font = Font::embedded();
        let m = font.text_mask("你好 QuickShot", 32.0, 0xFF0000).clone();
        assert!(m.w > 32 && m.h >= 32, "mask {}x{}", m.w, m.h);
        assert!(m.rgba.chunks_exact(4).any(|p| p[3] > 200), "CJK text produced no coverage");
        // Opaque glyph pixels are drawn in the annotation color (red).
        let p = m.rgba.chunks_exact(4).find(|p| p[3] == 255).expect("opaque px");
        assert!(p[0] > 200 && p[1] < 60 && p[2] < 60, "expected red, got {p:?}");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn emoji_keeps_its_own_colors() {
        // 🟦 (blue square) drawn with a RED foreground must stay blue.
        let mut font = Font::embedded();
        let m = font.text_mask("\u{1F7E6}", 48.0, 0xFF0000).clone();
        let opaque: Vec<&[u8]> = m.rgba.chunks_exact(4).filter(|p| p[3] == 255).collect();
        assert!(!opaque.is_empty(), "emoji produced no opaque pixels");
        let bluish = opaque.iter().filter(|p| p[2] > 150 && p[0] < 120).count();
        let reddish = opaque.iter().filter(|p| p[0] > 150 && p[2] < 80).count();
        assert!(bluish > reddish * 4, "emoji was tinted: blue={bluish} red={reddish}");
    }

    #[test]
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    fn multiline_mask_is_taller() {
        let mut font = Font::embedded();
        let (_, h1) = font.measure_annotation("a", 24.0, 0xFFFFFF);
        let (_, h2) = font.measure_annotation("a\nb", 24.0, 0xFFFFFF);
        assert!(h2 > h1 + 15, "h1={h1} h2={h2}");
    }

    #[test]
    fn mask_blit_scaled_and_rgba() {
        let mask = TextMask::from_coverage(2, 2, &[255, 0, 0, 255], 0x00FF0000);
        let mut buf = vec![0u32; 16];
        blit_mask_buf(&mut buf, 4, 4, 0, 0, &mask, 2.0);
        assert_eq!(buf[0], 0x00FF0000);
        assert_eq!(buf[1], 0x00FF0000);
        assert_eq!(buf[2], 0);
        let green = TextMask::from_coverage(2, 2, &[255, 0, 0, 255], 0x0000FF00);
        let mut img = RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        blit_mask_rgba(&mut img, 0, 0, &green);
        assert_eq!(img.get_pixel(0, 0).0, [0, 255, 0, 255]);
        assert_eq!(img.get_pixel(1, 0).0, [0, 0, 0, 255]);
        // Half coverage blends 50/50 over white.
        let half = TextMask::from_coverage(1, 1, &[128], 0x00000000);
        let mut img = RgbaImage::from_pixel(1, 1, image::Rgba([255, 255, 255, 255]));
        blit_mask_rgba(&mut img, 0, 0, &half);
        let v = img.get_pixel(0, 0).0[0];
        assert!((125..=129).contains(&v), "got {v}");
    }
}
