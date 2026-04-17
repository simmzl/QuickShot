use anyhow::{Context, Result};
use image::RgbaImage;
use xcap::Monitor;

/// Monitor geometry in CG screen coordinates.
#[allow(dead_code)]
pub struct MonitorGeom {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Captures the monitor under the current cursor position.
/// Falls back to the primary monitor if detection fails.
/// Returns the RGBA frame and the monitor's screen-space geometry.
pub fn capture_at_cursor() -> Result<(RgbaImage, MonitorGeom)> {
    let (cx, cy) = cursor_position();
    let monitor = Monitor::from_point(cx as i32, cy as i32)
        .or_else(|_| {
            // Fallback: primary monitor
            Monitor::all()?
                .into_iter()
                .find(|m| m.is_primary().unwrap_or(false))
                .ok_or_else(|| xcap::XCapError::new("no primary monitor"))
        })
        .context("find monitor at cursor")?;

    let geom = MonitorGeom {
        x: monitor.x().unwrap_or(0),
        y: monitor.y().unwrap_or(0),
        width: monitor.width().unwrap_or(0),
        height: monitor.height().unwrap_or(0),
    };
    let frame = monitor.capture_image().context("capture monitor image")?;
    Ok((frame, geom))
}

/// Get global cursor position in CG screen coordinates (origin = top-left of primary display).
#[cfg(target_os = "macos")]
fn cursor_position() -> (f64, f64) {
    #[repr(C)]
    #[derive(Copy, Clone)]
    struct CGPoint {
        x: f64,
        y: f64,
    }
    extern "C" {
        fn CGEventCreate(source: *const std::ffi::c_void) -> *mut std::ffi::c_void;
        fn CGEventGetLocation(event: *const std::ffi::c_void) -> CGPoint;
        fn CFRelease(cf: *const std::ffi::c_void);
    }
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return (0.0, 0.0);
        }
        let point = CGEventGetLocation(event);
        CFRelease(event);
        (point.x, point.y)
    }
}

#[cfg(target_os = "windows")]
fn cursor_position() -> (f64, f64) {
    // TODO: use GetCursorPos from winapi
    (0.0, 0.0)
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn cursor_position() -> (f64, f64) {
    (0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn capture_smoke() {
        let (img, geom) = capture_at_cursor().expect("capture");
        assert!(img.width() > 0 && img.height() > 0);
        println!(
            "captured {}x{} at ({}, {})",
            img.width(),
            img.height(),
            geom.x,
            geom.y
        );
    }
}
