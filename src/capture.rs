use anyhow::{anyhow, Context, Result};
use image::RgbaImage;
use xcap::Monitor;

/// Captures the primary monitor and returns its RGBA pixel buffer.
///
/// API note (xcap 0.9): `Monitor::is_primary()` returns `XCapResult<bool>` rather
/// than a plain `bool`, so we call `.unwrap_or(false)` to treat any error as
/// "not primary" and keep the iterator adapter infallible.
pub fn capture_primary() -> Result<RgbaImage> {
    let monitors = Monitor::all().context("enumerate monitors")?;
    let primary = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .ok_or_else(|| anyhow!("no primary monitor found"))?;
    let frame = primary.capture_image().context("capture monitor image")?;
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn capture_smoke() {
        let img = capture_primary().expect("capture");
        assert!(img.width() > 0 && img.height() > 0);
        println!("captured {}x{}", img.width(), img.height());
    }
}
