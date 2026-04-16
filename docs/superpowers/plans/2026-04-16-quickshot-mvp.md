# quickshot MVP (Iter 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a daemon-style screenshot tool where pressing `Ctrl/Cmd+Shift+A` shows a fullscreen overlay, the user drags a region, and on release the selected pixels land on the system clipboard as a PNG image.

**Architecture:** Pure Rust binary. Main thread runs a `winit` event loop (drives both the hidden daemon state and the on-demand overlay window). A background thread owned by `global-hotkey` posts hotkey events through an `EventLoopProxy`. When the hotkey fires we capture the screen once (via `xcap`), open a fullscreen window that renders that captured pixel buffer plus a dim overlay with a hole cut for the live selection rectangle (via `softbuffer`), track mouse drag, and on release crop the captured bitmap and push it into the clipboard (via `arboard`). macOS additionally gates startup on a Screen Recording permission check.

**Tech Stack:**
- Rust stable (edition 2021)
- `winit = "0.30"` — windowing / event loop / ApplicationHandler pattern
- `softbuffer = "0.4"` — CPU pixel buffer presentation (no GPU, small binary)
- `image = "0.25"` — RGBA buffer manipulation + PNG encode
- `xcap = "0.0.14"` — cross-platform screen capture
- `arboard = "3.4"` — cross-platform clipboard (supports image copy)
- `global-hotkey = "0.6"` — global hotkey registration
- `core-graphics = "0.23"` (macOS only, target-gated) — Screen Recording permission check
- `anyhow = "1"` — error plumbing for `main`

**Scope for this plan (MVP = Iter 1 only):**
- Single-monitor primary-display capture only (multi-monitor is Iter 2)
- No size label, no magnifier, no anchor-adjust, no ESC cancel (Iter 2)
- No settings window, no file saving, no notification, no tray icon (Iter 3)
- Daemon exits by Ctrl+C in terminal; no installer (Iter 4 skipped)

---

## File Structure

```
quickshot/
├── Cargo.toml
├── Cargo.lock          (generated)
├── .gitignore
├── README.md
├── docs/
│   └── superpowers/
│       └── plans/
│           └── 2026-04-16-quickshot-mvp.md
├── src/
│   ├── main.rs         // entry: permission check → hotkey register → run app
│   ├── app.rs          // winit ApplicationHandler; routes hotkey events → overlay
│   ├── capture.rs      // thin wrapper over xcap → image::RgbaImage
│   ├── crop.rs         // pure crop/normalize-rect helper (unit-tested)
│   ├── clipboard.rs    // arboard wrapper: push RgbaImage to clipboard
│   ├── hotkey.rs       // register Ctrl/Cmd+Shift+A and forward events via proxy
│   ├── overlay.rs      // fullscreen window: draw screenshot + dim + selection
│   └── permission.rs   // macOS Screen Recording preflight (cfg-gated)
└── tests/              // integration tests (only crop is unit-testable without a display)
```

**Responsibilities (one per file):**
- `crop.rs`: math only; normalize a drag rect (start/end points in any order) into `(x, y, w, h)` clamped to image bounds, and crop an `RgbaImage`. Pure functions, fully unit-testable.
- `capture.rs`: call into `xcap`, return `image::RgbaImage` + the monitor's logical origin.
- `clipboard.rs`: accept `&RgbaImage`, convert to `arboard::ImageData`, `set_image`.
- `hotkey.rs`: own the `GlobalHotKeyManager`, register the shortcut, spawn a forwarder thread that relays `GlobalHotKeyEvent`s to the winit `EventLoopProxy`.
- `overlay.rs`: construct a fullscreen borderless window, blit the captured bitmap + dim layer + selection rect each frame, track drag state.
- `app.rs`: implement `winit::application::ApplicationHandler`; on custom user event "HotkeyFired", call capture + spawn overlay window; on "SelectionDone(rect)" call crop + clipboard + close window.
- `permission.rs`: on macOS, `CGPreflightScreenCaptureAccess`; if missing, print guided instructions and exit 2. On other OSes, no-op returning `Ok(())`.
- `main.rs`: glue — run permission preflight, build event loop, create app state, register hotkey, run.

---

### Task 1: Project scaffold

**Files:**
- Create: `/Users/simmzl/Desktop/personal/quickshot/Cargo.toml`
- Create: `/Users/simmzl/Desktop/personal/quickshot/.gitignore`
- Create: `/Users/simmzl/Desktop/personal/quickshot/README.md`
- Create: `/Users/simmzl/Desktop/personal/quickshot/src/main.rs`

