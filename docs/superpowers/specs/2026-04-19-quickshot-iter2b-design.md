# quickshot Iter 2b — System Integration (Design Spec)

**Date:** 2026-04-19
**Status:** Design approved; ready for plan writing
**Predecessor:** Iter 2a (merge `1143984`, tag `v0.2.0-iter2a`) — selection UX
**Successor:** Iter 2c (polish: TTF subset, coord helper, advance-width query) or Iter 3 (settings window)

## Goal

Make quickshot feel like a complete background screenshot tool by adding (1) full-screen capture via `Cmd+Shift+S`, (2) a system notification when full-screen capture succeeds, and (3) a macOS menu-bar tray icon with a menu that exposes both capture modes and a Quit command. All three features live entirely outside the selection overlay; the existing `Cmd+Shift+A` region-capture flow is unchanged.

## Non-Goals (explicitly deferred)

- **Iter 2a leftover polish** (TTF subset, coord-scaling helper extraction, `estimate_text_width` real-advance-width query) — Iter 2c.
- **Settings window / file saving / config persistence / autostart** — Iter 3.
- **Cross-screen selection** — permanently out of scope per original brainstorm.
- **Notification for region capture** — overlay dismissal is already feedback; adding a notification there is noise.
- **Capture delay / timer mode** — Iter 3+.
- **Flash or shutter-sound feedback** on full-screen capture — notification is sufficient.

---

## UX Specification

### Full-screen hotkey

- **Trigger:** `Cmd+Shift+S` on macOS (`Ctrl+Shift+S` on non-macOS).
- **Behavior:** Capture the full monitor that the cursor is currently on (same monitor-selection logic as region capture). No overlay. PNG → clipboard immediately.
- **On success:** System notification fires (see "Notification" below).
- **On permission failure / capture error:** Silent in user-facing terms; error printed to stderr (same as region-capture error path). No notification.

### Tray icon (macOS menu bar)

- **Icon:** A simple 22×22 px PNG, black silhouette on transparent background, marked as a macOS template image so macOS renders it correctly for light/dark menu-bar themes. Design: a rounded-corner rectangle outline with a small solid dot inside ("selection + crosshair" visual motif).
- **Asset path:** `assets/tray-icon.png`, committed to the repo and embedded via `include_bytes!`.
- **Left-click on tray icon:** Opens the menu (default tray-icon behavior).
- **Menu items** (in order, with accelerator labels for discoverability):

  ```
  Capture Region    ⌘⇧A
  Capture Screen    ⌘⇧S
  ───────────────
  Quit
  ```

  Accelerators are **labels only** — they are not separately registered. The global hotkeys registered by `hotkey.rs` handle the actual key events. Clicking a menu item directly triggers the same action.
- **Menu click → capture delay:** When the user triggers a capture via the menu, wait **150 ms** before running the capture so the menu can animate closed without ending up in the screenshot. Hotkey-triggered captures have no delay.

### Notification (full-screen only)

- **When:** After a successful full-screen capture + clipboard write, and not before clipboard succeeds.
- **Summary:** `Screenshot copied`
- **Body:** `{width} × {height}` in physical pixels (U+00D7 multiplication sign, consistent with the size-label formatting from Iter 2a).
- **App name:** `quickshot`
- **No action buttons, no image attachment, no sound.**
- **On notification API failure:** Log to stderr and continue. Do not panic or block.

### Quit

