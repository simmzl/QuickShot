# quickshot Iter 6b — Pin to Desktop Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new toolbar button creates a borderless floating window with the cropped annotated image; the user can drag it, double-click to close it; multiple pins coexist. Copy + save still happen on pin (same as Enter). macOS only.

**Architecture:** New module `src/pin.rs` owns `PinWindow` (winit window + softbuffer surface + state for drag detection / double-click close). `App` gains `pins: Vec<PinWindow>` + `pin_window_ids: HashMap<WindowId, usize>` for `WindowEvent` dispatch. Overlay gets a new `Outcome::Pinned(Rect)` variant + a new toolbar button (`ToolbarHit::Pin`). Pins use `NSFloatingWindowLevel = 3` (below the overlay's 1500); they don't call `make_macos_key_window` so they never steal focus.

**Tech Stack:** Existing crates only. `winit::window::Window::set_outer_position` for drag. macOS `setLevel:3` via the existing `set_macos_window_level` helper (Iter 5b). No new FFI surface (right-click menu deferred per spec).

**Spec:** `docs/superpowers/specs/2026-05-11-quickshot-iter6b-design.md`

**Scope for this plan:**
- 1 new module: `src/pin.rs`
- 4 modified files: `src/app.rs`, `src/main.rs`, `src/overlay/mod.rs`, `src/overlay/toolbar.rs`
- 0 new deps
- New toolbar button + `Outcome::Pinned` variant
- Multi-pin lifecycle: create, drag, double-click close, drop on app exit
- Pin doesn't grab focus; sits at NSFloatingWindowLevel = 3 (above normal apps, below overlay)

**Not in this plan:**
- Right-click context menu (deferred per spec)
- Keyboard handling on pin window
- Persistence across restarts
- Windows / Linux (NSFloatingWindowLevel is macOS-specific)
- Resize / scale / opacity controls
- Pin window decorations (no border, no shadow customization)

---

## File Structure

```
src/
├── pin.rs                       (NEW — PinWindow + PinOutcome + lifecycle)
├── app.rs                       (modified — pins Vec, dispatch, pin() method)
├── main.rs                      (modified — `mod pin;`)
└── overlay/
    ├── mod.rs                   (modified — Outcome::Pinned variant + pin button routing)
    └── toolbar.rs               (modified — pin_button field + ToolbarHit::Pin + draw_icon_pin_thumbtack)
```

`macos_objc.rs` is NOT modified (right-click menu deferred → no new Obj-C bindings needed; existing `set_macos_window_level` already supports floating level).

---

## Task 1: `pin.rs` skeleton + module registration

Empty `PinWindow` shell + `PinOutcome` enum so subsequent tasks can refer to the types. No behavior yet. Verifies the module is wired into the crate.

**Files:**
- Create: `src/pin.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `src/pin.rs` with the shell**

```rust
//! A pinned floating window — a cropped screenshot image that the user
//! "tacked" onto the desktop. Multiple pins can coexist; each owns its own
//! winit Window + softbuffer Surface.

use anyhow::Result;
use image::RgbaImage;
use softbuffer::Surface;
use std::rc::Rc;
use std::time::Instant;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

#[derive(Debug, Clone, Copy)]
pub enum PinOutcome {
    Continue,
    Closed,
}

pub struct PinWindow {
    pub window: Rc<Window>,
    #[allow(dead_code)]
    surface: Surface<Rc<Window>, Rc<Window>>,
    #[allow(dead_code)]
    image: RgbaImage,
    #[allow(dead_code)]
    press_pos: Option<(i32, i32)>,
    #[allow(dead_code)]
    win_pos_at_press: Option<(i32, i32)>,
    #[allow(dead_code)]
    last_click: Option<Instant>,
}

impl PinWindow {
    /// Stub — actual implementation lands in Task 5.
    #[allow(dead_code)]
    pub fn create(
        _event_loop: &ActiveEventLoop,
        _image: RgbaImage,
        _screen_pos_logical: (i32, i32),
    ) -> Result<Self> {
        anyhow::bail!("PinWindow::create not yet implemented")
    }

    /// Stub — actual implementation lands in Tasks 7–9.
    #[allow(dead_code)]
    pub fn handle_event(&mut self, _event: WindowEvent) -> PinOutcome {
        PinOutcome::Continue
    }
}
```

The `#[allow(dead_code)]` attributes are temporary — they get removed as each task wires the relevant field / method.

- [ ] **Step 2: Register the module in `src/main.rs`**

In `src/main.rs`, the existing module declarations are in alphabetical order. Insert `mod pin;` between `permission` and `text` (since `permission` < `pin` < `text`):

```rust
mod overlay;
mod permission;
mod pin;          // NEW
mod text;
mod tray;
```

- [ ] **Step 3: Build**

```
cargo build
```

Expected: clean compile. The `dead_code` warnings on `PinWindow` fields are silenced by `#[allow(dead_code)]`.

- [ ] **Step 4: Tests still pass**

```
cargo test -p quickshot
```

Expected: 129 (or whatever the master baseline is — `cargo test -p quickshot 2>&1 | tail -3` should show the count after Iter 6a merged) tests pass, no regression.

- [ ] **Step 5: Commit**

```
git add src/pin.rs src/main.rs
git commit -m "feat(pin): PinWindow skeleton + PinOutcome enum

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Toolbar pin button (layout + hit + icon)

Adds the new pin button to `Toolbar`. Visible in the UI, hit-testable, but clicks have no effect yet (Task 3 wires `Outcome::Pinned`).

**Files:**
- Modify: `src/overlay/toolbar.rs`

- [ ] **Step 1: Add failing tests**

Append to `src/overlay/toolbar.rs::tests`:

```rust
    #[test]
    fn toolbar_has_pin_button() {
        let t = Toolbar::layout(sel(), (1440, 900));
        // Pin button sits to the right of redo on row 1.
        assert!(t.pin_button.origin.0 > t.redo_button.origin.0);
        // Same vertical center as the rest of row 1.
        assert_eq!(t.pin_button.origin.1, t.redo_button.origin.1);
        // Same size as other icons.
        assert_eq!(t.pin_button.size, (ICON_SIZE, ICON_SIZE));
    }

    #[test]
    fn hit_pin_button() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let p = &t.pin_button;
        let c = (p.origin.0 + 5, p.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Pin);
    }

    #[test]
    fn hit_pin_button_works_under_mosaic() {
        // Row 2 is greyed under Mosaic, but row 1 (incl. pin) stays clickable.
        let t = Toolbar::layout(sel(), (1440, 900));
        let p = &t.pin_button;
        let c = (p.origin.0 + 5, p.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Mosaic), ToolbarHit::Pin);
    }