- [ ] **Step 1: Initialize cargo project and git repo**

Run:
```bash
cd /Users/simmzl/Desktop/personal/quickshot
cargo init --bin --name quickshot --vcs git
```

Expected: prints `Creating binary (application) package`, creates `Cargo.toml`, `src/main.rs`, `.gitignore`, `.git/`. The `docs/` directory already exists and is preserved.

- [ ] **Step 2: Write `Cargo.toml`**

Overwrite `Cargo.toml` with:
```toml
[package]
name = "quickshot"
version = "0.1.0"
edition = "2021"
description = "Small, fast screenshot daemon for macOS and Windows"
license = "MIT OR Apache-2.0"

[dependencies]
anyhow = "1"
arboard = "3.4"
global-hotkey = "0.6"
image = { version = "0.25", default-features = false, features = ["png"] }
softbuffer = "0.4"
winit = "0.30"
xcap = "0.0.14"

[target.'cfg(target_os = "macos")'.dependencies]
core-graphics = "0.23"

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

The release profile is tuned for the "small binary" goal — `opt-level="z"` + `strip` typically lands a pure-Rust app of this shape in the 3–6 MB range.

- [ ] **Step 3: Write `.gitignore`**

Overwrite `.gitignore` with:
```
/target
Cargo.lock
.DS_Store
```

(Binary crates historically committed `Cargo.lock`, but for a hobby tool we'll skip it to keep the repo clean; `cargo build` will reproduce it.)

- [ ] **Step 4: Write `README.md`**

Create `README.md` with:
```markdown
# quickshot

Small, fast screenshot tool for macOS and Windows. Pure Rust.

## Build

    cargo build --release

Binary lands at `target/release/quickshot`.

## Run

    ./target/release/quickshot

Press `Ctrl/Cmd+Shift+A`, drag a region, release — the PNG is now on your clipboard.

Quit with Ctrl+C in the launching terminal.

## macOS first run

The daemon requires Screen Recording permission. On first launch it will
detect the missing permission and print a guided prompt pointing you to
System Settings → Privacy & Security → Screen Recording. After granting,
relaunch.
```

- [ ] **Step 5: Replace `src/main.rs` with a module skeleton**

Overwrite `src/main.rs` with:
```rust
mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod overlay;
mod permission;

fn main() -> anyhow::Result<()> {
    permission::preflight()?;
    println!("quickshot starting; press Ctrl/Cmd+Shift+A to capture.");
    Ok(())
}
```

- [ ] **Step 6: Create empty module files so `cargo check` compiles**

Create each of the following with a single-line body:
```bash
for f in app capture clipboard crop hotkey overlay permission; do
  echo "// TODO" > src/$f.rs
done
```

Then replace each with a minimal compilable stub:

`src/permission.rs`:
```rust
pub fn preflight() -> anyhow::Result<()> {
    Ok(())
}
```

`src/app.rs`, `src/capture.rs`, `src/clipboard.rs`, `src/crop.rs`, `src/hotkey.rs`, `src/overlay.rs` — each just:
```rust
// filled in by later tasks
```

- [ ] **Step 7: Verify it builds**

Run:
```bash
cargo check
```

Expected: `Checking quickshot v0.1.0` followed by `Finished` with zero errors. There will be warnings about unused modules — that's fine.

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "chore: scaffold quickshot project"
```

---

### Task 2: Pure `crop` module (TDD)

The only logic we can unit-test without a display or clipboard. Get it solid here so the rest of the code can trust it.

**Files:**
- Modify: `src/crop.rs`

- [ ] **Step 1: Write failing tests**

