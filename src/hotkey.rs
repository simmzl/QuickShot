use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::thread;
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;
use crate::config::ParsedHotkey;

/// Keep the manager alive for the program's lifetime.
/// Dropping it unregisters both hotkeys.
pub struct HotkeyGuard {
    _manager: GlobalHotKeyManager,
}

pub fn register(
    proxy: EventLoopProxy<UserEvent>,
    region: ParsedHotkey,
    fullscreen: ParsedHotkey,
) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new().context("new GlobalHotKeyManager")?;

    let hk_region = HotKey::new(Some(region.modifiers), region.code);
    let hk_screen = HotKey::new(Some(fullscreen.modifiers), fullscreen.code);
    manager
        .register(hk_region)
        .with_context(|| format!("register region hotkey {}", region.raw))?;
    manager
        .register(hk_screen)
        .with_context(|| format!("register fullscreen hotkey {}", fullscreen.raw))?;

    let region_id = hk_region.id();
    let screen_id = hk_screen.id();

    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(event) = receiver.try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            let msg = if event.id == region_id {
                Some(UserEvent::CaptureRegion)
            } else if event.id == screen_id {
                Some(UserEvent::CaptureScreen)
            } else {
                None
            };
            if let Some(m) = msg {
                let _ = proxy.send_event(m);
            }
        }
        thread::sleep(Duration::from_millis(25));
    });

    Ok(HotkeyGuard { _manager: manager })
}