- **Via tray menu `Quit`:** Cleanly exits the event loop. All overlay windows, tray resources, and hotkey registrations drop in a deterministic order.
- **Via `Ctrl+C` in the launching terminal:** Preserves the existing Iter 1 behavior (winit's SIGINT handling or terminal process termination).

---

## Architecture

### File layout

```
src/
├── app.rs              (modified — UserEvent enum extended + 4 new branches)
├── hotkey.rs           (modified — register two hotkeys, distinct events)
├── main.rs             (modified — initialize tray before event_loop.run_app)
├── notification.rs     (new — notify-rust wrapper)
├── tray.rs             (new — tray-icon + menu + event forwarding)
├── capture.rs          (unchanged — capture_at_cursor reused for full-screen)
├── clipboard.rs        (unchanged)
├── crop.rs             (unchanged)
├── permission.rs       (unchanged)
├── text.rs             (unchanged)
└── overlay/            (unchanged)

assets/
├── fonts/              (existing)
└── tray-icon.png       (new — 22×22 template PNG, black on transparent)
```

### `UserEvent` extension (in `src/app.rs`)

Replaces the current single-variant enum with:

```rust
#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    /// `Cmd/Ctrl+Shift+A` pressed, or "Capture Region" menu item clicked.
    CaptureRegion,
    /// `Cmd/Ctrl+Shift+S` pressed, or "Capture Screen" menu item clicked.
    CaptureScreen,
    /// "Quit" menu item clicked.
    Quit,
}
```

All four source-event cases collapse to three app-level intents. Menu and hotkey channels don't need separate enum variants because their downstream behavior is identical. The 150 ms menu-click delay is applied at the tray forwarder, not in `app.rs` — by the time `UserEvent` arrives, the delay has elapsed.

### `src/hotkey.rs` (modified)

Register **two** hotkeys during startup; forwarder thread maps each to a different `UserEvent`.

```rust
pub fn register(proxy: EventLoopProxy<UserEvent>) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new()?;

    #[cfg(target_os = "macos")]
    let mods = Modifiers::META | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;

    let hk_region = HotKey::new(Some(mods), Code::KeyA);
    let hk_screen = HotKey::new(Some(mods), Code::KeyS);
    manager.register(hk_region)?;
    manager.register(hk_screen)?;

    let region_id = hk_region.id();
    let screen_id = hk_screen.id();

    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(event) = receiver.try_recv() {
            let msg = match event.id {
                id if id == region_id => Some(UserEvent::CaptureRegion),
                id if id == screen_id => Some(UserEvent::CaptureScreen),
                _ => None,
            };
            if let Some(m) = msg {
                let _ = proxy.send_event(m);
            }
        }
        thread::sleep(Duration::from_millis(25));
    });

    Ok(HotkeyGuard { _manager: manager })
}
```

The `HotkeyGuard` wraps both registrations (dropping it unregisters both because `GlobalHotKeyManager` owns them).

### `src/notification.rs` (new, ~30 lines)

Thin wrapper over `notify-rust 4`:

```rust
use anyhow::{Context, Result};
use notify_rust::Notification;

pub fn screenshot_copied(width: u32, height: u32) -> Result<()> {
    Notification::new()
        .appname("quickshot")
        .summary("Screenshot copied")
        .body(&format!("{} × {}", width, height))
        .show()
        .context("display notification")?;
    Ok(())
}
```

Error handling at call site: caller logs to stderr and continues — a failed notification is never fatal.

On macOS, `notify-rust 4` uses `NSUserNotification` under the hood (via `mac-notification-sys`), which works for CLI daemons without a bundle. A one-time system permission prompt may appear the first time a notification fires; subsequent calls are silent.

### `src/tray.rs` (new, ~80 lines)

Owns the tray icon, the menu, and a forwarder thread that translates `tray_icon::menu::MenuEvent` into `UserEvent` messages sent through the winit `EventLoopProxy`.

```rust
use anyhow::{Context, Result};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    Icon, TrayIcon, TrayIconBuilder,
};
use winit::event_loop::EventLoopProxy;

use crate::app::UserEvent;

pub struct TrayGuard {
    // Tray icon must stay alive for the program's lifetime.
    _tray: TrayIcon,
}

const ICON_BYTES: &[u8] = include_bytes!("../assets/tray-icon.png");

pub fn install(proxy: EventLoopProxy<UserEvent>) -> Result<TrayGuard> {
    let icon = load_icon()?;
    let menu = Menu::new();

    let id_region = MenuId::new("capture-region");
    let id_screen = MenuId::new("capture-screen");
    let id_quit = MenuId::new("quit");

    menu.append(&MenuItem::with_id(
        id_region.clone(),
        "Capture Region    \u{2318}\u{21E7}A",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(
        id_screen.clone(),
        "Capture Screen    \u{2318}\u{21E7}S",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(id_quit.clone(), "Quit", true, None))?;

    let tray = TrayIconBuilder::new()
        .with_tooltip("quickshot")
        .with_icon(icon)
        .with_menu(Box::new(menu))
        .build()
        .context("build tray icon")?;

    // Forwarder thread: poll MenuEvent::receiver() and relay to winit.
    let proxy_for_forwarder = proxy.clone();
    std::thread::spawn(move || {
        let rx = MenuEvent::receiver();
        loop {
            if let Ok(event) = rx.try_recv() {
                let (msg, delay) = match event.id.0.as_str() {
                    "capture-region" => {
                        (Some(UserEvent::CaptureRegion), Some(std::time::Duration::from_millis(150)))
                    }
                    "capture-screen" => {
                        (Some(UserEvent::CaptureScreen), Some(std::time::Duration::from_millis(150)))
                    }
                    "quit" => (Some(UserEvent::Quit), None),
                    _ => (None, None),
                };
                if let Some(m) = msg {
                    if let Some(d) = delay {
                        std::thread::sleep(d);
                    }
                    let _ = proxy_for_forwarder.send_event(m);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    });

    Ok(TrayGuard { _tray: tray })
}

fn load_icon() -> Result<Icon> {
    let img = image::load_from_memory(ICON_BYTES)
        .context("decode tray icon PNG")?
        .into_rgba8();
    let (w, h) = img.dimensions();
    let raw = img.into_raw();
    Icon::from_rgba(raw, w, h).context("build tray icon")
}
```

**Icon PNG generation (one-time):** the 22×22 PNG at `assets/tray-icon.png` is generated by a short `build.rs`-free procedure during Iter 2b implementation — the plan will include a small Rust snippet that renders the icon pixels deterministically (rounded rect outline + center dot) and writes the PNG via the existing `image` crate. The result is committed so no build-time step is needed subsequently.

### `src/main.rs` (modified)

Add tray install between hotkey registration and `event_loop.run_app`:

```rust
fn main() -> Result<()> {
    permission::preflight()?;

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();

    let _hotkey_guard = hotkey::register(proxy.clone())?;
    let _tray_guard = tray::install(proxy.clone())?;

    println!(
        "quickshot running; press Cmd/Ctrl+Shift+A for region, Cmd/Ctrl+Shift+S for screen. Quit via tray or Ctrl+C."
    );

    let mut app = app::App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
```

Both guards must outlive `run_app` to keep the hotkey manager and tray icon registered.

### `src/app.rs` branches

`App::new()` gains no new state. `window_event` is unchanged (it only handles overlay window events). Only `user_event` changes:

```rust
fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
        UserEvent::CaptureRegion => {
            if let Err(e) = self.open_overlay(event_loop) {
                eprintln!("open overlay error: {e:?}");
            }
        }
        UserEvent::CaptureScreen => {
            self.capture_full_screen();
        }
        UserEvent::Quit => {
            event_loop.exit();
        }
    }
}
```

New private method `capture_full_screen()`:

```rust
fn capture_full_screen(&mut self) {
    match capture::capture_at_cursor() {
        Ok((frame, _geom)) => {
            let (w, h) = frame.dimensions();
            if let Err(e) = clipboard::put_image(&frame) {
                eprintln!("clipboard error: {e:?}");
                return;
            }
            println!("copied {}x{} (full screen) to clipboard", w, h);
            if let Err(e) = notification::screenshot_copied(w, h) {
                eprintln!("notification error: {e:?}");
            }
        }
        Err(e) => {
            eprintln!("capture error: {e:?}");
        }
    }
}
```

Note: full-screen capture suppresses any overlay. If the user presses `Cmd+Shift+S` while an overlay is open (e.g. they opened region-capture first and haven't confirmed), the current rule is: **ignore** the full-screen request until the overlay closes. Concretely, `capture_full_screen` checks `self.overlay.is_some()` and silently early-returns if an overlay is active. Rationale: the alternative (canceling the overlay + taking a full screenshot) is more surprising than doing nothing.

### `Cargo.toml` additions

```toml
notify-rust = "4"
tray-icon = "0.19"
```

Version numbers should be verified against crates.io at plan time. Binary size estimate: `notify-rust` ~30 KB, `tray-icon` ~100 KB (includes objc2-appkit bindings it shares with winit; actual overhead less because winit already pulls parts of objc2). Estimated binary: 855 KB → ~1.0 MB. Iter 2c's TTF subset will offset this by ~150 KB.

---

## Data Flow

### `Cmd+Shift+S` (full-screen)

```
global-hotkey thread    → HotkeyGuard forwarder
    → proxy.send_event(UserEvent::CaptureScreen)
    → winit delivers to App::user_event
    → capture_full_screen()
         → capture::capture_at_cursor()
         → clipboard::put_image(&frame)
         → notification::screenshot_copied(w, h)
```

### Tray menu click on "Capture Region"

```
tray-icon menu thread   → tray forwarder
    → sleep(150 ms)                        // let menu collapse off-screen
    → proxy.send_event(UserEvent::CaptureRegion)
    → winit delivers to App::user_event
    → open_overlay()                       // existing flow
```

### Tray menu click on "Quit"

```
tray-icon menu thread   → tray forwarder
    → proxy.send_event(UserEvent::Quit)    // no delay
    → winit delivers to App::user_event
    → event_loop.exit()
    → run_app returns                      // main.rs drops guards in reverse order:
                                           //   _tray_guard (unregisters icon)
                                           //   _hotkey_guard (unregisters hotkeys)
```

### `Ctrl+C` in terminal

Unchanged from Iter 1 — winit/OS handles SIGINT, `run_app` returns, guards drop in order.

---

## Error handling

- **`capture::capture_at_cursor()` error** (e.g. Screen Recording permission revoked mid-session): log to stderr, no notification, overlay (if any) stays closed. User sees no visible change — they can check the terminal for the error.
- **`clipboard::put_image()` error:** log to stderr; skip the notification (don't lie about what happened).
- **`notification::screenshot_copied()` error:** log to stderr; the clipboard write already succeeded, so the screenshot is still usable. User perceives success.
- **`tray::install()` error at startup:** this is fatal — main.rs returns the error via `?`, matching the existing hotkey-registration failure path. Rationale: if the tray can't be installed, the user has no way to see that quickshot is running, and `Ctrl+C` in the terminal is the only exit — we should fail loudly.
- **`hotkey::register()` error:** unchanged; fatal at startup.

---

## Testing strategy

### Unit tests

- `notification.rs`: none. The function has only side effects (shows a notification via the OS). A compile-only test would add nothing. Manually verified.
- `tray.rs`: none. Same reasoning.
- `hotkey.rs`: no change to existing test coverage (the module has no tests; both register-paths are exercised by manual run of the binary).

### Manual verification (acceptance)

1. `./target/release/quickshot` starts; console prints the banner mentioning both hotkeys and quit options.
2. Menu bar shows the quickshot tray icon.
3. Left-click tray icon → menu opens with three items in the expected order.
4. Click "Capture Region" → menu closes → ~150 ms later the overlay appears, exactly like `Cmd+Shift+A`.
5. Click "Capture Screen" → menu closes → ~150 ms later a notification "Screenshot copied — {W} × {H}" appears, current monitor's screenshot is on the clipboard, no overlay was shown.
6. `Cmd+Shift+A` still opens the overlay (region capture). Full Iter 2a flow still works (drag, anchors, Enter, ESC).
7. `Cmd+Shift+S` captures the cursor's monitor → clipboard + notification. No overlay appears.
8. `Cmd+Shift+S` while overlay is open → nothing happens; overlay stays open.
9. Click "Quit" → tray icon disappears, hotkeys become inactive (press `Cmd+Shift+A` after — no overlay appears), process exits cleanly with code 0.
10. `Ctrl+C` in the terminal (with overlay open or closed) → process exits cleanly with tray icon removed.

Regression (Iter 1 + 2a invariants):
11. Multi-display: cursor on secondary screen → region-capture overlay lands on that screen. Full-screen captures that screen only.
12. macOS: overlay covers dock and menu bar (window level 1000).
13. Post-capture-cycle hotkey still works (no wedged loop).
14. DPI: Retina → magnifier/size label font size visually consistent with Iter 2a.
15. Binary size target: ≤ 1.1 MB (Iter 2a was 855 KB; ~150 KB for notify-rust + tray-icon combined).

---

## Implementation order (for the plan)

1. **Generate the tray-icon PNG asset** via a one-off Rust snippet using the existing `image` crate; commit `assets/tray-icon.png`.
2. **Add dependencies** (`notify-rust`, `tray-icon`) to `Cargo.toml`; verify `cargo check` clean.
3. **Write `src/notification.rs`** with `screenshot_copied(w, h)`; declare `mod notification` in `main.rs`; compile-only smoke test.
4. **Extend `src/app.rs::UserEvent`** from the single `HotkeyFired` variant to `CaptureRegion | CaptureScreen | Quit`; update existing usage. Iter 2a interaction unchanged at this step.
5. **Extend `src/hotkey.rs`** to register both `KeyA` and `KeyS`; forwarder maps by event id to the new `UserEvent` variants.
6. **Add `App::capture_full_screen()`** + wire the `CaptureScreen` branch in `user_event`. `Cmd+Shift+S` now works; no tray yet.
7. **Write `src/tray.rs`** with `install(proxy) -> Result<TrayGuard>`; initialize in `main.rs` right after `hotkey::register`. Forwarder wires menu clicks to `UserEvent` with the 150 ms delay for capture items.
8. **Wire the `Quit` branch in `user_event`** to call `event_loop.exit()`. Manual quit via tray now works.
9. **Add the "overlay is open → ignore CaptureScreen" guard** in `capture_full_screen`.
10. **Polish:** README update (Iter 2b status + new hotkey + tray), binary size record, clippy clean, annotated tag `v0.3.0-iter2b`.

Each step is independently testable and commit-able. The plan document will detail the file edits, code blocks, manual test steps, and commit messages.

---

## Risks & mitigations

- **`tray-icon` crate API instability.** tray-icon is still evolving; version pin to the latest 0.19 and verify the `MenuEvent::receiver()` channel pattern matches the public API at plan time. If the API has moved, adapt the forwarder pattern inline.
- **macOS first-run notification prompt.** The first notification may trigger a system permission dialog ("quickshot wants to send you notifications"). We can't pre-approve. Mitigation: document this in README; subsequent runs are silent.
- **NSUserNotification is deprecated on macOS 11+.** notify-rust uses the deprecated API for CLI compatibility. It still works on macOS 14/15 as of 2026-04. If Apple removes it in a future macOS, we'll need to switch to UNUserNotificationCenter + a proper bundle, which is Iter 3 territory.
- **Tray-icon crate's AppKit integration might conflict with winit's.** Both use `NSApplication.sharedApplication`. They should cooperate (both read the same singleton), but if tray icon initialization resets state that winit expects, we may see visual glitches. Mitigation: initialize tray AFTER `EventLoop::build()` (already planned in main.rs).
- **Binary size overrun.** If the combined delta of notify-rust + tray-icon is larger than estimated and the binary exceeds 1.2 MB, we accept the overrun as a cost of the system-integration features and defer size work to Iter 2c's TTF subset.

## Open questions

None blocking. All UX and architectural decisions are resolved. Implementation details that surface during plan writing will be resolved inline.
