// Build script: embed the application icon into the Windows executable so it
// shows a real icon in Explorer / the taskbar / the desktop (the tray icon is
// a separate, runtime-loaded image). No-op on non-Windows hosts.
fn main() {
    #[cfg(target_os = "macos")]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rerun-if-changed=src/macos_feedback.m");
        cc::Build::new()
            .file("src/macos_feedback.m")
            .flag("-fobjc-arc")
            .compile("quickshot_feedback");
    }
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