```

- [ ] **Step 2: Run, expect FAIL**

```
cargo test -p quickshot toolbar
```

Expected: FAIL — `pin_button` field doesn't exist on `Toolbar`, `ToolbarHit::Pin` doesn't exist.

- [ ] **Step 3: Add `ToolbarHit::Pin` and `pin_button` field**

In `src/overlay/toolbar.rs`, find the `pub enum ToolbarHit { ... }` definition. Add `Pin` variant:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    Tool(Tool),
    Undo,
    Redo,
    Pin,                      // NEW
    Color(Color),
    Stroke(Stroke),
    None,
}
```

In the `pub struct Toolbar { ... }`, add the `pin_button` field after `redo_button`:

```rust
pub struct Toolbar {
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub tool_buttons: Vec<ToolButton>,
    pub undo_button: IconButton,
    pub redo_button: IconButton,
    pub pin_button: IconButton,           // NEW
    pub color_buttons: Vec<ColorButton>,
    pub stroke_buttons: Vec<StrokeButton>,
}
```

- [ ] **Step 4: Extend `Toolbar::layout` math**

In `Toolbar::layout`, find the existing `row1_content_w` calculation (currently `tools_w + SEP_WIDTH + (ICON_SIZE + ICON_PAD) + ICON_SIZE`). The trailing `(ICON_SIZE + ICON_PAD) + ICON_SIZE` is "undo + redo". Add another `(ICON_SIZE + ICON_PAD) + ICON_SIZE`... wait, more carefully: it's `(ICON_SIZE + ICON_PAD)` for undo (with pad before redo) `+ ICON_SIZE` for redo. We add `+ (ICON_PAD + ICON_SIZE)` for the pin button (pad before it, then its width):

```rust
        let row1_content_w = tools_w
            + SEP_WIDTH
            + ICON_SIZE                           // undo
            + ICON_PAD + ICON_SIZE                // redo
            + ICON_PAD + ICON_SIZE;               // pin (NEW)
```

(If the existing expression is laid out differently — read it first — adapt to add one `(ICON_PAD + ICON_SIZE)` term for pin.)

After `redo_button` placement in the layout loop, place `pin_button`:

```rust
        let redo_button = IconButton {
            origin: (x, row1_y),
            size: (ICON_SIZE, ICON_SIZE),
        };
        x += ICON_SIZE + ICON_PAD;
        let pin_button = IconButton {                    // NEW
            origin: (x, row1_y),
            size: (ICON_SIZE, ICON_SIZE),
        };
```

Update the `Toolbar { ... }` literal at the end of `layout` to include `pin_button`:

```rust
        Toolbar {
            origin: (bar_x, bar_y),
            size: (bar_w, bar_h),
            tool_buttons,
            undo_button,
            redo_button,
            pin_button,                          // NEW
            color_buttons,
            stroke_buttons,
        }
```

