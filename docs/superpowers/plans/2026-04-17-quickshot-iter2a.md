# quickshot Iter 2a — Selection Interaction Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Upgrade the overlay from the MVP drag-and-release model to a Snipaste-style two-phase interaction (Drag → Adjust → Confirm) with anchors, magnifier, size label, Enter/double-click confirm, and ESC cancel — without touching capture, crop, clipboard, hotkey, or permission modules.

**Architecture:** Split `src/overlay.rs` into a submodule (`src/overlay/{mod,state,hit,render}.rs`) with pure testable pieces for state transitions, hit-testing, and geometry math. Add a `src/text.rs` module wrapping `fontdue` for label rendering. `app.rs` becomes a thin dispatcher that feeds winit events into the overlay and reacts to its `Outcome` return.

**Tech Stack:**
- Rust 2021 (existing toolchain)
- `winit 0.30`, `softbuffer 0.4`, `xcap 0.9`, `arboard 3.4`, `global-hotkey 0.6`, `image 0.25`, `core-graphics 0.23`, `anyhow 1` (all existing)
- **New:** `fontdue 0.9` — pure-Rust TrueType rasterizer
- **New asset:** `assets/fonts/JetBrainsMono-Regular.ttf` — embedded via `include_bytes!` (OFL 1.1)

**Spec:** `docs/superpowers/specs/2026-04-17-quickshot-iter2a-design.md`

**Scope for this plan (Iter 2a only — everything else explicitly deferred):**
- Anchor-based adjustment of drafted selection (8 anchors, resize + translate)
- Enter / double-click confirm, ESC cancel
- Magnifier (Idle + Dragging only)
- Size label (Dragging + Adjusting)
- Submodule split of `overlay.rs`; no API break visible outside

**Not in this plan (Iter 2b / Iter 3):**
- Full-screen hotkey `Cmd/Ctrl+Shift+S`, system notifications, tray icon
- Settings window, file saving, config persistence, autostart
- Cross-screen selection, Shift-constrain, configurable magnifier

---

## File Structure

```
quickshot/
├── Cargo.toml                           (modified — add fontdue dep)
├── assets/
│   └── fonts/
│       └── JetBrainsMono-Regular.ttf    (new — embedded font)
├── src/
│   ├── main.rs                          (modified — declare text module)
│   ├── app.rs                           (modified — route overlay Outcome)
│   ├── capture.rs                       (unchanged)
│   ├── clipboard.rs                     (unchanged)
│   ├── crop.rs                          (unchanged)
│   ├── hotkey.rs                        (unchanged)
│   ├── permission.rs                    (unchanged)
│   ├── text.rs                          (new — fontdue wrapper + glyph cache)
│   ├── overlay.rs                       (deleted — replaced by submodule)
│   └── overlay/
│       ├── mod.rs                       (new — Overlay struct, window/surface,
│       │                                 event ingress, redraw orchestration,
│       │                                 macOS window level, frame↔window math)
│       ├── state.rs                     (new — OverlayState enum, Rect helpers,
│       │                                 pure transitions; unit-tested)
│       ├── hit.rs                       (new — cursor classification into
│       │                                 HitZone + cursor icon; unit-tested)
│       └── render.rs                    (new — pure draw_* layer fns +
│                                         position helpers; unit-tested math)
```

**Responsibilities (one per file):**

- `overlay/mod.rs` — owns winit `Window`, `softbuffer::Surface`, the captured `RgbaImage`, monitor geometry, cursor position, a `Font`, and `OverlayState`. Translates `WindowEvent`s into pure state transitions and calls `request_redraw()`. Exposes `Overlay::create`, `Overlay::handle_event(event) -> Outcome`, `Overlay::redraw`.
- `overlay/state.rs` — `OverlayState` enum + `Rect` + `Anchor` + `Edit` types. Pure functions for state transitions and rect math (normalize, translate, resize_from_anchor, clamp_to). Fully unit-testable.
- `overlay/hit.rs` — pure `classify(cursor, rect) -> HitZone` and `cursor_icon_for(zone) -> CursorIcon`. Fully unit-testable.
- `overlay/render.rs` — pure draw functions operating on `&mut [u32]` softbuffer slices + position math helpers. Math is unit-tested; pixel output is hand-verified.
- `text.rs` — `Font` struct wrapping `fontdue::Font` with a glyph cache and `render_text(buf, w, h, x, y, text, px_size, color_rgb)`.
- `app.rs` — creates `Overlay`, forwards winit events, reacts to `Outcome::{Continue, Confirmed(Rect), Cancelled}`.

---

## Task 1: Scaffold overlay submodule split (no behavior change)

Move the current `overlay.rs` into `overlay/mod.rs`, extract the draw functions into `overlay/render.rs`, and create empty stubs for `state.rs` / `hit.rs`. Goal: Iter 1 end-to-end behavior unchanged after this task; only file layout differs.

**Files:**
- Delete: `src/overlay.rs`
- Create: `src/overlay/mod.rs`
- Create: `src/overlay/render.rs`
- Create: `src/overlay/state.rs` (empty stub)
- Create: `src/overlay/hit.rs` (empty stub)

- [ ] **Step 1: Create the `overlay/` directory and empty stub files**

Run:
```bash
mkdir -p src/overlay
: > src/overlay/state.rs
: > src/overlay/hit.rs
```

- [ ] **Step 2: Write `src/overlay/render.rs` with the existing draw helpers**

Create `src/overlay/render.rs`:
```rust
use image::RgbaImage;

/// Copy the captured frame into the softbuffer surface, nearest-neighbor
/// scaled to the window's pixel dimensions. Softbuffer's pixel format is
/// 0x00RRGGBB (u32 per pixel).
pub fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
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

/// Darken everything in `buf` except the optional `inside` rect (x, y, w, h).
pub fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    for y in 0..h {
        for x in 0..w {
            let in_selection = match inside {
                Some((ix, iy, iw, ih)) => x >= ix && y >= iy && x < ix + iw && y < iy + ih,
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

/// Draw a 1-px-thick rectangle outline in the given color (0x00RRGGBB).
pub fn draw_selection_outline(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: (u32, u32, u32, u32),
    color: u32,
) {
    let (rx, ry, rw, rh) = rect;
    if rw == 0 || rh == 0 {
        return;
    }
    let x1 = rx;
    let y1 = ry;
    let x2 = rx + rw.saturating_sub(1);
    let y2 = ry + rh.saturating_sub(1);
    let xmax = w.saturating_sub(1);
    let ymax = h.saturating_sub(1);
    for x in x1..=x2.min(xmax) {
        buf[(y1.min(ymax) * w + x) as usize] = color;
        buf[(y2.min(ymax) * w + x) as usize] = color;
    }
    for y in y1..=y2.min(ymax) {
        buf[(y * w + x1.min(xmax)) as usize] = color;
        buf[(y * w + x2.min(xmax)) as usize] = color;
    }
}
```

- [ ] **Step 3: Write `src/overlay/mod.rs` (moved from the old `overlay.rs`)**

Create `src/overlay/mod.rs`:
```rust
pub mod hit;
pub mod render;
pub mod state;

use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::MonitorGeom;
use crate::crop;

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub drag_start: Option<(i32, i32)>,
    pub drag_end: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(
        event_loop: &ActiveEventLoop,
        frame: RgbaImage,
        monitor_geom: &MonitorGeom,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        let window = {
            let size = winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
                monitor_geom.width as f64,
                monitor_geom.height as f64,
            ));
            let position = winit::dpi::Position::Logical(winit::dpi::LogicalPosition::new(
                monitor_geom.x as f64,
                monitor_geom.y as f64,
            ));
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_inner_size(size)
                .with_position(position);
            let win = event_loop.create_window(attrs).context("create window")?;
            set_macos_window_level(&win, 1000);
            win
        };

        #[cfg(not(target_os = "macos"))]
        let window = {
            let target_monitor = event_loop.available_monitors().find(|m| {
                let pos = m.position();
                pos.x == monitor_geom.x && pos.y == monitor_geom.y
            });
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_fullscreen(Some(winit::window::Fullscreen::Borderless(target_monitor)));
            event_loop.create_window(attrs).context("create window")?
        };

        let window = Rc::new(window);
        let context = SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface =
            Surface::new(&context, window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            drag_start: None,
            drag_end: None,
        })
    }

    pub fn current_window_rect(&self) -> Option<(u32, u32, u32, u32)> {
        let (s, e) = (self.drag_start?, self.drag_end?);
        let size = self.window.inner_size();
        crop::normalize_rect(s, e, (size.width, size.height))
    }

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
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel = self.current_window_rect();

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, &self.frame);
        render::apply_dim(&mut buf, w, h, sel);
        if let Some(r) = sel {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn set_macos_window_level(window: &Window, level: i64) {
    use winit::raw_window_handle::HasWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let raw = handle.as_raw();
    let winit::raw_window_handle::RawWindowHandle::AppKit(appkit) = raw else {
        return;
    };
    extern "C" {
        fn objc_msgSend(
            obj: *mut std::ffi::c_void,
            sel: *mut std::ffi::c_void,
            ...
        ) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const u8) -> *mut std::ffi::c_void;
    }
    unsafe {
        let ns_view = appkit.ns_view.as_ptr();
        let sel_window = sel_registerName(b"window\0".as_ptr());
        let ns_window = objc_msgSend(ns_view, sel_window);
        if ns_window.is_null() {
            return;
        }
        let sel_set_level = sel_registerName(b"setLevel:\0".as_ptr());
        objc_msgSend(ns_window, sel_set_level, level);
    }
}
```

- [ ] **Step 4: Delete the old `src/overlay.rs`**

Run:
```bash
rm src/overlay.rs
```

- [ ] **Step 5: Verify the build is clean**

Run:
```bash
cargo check
```