Overwrite `src/crop.rs` with only the test module first (no implementation):
```rust
use image::RgbaImage;

pub fn normalize_rect(
    start: (i32, i32),
    end: (i32, i32),
    bounds: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    unimplemented!()
}

pub fn crop_rgba(
    img: &RgbaImage,
    rect: (u32, u32, u32, u32),
) -> RgbaImage {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn normalize_handles_reversed_drag() {
        // dragging right-to-left / bottom-to-top must still yield positive w/h
        let r = normalize_rect((80, 60), (20, 10), (100, 100)).unwrap();
        assert_eq!(r, (20, 10, 60, 50));
    }

    #[test]
    fn normalize_clamps_to_bounds() {
        // end point past the right/bottom edges is clipped
        let r = normalize_rect((10, 10), (1000, 1000), (100, 100)).unwrap();
        assert_eq!(r, (10, 10, 90, 90));
    }

    #[test]
    fn normalize_clamps_negative_start() {
        // negative start (e.g. mouse went above the window top) clips to 0
        let r = normalize_rect((-50, -50), (40, 40), (100, 100)).unwrap();
        assert_eq!(r, (0, 0, 40, 40));
    }

    #[test]
    fn normalize_rejects_degenerate() {
        // zero-area selection -> None
        assert!(normalize_rect((50, 50), (50, 50), (100, 100)).is_none());
    }

    #[test]
    fn crop_extracts_exact_region() {
        // build a 4x4 image; top-left 2x2 is red, rest blue
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]));
        for y in 0..2 {
            for x in 0..2 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let cropped = crop_rgba(&img, (0, 0, 2, 2));
        assert_eq!(cropped.dimensions(), (2, 2));
        for p in cropped.pixels() {
            assert_eq!(*p, Rgba([255, 0, 0, 255]));
        }
    }
}
```

- [ ] **Step 2: Run tests and confirm they fail**

Run:
```bash
cargo test -p quickshot crop::tests
```

Expected: compilation succeeds; tests panic with `not implemented` for all 5 cases.

- [ ] **Step 3: Implement `normalize_rect` and `crop_rgba`**

Replace the two function bodies in `src/crop.rs`:
```rust
use image::RgbaImage;

pub fn normalize_rect(
    start: (i32, i32),
    end: (i32, i32),
    bounds: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (bw, bh) = (bounds.0 as i32, bounds.1 as i32);
    let x0 = start.0.min(end.0).max(0).min(bw);
    let y0 = start.1.min(end.1).max(0).min(bh);
    let x1 = start.0.max(end.0).max(0).min(bw);
    let y1 = start.1.max(end.1).max(0).min(bh);
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    Some((x0 as u32, y0 as u32, w, h))
}

pub fn crop_rgba(img: &RgbaImage, rect: (u32, u32, u32, u32)) -> RgbaImage {
    let (x, y, w, h) = rect;
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}
```

- [ ] **Step 4: Run tests and confirm they pass**

Run:
```bash
cargo test -p quickshot crop::tests
```

Expected: `test result: ok. 5 passed; 0 failed`.

- [ ] **Step 5: Commit**

```bash
git add src/crop.rs
git commit -m "feat(crop): normalize drag rect and crop RGBA"
```

---

### Task 3: Clipboard module

`arboard::Clipboard::set_image` takes an `ImageData { width, height, bytes: Cow<[u8]> }` where `bytes` is raw RGBA. We accept a `RgbaImage` and push it.

**Files:**
- Modify: `src/clipboard.rs`

- [ ] **Step 1: Implement `put_image`**

Overwrite `src/clipboard.rs`:
```rust
use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use image::RgbaImage;
use std::borrow::Cow;

pub fn put_image(img: &RgbaImage) -> Result<()> {
    let (w, h) = img.dimensions();
    let data = ImageData {
        width: w as usize,
        height: h as usize,
        bytes: Cow::Borrowed(img.as_raw()),
    };
    let mut cb = Clipboard::new().context("open clipboard")?;
    cb.set_image(data).context("write image to clipboard")?;
    Ok(())
}
```

- [ ] **Step 2: Write a manual-verification test (marked `#[ignore]` so CI/green runs skip it)**

Append to `src/clipboard.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    // Requires a real OS clipboard; run with: cargo test -- --ignored
    #[test]
    #[ignore]
    fn roundtrip_clipboard() {
        let mut img = RgbaImage::new(16, 16);
        for (i, p) in img.pixels_mut().enumerate() {
            *p = Rgba([(i % 256) as u8, 0, 0, 255]);
        }
        put_image(&img).expect("put");

        let mut cb = arboard::Clipboard::new().unwrap();
        let got = cb.get_image().unwrap();
        assert_eq!(got.width, 16);
        assert_eq!(got.height, 16);
        assert_eq!(got.bytes.len(), 16 * 16 * 4);
    }
}
```

- [ ] **Step 3: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Finished with no errors.

- [ ] **Step 4: Run the ignored test manually to verify clipboard plumbing**

Run:
```bash
cargo test -- --ignored roundtrip_clipboard
```

