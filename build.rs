// Build script: embed the application icon into the Windows executable so it
// shows a real icon in Explorer / the taskbar / the desktop (the tray icon is
// a separate, runtime-loaded image). No-op on non-Windows hosts.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/app-icon.ico");
        if let Err(e) = res.compile() {
            // Don't fail the whole build if the resource compiler is missing;
            // the binary just ships without an embedded icon in that case.
            println!("cargo:warning=failed to embed Windows icon: {e}");
        }
    }
}