Expected: `Finished` with zero errors. Warnings from empty `state.rs` / `hit.rs` are fine.

- [ ] **Step 6: Smoke-test the binary still behaves like Iter 1**

Run:
```bash
cargo run --release
```

Press `Cmd/Ctrl+Shift+A`, drag a region, release. Verify: overlay appears, selection rectangle renders, release copies to clipboard. Paste into Preview to confirm. No regression vs the pre-refactor tag.

- [ ] **Step 7: Commit**

```bash
git add src/overlay src/overlay.rs 2>/dev/null; git add -A src/overlay src/
git commit -m "refactor(overlay): split into submodule (state/hit/render)"
```

Use this exact sequence if `git add` complains about the deleted file:
```bash
git rm src/overlay.rs
git add src/overlay/
git commit -m "refactor(overlay): split into submodule (state/hit/render)"
```

---

## Task 2: Add `fontdue` dependency, font asset, and `text.rs` module

Add the `fontdue` crate, embed JetBrains Mono Regular, write a minimal `Font` wrapper with a glyph cache, and smoke-test it by writing "Hello" into an in-memory buffer.

**Files:**
- Modify: `Cargo.toml`
- Create: `assets/fonts/JetBrainsMono-Regular.ttf`
- Create: `src/text.rs`
- Modify: `src/main.rs` (declare `mod text;`)

- [ ] **Step 1: Add the `fontdue` dependency**

Edit `Cargo.toml`. In the `[dependencies]` block, add `fontdue = "0.9"` alphabetically. After the edit the block should read:
```toml
[dependencies]
anyhow = "1"
arboard = "3.4"
fontdue = "0.9"
global-hotkey = "0.6"
image = { version = "0.25", default-features = false, features = ["png"] }
softbuffer = "0.4"
winit = "0.30"
xcap = "0.9"
```

- [ ] **Step 2: Download JetBrains Mono Regular and commit it as an asset**

Run:
```bash
mkdir -p assets/fonts
curl -L -o assets/fonts/JetBrainsMono-Regular.ttf \
  https://github.com/JetBrains/JetBrainsMono/raw/v2.304/fonts/ttf/JetBrainsMono-Regular.ttf
ls -lh assets/fonts/JetBrainsMono-Regular.ttf
```

Expected: a file of ~180 KB. If the size is < 100 KB or > 300 KB, re-verify the URL resolved correctly (the redirect must land on a real binary).

Also create `assets/fonts/LICENSE` with a note about the font license:
```bash
cat > assets/fonts/LICENSE <<'EOF'
JetBrains Mono is licensed under the SIL Open Font License 1.1.
Full license: https://github.com/JetBrains/JetBrainsMono/blob/master/OFL.txt
EOF
```

- [ ] **Step 3: Write `src/text.rs`**

Create `src/text.rs`:
```rust
use fontdue::{Font as FontdueFont, FontSettings};
use std::collections::HashMap;

const FONT_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf");

/// Rasterized glyph cache keyed by `(char, px_size_tenths)` so callers using
/// integer pt sizes (e.g. 12.0, 14.0) reuse the same bitmap.
pub struct Font {
    inner: Option<FontdueFont>,
    cache: HashMap<(char, u32), (fontdue::Metrics, Vec<u8>)>,
}

impl Font {
    /// Loads the embedded font. On failure returns a Font whose `render_text`
    /// is a silent no-op — the UI still works, labels just don't paint.
    pub fn embedded() -> Self {
        let inner = FontdueFont::from_bytes(FONT_BYTES, FontSettings::default()).ok();
        Self {
            inner,
            cache: HashMap::new(),
        }
    }

    fn rasterize(
        &mut self,
        ch: char,
        px_size: f32,
    ) -> Option<&(fontdue::Metrics, Vec<u8>)> {
        let font = self.inner.as_ref()?;
        let key = (ch, (px_size * 10.0) as u32);
        if !self.cache.contains_key(&key) {
            let (metrics, bitmap) = font.rasterize(ch, px_size);
            self.cache.insert(key, (metrics, bitmap));
        }
        self.cache.get(&key)
    }

    /// Draw `text` into the softbuffer `buf` (0x00RRGGBB per pixel) at pen
    /// position (x, y) = baseline *top-left* in pixels. `color_rgb` is the
    /// foreground color; alpha is taken from the glyph coverage and blended
    /// over whatever is already in `buf`.
    pub fn render_text(
        &mut self,
        buf: &mut [u32],
        w: u32,
        h: u32,
        x: i32,
        y: i32,
        text: &str,
        px_size: f32,
        color_rgb: u32,
    ) {
        if self.inner.is_none() {
            return;
        }
        let (fr, fg, fb) = (
            ((color_rgb >> 16) & 0xFF) as u32,
            ((color_rgb >> 8) & 0xFF) as u32,
            (color_rgb & 0xFF) as u32,
        );
        let mut pen_x = x as f32;
        for ch in text.chars() {
            let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else {
                continue;
            };
            let gx = pen_x.round() as i32 + metrics.xmin;
            // Fontdue's ymin is measured from the glyph's baseline upwards;
            // to place a "top-left" pen we shift the glyph down by the ascent
            // of the requested size. Approximate the ascent as px_size * 0.8.
            let ascent = (px_size * 0.8) as i32;
            let gy = y + ascent - metrics.height as i32 - metrics.ymin;
            blit_glyph(buf, w, h, gx, gy, &metrics, &bitmap, (fr, fg, fb));
            pen_x += metrics.advance_width;
        }
    }
}

fn blit_glyph(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    metrics: &fontdue::Metrics,
    bitmap: &[u8],
    color: (u32, u32, u32),
) {
    let (fr, fg, fb) = color;
    for gy in 0..metrics.height as i32 {
        for gx in 0..metrics.width as i32 {
            let alpha = bitmap[(gy * metrics.width as i32 + gx) as usize] as u32;
            if alpha == 0 {
                continue;
            }
            let px = x + gx;
            let py = y + gy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                continue;
            }
            let idx = (py as u32 * w + px as u32) as usize;
            let bg = buf[idx];
            let br = (bg >> 16) & 0xFF;
            let bgc = (bg >> 8) & 0xFF;
            let bb = bg & 0xFF;
            let r = (fr * alpha + br * (255 - alpha)) / 255;
            let g = (fg * alpha + bgc * (255 - alpha)) / 255;
            let b = (fb * alpha + bb * (255 - alpha)) / 255;
            buf[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_loads_and_renders_nonzero_pixels() {
        let mut font = Font::embedded();
        assert!(font.inner.is_some(), "embedded font failed to load");
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(
            buf.iter().any(|&p| p != 0),
            "rendering 'Hi' produced no non-zero pixels"
        );
    }

    #[test]
    fn missing_font_is_silent_noop() {
        let mut font = Font {
            inner: None,
            cache: HashMap::new(),
        };
        let (w, h) = (64u32, 32u32);
        let mut buf = vec![0u32; (w * h) as usize];
        font.render_text(&mut buf, w, h, 2, 2, "Hi", 16.0, 0x00FFFFFF);
        assert!(buf.iter().all(|&p| p == 0));
    }
}
```

- [ ] **Step 4: Declare the module in `src/main.rs`**

Edit `src/main.rs` — add `mod text;` to the module declarations. After the edit the head of the file reads:
```rust
mod app;
mod capture;
mod clipboard;
mod crop;
mod hotkey;
mod overlay;
mod permission;
mod text;

use anyhow::Result;
use winit::event_loop::EventLoop;
```

(The rest of `main.rs` is unchanged.)

- [ ] **Step 5: Build and run the text tests**

Run:
```bash
cargo test --lib text::tests
```

Expected: `test result: ok. 2 passed; 0 failed`.

- [ ] **Step 6: Verify the release binary still builds cleanly**

Run:
```bash
cargo build --release
ls -lh target/release/quickshot
```

Expected: clean build. Binary size now ~650–720 KB (Iter 1 was ~500 KB; fontdue + embedded TTF adds roughly 150–200 KB).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock assets/ src/text.rs src/main.rs
git commit -m "feat(text): embed JetBrains Mono + fontdue-based glyph renderer"
```

---

## Task 3: Introduce `OverlayState` enum with pure transitions (Iter 1 UX preserved)

Define the state types and pure transition helpers. Wire them into the overlay so event handling goes through `state::` transitions — but keep MouseUp as the confirmation trigger for now so Iter 1 end-to-end behavior stays intact. This task is a big internal change with no user-visible delta; we bank the interaction-model scaffolding, then flip UX in Task 4.

**Files:**
- Modify: `src/overlay/state.rs` (full implementation)
- Modify: `src/overlay/mod.rs` (route events through state, return `Outcome`)
- Modify: `src/app.rs` (consume `Outcome`)

- [ ] **Step 1: Write failing tests for `state.rs`**

Overwrite `src/overlay/state.rs`:
```rust
//! Pure state machine + geometry helpers for the overlay.
//! No winit, no softbuffer — everything here is unit-testable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor { TL, T, TR, R, BR, B, BL, L }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Resize { anchor: Anchor, origin: Rect, from: (i32, i32) },
    Translate { origin: Rect, from: (i32, i32) },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayState {
    Idle,
    Dragging { start: (i32, i32), end: (i32, i32) },
    Adjusting { rect: Rect, edit: Option<Edit> },
}

impl Rect {
    pub fn normalize(a: (i32, i32), b: (i32, i32)) -> Rect {
        let (ax, ay) = a;
        let (bx, by) = b;
        let x = ax.min(bx);
        let y = ay.min(by);
        let w = (ax - bx).abs();
        let h = (ay - by).abs();
        Rect { x, y, w, h }
    }