Expected: `test result: ok. 1 passed`. (On macOS this may prompt for clipboard/automation permission the first time.)

- [ ] **Step 5: Commit**

```bash
git add src/clipboard.rs
git commit -m "feat(clipboard): push RgbaImage into system clipboard"
```

---

### Task 4: Screen capture module

Wrap `xcap` so the rest of the app sees a simple `capture_primary() -> RgbaImage`.

**Files:**
- Modify: `src/capture.rs`

- [ ] **Step 1: Implement `capture_primary`**

Overwrite `src/capture.rs`:
```rust
use anyhow::{anyhow, Context, Result};
use image::RgbaImage;
use xcap::Monitor;

/// Captures the primary monitor and returns its RGBA pixel buffer
/// together with the monitor's logical width/height in physical pixels.
pub fn capture_primary() -> Result<RgbaImage> {
    let monitors = Monitor::all().context("enumerate monitors")?;
    let primary = monitors
        .into_iter()
        .find(|m| m.is_primary())
        .ok_or_else(|| anyhow!("no primary monitor found"))?;
    let frame = primary.capture_image().context("capture monitor image")?;
    // xcap returns image::RgbaImage directly in recent versions.
    Ok(frame)
}
```

Note on API drift: if `xcap::Monitor::capture_image` returns a different type in the version you end up with, convert by reading `.as_raw()` and rebuilding via `RgbaImage::from_raw(w, h, raw).unwrap()`. Verify with `cargo doc --open -p xcap` if the first compile fails here.

- [ ] **Step 2: Verify it compiles**

Run:
```bash
cargo check
```

Expected: Finished with no errors. If `capture_image` doesn't return `RgbaImage`, follow the fallback note above.

- [ ] **Step 3: Write a smoke test binary example for manual verification**

Add to `src/capture.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Requires a real display. Run: cargo test -- --ignored capture_smoke
    #[test]
    #[ignore]
    fn capture_smoke() {
        let img = capture_primary().expect("capture");
        assert!(img.width() > 0 && img.height() > 0);
        println!("captured {}x{}", img.width(), img.height());
    }
}
```

- [ ] **Step 4: Run the smoke test**

Run:
```bash
cargo test -- --ignored capture_smoke -- --nocapture
```

Expected on macOS first run: permission prompt for Screen Recording (grant it, then re-run). After grant: `test result: ok` and a printed size matching your display (e.g. `captured 3456x2234` on a 14" MBP Retina).

- [ ] **Step 5: Commit**

```bash
git add src/capture.rs
git commit -m "feat(capture): grab primary monitor RGBA frame via xcap"
```

---

### Task 5: Global hotkey registration

Registers `Ctrl/Cmd+Shift+A` and relays presses through a `winit` `EventLoopProxy`. We define a custom event type `UserEvent` that the `app` module will eventually own; to keep this module self-contained we accept a generic proxy + callback.

**Files:**
- Modify: `src/hotkey.rs`

- [ ] **Step 1: Implement hotkey registration**

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

// Returned to main so the manager stays alive for the program's lifetime.
// Dropping it unregisters the hotkey.
pub struct HotkeyGuard {
    _manager: GlobalHotKeyManager,
}

pub fn register(proxy: EventLoopProxy<UserEvent>) -> Result<HotkeyGuard> {
    let manager = GlobalHotKeyManager::new().context("new GlobalHotKeyManager")?;

    // Ctrl+Shift+A on Windows, Cmd+Shift+A on macOS.
    // global-hotkey's Modifiers::META maps to Cmd on macOS / Win on Windows.
    // Use META on macOS, CONTROL on Windows.
    #[cfg(target_os = "macos")]
    let mods = Modifiers::META | Modifiers::SHIFT;
    #[cfg(not(target_os = "macos"))]
    let mods = Modifiers::CONTROL | Modifiers::SHIFT;

    let hotkey = HotKey::new(Some(mods), Code::KeyA);
    manager.register(hotkey).context("register hotkey")?;

    // Forwarder thread: poll the crossbeam receiver and push events into winit.
    let receiver = GlobalHotKeyEvent::receiver();
    thread::spawn(move || loop {
        if let Ok(_event) = receiver.try_recv() {
            // Ignore event id — we only registered one.
            let _ = proxy.send_event(UserEvent::HotkeyFired);
        }
        thread::sleep(Duration::from_millis(25));
    });

    Ok(HotkeyGuard { _manager: manager })
}
```

- [ ] **Step 2: Add the `UserEvent` enum to `app.rs` so the hotkey module compiles**

Overwrite `src/app.rs`:
```rust
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    // Placeholder; fleshed out in Task 7.
}