- [ ] **Step 5: Extend `hit_with_tool` to route pin clicks**

In `Toolbar::hit_with_tool`, after the existing redo check and BEFORE the `if active_tool == Tool::Mosaic` row-2 guard:

```rust
    pub fn hit_with_tool(&self, cursor: (i32, i32), active_tool: Tool) -> ToolbarHit {
        for btn in &self.tool_buttons {
            if point_in(cursor, btn.origin, btn.size) { return ToolbarHit::Tool(btn.tool); }
        }
        if point_in(cursor, self.undo_button.origin, self.undo_button.size) { return ToolbarHit::Undo; }
        if point_in(cursor, self.redo_button.origin, self.redo_button.size) { return ToolbarHit::Redo; }
        // NEW: pin is row 1 (always clickable, even under Mosaic).
        if point_in(cursor, self.pin_button.origin, self.pin_button.size) { return ToolbarHit::Pin; }
        // Row 2 disabled under Mosaic.
        if active_tool == Tool::Mosaic { return ToolbarHit::None; }
        // ... existing color + stroke checks ...
    }
```

- [ ] **Step 6: Add `draw_icon_pin_thumbtack` helper**

In `src/overlay/toolbar.rs`, alongside the existing `draw_icon_*` helpers (search for `fn draw_icon_undo`), add:

```rust
fn draw_icon_pin_thumbtack(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Thumbtack: a horizontal head bar at top-center, a vertical needle below.
    let pad = ICON_SIZE / 5;
    let cx = o.0 + ICON_SIZE / 2;
    // Head: 12-wide bar near the top, ~6 px tall.
    let head_w = ICON_SIZE - 2 * pad - 4 * UI_SCALE;
    let head_h = 4 * UI_SCALE;
    let head_x = cx - head_w / 2;
    let head_y = o.1 + pad;
    fill_rect(buf, w, h, head_x, head_y, head_w, head_h, color);
    // Stem (needle): vertical line below the head down to near the bottom.
    let stem_h = ICON_SIZE - pad - (head_y - o.1) - head_h;
    fill_rect(buf, w, h, cx - STROKE / 2, head_y + head_h, STROKE, stem_h, color);
    // Tip: a small fat dot at the bottom of the stem to suggest a piercing point.
    let tip_y = head_y + head_h + stem_h - 2 * UI_SCALE;
    fill_rect(buf, w, h, cx - STROKE, tip_y, STROKE * 2, 2 * UI_SCALE, color);
}
```

- [ ] **Step 7: Update `draw_toolbar` to render the pin icon**

Find `draw_toolbar` (around the row 1 rendering, where it calls `draw_icon_redo`). After the redo draw, add:

```rust
    let undo_color = if can_undo { 0xFFFFFF } else { 0x888888 };
    draw_icon_undo(buf, win_w, win_h, toolbar.undo_button.origin, undo_color);
    let redo_color = if can_redo { 0xFFFFFF } else { 0x888888 };
    draw_icon_redo(buf, win_w, win_h, toolbar.redo_button.origin, redo_color);
    // NEW: pin icon is always white (no enabled/disabled state).
    draw_icon_pin_thumbtack(buf, win_w, win_h, toolbar.pin_button.origin, 0xFFFFFF);
```

- [ ] **Step 8: Update hint badge rendering to skip pin button (Iter 5b's `h` key)**

Search `draw_toolbar` for the `if show_hints { ... }` block. It currently iterates `toolbar.tool_buttons` and renders single-letter badges. It does NOT iterate `undo_button`, `redo_button`, or `color_buttons`/`stroke_buttons` (per Iter 5b's spec — only single-key shortcuts get badges). Pin button has no key shortcut, so no badge. **No change required** in the hint block — confirm by reading the existing code that the pin button isn't reached.

- [ ] **Step 9: Run all tests**

```
cargo test -p quickshot toolbar
```

Expected: all toolbar tests pass including the 3 new ones.

Also run the full suite to confirm no regressions:

```
cargo test -p quickshot
```

- [ ] **Step 10: Commit**

```
git add src/overlay/toolbar.rs
git commit -m "feat(toolbar): pin button (icon + hit) at end of row 1

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `Outcome::Pinned(Rect)` + Overlay routing

The overlay returns `Outcome::Pinned(rect)` when the user clicks the pin button. `app.rs` consumes the new variant with a placeholder logging path — actual pin creation lands in Task 10.

**Files:**
- Modify: `src/overlay/mod.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Add `Outcome::Pinned` variant**

In `src/overlay/mod.rs`, find:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Continue,
    Confirmed(Rect),
    Cancelled,
}
```

Add the new variant:

```rust
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Continue,
    Confirmed(Rect),
    Pinned(Rect),           // NEW
    Cancelled,
}
```

- [ ] **Step 2: Route `ToolbarHit::Pin` to `Outcome::Pinned`**

In `Overlay::handle_left_press`, find the `match tb.hit_with_tool(self.cursor, self.tool) { ... }` block inside the `OverlayState::Adjusting { rect, .. }` arm. Currently it has arms for `Tool(t)`, `Undo`, `Redo`, `Color(c)`, `Stroke(s)`, `None`. Add the `Pin` arm BEFORE the `None` arm:

```rust
                    toolbar::ToolbarHit::Pin => {
                        // Commit any in-flight TextEdit before handing off to
                        // App so it gets flattened into the pinned image.
                        self.commit_text_edit();
                        return Outcome::Pinned(rect);
                    }
                    toolbar::ToolbarHit::None => {}
