//! Screen capture.
//!
//! Hot path for "hotkey → overlay visible". Design notes:
//!   * macOS bypasses xcap and calls `CGDisplayCreateImage` directly. xcap
//!     copies the pixel data twice (`to_vec` + row-strip) and then swizzles in
//!     a third pass; we instead convert BGRA → (bright RGBA frame, dimmed
//!     0x00RRGGBB background) in ONE pass, split across a few threads.
//!   * Monitor lookup on macOS is a single `CGGetDisplaysWithPoint` call
//!     (xcap's `Monitor::from_point` enumerates every display + mode).
//!   * `warm_up()` runs once on a background thread at startup so the first
//!     capture doesn't pay WindowServer / CoreGraphics cold-start costs.

use anyhow::{Context, Result};
use image::RgbaImage;

/// Monitor geometry. On macOS `x/y/width/height` are CG screen coordinates in
/// points (logical); on Windows they're physical pixels (xcap convention).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorGeom {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Backing scale (physical px per logical point). Used to scale window
    /// snap rects before the overlay window (and its scale factor) exists.
    pub scale: f64,
    #[cfg(target_os = "macos")]
    pub display_id: u32,
}

/// One captured screen, already converted into both representations the
/// overlay needs.
pub struct Captured {
    /// Full-brightness frame, RGBA8 (export / magnifier / mosaic source).
    pub frame: RgbaImage,
    /// Pre-dimmed background, softbuffer layout 0x00RRGGBB with every channel
    /// halved. Empty when the caller asked for `with_dimmed = false`.
    pub dimmed: Vec<u32>,
}

/// Captures the monitor under the cursor. Used by the full-screen capture
/// path (no overlay → no dimmed buffer needed).
pub fn capture_at_cursor() -> Result<(Captured, MonitorGeom)> {
    let geom = monitor_at_cursor()?;
    let cap = capture(&geom, false)?;
    Ok((cap, geom))
}

/// Number of worker threads for the pixel conversion pass. Conversion is
/// memory-bound; beyond ~4 threads there's no further gain.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn convert_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 4)
}

/// Convert one row of 4-byte pixels into an RGBA row + optional dimmed row.
/// `bgra = true` for CoreGraphics' little-endian BGRA layout.
#[inline(always)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn convert_row(src: &[u8], dst_rgba: &mut [u8], dst_dim: Option<&mut [u32]>, bgra: bool) {
    let (ri, bi) = if bgra { (2, 0) } else { (0, 2) };
    match dst_dim {
        Some(dim) => {
            for ((s, d), m) in src
                .chunks_exact(4)
                .zip(dst_rgba.chunks_exact_mut(4))
                .zip(dim.iter_mut())
            {
                let (r, g, b) = (s[ri], s[1], s[bi]);
                d[0] = r;
                d[1] = g;
                d[2] = b;
                d[3] = 0xFF;
                *m = (((r >> 1) as u32) << 16) | (((g >> 1) as u32) << 8) | ((b >> 1) as u32);
            }
        }
        None => {
            for (s, d) in src.chunks_exact(4).zip(dst_rgba.chunks_exact_mut(4)) {
                d[0] = s[ri];
                d[1] = s[1];
                d[2] = s[bi];
                d[3] = 0xFF;
            }
        }
    }
}

/// Parallel single-pass conversion of a strided 32-bpp source buffer.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn convert_pixels(
    src: &[u8],
    width: usize,
    height: usize,
    bytes_per_row: usize,
    bgra: bool,
    with_dimmed: bool,
) -> Captured {
    let mut rgba = vec![0u8; width * height * 4];
    let mut dimmed = if with_dimmed { vec![0u32; width * height] } else { Vec::new() };

    let threads = convert_threads();
    let rows_per = height.div_ceil(threads).max(1);

    std::thread::scope(|s| {
        let rgba_chunks = rgba.chunks_mut(rows_per * width * 4);
        let mut dim_chunks: Vec<Option<&mut [u32]>> = if with_dimmed {
            dimmed.chunks_mut(rows_per * width).map(Some).collect()
        } else {
            Vec::new()
        };
        for (i, rgba_chunk) in rgba_chunks.enumerate() {
            let dim_chunk = if with_dimmed { dim_chunks[i].take() } else { None };
            let row0 = i * rows_per;
            s.spawn(move || {
                let rows = rgba_chunk.len() / (width * 4);
                let mut dim_chunk = dim_chunk;
                for r in 0..rows {
                    let y = row0 + r;
                    let src_row = &src[y * bytes_per_row..y * bytes_per_row + width * 4];
                    let dst_row = &mut rgba_chunk[r * width * 4..(r + 1) * width * 4];
                    let dim_row = dim_chunk
                        .as_deref_mut()
                        .map(|d| &mut d[r * width..(r + 1) * width]);
                    convert_row(src_row, dst_row, dim_row, bgra);
                }
            });
        }
    });

    let frame = RgbaImage::from_raw(width as u32, height as u32, rgba)
        .expect("buffer length matches dimensions");
    Captured { frame, dimmed }
}