impl App {
    pub fn new() -> Self {
        Self {}
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: WindowId,
        _event: WindowEvent,
    ) {
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::HotkeyFired => {
                println!("hotkey fired");
            }
        }
    }
}
```

- [ ] **Step 3: Wire hotkey + app + event loop in `main.rs`**

Overwrite `src/main.rs`:
```rust
mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod overlay;
mod permission;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    permission::preflight()?;

    let event_loop = EventLoop::<app::UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    let _hotkey_guard = hotkey::register(proxy)?;

    println!("quickshot running; press Ctrl/Cmd+Shift+A to capture. Ctrl+C to quit.");

    let mut app = app::App::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
```

- [ ] **Step 4: Build and manually verify the hotkey fires**

Run:
```bash
cargo run --release
```

Expected:
- Prints `quickshot running; ...`
- Press `Cmd+Shift+A` (macOS) / `Ctrl+Shift+A` (Windows)
- Each press prints `hotkey fired`
- Ctrl+C in the terminal quits cleanly

If nothing prints on macOS, the app needs Accessibility permission for global hotkeys in some macOS versions. Grant it in System Settings → Privacy & Security → Accessibility and retry.

- [ ] **Step 5: Commit**

```bash
git add src/hotkey.rs src/app.rs src/main.rs
git commit -m "feat(hotkey): register Ctrl/Cmd+Shift+A and forward to event loop"
```

---

### Task 6: Overlay window (fullscreen + draw captured frame)

Build the overlay window: fullscreen borderless, renders the previously captured screen as a background using `softbuffer`, then dims it. No selection yet (that lands in Task 7).

**Files:**
- Modify: `src/overlay.rs`

- [ ] **Step 1: Implement the overlay window type**

Overwrite `src/overlay.rs`:
```rust
use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window, WindowAttributes};

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub drag_start: Option<(i32, i32)>,
    pub drag_end: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(event_loop: &ActiveEventLoop, frame: RgbaImage) -> Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title("quickshot overlay")
            .with_decorations(false)
            .with_resizable(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
        let window = Rc::new(event_loop.create_window(attrs).context("create window")?);

        let context = SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            drag_start: None,
            drag_end: None,
        })
    }

    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(
                NonZeroU32::new(w).unwrap(),
                NonZeroU32::new(h).unwrap(),
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        draw_background(&mut buf, w, h, &self.frame);
        apply_dim(&mut buf, w, h, /*inside=*/ None);

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

/// Copy the captured frame into the softbuffer surface, nearest-neighbor
/// scaled to the window's pixel dimensions. softbuffer's pixel format is
/// 0x00RRGGBB (u32 per pixel, X in the high byte).
fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
    let (fw, fh) = frame.dimensions();
    for y in 0..h {
        for x in 0..w {
            // nearest-neighbor: map window pixel -> frame pixel
            let fx = (x as u64 * fw as u64 / w as u64) as u32;
            let fy = (y as u64 * fh as u64 / h as u64) as u32;
            let p = frame.get_pixel(fx.min(fw - 1), fy.min(fh - 1));
            let [r, g, b, _a] = p.0;
            buf[(y * w + x) as usize] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
}

/// Darkens the whole buffer except the optional `inside` rect (x, y, w, h).
fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    for y in 0..h {
        for x in 0..w {
            let in_selection = match inside {
                Some((ix, iy, iw, ih)) => {
                    x >= ix && y >= iy && x < ix + iw && y < iy + ih
                }
                None => false,
            };
            if !in_selection {
                let i = (y * w + x) as usize;
                let px = buf[i];
                let r = ((px >> 16) & 0xFF) / 2;
                let g = ((px >> 8) & 0xFF) / 2;
                let b = px & 0xFF;
                let b = b / 2;
                buf[i] = (r << 16) | (g << 8) | b;
            }
        }
    }
}

pub(crate) use apply_dim as _apply_dim_reexport; // keep Task 7 import ergonomic
```

The 2× nested-loop blit is slow for 4K but fine for MVP — we render once per selection change. Iter 2 can move this to wgpu if measured as a problem.

- [ ] **Step 2: Wire `App` to open the overlay on hotkey**

Overwrite `src/app.rs`:
```rust
use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::capture;
use crate::overlay::Overlay;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    overlay: Option<Overlay>,
}