```

`rect` is the current `Adjusting` selection (already bound by the outer match's pattern).

- [ ] **Step 3: Handle `Outcome::Pinned` in `app.rs`**

In `src/app.rs::window_event`, find:

```rust
        match overlay.handle_event(event) {
            Outcome::Continue => {}
            Outcome::Confirmed(rect) => self.confirm(rect),
            Outcome::Cancelled => self.cancel(),
        }
```

Add the new `Pinned` arm with a temporary `eprintln` placeholder — Task 10 replaces this with `self.pin(rect, event_loop)`:

```rust
        match overlay.handle_event(event) {
            Outcome::Continue => {}
            Outcome::Confirmed(rect) => self.confirm(rect),
            Outcome::Pinned(rect) => {
                // TEMPORARY — Task 10 replaces with App::pin(rect, event_loop).
                eprintln!("quickshot: Outcome::Pinned({:?}) — pin window not yet implemented", rect);
                self.cancel();  // close overlay so the test scenario doesn't hang
            }
            Outcome::Cancelled => self.cancel(),
        }
```

Note: `event_loop` isn't currently passed into `window_event` (the first arg is `_event_loop` underscore-prefixed). For Task 10 we'll need to remove the underscore. For Task 3 we don't need it yet — just call `self.cancel()` so the overlay closes.

- [ ] **Step 4: Build + tests**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 5: Manual smoke (optional)**

Restart quickshot (`pkill -f target/release/quickshot ; cargo build --release && target/release/quickshot &`). Press Cmd+Shift+A, drag a region, click the pin button at the end of the toolbar row 1. Expected: a line in stderr (`quickshot: Outcome::Pinned(...) — pin window not yet implemented`) and the overlay closes. No pin window appears yet (Task 10).

- [ ] **Step 6: Commit**

```
git add src/overlay/mod.rs src/app.rs
git commit -m "feat(overlay): Outcome::Pinned + pin button routing (no pin yet)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `compute_pin_screen_position` pure helper

Translates an overlay's outer position + a physical-pixel selection rect + scale_factor into a CG screen logical position where the pin should appear. Pure function, TDD.

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Failing tests**

Append to `src/pin.rs`:

```rust
/// Compute where to place a pin window so it visually "comes from" the
/// captured selection. Inputs are all in physical pixels and screen-logical
/// coordinates; output is screen-logical for `winit::dpi::LogicalPosition`.
///
/// `overlay_outer_logical`: top-left of the overlay window in CG screen-logical
/// coordinates (same units winit's `with_position(LogicalPosition::new(...))`
/// takes).
///
/// `selection_physical`: the rect inside the overlay in physical pixels
/// (overlay-window-local).
///
/// `scale_factor`: the overlay window's scale factor (Retina = 2.0).
///
/// Returns the screen-logical position where the pin window's top-left
/// should sit, plus an 8-px down-right offset so the pin doesn't perfectly
/// cover the captured area.
pub fn compute_pin_screen_position(
    overlay_outer_logical: (i32, i32),
    selection_physical: (i32, i32),
    scale_factor: f32,
) -> (i32, i32) {
    let (ox, oy) = overlay_outer_logical;
    let (sx, sy) = selection_physical;
    let sx_logical = (sx as f32 / scale_factor).round() as i32;
    let sy_logical = (sy as f32 / scale_factor).round() as i32;
    (ox + sx_logical + 8, oy + sy_logical + 8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_position_no_scale() {
        // Overlay at (100, 200) in logical screen coords; selection top-left
        // at physical (50, 30) with scale 1.0 → no division.
        let pos = compute_pin_screen_position((100, 200), (50, 30), 1.0);
        assert_eq!(pos, (100 + 50 + 8, 200 + 30 + 8));
    }

    #[test]
    fn pin_position_retina_scale_2() {
        // On a Retina display the selection rect is in physical pixels;
        // divide by 2.0 before adding to the overlay's logical origin.
        let pos = compute_pin_screen_position((0, 0), (400, 200), 2.0);
        assert_eq!(pos, (200 + 8, 100 + 8));
    }

    #[test]
    fn pin_position_overlay_offset() {
        // Overlay placed at (200, 100) on a second monitor; selection at
        // physical (60, 40) with scale 2.0 → logical (30, 20), then offset.
        let pos = compute_pin_screen_position((200, 100), (60, 40), 2.0);
        assert_eq!(pos, (200 + 30 + 8, 100 + 20 + 8));
    }

    #[test]
    fn pin_position_zero_selection() {
        // Edge case: selection at (0, 0) → pin sits exactly at the overlay
        // origin + 8.
        let pos = compute_pin_screen_position((50, 50), (0, 0), 2.0);
        assert_eq!(pos, (58, 58));
    }
}
```

