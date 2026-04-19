# quickshot Iter 2b — System Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add full-screen capture via `Cmd+Shift+S`, a macOS menu-bar tray icon with a menu exposing both capture modes and Quit, and a system notification that fires on successful full-screen capture — without touching the existing Iter 2a region-capture overlay.

**Architecture:** Thin extension of the existing module layout. `UserEvent` gains three variants (`CaptureRegion`, `CaptureScreen`, `Quit`); `hotkey.rs` registers two global hotkeys and dispatches via event id; new `notification.rs` wraps `notify-rust`; new `tray.rs` wraps `tray-icon`, owns the menu, and relays menu clicks through `EventLoopProxy` with a 150 ms delay so the menu can animate closed before a capture runs. `app.rs` gains one new method (`capture_full_screen`) and three matching `user_event` branches. `main.rs` initializes tray after hotkey.

**Tech Stack:**
- Rust 2021 (existing toolchain, stable 1.86+)
- All existing crates unchanged
- **New:** `notify-rust = "4"` — cross-platform system notifications (macOS via `mac-notification-sys` / NSUserNotification)
- **New:** `tray-icon = "0.19"` — macOS menu-bar icon + menu
- **New asset:** `assets/tray-icon.png` — 22×22 RGBA PNG, black silhouette on transparent, committed and embedded via `include_bytes!`

**Spec:** `docs/superpowers/specs/2026-04-19-quickshot-iter2b-design.md`

