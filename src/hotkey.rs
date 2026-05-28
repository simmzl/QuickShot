use anyhow::{Context, Result};
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;
use crate::config::ParsedHotkey;

/// Holds the live `GlobalHotKeyManager` plus the currently-registered
/// HotKey handles. The forwarder thread (spawned once in `register`) reads
/// the current ids through `Arc<Mutex<HotkeyIds>>` so that reloading the
/// config can swap them in-place without re-spawning a thread.
pub struct HotkeyGuard {
    manager: GlobalHotKeyManager,
    ids: Arc<Mutex<HotkeyIds>>,
    region: HotKey,
    screen: HotKey,
}

#[derive(Default, Copy, Clone)]
struct HotkeyIds {
    region_id: u32,
    screen_id: u32,
}

impl Drop for HotkeyGuard {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.region);
        let _ = self.manager.unregister(self.screen);
    }
}

impl HotkeyGuard {
    /// Swap the currently-registered region/screen hotkeys to the new pair.
    /// On failure, restores the previous binding and returns the error so
    /// the caller can surface it (e.g. show a toast / log).
    pub fn reregister(&mut self, region: ParsedHotkey, screen: ParsedHotkey) -> Result<()> {
        let new_region = HotKey::new(Some(region.modifiers), region.code);
        let new_screen = HotKey::new(Some(screen.modifiers), screen.code);

        // No-op fast path: the parsed combos hash to the same HotKey ids,
        // so re-registering would just produce "already registered" errors.
        if new_region.id() == self.region.id() && new_screen.id() == self.screen.id() {
            return Ok(());
        }

        // Unregister old first so identical-but-different (e.g. only region
        // changed) combinations don't collide.
        let _ = self.manager.unregister(self.region);
        let _ = self.manager.unregister(self.screen);

        if let Err(e) = self.manager.register(new_region) {
            // Roll back: try to put the old pair back in place.
            let _ = self.manager.register(self.region);
            let _ = self.manager.register(self.screen);
            return Err(e).with_context(|| format!("register region hotkey {}", region.raw));
        }
        if let Err(e) = self.manager.register(new_screen) {
            let _ = self.manager.unregister(new_region);
            let _ = self.manager.register(self.region);
            let _ = self.manager.register(self.screen);
            return Err(e).with_context(|| format!("register fullscreen hotkey {}", screen.raw));
        }

        self.region = new_region;
        self.screen = new_screen;
        if let Ok(mut g) = self.ids.lock() {
            g.region_id = new_region.id();
            g.screen_id = new_screen.id();
        }
        Ok(())
    }
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

    let ids = Arc::new(Mutex::new(HotkeyIds {
        region_id: hk_region.id(),
        screen_id: hk_screen.id(),
    }));

    let ids_fwd = ids.clone();
    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(event) = receiver.try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            let current = ids_fwd.lock().map(|g| *g).unwrap_or_default();
            let msg = if event.id == current.region_id {
                Some(UserEvent::CaptureRegion)
            } else if event.id == current.screen_id {
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

    Ok(HotkeyGuard {
        manager,
        ids,
        region: hk_region,
        screen: hk_screen,
    })
}
