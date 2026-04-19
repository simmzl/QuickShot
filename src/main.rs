mod app;
mod autostart;
mod capture;
mod clipboard;
mod config;
mod crop;
mod file_save;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    permission::preflight()?;

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let _hotkey_guard = hotkey::register(proxy.clone())?;
    let _tray_guard = tray::install(proxy.clone())?;

    println!(
        "quickshot running; press Cmd/Ctrl+Shift+A for region, Cmd/Ctrl+Shift+S for screen. Quit via tray or Ctrl+C."
    );

    let mut app = app::App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