- [ ] **Step 2: Run, expect PASS** (the function is implemented inline above):

```
cargo test -p quickshot pin::tests
```

Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```
git add src/pin.rs
git commit -m "feat(pin): compute_pin_screen_position pure helper + tests

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `PinWindow::create` — borderless floating window

Implement the create method: winit borderless window at logical-pixel size, NSFloatingWindowLevel = 3, no `make_macos_key_window` (don't steal focus), softbuffer Surface initialized.

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Expose `set_macos_window_level` from the overlay module**

The existing `set_macos_window_level` in `src/overlay/mod.rs` is a file-private fn. Change its visibility to `pub(crate)` so `pin.rs` can call it.

Find:

```rust
#[cfg(target_os = "macos")]
fn set_macos_window_level(window: &Window, level: i64) {
```

Change to:

```rust
#[cfg(target_os = "macos")]
pub(crate) fn set_macos_window_level(window: &Window, level: i64) {
```

- [ ] **Step 2: Implement `PinWindow::create`**

The signature takes `logical_size: (u32, u32)` separately from `image.dimensions()`. The caller (App::pin, Task 10) divides the image's physical-pixel dims by `scale_factor` to get logical dims. This keeps the `create` function platform-agnostic about scale.

In `src/pin.rs`, replace the `create` stub with:

```rust
impl PinWindow {
    pub fn create(
        event_loop: &ActiveEventLoop,
        image: RgbaImage,
        screen_pos_logical: (i32, i32),
        logical_size: (u32, u32),
    ) -> Result<Self> {
        use anyhow::Context;
        use softbuffer::Context as SoftContext;
        use winit::dpi::{LogicalPosition, LogicalSize};
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("quickshot pin")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(
                logical_size.0 as f64,
                logical_size.1 as f64,
            ))
            .with_position(LogicalPosition::new(
                screen_pos_logical.0 as f64,
                screen_pos_logical.1 as f64,
            ));
        let win = event_loop
            .create_window(attrs)
            .context("pin: create window")?;

        let window = Rc::new(win);

        // NSFloatingWindowLevel = 3. Above normal apps, below the overlay's
        // 1500 so a fresh capture appears above pins.
        #[cfg(target_os = "macos")]
        crate::overlay::set_macos_window_level(&window, 3);

        let context =
            SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            image,
            press_pos: None,
            win_pos_at_press: None,
            last_click: None,
        })
    }
}
```

- [ ] **Step 3: Remove `#[allow(dead_code)]` from `surface` and `image`** in `PinWindow` since `create` now uses them. Keep `#[allow(dead_code)]` on `press_pos`, `win_pos_at_press`, `last_click` until Tasks 7–9 use them.

- [ ] **Step 4: Build**

```
cargo build
```

Expected: clean compile.

- [ ] **Step 5: Tests still pass**

```
cargo test -p quickshot
```

Expected: same count, no regression.

- [ ] **Step 6: Commit**