impl App {
    pub fn new() -> Self {
        Self { overlay: None }
    }

    fn open_overlay(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let frame = capture::capture_primary()?;
        let overlay = Overlay::create(event_loop, frame)?;
        overlay.window.request_redraw();
        self.overlay = Some(overlay);
        Ok(())
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        if overlay.window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.overlay = None;
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = overlay.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
            }
            _ => {}
        }
        let _ = event_loop; // silence unused
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::HotkeyFired => {
                if let Err(e) = self.open_overlay(event_loop) {
                    eprintln!("open overlay error: {e:?}");
                }
            }
        }
    }
}
```

- [ ] **Step 3: Build and manually verify**

Run:
```bash
cargo run --release
```

Press `Cmd/Ctrl+Shift+A`. Expected:
- Screen dims (a frozen darkened copy of the desktop fills the display)
- No selection rectangle yet (Task 7)
- Close the window via the OS window-close key combo (Cmd+Q on macOS may not route to the borderless window — instead Ctrl+C the terminal) or move on to Task 7

If the overlay never appears on macOS, you likely hit the Screen Recording permission prompt from `xcap` — grant it and re-run.

- [ ] **Step 4: Commit**

```bash
git add src/overlay.rs src/app.rs
git commit -m "feat(overlay): fullscreen window with dimmed captured frame"
```

---

### Task 7: Selection tracking + copy-to-clipboard

Hook up mouse events on the overlay: mouse-down starts selection, mouse-move updates end point + triggers redraw, mouse-up completes selection → crop → clipboard → close overlay.

**Files:**
- Modify: `src/overlay.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Expose helpers + add selection redraw path in `overlay.rs`**

Replace the `redraw` method and add a `current_selection_rect` helper in `src/overlay.rs`. Full updated file:
```rust
use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window, WindowAttributes};

use crate::crop;

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub drag_start: Option<(i32, i32)>,
    pub drag_end: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(event_loop: &ActiveEventLoop, frame: RgbaImage) -> Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title("quickshot overlay")
            .with_decorations(false)
            .with_resizable(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
        let window = Rc::new(event_loop.create_window(attrs).context("create window")?);

        let context = SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            drag_start: None,
            drag_end: None,
        })
    }

    /// Current window-space rect (x, y, w, h) while dragging, if any.
    pub fn current_window_rect(&self) -> Option<(u32, u32, u32, u32)> {
        let (s, e) = (self.drag_start?, self.drag_end?);
        let size = self.window.inner_size();
        crop::normalize_rect(s, e, (size.width, size.height))
    }

    /// Translate a window-space rect into a frame-space rect.
    pub fn window_rect_to_frame_rect(
        &self,
        rect: (u32, u32, u32, u32),
    ) -> (u32, u32, u32, u32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1), size.height.max(1));
        let (fw, fh) = self.frame.dimensions();
        let (x, y, w, h) = rect;
        let fx = (x as u64 * fw as u64 / ww as u64) as u32;
        let fy = (y as u64 * fh as u64 / wh as u64) as u32;
        let fw2 = (w as u64 * fw as u64 / ww as u64) as u32;
        let fh2 = (h as u64 * fh as u64 / wh as u64) as u32;
        (fx, fy, fw2.max(1), fh2.max(1))
    }

    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(
                NonZeroU32::new(w).unwrap(),
                NonZeroU32::new(h).unwrap(),
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        draw_background(&mut buf, w, h, &self.frame);
        let sel = self.current_window_rect();
        apply_dim(&mut buf, w, h, sel);
        if let Some(r) = sel {
            draw_rect_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
    let (fw, fh) = frame.dimensions();
    for y in 0..h {
        for x in 0..w {
            let fx = (x as u64 * fw as u64 / w as u64) as u32;
            let fy = (y as u64 * fh as u64 / h as u64) as u32;
            let p = frame.get_pixel(fx.min(fw - 1), fy.min(fh - 1));
            let [r, g, b, _a] = p.0;
            buf[(y * w + x) as usize] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
}

fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    for y in 0..h {
        for x in 0..w {
            let in_selection = match inside {
                Some((ix, iy, iw, ih)) => {
                    x >= ix && y >= iy && x < ix + iw && y < iy + ih
                }
                None => false,
            };
            if !in_selection {
                let i = (y * w + x) as usize;
                let px = buf[i];
                let r = ((px >> 16) & 0xFF) / 2;
                let g = ((px >> 8) & 0xFF) / 2;
                let b = (px & 0xFF) / 2;
                buf[i] = (r << 16) | (g << 8) | b;
            }
        }
    }
}

fn draw_rect_outline(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: (u32, u32, u32, u32),
    color: u32,
) {
    let (rx, ry, rw, rh) = rect;
    let x1 = rx;
    let y1 = ry;
    let x2 = rx + rw.saturating_sub(1);
    let y2 = ry + rh.saturating_sub(1);
    for x in x1..=x2.min(w - 1) {
        buf[(y1 * w + x) as usize] = color;
        buf[(y2.min(h - 1) * w + x) as usize] = color;
    }
    for y in y1..=y2.min(h - 1) {
        buf[(y * w + x1) as usize] = color;
        buf[(y * w + x2.min(w - 1)) as usize] = color;
    }
}
```