    pub fn contains(&self, p: (i32, i32)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    pub fn clamp_to(&self, bounds: (u32, u32)) -> Rect {
        let (bw, bh) = (bounds.0 as i32, bounds.1 as i32);
        let x = self.x.clamp(0, bw);
        let y = self.y.clamp(0, bh);
        let x2 = (self.x + self.w).clamp(0, bw);
        let y2 = (self.y + self.h).clamp(0, bh);
        Rect { x, y, w: (x2 - x).max(0), h: (y2 - y).max(0) }
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, w: self.w, h: self.h }
    }

    pub fn resize_from_anchor(&self, anchor: Anchor, dx: i32, dy: i32) -> Rect {
        let (mut x, mut y, mut w, mut h) = (self.x, self.y, self.w, self.h);
        match anchor {
            Anchor::TL => { x += dx; y += dy; w -= dx; h -= dy; }
            Anchor::T  => { y += dy; h -= dy; }
            Anchor::TR => { y += dy; w += dx; h -= dy; }
            Anchor::R  => { w += dx; }
            Anchor::BR => { w += dx; h += dy; }
            Anchor::B  => { h += dy; }
            Anchor::BL => { x += dx; w -= dx; h += dy; }
            Anchor::L  => { x += dx; w -= dx; }
        }
        // If width or height went negative, collapse to 1 px rather than
        // flipping through zero — simpler for downstream consumers.
        if w < 1 { w = 1; }
        if h < 1 { h = 1; }
        Rect { x, y, w, h }
    }

    pub fn as_tuple_u32(&self) -> (u32, u32, u32, u32) {
        (
            self.x.max(0) as u32,
            self.y.max(0) as u32,
            self.w.max(0) as u32,
            self.h.max(0) as u32,
        )
    }
}

/// Returned by confirm/cancel helpers so the caller (app.rs) knows what to do.
#[derive(Debug, Clone, Copy)]
pub enum Transition {
    Stay,
    Confirm(Rect),
    Cancel,
}

pub fn on_mouse_down_idle(cursor: (i32, i32)) -> OverlayState {
    OverlayState::Dragging { start: cursor, end: cursor }
}

pub fn on_mouse_move_dragging(start: (i32, i32), cursor: (i32, i32)) -> OverlayState {
    OverlayState::Dragging { start, end: cursor }
}

pub fn on_mouse_up_dragging(start: (i32, i32), end: (i32, i32)) -> OverlayState {
    OverlayState::Adjusting {
        rect: Rect::normalize(start, end),
        edit: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reversed_drag() {
        let r = Rect::normalize((80, 60), (20, 10));
        assert_eq!(r, Rect { x: 20, y: 10, w: 60, h: 50 });
    }

    #[test]
    fn normalize_zero_drag() {
        let r = Rect::normalize((50, 50), (50, 50));
        assert_eq!(r, Rect { x: 50, y: 50, w: 0, h: 0 });
    }

    #[test]
    fn contains_inside_and_edge() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        assert!(r.contains((10, 10)));     // top-left edge (inclusive)
        assert!(r.contains((29, 29)));     // bottom-right (exclusive on x+w/y+h)
        assert!(!r.contains((30, 30)));
        assert!(!r.contains((9, 10)));
    }

    #[test]
    fn clamp_shrinks_rect() {
        let r = Rect { x: -5, y: -5, w: 100, h: 100 }.clamp_to((50, 50));
        assert_eq!(r, Rect { x: 0, y: 0, w: 50, h: 50 });
    }

    #[test]
    fn translate_offsets() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 }.translate(3, -4);
        assert_eq!(r, Rect { x: 13, y: 6, w: 5, h: 5 });
    }

    #[test]
    fn resize_from_br_grows_both() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 }
            .resize_from_anchor(Anchor::BR, 5, 7);
        assert_eq!(r, Rect { x: 10, y: 10, w: 25, h: 27 });
    }

    #[test]
    fn resize_from_tl_grows_up_left() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 }
            .resize_from_anchor(Anchor::TL, -3, -4);
        assert_eq!(r, Rect { x: 7, y: 6, w: 23, h: 24 });
    }

    #[test]
    fn resize_collapses_to_minimum_1px() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 }
            .resize_from_anchor(Anchor::BR, -100, -100);
        assert_eq!(r.w, 1);
        assert_eq!(r.h, 1);
    }

    #[test]
    fn resize_each_edge_only_moves_that_edge() {
        let base = Rect { x: 10, y: 10, w: 20, h: 20 };
        assert_eq!(
            base.resize_from_anchor(Anchor::T, 0, -5),
            Rect { x: 10, y: 5, w: 20, h: 25 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::B, 0, 5),
            Rect { x: 10, y: 10, w: 20, h: 25 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::L, -5, 0),
            Rect { x: 5, y: 10, w: 25, h: 20 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::R, 5, 0),
            Rect { x: 10, y: 10, w: 25, h: 20 }
        );
    }

    #[test]
    fn mouse_down_idle_starts_dragging() {
        let s = on_mouse_down_idle((42, 17));
        assert!(matches!(
            s,
            OverlayState::Dragging { start: (42, 17), end: (42, 17) }
        ));
    }

    #[test]
    fn mouse_move_dragging_updates_end() {
        let s = on_mouse_move_dragging((10, 10), (25, 30));
        assert!(matches!(
            s,
            OverlayState::Dragging { start: (10, 10), end: (25, 30) }
        ));
    }

    #[test]
    fn mouse_up_dragging_enters_adjusting() {
        let s = on_mouse_up_dragging((80, 60), (20, 10));
        match s {
            OverlayState::Adjusting { rect, edit } => {
                assert_eq!(rect, Rect { x: 20, y: 10, w: 60, h: 50 });
                assert!(edit.is_none());
            }
            _ => panic!("expected Adjusting, got {s:?}"),
        }
    }
}
```

- [ ] **Step 2: Run tests to confirm they pass**

Run:
```bash
cargo test --lib overlay::state::tests
```

Expected: `test result: ok. 12 passed; 0 failed`.

- [ ] **Step 3: Rewrite `src/overlay/mod.rs` to route events through state**

Overwrite `src/overlay/mod.rs`:
```rust
pub mod hit;
pub mod render;
pub mod state;

use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::MonitorGeom;
use state::{OverlayState, Rect, Transition};

/// What `Overlay::handle_event` reports back to the caller after processing
/// one winit event. Keeps `app.rs` from needing to inspect `OverlayState`.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Continue,
    Confirmed(Rect),
    Cancelled,
}

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub state: OverlayState,
    pub cursor: (i32, i32),
}

impl Overlay {
    pub fn create(
        event_loop: &ActiveEventLoop,
        frame: RgbaImage,
        monitor_geom: &MonitorGeom,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        let window = {
            let size = winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
                monitor_geom.width as f64,
                monitor_geom.height as f64,
            ));
            let position = winit::dpi::Position::Logical(winit::dpi::LogicalPosition::new(
                monitor_geom.x as f64,
                monitor_geom.y as f64,
            ));
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_inner_size(size)
                .with_position(position);
            let win = event_loop.create_window(attrs).context("create window")?;
            set_macos_window_level(&win, 1000);
            win
        };

        #[cfg(not(target_os = "macos"))]
        let window = {
            let target_monitor = event_loop.available_monitors().find(|m| {
                let pos = m.position();
                pos.x == monitor_geom.x && pos.y == monitor_geom.y
            });
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_fullscreen(Some(winit::window::Fullscreen::Borderless(target_monitor)));
            event_loop.create_window(attrs).context("create window")?
        };

        let window = Rc::new(window);
        let context = SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface =
            Surface::new(&context, window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            state: OverlayState::Idle,
            cursor: (0, 0),
        })
    }

    /// Translate one winit WindowEvent into a state transition and return
    /// what app.rs should do next.
    pub fn handle_event(&mut self, event: WindowEvent) -> Outcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                if let OverlayState::Dragging { start, .. } = self.state {
                    self.state = state::on_mouse_move_dragging(start, self.cursor);
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if matches!(self.state, OverlayState::Idle) {
                    self.state = state::on_mouse_down_idle(self.cursor);
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // Task 3 keeps Iter-1 behavior: MouseUp confirms immediately
                // if there's an actual drag. Task 4 will flip this to enter
                // Adjusting instead.
                if let OverlayState::Dragging { start, end } = self.state {
                    let rect = Rect::normalize(start, end);
                    self.state = OverlayState::Idle;
                    if rect.w > 0 && rect.h > 0 {
                        return Outcome::Confirmed(rect);
                    }
                    return Outcome::Cancelled;
                }
                Outcome::Continue
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
                Outcome::Continue
            }
            WindowEvent::CloseRequested => Outcome::Cancelled,
            _ => Outcome::Continue,
        }
    }

    fn current_selection_rect_window(&self) -> Option<(u32, u32, u32, u32)> {
        let r = match self.state {
            OverlayState::Idle => return None,
            OverlayState::Dragging { start, end } => Rect::normalize(start, end),
            OverlayState::Adjusting { rect, .. } => rect,
        };
        let size = self.window.inner_size();
        let r = r.clamp_to((size.width, size.height));
        if r.w == 0 || r.h == 0 {
            None
        } else {
            Some(r.as_tuple_u32())
        }
    }

    /// Translate a window-space rect into a frame-space rect.
    pub fn window_rect_to_frame_rect(&self, rect: Rect) -> (u32, u32, u32, u32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1), size.height.max(1));
        let (fw, fh) = self.frame.dimensions();
        let clamped = rect.clamp_to((ww, wh));
        let (x, y, w, h) = clamped.as_tuple_u32();
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
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel = self.current_selection_rect_window();

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, &self.frame);
        render::apply_dim(&mut buf, w, h, sel);
        if let Some(r) = sel {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }

    #[allow(dead_code)]
    fn mark_transition(&mut self, t: Transition) -> Outcome {
        // Helper so Task 4+ can route Enter/ESC/double-click results.
        match t {
            Transition::Stay => Outcome::Continue,
            Transition::Confirm(r) => Outcome::Confirmed(r),
            Transition::Cancel => Outcome::Cancelled,
        }
    }
}