**Scope for this plan (Iter 2b only):**
- `Cmd+Shift+S` full-screen capture (cursor's monitor)
- System notification on full-screen-capture success
- macOS menu-bar tray icon with Capture Region / Capture Screen / Quit menu
- 150 ms delay for menu-triggered captures (so menu closes off-screen before capture runs)
- Overlay-open guard: `Cmd+Shift+S` is silently ignored while an overlay is active

**Not in this plan (Iter 2c / Iter 3):**
- TTF font subset, coord-scaling utility extraction, `estimate_text_width` real-advance-width query (Iter 2c polish)
- Settings window, file saving, config persistence, autostart (Iter 3)

---

## File Structure

```
quickshot/
├── Cargo.toml                          (modified — add notify-rust, tray-icon)
├── README.md                           (modified — Iter 2b status)
├── assets/
│   ├── fonts/                          (unchanged)
│   └── tray-icon.png                   (new — 22×22 template PNG)
├── src/
│   ├── app.rs                          (modified — UserEvent enum + 3 branches + capture_full_screen)
│   ├── hotkey.rs                       (modified — register 2 hotkeys, dispatch by id)
│   ├── main.rs                         (modified — declare notification/tray mods, install tray)
│   ├── capture.rs                      (unchanged)
│   ├── clipboard.rs                    (unchanged)
│   ├── crop.rs                         (unchanged)
│   ├── permission.rs                   (unchanged)
│   ├── text.rs                         (unchanged)
│   ├── notification.rs                 (new — screenshot_copied helper)
│   ├── tray.rs                         (new — install + TrayGuard + menu forwarder)
│   └── overlay/                        (unchanged)
```

**Responsibilities (one per file):**

- `notification.rs` — accepts `(width: u32, height: u32)` and displays a system notification. No state. Thin.
- `tray.rs` — constructs the `TrayIcon` + `Menu`, spawns a forwarder thread that reads `MenuEvent` and sends `UserEvent` through an `EventLoopProxy` (with a 150 ms sleep before capture-class events). Returns a `TrayGuard` that must outlive `run_app`.
- `hotkey.rs` — as before, but now registers `KeyA` and `KeyS` with identical modifiers and dispatches each to a different `UserEvent` via the hotkey event id.
- `app.rs::capture_full_screen` — captures the cursor's monitor, pushes to clipboard, fires a notification on success. Silently returns early if an overlay is already open.
- `main.rs` — installs hotkey + tray in order before `event_loop.run_app`; both guards live until `run_app` returns.

---

## Task 1: Generate and commit the tray-icon PNG asset

Produce `assets/tray-icon.png`, a 22×22 black-on-transparent template image. We generate it once via a throwaway Rust snippet using the existing `image` crate (no new dep), commit the PNG, and remove the generator.

**Files:**
- Create: `assets/tray-icon.png` (22×22 RGBA PNG)

- [ ] **Step 1: Create the assets directory if missing**

Run (from worktree root `/Users/simmzl/Desktop/personal/quickshot/.worktrees/iter2b/`):
```bash
mkdir -p assets
ls assets/
```
Expected: `fonts/` (already exists from Iter 2a). After this step the directory exists.

- [ ] **Step 2: Create a throwaway binary that writes the icon**

Create `src/bin/gen_tray_icon.rs` (cargo picks up the `bin/` subdir automatically):
```rust
//! One-shot generator for `assets/tray-icon.png`. Run with
//! `cargo run --bin gen_tray_icon`. Output is committed; this file is
//! deleted in the next step.
//!
//! Design: 18×18 black rounded-rectangle outline centered on a 22×22 canvas,
//! with a 2×2 black dot at the center. Outline is 1 px thick, achieved by
//! filling an outer rounded rect in black then filling an inner rounded rect
//! (inset by 1 px) in transparent.

use image::{Rgba, RgbaImage};

fn main() {
    const SIZE: i32 = 22;
    let transparent = Rgba([0u8, 0, 0, 0]);
    let black = Rgba([0u8, 0, 0, 255]);

    let mut img = RgbaImage::from_pixel(SIZE as u32, SIZE as u32, transparent);

    // Outline rect: 18×18 centered on 22×22, so inclusive bounds [2..=19] on each axis.
    let (l, t, r, b) = (2, 2, 19, 19);
    let outer_radius: i32 = 3;
    let inner_radius: i32 = 2;

    fill_rounded(&mut img, l, t, r, b, outer_radius, black);
    // Inset by 1 on each side to leave a 1-px black outline.
    fill_rounded(&mut img, l + 1, t + 1, r - 1, b - 1, inner_radius, transparent);

    // Center dot: 2×2 solid black at the geometric center.
    let cx = SIZE / 2;
    let cy = SIZE / 2;
    for dy in -1..=0 {
        for dx in -1..=0 {
            img.put_pixel((cx + dx) as u32, (cy + dy) as u32, black);
        }
    }

    let out_path = "assets/tray-icon.png";
    img.save(out_path).expect("save tray icon");
    println!("wrote {out_path} ({}x{})", SIZE, SIZE);
}

fn fill_rounded(img: &mut RgbaImage, l: i32, t: i32, r: i32, b: i32, radius: i32, color: Rgba<u8>) {
    for y in t..=b {
        for x in l..=r {
            // Determine nearest corner center.
            let cx = if x - l < radius {
                l + radius
            } else if r - x < radius {
                r - radius
            } else {
                x
            };
            let cy = if y - t < radius {
                t + radius
            } else if b - y < radius {
                b - radius
            } else {
                y
            };
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= radius * radius {
                if x >= 0 && y >= 0 && x < img.width() as i32 && y < img.height() as i32 {
                    img.put_pixel(x as u32, y as u32, color);
                }
            }
        }
    }
}
```

- [ ] **Step 3: Run the generator and verify the PNG**

Run:
```bash
cargo run --bin gen_tray_icon
file assets/tray-icon.png
ls -l assets/tray-icon.png
```
Expected: `wrote assets/tray-icon.png (22x22)`; `file` reports `PNG image data, 22 x 22, 8-bit/color RGBA, non-interlaced`; size roughly 200–500 bytes.

Visual sanity check (optional): `open assets/tray-icon.png` shows a small rounded black rectangle with a 2×2 dot in the middle on a transparent background. If macOS QuickLook shows it, the image decodes correctly.

- [ ] **Step 4: Delete the generator binary**

We only needed it to produce the PNG. Run:
```bash
rm src/bin/gen_tray_icon.rs
# Clean up empty bin dir if present
rmdir src/bin 2>/dev/null || true
```

- [ ] **Step 5: Verify cargo still builds the main binary**

Run:
```bash
cargo check
```
Expected: clean. No more "binary target `gen_tray_icon`" mentioned.

- [ ] **Step 6: Commit the asset**

```bash
git add assets/tray-icon.png
git commit -m "feat(assets): add 22x22 template PNG for tray icon"
```

---

## Task 2: Add dependencies and write `notification.rs`

Add the two new crates to `Cargo.toml` and write the thin `notification.rs` wrapper with a compile-time smoke test.

**Files:**
- Modify: `Cargo.toml`
- Create: `src/notification.rs`
- Modify: `src/main.rs` (declare `mod notification;`)

- [ ] **Step 1: Add the dependencies**

Edit `Cargo.toml`. In the `[dependencies]` block, add `notify-rust = "4"` and `tray-icon = "0.19"` alphabetically. After the edit the block reads:
```toml
[dependencies]
anyhow = "1"
arboard = "3.4"
fontdue = "0.9"
global-hotkey = "0.6"
image = { version = "0.25", default-features = false, features = ["png"] }
notify-rust = "4"
softbuffer = "0.4"
tray-icon = "0.19"
winit = "0.30"
xcap = "0.9"
```

- [ ] **Step 2: Verify dependencies resolve**

Run:
```bash
cargo check
```
Expected: cargo downloads `notify-rust`, `tray-icon`, and their transitive deps, then reports `Finished`. No warnings yet (nothing uses them).

If `tray-icon 0.19` or `notify-rust 4` have published only a minor bump since this plan was written (e.g., `0.20`, `4.1`), use the latest compatible minor and note the version in the commit message. API should match unless a major version bumped.

- [ ] **Step 3: Write `src/notification.rs`**

Create `src/notification.rs`:
```rust
use anyhow::{Context, Result};
use notify_rust::Notification;

/// Fire a "Screenshot copied" system notification with the captured image's
/// physical pixel dimensions in the body. Safe to call on any platform;
/// failures are returned as errors so the caller can log them without
/// aborting the capture flow.
pub fn screenshot_copied(width: u32, height: u32) -> Result<()> {
    Notification::new()
        .appname("quickshot")
        .summary("Screenshot copied")
        .body(&format!("{} \u{00D7} {}", width, height))
        .show()
        .context("display notification")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // No runtime test — the notification API is a side effect against the OS.
    // The compile-only test below exists to catch signature drift in notify-rust.
    #[test]
    fn screenshot_copied_signature_is_stable() {
        let _ = super::screenshot_copied(1920, 1080);
    }
}
```

The single `#[test]` calls the function and discards the result. Depending on the CI environment this either shows a notification or fails (both are acceptable — `Result` is discarded with `let _`). On a developer machine running `cargo test`, a notification may briefly appear — this is a known side effect.

- [ ] **Step 4: Declare the module in `src/main.rs`**

Edit `src/main.rs` — add `mod notification;` to the module list. After the edit the head of the file reads:
```rust
mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
```

- [ ] **Step 5: Verify it compiles and the new test runs**

Run:
```bash
cargo build --release
cargo test notification
```
Expected: clean release build; 1 test passes (`screenshot_copied_signature_is_stable`). A visible notification may briefly appear on macOS during the test.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/notification.rs src/main.rs
git commit -m "feat(notification): add notify-rust + tray-icon deps and notification wrapper"
```

---

## Task 3: Refactor `UserEvent` and register both hotkeys

Rename the single `UserEvent::HotkeyFired` variant into three variants covering all Iter 2b intents. Extend `hotkey.rs` to register `KeyA` and `KeyS` with the same modifiers and dispatch by event id. `Cmd/Ctrl+Shift+S` will trigger a new `UserEvent` that isn't wired yet — Task 4 handles it.

**Files:**
- Modify: `src/app.rs` (UserEvent enum + existing `HotkeyFired` usage)
- Modify: `src/hotkey.rs`

- [ ] **Step 1: Extend `UserEvent` in `src/app.rs`**

Find the current enum:
```rust
#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}
```
Replace with:
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

(`Copy` is added because all variants are unit-like — this avoids spurious `.clone()` at the forwarder thread boundary.)

- [ ] **Step 2: Update the existing `user_event` handler**

In `src/app.rs`, find:
```rust
fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
        UserEvent::HotkeyFired => {
            if let Err(e) = self.open_overlay(event_loop) {
                eprintln!("open overlay error: {e:?}");
            }
        }
    }
}
```
Replace the match body to handle all three variants. Task 4 fills in `CaptureScreen` and `Quit`; for now, route `CaptureRegion` to the existing behavior and leave the other two as explicit no-ops with a log line so we can observe the events landing:
```rust
fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
    match event {
        UserEvent::CaptureRegion => {
            if let Err(e) = self.open_overlay(event_loop) {
                eprintln!("open overlay error: {e:?}");
            }
        }
        UserEvent::CaptureScreen => {
            // Filled in by Task 4.
            eprintln!("capture screen (stub) — wired up in Task 4");
        }
        UserEvent::Quit => {
            // Filled in by Task 5 (after tray is installed; Task 5 replaces this stub
            // with `event_loop.exit()`).
            eprintln!("quit (stub)");
        }
    }
}
```

- [ ] **Step 3: Extend `src/hotkey.rs` to register both hotkeys**

Overwrite `src/hotkey.rs`:
```rust
use anyhow::{Context, Result};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager,
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
```

- [ ] **Step 4: Build and sanity-check**

Run:
```bash
cargo build --release
cargo test
```
Expected: clean build; test count unchanged (41 — 40 from Iter 2a + 1 notification signature test), 2 ignored.

- [ ] **Step 5: Manual end-to-end check (controller)**

Run:
```bash
pkill quickshot 2>/dev/null
./target/release/quickshot
```
- Press `Cmd+Shift+A` → overlay opens (existing Iter 2a behavior).
- Press `Cmd+Shift+S` → terminal prints `capture screen (stub) — wired up in Task 4`.
- `Ctrl+C` to quit.

Note from the controller: this manual check is deferred to the user during the review loop. The implementer should report DONE_WITH_CONCERNS noting the stub behavior is intentional for this task.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/hotkey.rs
git commit -m "feat(hotkey): register Cmd+Shift+S and split UserEvent into 3 variants"
```