```
git add src/pin.rs src/overlay/mod.rs
git commit -m "feat(pin): PinWindow::create — borderless floating window at level 3

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: `PinWindow::redraw` — paint image into softbuffer

Render the cropped image into the pin window's softbuffer. The image is physical-pixel sized; softbuffer's inner buffer is sized by `window.inner_size()` which gives physical pixels. They should match — one-to-one blit.

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Implement `redraw`**

Add to `impl PinWindow`:

```rust
    pub fn redraw(&mut self) -> Result<()> {
        use std::num::NonZeroU32;

        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let (iw, ih) = self.image.dimensions();
        // softbuffer expects 0x00RRGGBB per pixel. The image is RGBA8;
        // copy + drop alpha. If the window size differs from the image
        // (shouldn't happen but defend against it), clip.
        for y in 0..h.min(ih) {
            for x in 0..w.min(iw) {
                let p = self.image.get_pixel(x, y);
                let argb = ((p[0] as u32) << 16) | ((p[1] as u32) << 8) | (p[2] as u32);
                buf[(y * w + x) as usize] = argb;
            }
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
```

- [ ] **Step 2: Build**

```
cargo build
```

Expected: clean compile.

- [ ] **Step 3: Tests still pass**

```
cargo test -p quickshot
```

No new tests in this task — redraw is GUI-only; smoke-tested via Task 11.

- [ ] **Step 4: Commit**

```
git add src/pin.rs
git commit -m "feat(pin): PinWindow::redraw — RGBA image → softbuffer

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: `PinWindow::handle_event` MVP — RedrawRequested + CloseRequested

Wire up the two simplest events. After this task the pin renders correctly and can be force-closed via the OS (Cmd+Q or app quit).

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Replace the `handle_event` stub**

In `src/pin.rs`, replace the existing `handle_event` stub body:

```rust
    pub fn handle_event(&mut self, event: WindowEvent) -> PinOutcome {
        match event {
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("pin redraw error: {e:?}");
                }
                PinOutcome::Continue
            }
            WindowEvent::CloseRequested => PinOutcome::Closed,
            _ => PinOutcome::Continue,
        }
    }
```

- [ ] **Step 2: Build + tests**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 3: Commit**

```
git add src/pin.rs
git commit -m "feat(pin): handle RedrawRequested + CloseRequested

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: `PinWindow::handle_event` — single-click drag

Mouse press records position + window origin; mouse move with button held + drag threshold → `set_outer_position`; mouse release clears the drag state.

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Extend `handle_event` with drag handling**

In `src/pin.rs`, expand `handle_event` to also handle Mouse + CursorMoved events. Replace the existing match body:

```rust
    pub fn handle_event(&mut self, event: WindowEvent) -> PinOutcome {
        use winit::event::{ElementState, MouseButton};
        use winit::dpi::PhysicalPosition;

        match event {
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("pin redraw error: {e:?}");
                }
                PinOutcome::Continue
            }
            WindowEvent::CloseRequested => PinOutcome::Closed,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Record press for drag detection.
                self.press_pos = self.last_cursor;
                // Record current outer position so we can apply a delta on move.
                let outer = self
                    .window
                    .outer_position()
                    .unwrap_or_default();
                self.win_pos_at_press = Some((outer.x, outer.y));
                PinOutcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                self.press_pos = None;
                self.win_pos_at_press = None;
                PinOutcome::Continue
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor = (position.x as i32, position.y as i32);
                self.last_cursor = Some(cursor);
                if let (Some(press), Some(win_at_press)) = (self.press_pos, self.win_pos_at_press) {
                    // Drag in progress: move window if past threshold.
                    let dx = cursor.0 - press.0;
                    let dy = cursor.1 - press.1;
                    if dx.abs() + dy.abs() >= 4 {
                        let new_x = win_at_press.0 + dx;
                        let new_y = win_at_press.1 + dy;
                        self.window.set_outer_position(PhysicalPosition::new(new_x, new_y));
                    }
                }
                PinOutcome::Continue
            }
            _ => PinOutcome::Continue,
        }
    }
```

- [ ] **Step 2: Add `last_cursor` field to `PinWindow`**

The drag handler needs to know the cursor position at press time. winit's `MouseInput` event doesn't carry the cursor position — we track it via `CursorMoved`. Add `last_cursor` to the struct (and remove `#[allow(dead_code)]` from `press_pos` and `win_pos_at_press` since they're all read now):

```rust
pub struct PinWindow {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    image: RgbaImage,
    press_pos: Option<(i32, i32)>,
    win_pos_at_press: Option<(i32, i32)>,
    last_cursor: Option<(i32, i32)>,    // NEW
    #[allow(dead_code)]
    last_click: Option<Instant>,
}
```

Update `create`'s `Ok(Self { ... })` initializer to include `last_cursor: None`:

```rust
        Ok(Self {
            window,
            surface,
            image,
            press_pos: None,
            win_pos_at_press: None,
            last_cursor: None,
            last_click: None,
        })
```

- [ ] **Step 3: Build**

```
cargo build
```

Expected: clean compile.

- [ ] **Step 4: Tests still pass**

```
cargo test -p quickshot
```

Expected: same count, no regression.

- [ ] **Step 5: Commit**

```
git add src/pin.rs
git commit -m "feat(pin): single-click drag with 4 px threshold

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: `PinWindow::handle_event` — double-click closes

Two `MouseInput::Pressed` events within 400 ms (position-agnostic) trigger `PinOutcome::Closed`. The drag press handler already records `press_pos` / `win_pos_at_press`; we add `last_click` tracking and a double-click check.

**Files:**
- Modify: `src/pin.rs`

- [ ] **Step 1: Update the Pressed arm in `handle_event`**

In `src/pin.rs::handle_event`, find the Mouse Left Pressed arm:

```rust
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.press_pos = self.last_cursor;
                let outer = self.window.outer_position().unwrap_or_default();
                self.win_pos_at_press = Some((outer.x, outer.y));
                PinOutcome::Continue
            }