- [ ] **Step 2: Handle mouse events in `app.rs` and complete the selection**

Replace `src/app.rs`:
```rust
use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::crop;
use crate::overlay::Overlay;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    overlay: Option<Overlay>,
    cursor: (i32, i32),
}

impl App {
    pub fn new() -> Self {
        Self {
            overlay: None,
            cursor: (0, 0),
        }
    }

    fn open_overlay(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let frame = capture::capture_primary()?;
        let overlay = Overlay::create(event_loop, frame)?;
        overlay.window.request_redraw();
        self.overlay = Some(overlay);
        Ok(())
    }

    fn finish_selection(&mut self) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let Some(win_rect) = overlay.current_window_rect() else {
            // zero-area -> silently cancel
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(win_rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        if let Err(e) = clipboard::put_image(&cropped) {
            eprintln!("clipboard error: {e:?}");
        } else {
            println!(
                "copied {}x{} to clipboard",
                cropped.width(),
                cropped.height()
            );
        }
        drop(overlay); // closes the window
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        if overlay.window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.overlay = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                if overlay.drag_start.is_some() {
                    overlay.drag_end = Some(self.cursor);
                    overlay.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                overlay.drag_start = Some(self.cursor);
                overlay.drag_end = Some(self.cursor);
                overlay.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                overlay.drag_end = Some(self.cursor);
                self.finish_selection();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = overlay.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::HotkeyFired => {
                if let Err(e) = self.open_overlay(event_loop) {
                    eprintln!("open overlay error: {e:?}");
                }
            }
        }
    }
}
```

- [ ] **Step 3: Build**

Run:
```bash
cargo build --release
```

Expected: clean build. Fix any type/borrow errors before continuing.

- [ ] **Step 4: Manual end-to-end verification**

Run:
```bash
./target/release/quickshot
```

Test script:
1. Press `Cmd/Ctrl+Shift+A` → overlay appears with dimmed desktop
2. Click-drag a region → white rectangle follows; inside stays undimmed
3. Release mouse → overlay closes, terminal prints `copied WxH to clipboard`
4. Paste into Preview / Paint / a chat app → the selected region shows up as an image
5. Press hotkey again → overlay reappears; selecting a different region and pasting yields different contents

If the pasted image is offset or scaled wrongly, the `window_rect_to_frame_rect` math is off — verify by printing `(fw, fh)` vs `(ww, wh)` and the intermediate rect.

- [ ] **Step 5: Commit**

```bash
git add src/overlay.rs src/app.rs
git commit -m "feat(overlay): drag-select, crop, and copy to clipboard"
```

---

### Task 8: macOS permission preflight

On macOS, call `CGPreflightScreenCaptureAccess` at startup. If false, print a guided message and exit non-zero. Windows build gets a no-op.

**Files:**
- Modify: `src/permission.rs`

- [ ] **Step 1: Implement preflight with cfg gating**

Overwrite `src/permission.rs`:
```rust
use anyhow::Result;

#[cfg(target_os = "macos")]
pub fn preflight() -> Result<()> {
    use core_graphics::access::ScreenCaptureAccess;

    let access = ScreenCaptureAccess::default();
    if access.preflight() {
        return Ok(());
    }

    eprintln!(
        "\nquickshot needs Screen Recording permission to work on macOS.\n\n\
         1. Open System Settings → Privacy & Security → Screen Recording\n\
         2. Enable `quickshot` (or the terminal running it) in the list\n\
         3. Relaunch quickshot\n\n\
         You can trigger the system prompt now by pressing Enter (this will\n\
         also exit quickshot so you can grant permission and restart).\n"
    );
    let _ = access.request();
    std::process::exit(2);
}

#[cfg(not(target_os = "macos"))]
pub fn preflight() -> Result<()> {
    Ok(())
}
```