---

## Task 4: Wire `CaptureScreen` end-to-end

Add `App::capture_full_screen()` and replace the Task 3 stub. After this task, `Cmd+Shift+S` actually produces a screenshot + clipboard + notification.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the `capture_full_screen` method**

In `src/app.rs`, inside `impl App`, add a new private method right after `fn cancel(...)`:
```rust
    fn capture_full_screen(&mut self) {
        // If the region overlay is open, ignore the full-screen request so we
        // don't steal the monitor while the user is mid-selection.
        if self.overlay.is_some() {
            return;
        }
        match capture::capture_at_cursor() {
            Ok((frame, _geom)) => {
                let (w, h) = frame.dimensions();
                if let Err(e) = clipboard::put_image(&frame) {
                    eprintln!("clipboard error: {e:?}");
                    return;
                }
                println!("copied {}x{} (full screen) to clipboard", w, h);
                if let Err(e) = crate::notification::screenshot_copied(w, h) {
                    eprintln!("notification error: {e:?}");
                }
            }
            Err(e) => {
                eprintln!("capture error: {e:?}");
            }
        }
    }
```

- [ ] **Step 2: Replace the stub branch**

In the `user_event` method of `App`, replace the `CaptureScreen` arm:
```rust
        UserEvent::CaptureScreen => {
            eprintln!("capture screen (stub) — wired up in Task 4");
        }
```
with:
```rust
        UserEvent::CaptureScreen => {
            self.capture_full_screen();
        }
```

