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
    region_item: MenuItem,
    screen_item: MenuItem,
}

impl TrayGuard {
    /// Update the hotkey hint suffix on the two capture menu items. Called
    /// after a config reload swaps the bound hotkeys to a different combo.
    pub fn set_capture_labels(&self, region_label: &str, screen_label: &str) {
        self.region_item
            .set_text(format!("Capture Region    {region_label}"));
        self.screen_item
            .set_text(format!("Capture Screen    {screen_label}"));
    }
}

// macOS uses a template image (black on transparent, auto-tinted by the
// system to match the menu-bar theme). Windows/Linux trays don't support
// template images, so a pure-black icon would be near-invisible on dark
// taskbars — use a full-color icon there instead.
#[cfg(target_os = "macos")]
const ICON_BYTES: &[u8] = include_bytes!("../assets/tray-icon.png");
#[cfg(not(target_os = "macos"))]
const ICON_BYTES: &[u8] = include_bytes!("../assets/windows-tray-icon.png");

/// Whether the tray icon should be treated as a macOS template image.
#[cfg(target_os = "macos")]
const ICON_AS_TEMPLATE: bool = true;
#[cfg(not(target_os = "macos"))]
const ICON_AS_TEMPLATE: bool = false;

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

    let region_item = MenuItem::with_id(
        ID_REGION,
        format!("Capture Region    {region_label}"),
        true,
        None,
    );
    menu.append(&region_item)
        .context("append Capture Region menu item")?;
    let screen_item = MenuItem::with_id(
        ID_SCREEN,
        format!("Capture Screen    {screen_label}"),
        true,
        None,
    );
    menu.append(&screen_item)
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

    // Template image (macOS only): the system auto-inverts non-transparent
    // pixels based on menu-bar theme (black in light mode, white in dark
    // mode), the way WiFi/battery icons stay visible in both themes. On
    // Windows/Linux this is false and we ship a full-color icon instead.
    let tray = TrayIconBuilder::new()
        .with_tooltip("QuickShot")
        .with_icon(icon)
        .with_icon_as_template(ICON_AS_TEMPLATE)
        .with_menu(Box::new(menu))
        .build()
        .context("build tray icon")?;

    spawn_forwarder(proxy);

    Ok(TrayGuard {
        _tray: tray,
        autostart_item,
        region_item,
        screen_item,
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
