mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod overlay;
mod permission;
mod text;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    permission::preflight()?;

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let _hotkey_guard = hotkey::register(proxy)?;

    println!("quickshot running; press Ctrl/Cmd+Shift+A to capture. Ctrl+C to quit.");

    let mut app = app::App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
