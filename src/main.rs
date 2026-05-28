#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod autostart;
mod capture;
mod clipboard;
mod config;
mod crop;
mod file_save;
mod hotkey;
mod icon_font;
#[cfg(target_os = "macos")]
mod macos_objc;
mod notification;
mod overlay;
mod permission;
mod pin;
mod text;
mod tray;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() {
    if let Err(e) = run() {
        let msg = format!("{e:#}");
        eprintln!("quickshot: {msg}");
        #[cfg(target_os = "windows")]
        {
            let _ = rfd::MessageDialog::new()
                .set_title("quickshot")
                .set_description(format!("quickshot failed to start:\n\n{msg}"))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--install-autostart") => {
            return autostart::install();
        }
        Some("--uninstall-autostart") => {
            return autostart::uninstall();
        }
        Some("--help") | Some("-h") => {
            print_usage();
            return Ok(());
        }
        Some(unknown) => {
            eprintln!("unknown argument: {unknown}");
            print_usage();
            std::process::exit(2);
        }
        None => {}
    }

    permission::preflight()?;
    let config = config::Config::load();

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let hotkey_guard = hotkey::register(
        proxy.clone(),
        config.hotkey.region.clone(),
        config.hotkey.fullscreen.clone(),
    )?;

    println!(
        "quickshot running; {} (region), {} (fullscreen). Quit via tray.",
        config.hotkey.region.raw, config.hotkey.fullscreen.raw
    );

    spawn_config_watcher(proxy.clone());

    let mut app = app::App::new(config, proxy, hotkey_guard);
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Poll `config.toml`'s mtime every second on a background thread and emit
/// `UserEvent::ReloadConfig` whenever it changes. Cheap (one stat per
/// second) and avoids pulling in a notify-crate dependency.
fn spawn_config_watcher(proxy: winit::event_loop::EventLoopProxy<app::UserEvent>) {
    let Some(path) = config::config_path() else {
        return;
    };
    std::thread::spawn(move || {
        let mut last_mtime = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok());
            if mtime != last_mtime {
                last_mtime = mtime;
                if mtime.is_some() {
                    let _ = proxy.send_event(app::UserEvent::ReloadConfig);
                }
            }
        }
    });
}

fn print_usage() {
    println!(
        "quickshot \u{2014} small fast screenshot daemon\n\
         \n\
         USAGE:\n\
             quickshot                       run the daemon (default)\n\
             quickshot --install-autostart   install LaunchAgent (macOS)\n\
             quickshot --uninstall-autostart remove LaunchAgent (macOS)\n\
             quickshot --help                show this message\n\
         \n\
         Config: ~/.config/quickshot/config.toml\n"
    );
}
