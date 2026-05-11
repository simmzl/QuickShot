# quickshot Iter 6b — Pin to Desktop (Design Spec)

**Date:** 2026-05-11
**Status:** Design approved; ready for implementation plan.
**Predecessor:** Iter 6a (commit `3e0a8b1`) — smart window snapping.
**Successor:** Iter 7+ (OCR, long screenshot, Windows port, etc.) — out of scope here.

## Goal

After region capture + annotation, give the user a third commit option besides "Enter = copy+save" and "Esc = discard": **pin the cropped image as a floating window** on the desktop. The pinned window floats above normal apps but doesn't steal focus; the user can drag it, double-click to close it, or right-click for a Copy / Close menu. Multiple pins can coexist.

The triggering action is a new toolbar button at the right end of row 1 (next to Redo). Clicking it copies the cropped image to clipboard, saves to disk (if the existing `config.save.enabled` flag is set), creates a new floating window with the image, and closes the overlay.

## Non-Goals (explicitly deferred)

- **Windows / Linux implementation** — macOS only. NSFloatingWindowLevel is macOS-specific; Windows would need `SetWindowPos(.., HWND_TOPMOST, ..)` + custom paint logic.
- **Persistence across restarts** — pins live in memory only; quitting quickshot closes all pins.
- **Scaling / transparency / opacity controls** — pin renders at the captured image's logical pixel size. Users who want a smaller pin can re-capture.
- **Screen-edge snapping or auto-stacking** — pins go wherever the user drags them.
- **Resize handle on the pin window** — fixed size.
- **`NSSavePanel`-driven "Save as..." dialog** — Save happens once at pin creation per `config.save` settings. Right-click menu has no "Save again" entry.
- **Trans-Space pinning (`canJoinAllSpaces`)** — pins stay on the Space where they were created. Switching Spaces hides them; switching back reveals.
- **Pin window theme variants** — no border, no title bar, no shadow customization. The window is just the image bitmap.

---

## UX Specification

### Trigger

A new icon at the end of toolbar row 1, immediately after Redo. Position rationale:

- It's not a tool (no annotation effect on the captured frame), so it doesn't belong in the M/A/R/E/P/T/B group.
- It is an "action that commits" (like Enter), so it sits visually next to Undo/Redo which are the existing action buttons.

Icon: a thumbtack drawn with `fill_rect` / `stroke_line` primitives (same approach as Pen / Text icons in Iter 5b). Approximate shape: a horizontal rectangle for the head + a vertical needle below.

### Click semantics

When the pin button is clicked while in `OverlayState::Adjusting`:

1. Flatten the cropped annotated image (`Overlay::flatten_for_export(rect)`) — same call as Enter.
2. Copy the result to the clipboard (`clipboard::put_image`) — same as Enter.
3. If `config.save.enabled`, save as PNG to `config.save.directory` (same path as Enter).
4. Create a new `PinWindow` at the snap rect's monitor-screen position (offset by 8 px down-right so it doesn't perfectly cover the captured area).
5. Close the overlay (`Outcome::Pinned(rect)` → `App` consumes both the rect and the pin creation).

The toolbar button **doesn't have a keyboard shortcut** in this iter (user explicitly chose toolbar-only trigger). Users who want a key shortcut can request it later.

### Pin window appearance

- **Borderless NSWindow** — no title bar, no close button, no resize handles.
- **Size** — `(image.width / scale_factor, image.height / scale_factor)` in logical pixels. On a Retina display this is half the physical pixel count, matching what the user saw inside the overlay.
- **Initial position** — `(monitor_geom.x + snap_rect.x + 8, monitor_geom.y + snap_rect.y + 8)` in CG screen-space (logical). The 8 px offset keeps the pin visually distinct from the original area so the user immediately sees a floating copy.
- **Level** — `NSFloatingWindowLevel` (= 3). Floats above normal apps but BELOW the overlay's `kCGAssistiveTechHighWindowLevel` (= 1500), so a fresh Cmd+Shift+A while a pin is on screen renders the new overlay above the pin.
- **No focus stealing** — pin window doesn't call `makeKeyAndOrderFront:` or activate the app; it just orders front without becoming key. Keyboard input keeps going to whatever the user was doing before clicking the pin button.

### Pin window interactions