Note on the `core-graphics` API: the `ScreenCaptureAccess` type exposes `preflight()` → `bool` and `request()` which triggers the system dialog. If your `core-graphics` minor version lacks `ScreenCaptureAccess`, drop to raw FFI:
```rust
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}
```
and call them inside `unsafe` blocks.

- [ ] **Step 2: Verify it compiles on the current OS**

Run:
```bash
cargo check
```

Expected: clean. On a non-macOS host only the no-op branch compiles.

- [ ] **Step 3: Manual verification (macOS only)**

On a Mac where you have not yet granted Screen Recording permission to the terminal running cargo:
```bash
./target/release/quickshot
```
Expected: prints the guided message and exits with status 2 (check with `echo $?`). Grant permission in System Settings and re-run; the daemon should start normally.

On Windows the preflight is a no-op; nothing to verify beyond a successful build.

- [ ] **Step 4: Commit**

```bash
git add src/permission.rs
git commit -m "feat(permission): macOS Screen Recording preflight + guidance"
```

---

### Task 9: End-to-end polish + README update

Final pass: confirm warnings are clean, tests pass, and the README accurately describes MVP behavior.

**Files:**
- Modify: `README.md` (already created in Task 1)
- No code changes unless cleanup is needed

- [ ] **Step 1: Run the full test suite**

Run:
```bash
cargo test
cargo test -- --ignored
```

Expected: all unit tests pass; ignored tests (`capture_smoke`, `roundtrip_clipboard`) pass when run manually against a real display/clipboard.

- [ ] **Step 2: Run clippy and fix anything flagged**

Run:
```bash
cargo clippy --release --all-targets -- -D warnings
```

Expected: no warnings. Fix any that appear (usually easy — iterator idioms, redundant clones).

- [ ] **Step 3: Build the release binary and record its size**

Run:
```bash
cargo build --release
ls -lh target/release/quickshot
```

Expected: a single binary under ~8 MB. If it's larger, confirm `strip = true` and `lto = true` are set in `Cargo.toml`'s release profile.

- [ ] **Step 4: Update README with verified size + known limitations**

Open `README.md` and add a section after the Run block:
```markdown
## MVP status (Iter 1)

- Primary monitor only
- No size label, magnifier, anchor-adjust, or ESC cancel yet
- No settings window, file saving, notifications, or tray icon yet
- Exit with Ctrl+C in the launching terminal

Release binary size on this machine: <paste `ls -lh` output here>.
```

- [ ] **Step 5: Commit the polish pass**

```bash
git add README.md
git commit -m "docs: record MVP status and release binary size"
```

- [ ] **Step 6: Tag the MVP**

```bash
git tag -a v0.1.0-mvp -m "Iter 1 MVP: hotkey -> drag -> clipboard"
```

---

## Manual Verification Checklist (whole plan)

After finishing all tasks, run through this end-to-end script on one platform (ideally macOS first, since it has the trickier permissions):

1. Fresh terminal: `./target/release/quickshot` — starts and prints banner
2. Press hotkey — overlay appears, desktop dimmed
3. Drag — white rect tracks cursor, inside stays bright
4. Release — overlay closes, terminal prints `copied WxH to clipboard`
5. Paste into an image viewer — correct region shows up
6. Repeat with a different region — different paste result
7. Ctrl+C the terminal — exits cleanly, no panic, no hung window
8. Re-run — hotkey still works (no "hotkey already registered" errors)
9. (macOS only) Revoke Screen Recording permission, run — exits with status 2 and the guidance message

All nine green = MVP done.

---

## Out of scope (deferred to Iter 2+)

- Multi-monitor capture and cross-monitor selection
- Size label near selection
- Magnifier + crosshair
- Selection anchor adjustment after drag
- ESC to cancel
- Full-screen hotkey (Ctrl/Cmd+Shift+S)
- System notifications
- Tray icon
- Settings window (egui)
- Persistent config (`~/.config/quickshot/config.toml`)
- File saving (PNG + filename template)
- Auto-start on login
- Installer / auto-update (Iter 4 — skipped per spec)
