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

    let _hotkey_guard = hotkey::register(
        proxy.clone(),
        config.hotkey.region.clone(),
        config.hotkey.fullscreen.clone(),
    )?;

    println!(
        "quickshot running; {} (region), {} (fullscreen). Quit via tray.",
        config.hotkey.region.raw, config.hotkey.fullscreen.raw
    );

    let mut app = app::App::new(config, proxy);
    event_loop.run_app(&mut app)?;
    Ok(())
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