#[cfg(target_os = "macos")]
fn set_macos_window_level(window: &Window, level: i64) {
    use winit::raw_window_handle::HasWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let raw = handle.as_raw();
    let winit::raw_window_handle::RawWindowHandle::AppKit(appkit) = raw else {
        return;
    };
    extern "C" {
        fn objc_msgSend(
            obj: *mut std::ffi::c_void,
            sel: *mut std::ffi::c_void,
            ...
        ) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const u8) -> *mut std::ffi::c_void;
    }
    unsafe {
        let ns_view = appkit.ns_view.as_ptr();
        let sel_window = sel_registerName(b"window\0".as_ptr());
        let ns_window = objc_msgSend(ns_view, sel_window);
        if ns_window.is_null() {
            return;
        }
        let sel_set_level = sel_registerName(b"setLevel:\0".as_ptr());
        objc_msgSend(ns_window, sel_set_level, level);
    }
}
```

- [ ] **Step 4: Rewrite `src/app.rs` to consume `Outcome`**

Overwrite `src/app.rs`:
```rust
use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::crop;
use crate::overlay::{state::Rect, Outcome, Overlay};

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
        let (frame, geom) = capture::capture_at_cursor()?;
        let overlay = Overlay::create(event_loop, frame, &geom)?;
        overlay.window.request_redraw();
        self.overlay = Some(overlay);
        Ok(())
    }

    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        if let Err(e) = clipboard::put_image(&cropped) {
            eprintln!("clipboard error: {e:?}");
        } else {
            println!("copied {}x{} to clipboard", cropped.width(), cropped.height());
        }
        drop(overlay);
    }

    fn cancel(&mut self) {
        self.overlay = None;
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

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
        match overlay.handle_event(event) {
            Outcome::Continue => {}
            Outcome::Confirmed(rect) => self.confirm(rect),
            Outcome::Cancelled => self.cancel(),
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

- [ ] **Step 5: Build and verify tests pass**

Run:
```bash
cargo test --lib
cargo check
```

Expected: all tests pass (12 in `overlay::state::tests` + 2 in `text::tests` + existing crop tests); clean `cargo check`.

- [ ] **Step 6: Manual smoke test — Iter 1 UX must still work identically**

Run:
```bash
cargo run --release
```

Verify: hotkey opens overlay, drag selects, release copies. Same as Iter 1. No visual changes yet.

- [ ] **Step 7: Commit**

```bash
git add src/overlay/state.rs src/overlay/mod.rs src/app.rs
git commit -m "refactor(overlay): route events through pure state machine"
```

---

## Task 4: Switch confirmation to Enter + double-click; add ESC cancel

This is the first user-visible behavior change. MouseUp now enters `Adjusting` (the selection persists until confirmed); Enter and double-click inside confirm; ESC cancels from any state. No anchors yet — Task 5 adds those — so in Adjusting the user can only confirm-or-cancel; they cannot yet resize. That's expected for this intermediate commit.

**Files:**
- Modify: `src/overlay/mod.rs`
- Modify: `src/overlay/state.rs` (add confirm/cancel/double-click helpers + tests)

- [ ] **Step 1: Add confirm/cancel transition helpers + tests to `state.rs`**

Append to `src/overlay/state.rs` (before the `#[cfg(test)] mod tests` block):
```rust
/// Enter key: confirm if we have a usable rect, otherwise stay.
pub fn on_enter(current: OverlayState) -> Transition {
    match current {
        OverlayState::Idle => Transition::Stay,
        OverlayState::Dragging { start, end } => {
            let r = Rect::normalize(start, end);
            if r.w > 0 && r.h > 0 { Transition::Confirm(r) } else { Transition::Stay }
        }
        OverlayState::Adjusting { rect, .. } => {
            if rect.w > 0 && rect.h > 0 { Transition::Confirm(rect) } else { Transition::Stay }
        }
    }
}

/// ESC: cancel from any state.
pub fn on_escape(_current: OverlayState) -> Transition {
    Transition::Cancel
}

/// Double-click: confirm only if the click falls inside the current rect.
/// Called only from Adjusting state (the event source filters).
pub fn on_double_click_adjusting(rect: Rect, cursor: (i32, i32)) -> Transition {
    if rect.contains(cursor) && rect.w > 0 && rect.h > 0 {
        Transition::Confirm(rect)
    } else {
        Transition::Stay
    }
}
```

Add these tests inside the existing `#[cfg(test)] mod tests` block:
```rust
    #[test]
    fn enter_in_idle_stays() {
        assert!(matches!(on_enter(OverlayState::Idle), Transition::Stay));
    }

    #[test]
    fn enter_in_adjusting_confirms() {
        let s = OverlayState::Adjusting {
            rect: Rect { x: 1, y: 1, w: 10, h: 10 },
            edit: None,
        };
        match on_enter(s) {
            Transition::Confirm(r) => assert_eq!(r, Rect { x: 1, y: 1, w: 10, h: 10 }),
            t => panic!("expected Confirm, got {t:?}"),
        }
    }

    #[test]
    fn enter_in_dragging_confirms_normalized() {
        let s = OverlayState::Dragging { start: (5, 5), end: (25, 15) };
        match on_enter(s) {
            Transition::Confirm(r) => assert_eq!(r, Rect { x: 5, y: 5, w: 20, h: 10 }),
            t => panic!("expected Confirm, got {t:?}"),
        }
    }

    #[test]
    fn escape_always_cancels() {
        assert!(matches!(on_escape(OverlayState::Idle), Transition::Cancel));
        assert!(matches!(
            on_escape(OverlayState::Dragging { start: (0, 0), end: (10, 10) }),
            Transition::Cancel
        ));
    }

    #[test]
    fn double_click_inside_confirms() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        match on_double_click_adjusting(r, (15, 15)) {
            Transition::Confirm(got) => assert_eq!(got, r),
            t => panic!("expected Confirm, got {t:?}"),
        }
    }

    #[test]
    fn double_click_outside_stays() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        assert!(matches!(
            on_double_click_adjusting(r, (40, 40)),
            Transition::Stay
        ));
    }
```

- [ ] **Step 2: Run the new state tests**

Run:
```bash
cargo test --lib overlay::state::tests
```

Expected: `test result: ok. 18 passed; 0 failed` (12 original + 6 new).

- [ ] **Step 3: Flip the overlay event handler to use Adjusting**

In `src/overlay/mod.rs`, replace the existing `MouseInput Released` branch inside `handle_event` with the body below, and add the new `KeyboardInput` + double-click handling. Also update the imports.

Update the `use winit::event::...` line at the top of `mod.rs` to:
```rust
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey};
```

Add a new field to `Overlay` for tracking double-click timing. At the top of the struct declaration:
```rust
pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub state: OverlayState,
    pub cursor: (i32, i32),
    last_click: Option<std::time::Instant>,
}
```

Update `create` to initialize `last_click: None`:
```rust
        Ok(Self {
            window,
            surface,
            frame,
            state: OverlayState::Idle,
            cursor: (0, 0),
            last_click: None,
        })
```

Replace the body of `handle_event` with:
```rust
    pub fn handle_event(&mut self, event: WindowEvent) -> Outcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                if let OverlayState::Dragging { start, .. } = self.state {
                    self.state = state::on_mouse_move_dragging(start, self.cursor);
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.handle_left_press(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.handle_left_release(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => self.handle_key(logical_key),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
                Outcome::Continue
            }
            WindowEvent::CloseRequested => Outcome::Cancelled,
            _ => Outcome::Continue,
        }
    }

    fn handle_left_press(&mut self) -> Outcome {
        // Detect double-click: two presses within 400 ms, same rough position.
        let now = std::time::Instant::now();
        let is_double_click = matches!(
            self.last_click,
            Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
        );
        self.last_click = Some(now);

        match self.state {
            OverlayState::Idle => {
                self.state = state::on_mouse_down_idle(self.cursor);
                self.window.request_redraw();
                Outcome::Continue
            }
            OverlayState::Dragging { .. } => Outcome::Continue,
            OverlayState::Adjusting { rect, .. } => {
                if is_double_click {
                    match state::on_double_click_adjusting(rect, self.cursor) {
                        Transition::Confirm(r) => return Outcome::Confirmed(r),
                        Transition::Cancel => return Outcome::Cancelled,
                        Transition::Stay => {}
                    }
                }
                // Task 5 will add: if cursor hits an anchor, start Resize;
                //                  else if inside, start Translate;
                //                  else (outside) return to Idle.
                // For Task 4 we simply stay put — user must use Enter/ESC.
                Outcome::Continue
            }
        }
    }

    fn handle_left_release(&mut self) -> Outcome {
        if let OverlayState::Dragging { start, end } = self.state {
            let rect = Rect::normalize(start, end);
            if rect.w > 0 && rect.h > 0 {
                self.state = OverlayState::Adjusting { rect, edit: None };
            } else {
                // zero-area drag → reset to Idle
                self.state = OverlayState::Idle;
            }
            self.window.request_redraw();
        }
        Outcome::Continue
    }

    fn handle_key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Named(NamedKey::Escape) => match state::on_escape(self.state) {
                Transition::Cancel => Outcome::Cancelled,
                _ => Outcome::Continue,
            },
            Key::Named(NamedKey::Enter) => match state::on_enter(self.state) {
                Transition::Confirm(r) => Outcome::Confirmed(r),
                _ => Outcome::Continue,
            },
            _ => Outcome::Continue,
        }
    }
```

Verify the `state::` imports at the top of `mod.rs` are (no change needed if Task 3 already landed this — just double-check):
```rust
use state::{OverlayState, Rect, Transition};
```

Remove the now-unused helper `mark_transition` (the `#[allow(dead_code)] fn mark_transition` block from Task 3).

- [ ] **Step 4: Build and run tests**

Run:
```bash
cargo build --release
cargo test --lib
```

Expected: clean build, all tests pass (crop + text + overlay::state).

- [ ] **Step 5: Manual UX verification**

Run:
```bash
./target/release/quickshot
```

Test script:
1. Hotkey → overlay appears.
2. Drag a region → white outline follows cursor.
3. Release mouse → **overlay stays open**; selection stays visible with white outline.
4. Press **Enter** → overlay closes; paste into Preview/TextEdit → exact region appears.
5. Hotkey again → drag → release → press **ESC** → overlay closes; clipboard is unchanged (last image paste-check).
6. Hotkey → drag → release → **double-click inside** the selection → overlay closes with copy.
7. Hotkey → drag → release → **double-click outside** the selection → overlay remains open (double-click outside is a no-op in Task 4; Task 5 will make it clear-and-restart).
8. Hotkey → drag a zero-area (just click without moving) → release → overlay stays open with no selection (state returned to Idle); ESC closes.

All pass = Task 4 good.

- [ ] **Step 6: Commit**

```bash
git add src/overlay/mod.rs src/overlay/state.rs
git commit -m "feat(overlay): Enter/double-click confirm, ESC cancel, Adjusting state"
```

---

## Task 5: Anchors, hit-testing, cursor icons, resize + translate edits

Implement the full Adjusting-state interaction model: 8 anchors with cursor feedback, click-and-drag to resize from anchors, click-and-drag inside to translate, click outside to clear and return to Idle.

**Files:**
- Modify: `src/overlay/hit.rs` (full implementation + tests)
- Modify: `src/overlay/state.rs` (add Edit transition helpers)
- Modify: `src/overlay/render.rs` (add `draw_anchors`)
- Modify: `src/overlay/mod.rs` (wire edits + cursor icon updates)

- [ ] **Step 1: Write `src/overlay/hit.rs`**

Overwrite `src/overlay/hit.rs`:
```rust
//! Classify a cursor position relative to an Adjusting-state selection rect.
//! Pure — no winit Window refs, no state. Unit-tested.

use winit::window::CursorIcon;

use super::state::{Anchor, Rect};

pub const ANCHOR_SIZE: i32 = 6;
/// Extra padding around each visible anchor that still counts as a hit.
/// Visible anchor is 6×6, hit box is 12×12 centered on the anchor point.
pub const ANCHOR_HIT_PAD: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitZone {
    Anchor(Anchor),
    Inside,
    Outside,
}

fn anchor_center(rect: Rect, anchor: Anchor) -> (i32, i32) {
    let (l, t) = (rect.x, rect.y);
    let (r, b) = (rect.x + rect.w - 1, rect.y + rect.h - 1);
    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    match anchor {
        Anchor::TL => (l, t),
        Anchor::T  => (cx, t),
        Anchor::TR => (r, t),
        Anchor::R  => (r, cy),
        Anchor::BR => (r, b),
        Anchor::B  => (cx, b),
        Anchor::BL => (l, b),
        Anchor::L  => (l, cy),
    }
}

const ALL_ANCHORS: [Anchor; 8] = [
    Anchor::TL, Anchor::T, Anchor::TR, Anchor::R,
    Anchor::BR, Anchor::B, Anchor::BL, Anchor::L,
];

pub fn classify(cursor: (i32, i32), rect: Rect) -> HitZone {
    let half = ANCHOR_SIZE / 2 + ANCHOR_HIT_PAD;
    for a in ALL_ANCHORS {
        let (ax, ay) = anchor_center(rect, a);
        if (cursor.0 - ax).abs() <= half && (cursor.1 - ay).abs() <= half {
            return HitZone::Anchor(a);
        }
    }
    if rect.contains(cursor) {
        HitZone::Inside
    } else {
        HitZone::Outside
    }
}

pub fn cursor_icon_for(zone: HitZone) -> CursorIcon {
    match zone {
        HitZone::Anchor(Anchor::TL) | HitZone::Anchor(Anchor::BR) => CursorIcon::NwseResize,
        HitZone::Anchor(Anchor::TR) | HitZone::Anchor(Anchor::BL) => CursorIcon::NeswResize,
        HitZone::Anchor(Anchor::T)  | HitZone::Anchor(Anchor::B)  => CursorIcon::NsResize,
        HitZone::Anchor(Anchor::L)  | HitZone::Anchor(Anchor::R)  => CursorIcon::EwResize,
        HitZone::Inside => CursorIcon::Move,
        HitZone::Outside => CursorIcon::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect { Rect { x: 100, y: 100, w: 100, h: 100 } }

    #[test]
    fn anchor_centers_are_rect_corners_and_edge_midpoints() {
        let r = rect();
        assert_eq!(anchor_center(r, Anchor::TL), (100, 100));
        assert_eq!(anchor_center(r, Anchor::TR), (199, 100));
        assert_eq!(anchor_center(r, Anchor::BR), (199, 199));
        assert_eq!(anchor_center(r, Anchor::BL), (100, 199));
        assert_eq!(anchor_center(r, Anchor::T),  (150, 100));
        assert_eq!(anchor_center(r, Anchor::B),  (150, 199));
        assert_eq!(anchor_center(r, Anchor::L),  (100, 150));
        assert_eq!(anchor_center(r, Anchor::R),  (199, 150));
    }

    #[test]
    fn classify_each_anchor_center_hits() {
        for a in ALL_ANCHORS {
            let c = anchor_center(rect(), a);
            assert_eq!(classify(c, rect()), HitZone::Anchor(a), "{a:?} miss");
        }
    }

    #[test]
    fn classify_respects_hit_pad() {
        let half = ANCHOR_SIZE / 2 + ANCHOR_HIT_PAD;
        // Just inside the hit pad on TL anchor:
        assert_eq!(classify((100 + half, 100 + half), rect()), HitZone::Anchor(Anchor::TL));
        // Just outside the hit pad: inside the rect, not on an anchor.
        assert_eq!(classify((100 + half + 1, 100 + half + 1), rect()), HitZone::Inside);
    }

    #[test]
    fn classify_inside_not_on_anchor() {
        assert_eq!(classify((150, 150), rect()), HitZone::Inside);
    }

    #[test]
    fn classify_outside() {
        assert_eq!(classify((10, 10), rect()), HitZone::Outside);
        assert_eq!(classify((300, 300), rect()), HitZone::Outside);
    }

    #[test]
    fn cursor_icons() {
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::TL)), CursorIcon::NwseResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::TR)), CursorIcon::NeswResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::T)),  CursorIcon::NsResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::L)),  CursorIcon::EwResize);
        assert_eq!(cursor_icon_for(HitZone::Inside),  CursorIcon::Move);
        assert_eq!(cursor_icon_for(HitZone::Outside), CursorIcon::Default);
    }
}
```

- [ ] **Step 2: Add Edit transition helpers to `state.rs`**

Append to `src/overlay/state.rs` (before the test module):
```rust
/// In Adjusting state, a MouseDown on an anchor begins a resize edit.
pub fn start_resize(rect: Rect, anchor: Anchor, cursor: (i32, i32)) -> OverlayState {
    OverlayState::Adjusting {
        rect,
        edit: Some(Edit::Resize { anchor, origin: rect, from: cursor }),
    }
}

/// In Adjusting state, a MouseDown inside the rect begins a translate edit.
pub fn start_translate(rect: Rect, cursor: (i32, i32)) -> OverlayState {
    OverlayState::Adjusting {
        rect,
        edit: Some(Edit::Translate { origin: rect, from: cursor }),
    }
}

/// Apply cursor movement to an in-flight edit and return the updated state.
pub fn update_edit(state: OverlayState, cursor: (i32, i32)) -> OverlayState {
    let OverlayState::Adjusting { edit: Some(edit), .. } = state else {
        return state;
    };
    match edit {
        Edit::Resize { anchor, origin, from } => {
            let (dx, dy) = (cursor.0 - from.0, cursor.1 - from.1);
            let new_rect = origin.resize_from_anchor(anchor, dx, dy);
            OverlayState::Adjusting {
                rect: new_rect,
                edit: Some(Edit::Resize { anchor, origin, from }),
            }
        }
        Edit::Translate { origin, from } => {
            let (dx, dy) = (cursor.0 - from.0, cursor.1 - from.1);
            let new_rect = origin.translate(dx, dy);
            OverlayState::Adjusting {
                rect: new_rect,
                edit: Some(Edit::Translate { origin, from }),
            }
        }
    }
}

/// Commit the in-flight edit on MouseUp: clears `edit`, keeps the final rect.
pub fn commit_edit(state: OverlayState) -> OverlayState {
    match state {
        OverlayState::Adjusting { rect, edit: Some(_) } => {
            OverlayState::Adjusting { rect, edit: None }
        }
        other => other,
    }
}
```

Add these tests inside the `#[cfg(test)] mod tests` block of `state.rs`:
```rust
    #[test]
    fn start_resize_stores_edit() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        let s = start_resize(r, Anchor::BR, (29, 29));
        match s {
            OverlayState::Adjusting { rect, edit: Some(Edit::Resize { anchor, origin, from }) } => {
                assert_eq!(rect, r);
                assert_eq!(anchor, Anchor::BR);
                assert_eq!(origin, r);
                assert_eq!(from, (29, 29));
            }
            _ => panic!("expected Adjusting with Resize edit"),
        }
    }

    #[test]
    fn update_resize_grows_br() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        let s = start_resize(r, Anchor::BR, (29, 29));
        let s2 = update_edit(s, (35, 37));
        match s2 {
            OverlayState::Adjusting { rect, .. } => {
                assert_eq!(rect, Rect { x: 10, y: 10, w: 26, h: 28 });
            }
            _ => panic!("expected Adjusting"),
        }
    }

    #[test]
    fn update_translate_moves_whole_rect() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        let s = start_translate(r, (15, 15));
        let s2 = update_edit(s, (18, 12));
        match s2 {
            OverlayState::Adjusting { rect, .. } => {
                assert_eq!(rect, Rect { x: 13, y: 7, w: 20, h: 20 });
            }
            _ => panic!("expected Adjusting"),
        }
    }

    #[test]
    fn commit_clears_edit_but_keeps_rect() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        let s = start_resize(r, Anchor::BR, (29, 29));
        let s2 = update_edit(s, (35, 37));
        let s3 = commit_edit(s2);
        match s3 {
            OverlayState::Adjusting { rect, edit: None } => {
                assert_eq!(rect, Rect { x: 10, y: 10, w: 26, h: 28 });
            }
            _ => panic!("expected Adjusting with edit=None"),
        }
    }
```

- [ ] **Step 3: Run the new tests**

Run:
```bash
cargo test --lib
```

Expected: all pass (18 state + 6 hit + 2 text + 5 crop = 31+).

- [ ] **Step 4: Add `draw_anchors` to `render.rs`**

Append to `src/overlay/render.rs`:
```rust
use crate::overlay::hit::ANCHOR_SIZE;
use crate::overlay::state::{Anchor, Rect};

pub fn draw_anchors(buf: &mut [u32], w: u32, h: u32, rect: Rect) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    const WHITE: u32 = 0x00FFFFFF;
    const BLACK: u32 = 0x00000000;
    let anchors = [
        Anchor::TL, Anchor::T, Anchor::TR, Anchor::R,
        Anchor::BR, Anchor::B, Anchor::BL, Anchor::L,
    ];
    let half = ANCHOR_SIZE / 2;
    let (l, t) = (rect.x, rect.y);
    let (r, b) = (rect.x + rect.w - 1, rect.y + rect.h - 1);
    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    for a in anchors {
        let (ax, ay) = match a {
            Anchor::TL => (l, t),
            Anchor::T  => (cx, t),
            Anchor::TR => (r, t),
            Anchor::R  => (r, cy),
            Anchor::BR => (r, b),
            Anchor::B  => (cx, b),
            Anchor::BL => (l, b),
            Anchor::L  => (l, cy),
        };
        // Outer 8×8 black border, inner 6×6 white fill.
        fill_square(buf, w, h, ax - half - 1, ay - half - 1, ANCHOR_SIZE + 2, BLACK);
        fill_square(buf, w, h, ax - half,     ay - half,     ANCHOR_SIZE,     WHITE);
    }
}

fn fill_square(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, size: i32, color: u32) {
    for dy in 0..size {
        for dx in 0..size {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
            buf[(py as u32 * w + px as u32) as usize] = color;
        }
    }
}
```

- [ ] **Step 5: Wire anchors + cursor icons + edits into `overlay/mod.rs`**

Make three edits to `src/overlay/mod.rs`:

**5a.** Update `handle_left_press` to delegate to `hit::classify` for Adjusting:
```rust
    fn handle_left_press(&mut self) -> Outcome {
        let now = std::time::Instant::now();
        let is_double_click = matches!(
            self.last_click,
            Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
        );
        self.last_click = Some(now);

        match self.state {
            OverlayState::Idle => {
                self.state = state::on_mouse_down_idle(self.cursor);
                self.window.request_redraw();
                Outcome::Continue
            }
            OverlayState::Dragging { .. } => Outcome::Continue,
            OverlayState::Adjusting { rect, .. } => {
                if is_double_click {
                    match state::on_double_click_adjusting(rect, self.cursor) {
                        Transition::Confirm(r) => return Outcome::Confirmed(r),
                        Transition::Cancel => return Outcome::Cancelled,
                        Transition::Stay => {}
                    }
                }
                match hit::classify(self.cursor, rect) {
                    hit::HitZone::Anchor(a) => {
                        self.state = state::start_resize(rect, a, self.cursor);
                        self.window.request_redraw();
                    }
                    hit::HitZone::Inside => {
                        self.state = state::start_translate(rect, self.cursor);
                        self.window.request_redraw();
                    }
                    hit::HitZone::Outside => {
                        self.state = OverlayState::Idle;
                        self.window.request_redraw();
                    }
                }
                Outcome::Continue
            }
        }
    }
```

**5b.** Update `handle_left_release` to commit any in-flight edit:
```rust
    fn handle_left_release(&mut self) -> Outcome {
        match self.state {
            OverlayState::Dragging { start, end } => {
                let rect = Rect::normalize(start, end);
                if rect.w > 0 && rect.h > 0 {
                    self.state = OverlayState::Adjusting { rect, edit: None };
                } else {
                    self.state = OverlayState::Idle;
                }
                self.window.request_redraw();
            }
            OverlayState::Adjusting { edit: Some(_), .. } => {
                self.state = state::commit_edit(self.state);
                self.window.request_redraw();
            }
            _ => {}
        }
        Outcome::Continue
    }
```

**5c.** Update the `CursorMoved` branch in `handle_event` to drive edits and cursor icons:
```rust
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                match self.state {
                    OverlayState::Dragging { start, .. } => {
                        self.state = state::on_mouse_move_dragging(start, self.cursor);
                        self.window.request_redraw();
                    }
                    OverlayState::Adjusting { edit: Some(_), .. } => {
                        self.state = state::update_edit(self.state, self.cursor);
                        self.window.request_redraw();
                    }
                    OverlayState::Adjusting { rect, edit: None } => {
                        let icon = hit::cursor_icon_for(hit::classify(self.cursor, rect));
                        self.window.set_cursor(icon);
                    }
                    OverlayState::Idle => {}
                }
                Outcome::Continue
            }
```

**5d.** Update `redraw` to paint anchors when in Adjusting state. Replace the body of `redraw` with:
```rust
    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel_tuple = self.current_selection_rect_window();
        let sel_rect = self.current_selection_rect();

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, &self.frame);
        render::apply_dim(&mut buf, w, h, sel_tuple);
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }
        if matches!(self.state, OverlayState::Adjusting { .. }) {
            if let Some(r) = sel_rect {
                render::draw_anchors(&mut buf, w, h, r);
            }
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
```

Add the helper `current_selection_rect` (returns an `Option<Rect>` — same rect as `current_selection_rect_window` but in the state's native coordinate space, without the u32 conversion):
```rust
    fn current_selection_rect(&self) -> Option<Rect> {
        let r = match self.state {
            OverlayState::Idle => return None,
            OverlayState::Dragging { start, end } => Rect::normalize(start, end),
            OverlayState::Adjusting { rect, .. } => rect,
        };
        let size = self.window.inner_size();
        let r = r.clamp_to((size.width, size.height));
        if r.w == 0 || r.h == 0 { None } else { Some(r) }
    }
```

Add `use hit;` (already implicit via `pub mod hit;` at the top — just reference it as `hit::classify` / `hit::HitZone` / `hit::cursor_icon_for` directly; no extra `use` needed).

- [ ] **Step 6: Build + run tests**

Run:
```bash
cargo build --release
cargo test --lib
```

Expected: clean build; all tests pass.

- [ ] **Step 7: Manual UX verification (anchors + edits)**

Run:
```bash
./target/release/quickshot
```

Test script:
1. Hotkey → drag a ~300×200 region → release → 8 anchors appear (white squares with black borders on all four corners + edge midpoints).
2. Hover each anchor → cursor icon changes to the correct resize arrow (diagonal on corners, axial on edges).
3. Drag TR anchor up and to the right → top edge moves up, right edge moves right; bottom and left stay put.
4. Drag L anchor to the right → left edge shrinks; right edge fixed.
5. Drag inside the selection → whole rect moves; release → anchors follow new position.
6. Click outside the selection → selection clears (back to empty dimmed overlay in Idle); drag again to restart.
7. After edits, press Enter → paste → final edited region is what lands in clipboard.
8. ESC from Adjusting + active edit → overlay closes without copy.
9. Very tiny selection → anchors may overlap but still function; resize to a reasonable size and confirm.

All pass = Task 5 good.

- [ ] **Step 8: Commit**

```bash
git add src/overlay/hit.rs src/overlay/state.rs src/overlay/render.rs src/overlay/mod.rs
git commit -m "feat(overlay): 8 anchors + hit-testing + resize/translate edits"
```

---

## Task 6: Size label (live `W × H` display)

Add a live size label that shows physical-pixel width × height of the eventual crop. Visible in `Dragging` and `Adjusting` states.

**Files:**
- Modify: `src/overlay/render.rs` (add `draw_size_label` + `size_label_position`)
- Modify: `src/overlay/mod.rs` (instantiate `Font`, pass to render, call `draw_size_label`)

- [ ] **Step 1: Add `size_label_position` + `draw_size_label` to `render.rs`**

Append to `src/overlay/render.rs`:
```rust
use crate::text::Font;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPlacement {
    AboveOutside,
    InsideTopLeft,
}

/// Decide where to place the size label given the selection rect and the
/// label's own pixel dimensions.
pub fn size_label_position(
    rect: Rect,
    label_size: (i32, i32),
    gap: i32,
) -> (i32, i32, LabelPlacement) {
    let (_lw, lh) = label_size;
    let above_y = rect.y - gap - lh;
    if above_y >= 0 {
        (rect.x, above_y, LabelPlacement::AboveOutside)
    } else {
        (rect.x + gap, rect.y + gap, LabelPlacement::InsideTopLeft)
    }
}

const LABEL_FONT_PX: f32 = 12.0;
const LABEL_PAD_X: i32 = 4;
const LABEL_PAD_Y: i32 = 2;
const LABEL_GAP: i32 = 4;
const LABEL_CORNER_RADIUS: i32 = 4;

/// Draw the `W × H` label pinned to the selection's top-left. `frame_size`
/// is the captured frame's pixel dimensions; `window_size` is the overlay
/// window's pixel dimensions. The label shows physical-pixel dims of the
/// cropped region so the number matches the eventual PNG.
pub fn draw_size_label(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: Rect,
    frame_size: (u32, u32),
    window_size: (u32, u32),
    font: &mut Font,
) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let (fw, fh) = frame_size;
    let (ww, wh) = (window_size.0.max(1), window_size.1.max(1));
    // Window-space rect -> frame-space dims (same math as Overlay::window_rect_to_frame_rect)
    let phys_w = (rect.w as u64 * fw as u64 / ww as u64) as u32;
    let phys_h = (rect.h as u64 * fh as u64 / wh as u64) as u32;
    let text = format!("{} \u{00D7} {}", phys_w.max(1), phys_h.max(1));

    let text_w = estimate_text_width(&text, LABEL_FONT_PX);
    let text_h = LABEL_FONT_PX as i32; // approximate cap height
    let box_w = text_w + LABEL_PAD_X * 2;
    let box_h = text_h + LABEL_PAD_Y * 2;

    let (bx, by, _placement) = size_label_position(rect, (box_w, box_h), LABEL_GAP);
    draw_rounded_rect_alpha(buf, w, h, bx, by, box_w, box_h, LABEL_CORNER_RADIUS, 0x000000, 0.7);
    font.render_text(
        buf,
        w,
        h,
        bx + LABEL_PAD_X,
        by + LABEL_PAD_Y,
        &text,
        LABEL_FONT_PX,
        0x00FFFFFF,
    );
}

/// Rough width estimate for monospace: each glyph is ~`0.6 * px_size` wide.
/// Used only for laying out the background pill; the text renderer handles
/// real advance widths.
fn estimate_text_width(text: &str, px_size: f32) -> i32 {
    (text.chars().count() as f32 * px_size * 0.6).ceil() as i32
}

/// Filled rect with alpha-blended solid color and squared corners masked into
/// a 4-px rounded pill.
fn draw_rounded_rect_alpha(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    radius: i32,
    color_rgb: u32,
    alpha: f32,
) {
    let (fr, fg, fb) = (
        ((color_rgb >> 16) & 0xFF) as f32,
        ((color_rgb >> 8) & 0xFF) as f32,
        (color_rgb & 0xFF) as f32,
    );
    for dy in 0..rh {
        for dx in 0..rw {
            // Corner mask: distance from nearest corner center.
            let in_corner_tl = dx < radius && dy < radius;
            let in_corner_tr = dx >= rw - radius && dy < radius;
            let in_corner_bl = dx < radius && dy >= rh - radius;
            let in_corner_br = dx >= rw - radius && dy >= rh - radius;
            if in_corner_tl || in_corner_tr || in_corner_bl || in_corner_br {
                let (cx, cy) = (
                    if dx < radius { radius } else { rw - radius - 1 },
                    if dy < radius { radius } else { rh - radius - 1 },
                );
                let d2 = (dx - cx).pow(2) + (dy - cy).pow(2);
                if d2 > radius * radius {
                    continue;
                }
            }
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
            let idx = (py as u32 * w + px as u32) as usize;
            let bg = buf[idx];
            let br = ((bg >> 16) & 0xFF) as f32;
            let bgc = ((bg >> 8) & 0xFF) as f32;
            let bb = (bg & 0xFF) as f32;
            let r = (fr * alpha + br * (1.0 - alpha)) as u32;
            let g = (fg * alpha + bgc * (1.0 - alpha)) as u32;
            let b = (fb * alpha + bb * (1.0 - alpha)) as u32;
            buf[idx] = (r << 16) | (g << 8) | b;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_positions_above_when_space_available() {
        let r = Rect { x: 100, y: 100, w: 50, h: 50 };
        let (lx, ly, placement) = size_label_position(r, (40, 16), 4);
        assert_eq!(placement, LabelPlacement::AboveOutside);
        assert_eq!(lx, 100);
        assert_eq!(ly, 100 - 4 - 16);
    }

    #[test]
    fn label_flips_inside_when_no_room_above() {
        let r = Rect { x: 100, y: 5, w: 50, h: 50 };
        let (lx, ly, placement) = size_label_position(r, (40, 16), 4);
        assert_eq!(placement, LabelPlacement::InsideTopLeft);
        assert_eq!(lx, 100 + 4);
        assert_eq!(ly, 5 + 4);
    }
}
```

- [ ] **Step 2: Instantiate `Font` in `Overlay` and thread it through `redraw`**

In `src/overlay/mod.rs`:

Add the font field to `Overlay`:
```rust
pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub state: OverlayState,
    pub cursor: (i32, i32),
    last_click: Option<std::time::Instant>,
    font: crate::text::Font,
}
```

Initialize `font: crate::text::Font::embedded()` in `create`:
```rust
        Ok(Self {
            window,
            surface,
            frame,
            state: OverlayState::Idle,
            cursor: (0, 0),
            last_click: None,
            font: crate::text::Font::embedded(),
        })
```

Update `redraw` to call `draw_size_label` when appropriate:
```rust
    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel_tuple = self.current_selection_rect_window();
        let sel_rect = self.current_selection_rect();
        let show_label = matches!(
            self.state,
            OverlayState::Dragging { .. } | OverlayState::Adjusting { .. }
        );
        let frame_size = self.frame.dimensions();
        let window_size = (w, h);

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, &self.frame);
        render::apply_dim(&mut buf, w, h, sel_tuple);
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }
        if matches!(self.state, OverlayState::Adjusting { .. }) {
            if let Some(r) = sel_rect {
                render::draw_anchors(&mut buf, w, h, r);
            }
        }
        if show_label {
            if let Some(r) = sel_rect {
                render::draw_size_label(&mut buf, w, h, r, frame_size, window_size, &mut self.font);
            }
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
```

- [ ] **Step 3: Build and run tests**

Run:
```bash
cargo test --lib
cargo build --release
```

Expected: clean; all tests pass (now includes 2 render tests).

- [ ] **Step 4: Manual verification**

Run:
```bash
./target/release/quickshot
```

Test script:
1. Hotkey → drag a region → size label with `W × H` appears above the top-left corner, updates live as you drag.
2. Drag a rect right up to the top of the screen (within 20 px) → label flips to inside-top-left.
3. Release → label persists during Adjusting.
4. Resize via anchor → label updates.
5. Translate via inside-drag → label follows the rect, same dimensions.
6. Enter → confirm → paste → pasted image dimensions equal the label's reported size.
7. On a Retina macOS display, the label's values should be roughly 2× the window-space rect dimensions (physical vs logical pixels) — that's expected; the label reports PNG pixels.

All pass = Task 6 good.

- [ ] **Step 5: Commit**

```bash
git add src/overlay/render.rs src/overlay/mod.rs
git commit -m "feat(overlay): live size label with W × H (physical pixels)"
```

---

## Task 7: Magnifier (4× loupe with crosshair + color/coord readout)

Add the Snipaste-style magnifier. Visible in `Idle` and `Dragging` states only. Shows the pixel color under the cursor plus its physical-pixel coordinates.

**Files:**
- Modify: `src/overlay/render.rs` (add `draw_magnifier` + `magnifier_position`)
- Modify: `src/overlay/mod.rs` (call it in `redraw`)

- [ ] **Step 1: Add `magnifier_position` + `draw_magnifier` to `render.rs`**

Append to `src/overlay/render.rs`:
```rust
const MAG_SIZE: i32 = 120;
const MAG_ZOOM: i32 = 4;
const MAG_OFFSET: i32 = 20;
const MAG_LABEL_H: i32 = 18;
const MAG_FONT_PX: f32 = 11.0;

/// Decide where to put the magnifier given cursor + window size.
/// Default: bottom-right of cursor with a gap. Flip to the opposite side
/// on each axis when the default would clip the window edge.
pub fn magnifier_position(
    cursor: (i32, i32),
    window_size: (u32, u32),
    mag_size: i32,
    offset: i32,
) -> (i32, i32) {
    let (ww, wh) = (window_size.0 as i32, window_size.1 as i32);
    let mut x = cursor.0 + offset;
    let mut y = cursor.1 + offset;
    if x + mag_size >= ww {
        x = cursor.0 - offset - mag_size;
    }
    if y + mag_size >= wh {
        y = cursor.1 - offset - mag_size;
    }
    (x.max(0), y.max(0))
}

pub fn draw_magnifier(
    buf: &mut [u32],
    w: u32,
    h: u32,
    frame: &image::RgbaImage,
    cursor: (i32, i32),
    window_size: (u32, u32),
    font: &mut Font,
) {
    let (mx, my) = magnifier_position(cursor, window_size, MAG_SIZE, MAG_OFFSET);
    let (fw, fh) = frame.dimensions();
    let (ww, wh) = (window_size.0.max(1) as u64, window_size.1.max(1) as u64);

    // Map cursor (window-space) to physical pixel coords in the frame.
    let cfx = (cursor.0.max(0) as u64 * fw as u64 / ww) as i32;
    let cfy = (cursor.1.max(0) as u64 * fh as u64 / wh) as i32;
    let src_span = MAG_SIZE / MAG_ZOOM; // 30

    // 1 px white border + black backfill, then upscaled pixels, then crosshair, then label.
    fill_square(buf, w, h, mx - 1, my - 1, MAG_SIZE + 2, 0x00FFFFFF);
    fill_square(buf, w, h, mx,     my,     MAG_SIZE,     0x00000000);

    for dy in 0..(MAG_SIZE - MAG_LABEL_H) {
        for dx in 0..MAG_SIZE {
            let sx = cfx - src_span / 2 + dx / MAG_ZOOM;
            let sy = cfy - src_span / 2 + dy / MAG_ZOOM;
            let sx = sx.clamp(0, fw as i32 - 1);
            let sy = sy.clamp(0, fh as i32 - 1);
            let p = frame.get_pixel(sx as u32, sy as u32);
            let [r, g, b, _a] = p.0;
            let px = mx + dx;
            let py = my + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
            buf[(py as u32 * w + px as u32) as usize] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }

    // Center crosshair (1 px horizontal + 1 px vertical lines across the zoom area).
    let cx = mx + MAG_SIZE / 2;
    let cy = my + (MAG_SIZE - MAG_LABEL_H) / 2;
    for dx in 0..MAG_SIZE {
        let px = mx + dx;
        if px < 0 || cy < 0 || px >= w as i32 || cy >= h as i32 { continue; }
        buf[(cy as u32 * w + px as u32) as usize] = 0x0000FFFF; // cyan
    }
    for dy in 0..(MAG_SIZE - MAG_LABEL_H) {
        let py = my + dy;
        if cx < 0 || py < 0 || cx >= w as i32 || py >= h as i32 { continue; }
        buf[(py as u32 * w + cx as u32) as usize] = 0x0000FFFF;
    }

    // Label strip at the bottom (black, 70% opaque per spec).
    let label_y = my + MAG_SIZE - MAG_LABEL_H;
    draw_rounded_rect_alpha(buf, w, h, mx, label_y, MAG_SIZE, MAG_LABEL_H, 0, 0x000000, 0.7);

    let cfx_clamped = cfx.clamp(0, fw as i32 - 1) as u32;
    let cfy_clamped = cfy.clamp(0, fh as i32 - 1) as u32;
    let center = frame.get_pixel(cfx_clamped, cfy_clamped);
    let [r, g, b, _a] = center.0;
    let text = format!(
        "#{:02X}{:02X}{:02X} {}px, {}px",
        r, g, b, cfx_clamped, cfy_clamped
    );
    font.render_text(buf, w, h, mx + 4, label_y + 2, &text, MAG_FONT_PX, 0x00FFFFFF);
}

#[cfg(test)]
mod magnifier_tests {
    use super::*;

    #[test]
    fn magnifier_default_goes_bottom_right() {
        let (x, y) = magnifier_position((100, 100), (800, 600), 120, 20);
        assert_eq!((x, y), (120, 120));
    }

    #[test]
    fn magnifier_flips_when_near_right_edge() {
        let (x, _y) = magnifier_position((750, 100), (800, 600), 120, 20);
        // x would be 770, mag ends at 890 > 800 → flip to left.
        assert_eq!(x, 750 - 20 - 120);
    }

    #[test]
    fn magnifier_flips_when_near_bottom() {
        let (_x, y) = magnifier_position((100, 550), (800, 600), 120, 20);
        assert_eq!(y, 550 - 20 - 120);
    }

    #[test]
    fn magnifier_clamps_to_zero_in_extreme_corner() {
        let (x, y) = magnifier_position((5, 5), (800, 600), 120, 20);
        // Default would be (25, 25), fits — no flip.
        assert_eq!((x, y), (25, 25));
    }
}
```

- [ ] **Step 2: Wire the magnifier into `redraw`**

Update the `redraw` method in `src/overlay/mod.rs`. Replace its body with:
```rust
    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel_tuple = self.current_selection_rect_window();
        let sel_rect = self.current_selection_rect();
        let show_label = matches!(
            self.state,
            OverlayState::Dragging { .. } | OverlayState::Adjusting { .. }
        );
        let show_magnifier = matches!(
            self.state,
            OverlayState::Idle | OverlayState::Dragging { .. }
        );
        let frame_size = self.frame.dimensions();
        let window_size = (w, h);
        let cursor = self.cursor;
        let frame_ref = &self.frame;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, frame_ref);
        render::apply_dim(&mut buf, w, h, sel_tuple);
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }
        if matches!(self.state, OverlayState::Adjusting { .. }) {
            if let Some(r) = sel_rect {
                render::draw_anchors(&mut buf, w, h, r);
            }
        }
        if show_magnifier {
            render::draw_magnifier(
                &mut buf,
                w,
                h,
                frame_ref,
                cursor,
                window_size,
                &mut self.font,
            );
        }
        if show_label {
            if let Some(r) = sel_rect {
                render::draw_size_label(
                    &mut buf,
                    w,
                    h,
                    r,
                    frame_size,
                    window_size,
                    &mut self.font,
                );
            }
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
```

- [ ] **Step 3: Trigger a redraw whenever the cursor moves in Idle state**

In the `CursorMoved` branch of `handle_event`, update the Idle arm:
```rust
                    OverlayState::Idle => {
                        // Magnifier must follow cursor while idle.
                        self.window.request_redraw();
                    }
```

- [ ] **Step 4: Build and run**

Run:
```bash
cargo build --release
cargo test --lib
```

Expected: clean build, all tests pass.

- [ ] **Step 5: Manual verification**

Run:
```bash
./target/release/quickshot
```

Test script:
1. Hotkey → overlay appears → magnifier follows cursor with 4× zoom + cyan crosshair + `#RRGGBB X,Y` label.
2. Move cursor near the right edge → magnifier flips to the left of cursor.
3. Move cursor near the bottom edge → magnifier flips above cursor.
4. Move cursor into each corner → magnifier stays on-screen.
5. Start drag → magnifier remains visible, keeps tracking.
6. Release → enter Adjusting → **magnifier disappears**; only anchors + size label remain.
7. Click outside → back to Idle → magnifier reappears.
8. Confirm the color readout is roughly accurate: hover over a known-colored region (e.g. your desktop background with a known hex value). Values within ±2 per channel are fine (subpixel rounding).
9. Confirm coordinates are in physical pixels: on a Retina display, the X/Y should increase at ~2× the pixel rate compared to window-space.

All pass = Task 7 good.

- [ ] **Step 6: Commit**

```bash
git add src/overlay/render.rs src/overlay/mod.rs
git commit -m "feat(overlay): magnifier with 4x zoom, crosshair, color/coord label"
```

---

## Task 8: Polish pass — clippy, binary size, README, tag

- [ ] **Step 1: Run the full test suite**

Run:
```bash
cargo test
```

Expected: all unit tests pass. Anchor-count check: 18 state + 6 hit + 2 text + 2 render + 4 magnifier + 5 crop = 37 tests (plus any you added). All green.

- [ ] **Step 2: Clippy clean**

Run:
```bash
cargo clippy --release --all-targets -- -D warnings
```

Fix anything clippy reports. Common post-2a issues: unused imports (leftover from refactors), redundant clones, `match` → `if let` simplifications in `mod.rs`.

- [ ] **Step 3: Record binary size**

Run:
```bash
cargo build --release
ls -lh target/release/quickshot
```

Expected: ≤ 700 KB. If larger, the most likely culprit is the full TTF — note the size in the README and optionally defer a subsetting pass (out of scope for this plan).

- [ ] **Step 4: Update `README.md` with Iter 2a status**

Replace the "MVP status (Iter 1)" section in `README.md` with:
```markdown
## Status (Iter 2a)

- Primary-cursor monitor capture (multi-display: follows cursor's screen)
- Drag → draft → anchor-adjust → Enter/double-click confirm
- ESC cancels
- Live size label (W × H in physical pixels)
- Magnifier with 4× zoom, crosshair, hex + coord readout (visible while aiming/drafting)
- No system notification, no tray icon, no settings window yet (Iter 2b / Iter 3)
- No cross-screen selection
- Exit with Ctrl+C in the launching terminal

Release binary size on this machine: <paste `ls -lh` output here>.
```

- [ ] **Step 5: Commit polish**

```bash
git add README.md
git commit -m "docs: record Iter 2a status and binary size"
```

- [ ] **Step 6: Tag**

```bash
git tag -a v0.2.0-iter2a -m "Iter 2a: anchor adjust, magnifier, size label, Enter/ESC"
```

---

## Manual verification checklist (whole plan — Iter 2a acceptance)

After all tasks finish, run through this end-to-end. All ten must pass:

1. Hotkey opens overlay on the cursor's monitor; magnifier follows cursor.
2. Magnifier flips on all four edges correctly; color readout updates live.
3. Drag → white outline + live size label appear; magnifier continues to follow.
4. Release → anchors appear; magnifier disappears; size label persists.
5. Hover each anchor → correct resize cursor; drag each anchor → correct edge/corner moves.
6. Drag inside selection → whole rect moves; anchors track.
7. Click outside → selection clears; magnifier reappears; can restart drag.
8. Enter → copies; paste into Preview → exact region appears with dimensions matching the label.
9. Double-click inside → same as Enter.
10. ESC from any state → overlay closes, clipboard unchanged.

Regression (Iter 1 invariants):
11. Multi-display: cursor on secondary screen → capture + overlay land on that screen.
12. macOS: overlay covers dock and menu bar (window level 1000).
13. After any capture cycle the hotkey still works (no wedged event loop).
14. Release binary ≤ 700 KB.

All green = Iter 2a done.

---

## Out of scope (deferred)

**Iter 2b (next plan):**
- Full-screen hotkey `Cmd/Ctrl+Shift+S`
- System notification on copy
- Tray icon

**Iter 3:**
- egui settings window (hotkey rebinding, save-to-file toggle, autostart)
- PNG file saving with template name
- `~/.config/quickshot/config.toml` persistence

**Never (per brainstorm):**
- Cross-screen selection (selection spanning multiple monitors)

**Polish items (if size/perf drive):**
- Subset the embedded TTF to shrink binary
- Throttle magnifier redraws during rapid cursor movement
