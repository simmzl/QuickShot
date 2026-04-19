use anyhow::{Context, Result};
use notify_rust::Notification;

/// Fire a "Screenshot copied" system notification with the captured image's
/// physical pixel dimensions in the body. Safe to call on any platform;
/// failures are returned as errors so the caller can log them without
/// aborting the capture flow.
pub fn screenshot_copied(width: u32, height: u32) -> Result<()> {
    Notification::new()
        .appname("quickshot")
        .summary("Screenshot copied")
        .body(&format!("{} \u{00D7} {}", width, height))
        .show()
        .context("display notification")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // No runtime test — the notification API is a side effect against the OS.
    // The compile-only test below exists to catch signature drift in notify-rust.
    #[test]
    fn screenshot_copied_signature_is_stable() {
        let _ = super::screenshot_copied(1920, 1080);
    }
}
