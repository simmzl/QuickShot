use anyhow::{Context, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
};
use std::thread;
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

pub struct HotkeyGuard {
    _manager: GlobalHotKeyManager,
}

pub fn register(proxy: EventLoopProxy<UserEvent>) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new().context("new GlobalHotKeyManager")?;

    #[cfg(target_os = "macos")]
    let mods = Modifiers::META | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;

    let hotkey = HotKey::new(Some(mods), Code::KeyA);
    manager.register(hotkey).context("register hotkey")?;

    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(_event) = receiver.try_recv() {
            let _ = proxy.send_event(UserEvent::HotkeyFired);
        }
        thread::sleep(Duration::from_millis(25));
    });

    Ok(HotkeyGuard { _manager: manager })
}