// ---------------------------------------------------------------- macOS ----

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_graphics::display::CGDisplay;
    use core_graphics::geometry::CGPoint;

    extern "C" {
        fn CGGetDisplaysWithPoint(
            point: CGPoint,
            max_displays: u32,
            displays: *mut u32,
            matching_display_count: *mut u32,
        ) -> i32;
    }

    fn geom_for(id: u32) -> MonitorGeom {
        let d = CGDisplay::new(id);
        let b = d.bounds();
        let logical_w = b.size.width.max(1.0);
        let scale = d
            .display_mode()
            .map(|m| m.pixel_width() as f64 / logical_w)
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0);
        MonitorGeom {
            x: b.origin.x as i32,
            y: b.origin.y as i32,
            width: b.size.width as u32,
            height: b.size.height as u32,
            scale,
            display_id: id,
        }
    }

    pub fn monitor_at_cursor() -> Result<MonitorGeom> {
        let (cx, cy) = cursor_position();
        let mut id: u32 = 0;
        let mut count: u32 = 0;
        let err = unsafe {
            CGGetDisplaysWithPoint(CGPoint::new(cx, cy), 1, &mut id, &mut count)
        };
        if err != 0 || count == 0 {
            // Cursor in a gap between displays (or API failure) → main display.
            id = CGDisplay::main().id;
        }
        Ok(geom_for(id))
    }

    pub fn capture(geom: &MonitorGeom, with_dimmed: bool) -> Result<Captured> {
        let t_start = std::time::Instant::now();
        let image = CGDisplay::new(geom.display_id)
            .image()
            .context("CGDisplayCreateImage returned null (Screen Recording permission?)")?;
        let (w, h) = (image.width(), image.height());
        let bpr = image.bytes_per_row();
        if image.bits_per_pixel() != 32 || bpr < w * 4 {
            anyhow::bail!(
                "unexpected CGImage layout: {} bpp, {} bytes/row for width {}",
                image.bits_per_pixel(),
                bpr,
                w
            );
        }
        let t_img = std::time::Instant::now();
        let data = image.data();
        let bytes = data.bytes();
        if bytes.len() < bpr * h {
            anyhow::bail!("CGImage data shorter than expected");
        }
        // CGDisplayCreateImage yields 32-bit little-endian BGRA (premultiplied
        // first / skip-first) on every Mac we ship to — the same assumption
        // xcap makes.
        let out = convert_pixels(bytes, w, h, bpr, true, with_dimmed);
        if std::env::var_os("QUICKSHOT_DEBUG").is_some() {
            eprintln!(
                "QuickShot timing: CGDisplayCreateImage {:?}, pixel data+convert {:?}",
                t_img - t_start,
                t_img.elapsed()
            );
        }
        Ok(out)
    }

    pub fn warm_up() {
        // A 1×1 capture forces the WindowServer connection + capture service
        // to spin up; the window list call warms CGWindowList for snap.
        let main = CGDisplay::main();
        let rect = core_graphics::geometry::CGRect::new(
            &CGPoint::new(main.bounds().origin.x, main.bounds().origin.y),
            &core_graphics::geometry::CGSize::new(1.0, 1.0),
        );
        let _ = main.image_for_rect(rect);
        let _ = CGDisplay::window_list_info(
            core_graphics::display::kCGWindowListOptionOnScreenOnly,
            None,
        );
    }

    /// Global cursor position in CG coordinates (origin = top-left of primary).
    pub(super) fn cursor_position() -> (f64, f64) {
        #[repr(C)]
        #[derive(Copy, Clone)]
        struct RawPoint {
            x: f64,
            y: f64,
        }
        extern "C" {
            fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
            fn CGEventGetLocation(event: *const std::ffi::c_void) -> RawPoint;
            fn CFRelease(cf: *const std::ffi::c_void);
        }
        unsafe {
            let event = CGEventCreate(std::ptr::null());
            if event.is_null() {
                return (0.0, 0.0);
            }
            let p = CGEventGetLocation(event);
            CFRelease(event);
            (p.x, p.y)
        }
    }
}

