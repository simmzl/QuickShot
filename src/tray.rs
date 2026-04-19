use anyhow::{Context, Result};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// Keeps the tray icon alive for the program's lifetime.
/// Dropping removes the icon from the menu bar.
pub struct TrayGuard {
    _tray: TrayIcon,
}

const ICON_BYTES: &[u8] = include_bytes!("../assets/tray-icon.png");

const ID_REGION: &str = "capture-region";
const ID_SCREEN: &str = "capture-screen";
const ID_QUIT: &str = "quit";

/// Install the menu-bar tray icon and spawn the menu-event forwarder.
pub fn install(
    proxy: EventLoopProxy<UserEvent>,
    region_label: &str,
    screen_label: &str,
) -> Result<TrayGuard> {
    let icon = load_icon()?;
    let menu = Menu::new();

    menu.append(&MenuItem::with_id(
        ID_REGION,
        format!("Capture Region    {region_label}"),
        true,
        None,
    ))
    .context("append Capture Region menu item")?;
    menu.append(&MenuItem::with_id(
        ID_SCREEN,
        format!("Capture Screen    {screen_label}"),
        true,
        None,
    ))
    .context("append Capture Screen menu item")?;
    menu.append(&PredefinedMenuItem::separator())
        .context("append separator")?;
    menu.append(&MenuItem::with_id(ID_QUIT, "Quit", true, None))
        .context("append Quit menu item")?;

    let tray = TrayIconBuilder::new()
        .with_tooltip("quickshot")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("build tray icon")?;

    spawn_forwarder(proxy);

    Ok(TrayGuard { _tray: tray })
}

fn load_icon() -> Result<Icon> {
    let img = image::load_from_memory(ICON_BYTES)
        .context("decode tray icon PNG")?
        .into_rgba8();
    let (w, h) = img.dimensions();
    let raw = img.into_raw();
    Icon::from_rgba(raw, w, h).context("build tray Icon from RGBA")
}

fn spawn_forwarder(proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        let rx = MenuEvent::receiver();
        loop {
            if let Ok(event) = rx.try_recv() {
                // In muda 0.17 (used by tray-icon 0.22), MenuId is a newtype `pub struct MenuId(pub String)`.
                // It implements PartialEq<&str>, so we can compare directly: `event.id == ID_REGION`.
                // Fallback: `event.id.0.as_str()` also works via the inner String field.
                let (msg, delay) = if event.id == ID_REGION {
                    (
                        Some(UserEvent::CaptureRegion),
                        Some(std::time::Duration::from_millis(150)),
                    )
                } else if event.id == ID_SCREEN {
                    (
                        Some(UserEvent::CaptureScreen),
                        Some(std::time::Duration::from_millis(150)),
                    )
                } else if event.id == ID_QUIT {
                    (Some(UserEvent::Quit), None)
                } else {
                    (None, None)
                };

                if let Some(m) = msg {
                    if let Some(d) = delay {
                        std::thread::sleep(d);
                    }
                    let _ = proxy.send_event(m);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    });
}
