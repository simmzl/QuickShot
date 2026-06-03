use anyhow::Result;

#[cfg(target_os = "macos")]
pub fn preflight() -> Result<()> {
    use core_graphics::access::ScreenCaptureAccess;

    let access = ScreenCaptureAccess;
    if access.preflight() {
        return Ok(());
    }

    eprintln!(
        "\nQuickShot needs Screen Recording permission to work on macOS.\n\n\
         1. Open System Settings → Privacy & Security → Screen Recording\n\
         2. Enable `QuickShot` (or the terminal running it) in the list\n\
         3. Relaunch QuickShot\n\n\
         You can trigger the system prompt now by pressing Enter (this will\n\
         also exit QuickShot so you can grant permission and restart).\n"
    );
    let _ = access.request();
    std::process::exit(2);
}

#[cfg(not(target_os = "macos"))]
pub fn preflight() -> Result<()> {
    Ok(())
}