// ------------------------------------------------------ Windows / other ----

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    use xcap::Monitor;

    fn find_monitor(x: i32, y: i32) -> Result<Monitor> {
        Monitor::from_point(x, y)
            .or_else(|_| {
                Monitor::all()?
                    .into_iter()
                    .find(|m| m.is_primary().unwrap_or(false))
                    .ok_or_else(|| xcap::XCapError::new("no primary monitor"))
            })
            .context("find monitor at cursor")
    }

    pub fn monitor_at_cursor() -> Result<MonitorGeom> {
        let (cx, cy) = cursor_position();
        let m = find_monitor(cx, cy)?;
        Ok(MonitorGeom {
            x: m.x().unwrap_or(0),
            y: m.y().unwrap_or(0),
            width: m.width().unwrap_or(0),
            height: m.height().unwrap_or(0),
            scale: m.scale_factor().map(|s| s as f64).unwrap_or(1.0),
        })
    }

    pub fn capture(geom: &MonitorGeom, with_dimmed: bool) -> Result<Captured> {
        let m = find_monitor(geom.x + 1, geom.y + 1)?;
        let img = m.capture_image().context("capture monitor image")?;
        if !with_dimmed {
            return Ok(Captured { frame: img, dimmed: Vec::new() });
        }
        let dimmed = crate::overlay::render::precompute_dimmed(&img);
        Ok(Captured { frame: img, dimmed })
    }

    pub fn warm_up() {
        let _ = Monitor::all();
    }

    #[cfg(target_os = "windows")]
    pub(super) fn cursor_position() -> (i32, i32) {
        use windows_sys::Win32::Foundation::POINT;
        use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut p = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut p) == 0 {
                return (0, 0);
            }
        }
        (p.x, p.y)
    }

    #[cfg(not(target_os = "windows"))]
    fn cursor_position() -> (i32, i32) {
        (0, 0)
    }
}

pub use imp::{capture, monitor_at_cursor, warm_up};

/// Global cursor position in the same coordinate space as `MonitorGeom`
/// (CG points on macOS, physical px on Windows).
pub fn cursor_global() -> Option<(f64, f64)> {
    #[cfg(target_os = "macos")]
    {
        Some(imp::cursor_position())
    }
    #[cfg(target_os = "windows")]
    {
        let (x, y) = imp::cursor_position();
        Some((x as f64, y as f64))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_matches_reference_bgra_with_stride() {
        let (w, h, bpr) = (5usize, 7usize, 5 * 4 + 12);
        let mut src = vec![0u8; bpr * h];
        for y in 0..h {
            for x in 0..w {
                let i = y * bpr + x * 4;
                src[i] = (x * 31 + y) as u8; // B
                src[i + 1] = (y * 17 + 3) as u8; // G
                src[i + 2] = (x * 7 + y * 5) as u8; // R
                src[i + 3] = 0x12;
            }
        }
        let cap = convert_pixels(&src, w, h, bpr, true, true);
        assert_eq!(cap.frame.dimensions(), (w as u32, h as u32));
        for y in 0..h {
            for x in 0..w {
                let i = y * bpr + x * 4;
                let (b, g, r) = (src[i], src[i + 1], src[i + 2]);
                assert_eq!(cap.frame.get_pixel(x as u32, y as u32).0, [r, g, b, 255]);
                let expect = (((r >> 1) as u32) << 16) | (((g >> 1) as u32) << 8) | (b >> 1) as u32;
                assert_eq!(cap.dimmed[y * w + x], expect);
            }
        }
        // Must equal the overlay's reference precompute.
        assert_eq!(cap.dimmed, crate::overlay::render::precompute_dimmed(&cap.frame));
    }

    #[test]
    fn convert_without_dimmed_leaves_it_empty() {
        let src = vec![9u8; 3 * 2 * 4];
        let cap = convert_pixels(&src, 3, 2, 12, false, false);
        assert!(cap.dimmed.is_empty());
        assert_eq!(cap.frame.get_pixel(0, 0).0, [9, 9, 9, 255]);
    }

    /// `cargo test --release capture_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn capture_timing() {
        use std::time::Instant;
        for i in 0..5 {
            let t = Instant::now();
            let geom = monitor_at_cursor().expect("monitor");
            let t_geom = t.elapsed();
            let (cap, list) = std::thread::scope(|s| {
                let h = s.spawn(|| {
                    crate::overlay::snap::enumerate_windows(&geom, std::process::id(), geom.scale as f32)
                });
                (capture(&geom, true).expect("capture"), h.join().unwrap())
            });
            println!(
                "run {i}: monitor {:?}, capture+convert+enum {:?} ({}x{}, {} windows, scale {})",
                t_geom,
                t.elapsed(),
                cap.frame.width(),
                cap.frame.height(),
                list.len(),
                geom.scale
            );
        }
        let t = Instant::now();
        let _f = crate::text::Font::embedded();
        let _i = crate::icon_font::IconFont::embedded();
        println!("fonts (label+icon) {:?}", t.elapsed());
        let mut f = crate::text::Font::embedded();
        let t = Instant::now();
        let m = f.text_mask("你好，QuickShot 截图\n第二行", 40.0, 0xFF3B30).clone();
        println!("first CJK text mask {:?} ({}x{})", t.elapsed(), m.w, m.h);
        let t = Instant::now();
        let _ = f.text_mask("再来一次", 40.0, 0xFF3B30);
        println!("second CJK text mask {:?}", t.elapsed());
    }

    #[test]
    #[ignore]
    fn capture_smoke() {
        let (cap, geom) = capture_at_cursor().expect("capture");
        assert!(cap.frame.width() > 0 && cap.frame.height() > 0);
        println!("captured {:?} at {:?}", cap.frame.dimensions(), geom);
    }
}
