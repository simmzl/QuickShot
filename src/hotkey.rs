use anyhow::{Context, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use std::thread;
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

/// Keep the manager alive for the program's lifetime.
/// Dropping it unregisters both hotkeys.
pub struct HotkeyGuard {
    _manager: GlobalHotKeyManager,
}

pub fn register(proxy: EventLoopProxy<UserEvent>) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new().context("new GlobalHotKeyManager")?;

    #[cfg(target_os = "macos")]
    let mods = Modifiers::META | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;

    let hk_region = HotKey::new(Some(mods), Code::KeyA);
    let hk_screen = HotKey::new(Some(mods), Code::KeyS);
    manager.register(hk_region).context("register region hotkey")?;
    manager.register(hk_screen).context("register screen hotkey")?;

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