```

Add a double-click check at the top of the arm:

```rust
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Double-click detection: two Pressed events within 400 ms.
                let now = std::time::Instant::now();
                let is_double_click = matches!(
                    self.last_click,
                    Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
                );
                self.last_click = Some(now);
                if is_double_click {
                    return PinOutcome::Closed;
                }
                // Single press: record for drag detection.
                self.press_pos = self.last_cursor;
                let outer = self.window.outer_position().unwrap_or_default();
                self.win_pos_at_press = Some((outer.x, outer.y));
                PinOutcome::Continue
            }
```

Remove the `#[allow(dead_code)]` on `last_click` since it's now read.

- [ ] **Step 2: Build + tests**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 3: Commit**

```
git add src/pin.rs
git commit -m "feat(pin): double-click closes the pin window

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: `App::pin` + multi-pin dispatch

App owns `Vec<PinWindow>` + `HashMap<WindowId, usize>`. The temporary `Outcome::Pinned` placeholder in `window_event` gets replaced with real pin creation. `close_pin` handles `swap_remove` + HashMap update.

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add `pins` and `pin_window_ids` fields to `App`**

In `src/app.rs`, find the `App` struct. Add the two new fields:

```rust
use std::collections::HashMap;
// `WindowId` already imported via `use winit::window::WindowId;`

pub struct App {
    overlay: Option<Overlay>,
    pins: Vec<crate::pin::PinWindow>,                 // NEW
    pin_window_ids: HashMap<WindowId, usize>,          // NEW
    config: crate::config::Config,
    proxy: EventLoopProxy<UserEvent>,
    region_label: String,
    fullscreen_label: String,
    tray: Option<TrayGuard>,
}
```

Update `App::new` to initialize them:

```rust
impl App {
    pub fn new(
        config: crate::config::Config,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let region_label = config.hotkey.region.raw.clone();
        let fullscreen_label = config.hotkey.fullscreen.raw.clone();
        Self {
            overlay: None,
            pins: Vec::new(),                          // NEW
            pin_window_ids: HashMap::new(),            // NEW
            config,
            proxy,
            region_label,
            fullscreen_label,
            tray: None,
        }
    }
```

- [ ] **Step 2: Add `App::pin` method**

After the existing `App::confirm` method, add:

```rust
    fn pin(&mut self, rect: Rect, event_loop: &ActiveEventLoop) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        let (img_w, img_h) = final_image.dimensions();