- [ ] **Step 3: Build and run tests**

Run:
```bash
cargo build --release
cargo test
```
Expected: clean build; all tests pass.

- [ ] **Step 4: Manual verification (deferred to controller)**

Manual flow:
```bash
pkill quickshot 2>/dev/null
./target/release/quickshot
```
- Press `Cmd+Shift+S` on each monitor → PNG for that monitor lands on clipboard → a `Screenshot copied — {W} × {H}` notification appears → paste into Preview confirms dimensions.
- Press `Cmd+Shift+A` → overlay opens → while overlay is open, press `Cmd+Shift+S` → nothing happens (overlay stays open, no notification).

The implementer should report DONE_WITH_CONCERNS noting manual GUI verification is deferred to the controller.

- [ ] **Step 5: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): Cmd+Shift+S captures cursor's monitor to clipboard + notifies"
```

---

## Task 5: Write `tray.rs` and install the tray in `main.rs`

Create the tray-icon module. The tray menu has three items; the forwarder thread translates each click to a `UserEvent` with a 150 ms delay for capture items.

**Files:**
- Create: `src/tray.rs`
- Modify: `src/main.rs`
- Modify: `src/app.rs` (replace the Quit stub with `event_loop.exit()`)

- [ ] **Step 1: Write `src/tray.rs`**

Create `src/tray.rs`:
```rust
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
pub fn install(proxy: EventLoopProxy<UserEvent>) -> Result<TrayGuard> {
    let icon = load_icon()?;
    let menu = Menu::new();

    menu.append(&MenuItem::with_id(
        ID_REGION,
        "Capture Region    \u{2318}\u{21E7}A",
        true,
        None,
    ))
    .context("append Capture Region menu item")?;
    menu.append(&MenuItem::with_id(
        ID_SCREEN,
        "Capture Screen    \u{2318}\u{21E7}S",
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
                let id_str = event.id.0.as_str();
                let (msg, delay) = match id_str {
                    s if s == ID_REGION => (
                        Some(UserEvent::CaptureRegion),
                        Some(std::time::Duration::from_millis(150)),
                    ),
                    s if s == ID_SCREEN => (
                        Some(UserEvent::CaptureScreen),
                        Some(std::time::Duration::from_millis(150)),
                    ),
                    s if s == ID_QUIT => (Some(UserEvent::Quit), None),
                    _ => (None, None),
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
```

Note: the `MenuEvent::id.0` access assumes `MenuId` is a newtype over `String` in `tray-icon 0.19`. If the actual API wraps differently (e.g., `id.as_str()` directly), adapt the `id_str` extraction. Keep the rest of the logic intact.

- [ ] **Step 2: Declare the module in `src/main.rs` and install the tray**

Edit `src/main.rs` — add `mod tray;` alongside `mod notification;`. After the edit the module list reads:
```rust
mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod notification;
mod overlay;
mod permission;
mod text;
mod tray;
```

Then in `fn main()`, after the `let _hotkey_guard = hotkey::register(proxy.clone())?;` line, install the tray:
```rust
    let _hotkey_guard = hotkey::register(proxy.clone())?;
    let _tray_guard = tray::install(proxy.clone())?;

    println!(
        "quickshot running; press Cmd/Ctrl+Shift+A for region, Cmd/Ctrl+Shift+S for screen. Quit via tray or Ctrl+C."
    );
```
(Replace the old banner if it reads differently.)

- [ ] **Step 3: Replace the `Quit` stub in `src/app.rs`**

In `user_event`, replace:
```rust
        UserEvent::Quit => {
            eprintln!("quit (stub)");
        }
```
with:
```rust
        UserEvent::Quit => {
            event_loop.exit();
        }
```

- [ ] **Step 4: Build**

Run:
```bash
cargo build --release
cargo test
```
Expected: clean build; all tests pass.

- [ ] **Step 5: Check that `cargo test` + `cargo clippy` pass**

Run:
```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. If `tray-icon` or `notify-rust` trigger clippy warnings in our code (e.g., unused imports, `let _` patterns), fix with minimal-scope `#[allow]` + comment. Pre-existing Iter 2a allows remain.

- [ ] **Step 6: Manual verification (deferred to controller)**

```bash
pkill quickshot 2>/dev/null
./target/release/quickshot
```
Expected:
- Menu bar gets a new quickshot icon.
- Click icon → menu shows Capture Region / Capture Screen / — / Quit.
- Click Capture Region → menu closes → ~150 ms later overlay opens.
- Click Capture Screen → menu closes → ~150 ms later notification + clipboard.
- Click Quit → tray icon disappears, process exits cleanly (return code 0).

The implementer should report DONE_WITH_CONCERNS noting GUI verification deferred.

- [ ] **Step 7: Commit**

```bash
git add src/tray.rs src/main.rs src/app.rs
git commit -m "feat(tray): menu-bar icon with Capture Region / Screen / Quit"
```

---

## Task 6: Polish pass

Clippy clean, binary size record, README update, annotated tag.

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Test suite + clippy**

Run in order:
```bash
cargo test
cargo clippy --release --all-targets -- -D warnings
```
Expected: all existing tests pass (41 / 0 / 2 total); clippy clean. Fix any new warnings with minimal `#[allow]` + comment, same pattern as Iter 2a polish.

- [ ] **Step 2: Record binary size**

Run:
```bash
cargo build --release
ls -lh target/release/quickshot
```
Expected: somewhere in 900 KB – 1.1 MB. Iter 2a was 855 KB; `notify-rust` + `tray-icon` together add roughly 100–200 KB.

Optional: if `cargo-bloat` is installed, record the top contributors:
```bash
cargo bloat --release --crates -n 10 2>&1 | head -15 || echo "cargo-bloat unavailable"
```

- [ ] **Step 3: Update `README.md` with Iter 2b status**

In `README.md`, replace the "Status (Iter 2a)" section header with "Status (Iter 2b)" and update the bullet list. The section now reads:
```markdown
## Status (Iter 2b)

- Region capture via `Cmd+Shift+A` with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- Full-screen capture via `Cmd+Shift+S` (cursor's monitor, clipboard + notification)
- Menu-bar tray icon with Capture Region / Capture Screen / Quit
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No settings window / file saving yet (Iter 3)
- No cross-screen selection

Release binary size on this machine: <PASTE-SIZE-HERE>.
```

Replace `<PASTE-SIZE-HERE>` with the actual `ls -lh` output from Step 2 (e.g., `~1.0M`). If `cargo bloat` was run, append one line noting the largest crates (e.g., "Dominant contributors: std (backtrace/DWARF), winit, tray-icon, notify-rust").

- [ ] **Step 4: Commit polish**

```bash
git add README.md
git commit -m "docs: record Iter 2b status and binary size"
```

- [ ] **Step 5: Tag**

```bash
git tag -a v0.3.0-iter2b -m "Iter 2b: full-screen hotkey, tray icon, notifications"
```

---

## Manual verification checklist (whole plan)

Before declaring Iter 2b done, run through this end-to-end. All ten items must pass:

1. `./target/release/quickshot` starts; console banner mentions both hotkeys and exit options.
2. Menu-bar tray icon is visible.
3. Left-click tray icon → menu shows Capture Region (⌘⇧A) / Capture Screen (⌘⇧S) / separator / Quit in that order.
4. Click Capture Region → menu closes → overlay appears shortly after; full Iter 2a region flow works (drag, anchors, Enter, ESC).
5. Click Capture Screen → menu closes → notification "Screenshot copied — {W} × {H}" appears; clipboard has the cursor's monitor as a PNG.
6. Press `Cmd+Shift+A` directly → overlay opens (no tray-click delay).
7. Press `Cmd+Shift+S` directly → notification + clipboard, no overlay.
8. While overlay is open, press `Cmd+Shift+S` → nothing happens; overlay stays open.
9. Click tray Quit → tray icon removed; process exits with code 0; subsequent `Cmd+Shift+A` presses do nothing (daemon is gone).
10. Relaunch + `Ctrl+C` in terminal → clean exit; tray icon removed.

Regression checks (Iter 1 + 2a invariants):
11. Multi-display — capture follows cursor monitor for both region and full-screen modes.
12. macOS overlay covers dock + menu bar (level 1000).
13. Region-capture hotkey still works after any capture (no wedged loop).
14. Retina font sizing remains crisp.
15. Binary size ≤ 1.1 MB.

---

## Out of scope (deferred)

**Iter 2c polish:**
- Subset the embedded JetBrains Mono TTF (~267 KB → ~60 KB)
- Extract a shared `window_to_frame_scale(rect, window_size, frame_size)` helper (currently duplicated in `overlay/mod.rs::window_rect_to_frame_rect`, `render.rs::draw_size_label`, `render.rs::draw_magnifier`)
- Replace `estimate_text_width`'s `0.6 × px_size` heuristic with a real fontdue advance-width query (TODO already in source)

**Iter 3:**
- egui settings window (hotkey rebinding, autostart toggle, save-to-file toggle, path, filename template)
- PNG file saving with timestamp template
- `~/.config/quickshot/config.toml` persistence
- Auto-start on login

**Never:**
- Cross-screen selection (per original brainstorm)