- **Single click + drag** — move the pin. Drag threshold 4 px (matches Iter 6a's snap convention). Mouse-down records press position; subsequent mouse-move while held checks `|Δx| + |Δy| ≥ 4` before starting to apply `setFrameOrigin:` to the underlying NSWindow.
- **Double click** — close this pin window. Two `mouseDown` events within 400 ms position-agnostic, same threshold as Iter 2a's overlay double-click.
- **Right click** — no-op in this iter. A right-click context menu (Copy / Close) requires dynamically creating an Obj-C class with `target:action:` selectors, which is a meaningful chunk of macOS FFI work. Dropped per YAGNI; users who need a fresh copy can re-trigger Cmd+Shift+A (the original capture is still in the clipboard until they copy something else). Reconsider in Iter 6c if user feedback warrants it.
- **No keyboard handling** — the pin window doesn't become key, so it receives no keyboard events. (If we ever want Esc-to-close, that requires re-introducing the `canBecomeKeyWindow` swizzle pattern; not in this iter.)

### Multiple pins

`App` owns `pins: Vec<PinWindow>` plus a `HashMap<WindowId, usize>` for fast `WindowEvent` dispatch. Each pin's `WindowId` is unique; events route directly to the right pin. Closing one pin removes it from both collections; the others continue to operate independently.

No cap on number of pins. If the user spawns 100 pins, the OS will eventually complain but quickshot doesn't enforce a limit. Memory cost per pin: the cropped image bytes (a few KB to a few MB) + a winit window + a softbuffer Surface. Negligible for normal use.

### Pin closure

Three paths:

1. Double-click → `PinOutcome::Closed` → `App` drops the pin from `self.pins`.
2. Right-click → Close → same as above.
3. `App` quit (tray menu) → all pins drop naturally as `App` drops.

No animation on close — the window disappears immediately. (NSWindow appear/disappear animation is already disabled via the existing `set_macos_window_animation_none` helper if we choose to apply it; the spec leaves animation disabled by default.)

---

## Implementation

### File structure

```
src/
├── pin.rs                       (new — PinWindow struct + lifecycle)
├── app.rs                       (modified — pins Vec, dispatch, pin() method)
├── overlay/
│   ├── mod.rs                   (modified — Outcome::Pinned variant)
│   └── toolbar.rs               (modified — pin_button field, ToolbarHit::Pin, draw_icon_pin_thumbtack)
```

`macos_objc.rs` is NOT modified in this iter — right-click menu is dropped, so no new Obj-C bindings are needed beyond what's already there (the existing `set_macos_window_level` is reused for `NSFloatingWindowLevel = 3`).

### `src/pin.rs`

```rust
use anyhow::Result;
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::Instant;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

pub struct PinWindow {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    image: RgbaImage,
    press_pos: Option<(i32, i32)>,
    win_pos_at_press: Option<(i32, i32)>,
    last_click: Option<Instant>,
    scale_factor: f32,
}

#[derive(Debug, Clone, Copy)]
pub enum PinOutcome {
    Continue,
    Closed,
}

impl PinWindow {
    pub fn create(
        event_loop: &ActiveEventLoop,
        image: RgbaImage,
        screen_pos_logical: (i32, i32),
    ) -> Result<Self> { /* implementation in plan */ }

    pub fn handle_event(&mut self, event: WindowEvent) -> PinOutcome { /* implementation in plan */ }

    pub fn redraw(&mut self) -> Result<()> { /* implementation in plan */ }
}
```

The full bodies live in the implementation plan (Task 2). Key design points:

- `create` uses `WindowAttributes::default().with_decorations(false).with_resizable(false).with_inner_size(LogicalSize::new(w_logical, h_logical)).with_position(LogicalPosition::new(x_logical, y_logical))`.
- After winit creates the window, set NSWindow level to 3 (`NSFloatingWindowLevel`). Reuse the existing `set_macos_window_level` helper.
- Do NOT call `make_macos_key_window` — pins must not steal focus.
- Do NOT apply `set_macos_overlay_collection_behavior` — its `canJoinAllSpaces | stationary | fullScreenAuxiliary` combo is for the high-level overlay. Pins use default collection behavior (Space-local).
- `handle_event` matches:
  - `MouseInput { state: Pressed, button: Left, .. }` → record press_pos + win_pos_at_press; check double-click within 400 ms → `PinOutcome::Closed`.
  - `MouseInput { state: Pressed, button: Right, .. }` → no-op (right-click menu dropped from this iter; see UX section).
  - `MouseInput { state: Released, button: Left, .. }` → clear press_pos.
  - `CursorMoved` → if press held + drag threshold exceeded, compute delta and call `set_outer_position`.
  - `RedrawRequested` → call `redraw`.
  - `CloseRequested` → `PinOutcome::Closed`.

### `src/overlay/toolbar.rs`

Add to `Toolbar` struct:

```rust
pub pin_button: IconButton,
```

Add to `ToolbarHit`:

```rust
Pin,
```

Layout: in `Toolbar::layout`, after the existing redo_button placement, advance `x += ICON_SIZE + ICON_PAD;` and place the pin button:

```rust
let pin_button = IconButton {
    origin: (x, row1_y),
    size: (ICON_SIZE, ICON_SIZE),
};
```

`row1_content_w` calculation gains one more `(ICON_SIZE + ICON_PAD)` term. `bar_w` adjusts automatically.

Hit testing: in `hit_with_tool`, after the existing undo/redo checks and before the row 2 / Mosaic-guard:

```rust
if point_in(cursor, self.pin_button.origin, self.pin_button.size) { return ToolbarHit::Pin; }
```

Drawing: in `draw_toolbar` row 1 section, after `draw_icon_redo`:

```rust
draw_icon_pin_thumbtack(buf, win_w, win_h, toolbar.pin_button.origin, 0xFFFFFF);
```

`draw_icon_pin_thumbtack` is a new private helper that draws a thumbtack: a horizontal bar at top (the head) and a vertical line below (the needle), with a small fill at the head's center for visual weight.

### `src/overlay/mod.rs`

`Outcome` enum gains a new variant:

```rust
pub enum Outcome {
    Continue,
    Confirmed(Rect),
    Pinned(Rect),     // NEW
    Cancelled,
}
```

`handle_left_press` `ToolbarHit::Pin` arm:

```rust
toolbar::ToolbarHit::Pin => {
    self.commit_text_edit();
    return Outcome::Pinned(rect);   // `rect` is the Adjusting selection
}
```

Pin commit reuses the same `commit_text_edit` helper from Iter 5b so any in-flight Text annotation gets pushed to history before the pin is created.

Hint badges (Iter 5b's `h` key): no hint for pin button (no single-key shortcut). The `if show_hints { ... }` block in `draw_toolbar` skips the pin button entry.

### `src/app.rs`

Struct additions:

```rust
use std::collections::HashMap;
use winit::window::WindowId;

pub struct App {
    overlay: Option<Overlay>,
    pins: Vec<pin::PinWindow>,                       // NEW
    pin_window_ids: HashMap<WindowId, usize>,        // NEW: window_id → pins[idx]
    config: crate::config::Config,
    proxy: EventLoopProxy<UserEvent>,
    region_label: String,
    fullscreen_label: String,
    tray: Option<TrayGuard>,
}
```

`App::new` initializes `pins: Vec::new()` and `pin_window_ids: HashMap::new()`.

`App::pin(rect, event_loop)`:

```rust
fn pin(&mut self, rect: Rect, event_loop: &ActiveEventLoop) {
    let Some(mut overlay) = self.overlay.take() else { return };
    let final_image = overlay.flatten_for_export(rect);

    // Same copy + save logic as App::confirm.
    if let Err(e) = clipboard::put_image(&final_image) {
        eprintln!("clipboard error: {e:?}");
    }
    if self.config.save.enabled {
        let _ = crate::file_save::save_png(
            &final_image,
            &self.config.save.directory,
            &self.config.save.filename_template,
            crate::file_save::CaptureMode::Region,
        );
    }

    // Compute pin screen position (logical CG screen coords).
    let screen_pos = compute_pin_screen_position(&overlay, rect);

    match pin::PinWindow::create(event_loop, final_image, screen_pos) {
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
```

`compute_pin_screen_position` translates the rect (which is in overlay-window physical pixels) to CG screen logical-pixel space:

```rust
// overlay.window's outer position + rect.x / scale_factor → CG screen logical x
```

The exact math goes in the implementation plan.

`App::close_pin(idx)`:

```rust
fn close_pin(&mut self, idx: usize) {
    if idx >= self.pins.len() { return; }
    let pin = self.pins.swap_remove(idx);
    self.pin_window_ids.remove(&pin.window.id());
    // After swap_remove, the pin that moved into position `idx` (if any) needs
    // its WindowId remapped. swap_remove takes the last element and puts it at
    // idx, so the moved pin's WindowId now points to idx.
    if idx < self.pins.len() {
        let moved_id = self.pins[idx].window.id();
        self.pin_window_ids.insert(moved_id, idx);
    }
    // pin dropped here → winit window closes
}
```

`window_event` dispatch:

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
            pin::PinOutcome::Continue => {}
            pin::PinOutcome::Closed => self.close_pin(idx),
        }
    }
}
```

### Right-click menu — dropped from this iter

Right-click context menus on macOS require dynamic Obj-C class creation (target/action bindings for `NSMenuItem`) plus `NSMenu.popUpContextMenuAtLocation` wiring. That's a non-trivial chunk of FFI work for one feature; per the UX section above, the right-click menu is dropped from this iter and revisited only if user feedback warrants it. No `NSMenu` / `class_addMethod` / `objc_allocateClassPair` bindings are added in this iter.

---

## Testing

Pure-types tests in `pin.rs`:

```rust
#[test]
fn drag_threshold_for_pin_matches_overlay() {
    // The pin uses the same 4 px Manhattan threshold as the overlay snap drag.
    // No new helper — reuse snap::drag_threshold_exceeded. Verify by call.
    assert!(crate::overlay::snap::drag_threshold_exceeded((0, 0), (4, 0)));
    assert!(!crate::overlay::snap::drag_threshold_exceeded((0, 0), (3, 0)));
}
```

Hmm — that's not really testing pin code. Skip; the threshold helper is already tested in `snap.rs`.

Pure-types in `pin.rs` that ARE worth testing:
- `compute_pin_screen_position(overlay_outer_pos, rect_physical, scale_factor) -> (i32, i32)` — extract this as a pure function and unit-test it.

Manual smoke (Task 11):
- Pin a window → drag it → release → it stays put.
- Pin another window → both visible.
- Double click the first pin → it closes, the second remains.
- Pin while another app is frontmost → focus doesn't change; the pin appears but the other app stays active.
- Trigger Cmd+Shift+A while pin is on screen → new overlay appears ABOVE the pin (level 1500 > 3).
- Pin → switch Space → pin is gone (Space-local). Switch back → pin returns.

---

## Risks

- **NSWindow.setFrameOrigin: vs winit's set_outer_position** — winit 0.30 should expose `Window::set_outer_position(Position)`. Verify; if not, fall back to direct AppKit call.
- **Pin window focus stealing** — macOS sometimes auto-activates a process when it creates a new window. If pins create steal focus despite us not calling makeKeyAndOrderFront, may need `[NSWindow setActivationPolicy:NSWindowActivationPolicyNone]` or similar. Test early.
- **Many pins memory** — each pin holds a full RgbaImage (uncompressed). A 1080p region capture is ~8 MB. 10 pins = 80 MB. Acceptable for normal use.
- **swap_remove + index reuse correctness** — straightforward but easy to get wrong. Test by closing pin idx=1 of [a, b, c] → expected pins = [a, c] with c at idx=1. The `pin_window_ids` HashMap must update.

---

## Out-of-spec items worth noting in the implementation plan

- Toolbar test count: Iter 6a's `tool_order_includes_pen_and_text` doesn't reference the pin button. Add a new `toolbar_has_pin_button` test.
- `row1_content_w` math: verify that adding the pin button doesn't push the toolbar off the right edge of small selections. The flip-above-when-no-room logic already handles vertical clamping; horizontal clamp is `bar_x = (wwi - 4 - bar_w).max(4)` — verify it still works.
- `Outcome::Pinned(Rect)` adds a third "confirm-like" variant. Verify that `Outcome::Cancelled` paths (Esc) still work as before — no regression.
- App.confirm currently does `let Some(mut overlay) = self.overlay.take()`. App.pin does the same. Extract a shared helper `let final_image = overlay.flatten_for_export(rect); copy_and_save(&self.config, &final_image, CaptureMode::Region);` if the duplication grows. For two call sites, inline duplication is fine.