        // Same clipboard + save logic as App::confirm.
        match clipboard::put_image(&final_image) {
            Ok(()) => {
                println!("copied {}x{} to clipboard (pinned)", img_w, img_h);
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &final_image,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved \u{2192} {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => eprintln!("clipboard error: {e:?}"),
        }

        // Compute pin position + logical size.
        let scale_factor = overlay.scale_factor();
        let overlay_outer = overlay
            .window
            .outer_position()
            .unwrap_or_default();
        let screen_pos = crate::pin::compute_pin_screen_position(
            (overlay_outer.x, overlay_outer.y),
            (rect.x, rect.y),
            scale_factor,
        );
        let logical_size = (
            (img_w as f32 / scale_factor).round() as u32,
            (img_h as f32 / scale_factor).round() as u32,
        );

        match crate::pin::PinWindow::create(event_loop, final_image, screen_pos, logical_size) {
            Ok(pin_win) => {
                let id = pin_win.window.id();
                let idx = self.pins.len();
                self.pins.push(pin_win);
                self.pin_window_ids.insert(id, idx);
            }
            Err(e) => eprintln!("pin create error: {e:?}"),
        }

        drop(overlay);
    }

    fn close_pin(&mut self, idx: usize) {
        if idx >= self.pins.len() {
            return;
        }
        let pin = self.pins.swap_remove(idx);
        self.pin_window_ids.remove(&pin.window.id());
        // swap_remove moved the last element into `idx`; remap its WindowId.
        if idx < self.pins.len() {
            let moved_id = self.pins[idx].window.id();
            self.pin_window_ids.insert(moved_id, idx);
        }
        // `pin` drops here → winit closes the window.
    }
```

The `overlay.scale_factor()` call requires the `Overlay` to expose `scale_factor`. Check `src/overlay/mod.rs` — the field exists (`scale_factor: f32`) but is private. Add a public accessor:

```rust
// In src/overlay/mod.rs, near current_selection_rect:
pub(crate) fn scale_factor(&self) -> f32 {
    self.scale_factor
}
```

- [ ] **Step 3: Update `window_event` dispatch**

In `src/app.rs::window_event`, replace the entire body with:

```rust
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        // Try overlay first.
        if let Some(overlay) = self.overlay.as_mut() {
            if overlay.window.id() == id {
                match overlay.handle_event(event) {
                    Outcome::Continue => {}
                    Outcome::Confirmed(rect) => self.confirm(rect),
                    Outcome::Pinned(rect) => self.pin(rect, event_loop),
                    Outcome::Cancelled => self.cancel(),
                }
                return;
            }
        }
        // Try pins.
        if let Some(&idx) = self.pin_window_ids.get(&id) {
            match self.pins[idx].handle_event(event) {
                crate::pin::PinOutcome::Continue => {}
                crate::pin::PinOutcome::Closed => self.close_pin(idx),
            }
        }
    }
```

Note: the first arg is now `event_loop: &ActiveEventLoop` (drop the underscore — we need it for `App::pin`).

- [ ] **Step 4: Build**

```
cargo build
```

Expected: clean compile. Some clippy warnings about `dead_code` may go away (pins / pin_window_ids are now used).

- [ ] **Step 5: Tests still pass**

```
cargo test -p quickshot
```

Expected: same count, no regression.

- [ ] **Step 6: Commit**

```
git add src/app.rs src/overlay/mod.rs
git commit -m "feat(app): App::pin creates floating window; multi-pin dispatch

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Final smoke + verification

**Files:** none (verification only).

- [ ] **Step 1: Full test suite**

```
cargo test -p quickshot
```

Expected: all tests pass.

- [ ] **Step 2: Build release**

```
cargo build --release
```

Expected: clean compile, only pre-existing warnings.

- [ ] **Step 3: Stop old quickshot + start the new build**

```
pkill -f 'quickshot' 2>/dev/null ; sleep 1 ; /Users/simmzl/Desktop/personal/quickshot/target/release/quickshot > /tmp/quickshot.log 2>&1 &
```

- [ ] **Step 4: Manual smoke matrix**

For each, hit the hotkey, do the action, confirm behavior:

- [ ] **Basic pin**: Cmd+Shift+A → drag a region → click the pin button (last icon on row 1). A floating window appears with the cropped image at the captured location (offset 8 px down-right). Overlay closes.
- [ ] **Clipboard still works**: After pinning, Cmd+V in any text app pastes the screenshot. (Pin = pin + copy + save).
- [ ] **Save still works**: If `config.save.enabled = true` (check `~/.config/quickshot/config.toml`), confirm a PNG appears in the configured directory.
- [ ] **Drag the pin**: Click + drag the pin window. After 4 px of motion the window starts moving with the cursor. Release the mouse — pin stays put.
- [ ] **Double-click closes**: Double-click anywhere on the pin window. It disappears.
- [ ] **Multiple pins**: Pin a window, then immediately trigger Cmd+Shift+A again, pin another window. Two pins visible simultaneously. Close one (double-click) — the other remains.
- [ ] **Pin doesn't steal focus**: While typing in a text app, trigger a region capture + pin. The pin appears but your text app stays frontmost (the cursor doesn't move to the pin).
- [ ] **Overlay above pin**: With a pin on screen, trigger Cmd+Shift+A. The new overlay appears ABOVE the pin (since overlay level 1500 > pin level 3).
- [ ] **Esc cancels in Adjusting with pin button visible**: Cmd+Shift+A → drag → Esc → overlay closes, no pin created.
- [ ] **Iter 5b/6a regressions**: snap-click a window → switch to red Arrow → drag arrow → click pin → pin appears with the red arrow flattened in. CJK input still works.
- [ ] **Pin survives Space switch via reveal**: pin a window, switch Space, switch back — pin reappears (Space-local but persistent within a Space).

- [ ] **Step 5: Stop dev binary**

```
pkill -f 'target/release/quickshot' 2>/dev/null
```

- [ ] **Step 6: If all green, commit a marker (optional)**

```
git tag v0.8.0-iter6b -m "iter6b: pin to desktop"
```

(Don't push the tag automatically.)

- [ ] **Step 7: If any item fails**

Stop. Diagnose. Fix as a follow-up commit. Re-run the smoke matrix before declaring done.

---

## Summary

Total tasks: 11. Estimated effort: 1.5–2 days.

The plan is layered so each task ends in a clean build + meaningful commit. Tasks 1–4 set up the pin module's skeleton and pure helpers. Tasks 5–9 build the pin window's behavior incrementally (create → render → drag → close). Task 10 wires it into App's lifecycle. Task 11 is GUI smoke. By the final commit every requirement in `docs/superpowers/specs/2026-05-11-quickshot-iter6b-design.md` has a corresponding implementation site or an explicit Non-Goal entry.
