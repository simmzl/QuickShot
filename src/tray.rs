use anyhow::{Context, Result};
use tray_icon::{
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// Keeps the tray icon alive for the program's lifetime.
/// Dropping removes the icon from the menu bar.
/// Holds a reference to the autostart CheckMenuItem so App can re-sync
/// its checked state after install/uninstall.
pub struct TrayGuard {
    _tray: TrayIcon,
    pub autostart_item: CheckMenuItem,
}

const ICON_BYTES: &[u8] = include_bytes!("../assets/tray-icon.png");

const ID_REGION: &str = "capture-region";
const ID_SCREEN: &str = "capture-screen";
const ID_EDIT_CONFIG: &str = "edit-config";
const ID_TOGGLE_AUTOSTART: &str = "toggle-autostart";
const ID_QUIT: &str = "quit";

/// Install the menu-bar tray icon and spawn the menu-event forwarder.
pub fn install(
    proxy: EventLoopProxy<UserEvent>,
    region_label: &str,
    screen_label: &str,
    autostart_initially_on: bool,
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

    menu.append(&MenuItem::with_id(
        ID_EDIT_CONFIG,
        "Edit Config\u{2026}",
        true,
        None,
    ))
    .context("append Edit Config menu item")?;

    let autostart_item = CheckMenuItem::with_id(
        ID_TOGGLE_AUTOSTART,
        "Start at Login",
        true,
        autostart_initially_on,
        None,
    );
    menu.append(&autostart_item)
        .context("append Start at Login menu item")?;

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

    Ok(TrayGuard {
        _tray: tray,
        autostart_item,
    })
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
                } else if event.id == ID_EDIT_CONFIG {
                    (Some(UserEvent::EditConfig), None)
                } else if event.id == ID_TOGGLE_AUTOSTART {
                    (Some(UserEvent::ToggleAutostart), None)
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
