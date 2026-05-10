# quickshot Iter 6a — Smart Window Snapping (Design Spec)

**Date:** 2026-05-10
**Status:** Design approved; ready for implementation plan.
**Predecessor:** Iter 5b (commit `87134fa`) — annotation toolkit completion.
**Successor:** Iter 6b — pin-to-desktop (floating screenshot windows after capture).

## Goal

When the user triggers a region capture (Cmd+Shift+A), don't force them to drag a rectangle. Default behavior becomes **hover-to-snap**: as the cursor moves over a visible window, that window's bounds light up; one click selects the entire window as the capture region. The moment the user actually drags (mouse press + cursor moved ≥ 4 px), fall back to the existing manual rectangle-drag flow.

Outcome: matches the WeChat / CleanShot X / Snipaste behavior; reduces a typical "snap a Slack window" capture from "press hotkey → drag from corner to corner pixel-by-pixel" to "press hotkey → click".

## Non-Goals (explicitly deferred)

- **Pin-to-desktop / floating capture window** — Iter 6b.
- **Sub-region snapping** (browser tabs, individual buttons via Accessibility API) — out of scope.
- **Windows / Linux implementation** — macOS only this iter. `EnumWindows` (Windows) is straightforward and can be added later; Linux has no good equivalent.
- **Cross-monitor snapping** — overlay only covers the cursor's monitor at capture time; windows on other monitors can't be hovered, so they can't be snapped.
- **Element-level snapping inside fullscreen apps** — fullscreen apps cover the whole monitor, so "snap" returns the same rect as the existing fullscreen capture; no value added.
- **Modifier-toggle to disable snap** — the auto-detect (drag ≥ 4 px → drag mode) is sufficient. No `Esc-to-clear-snap` or `hold-Cmd-to-disable-snap` shortcut.
- **Snap target reordering / depth heuristics** — first window in z-order containing the cursor wins. Don't try to be clever about "skip transparent windows" beyond the basic layer/size filter.
- **Refresh window list mid-capture** — captured once at overlay creation. If the user switches Spaces or new windows appear during a capture, the snap targets are stale; that's acceptable for a sub-second interaction.

---

## UX Specification

### State machine

The existing overlay states (`Idle`, `Dragging`, `Adjusting`) stay; snapping is layered onto `Idle` as overlay-level state, not a new variant:

```
Cmd+Shift+A → Overlay::create
  state = Idle
  snap_target = None
  window_list = enumerate_windows()    // captured once
  ↓
[CursorMoved while Idle]
  snap_target = window_under_cursor(cursor, window_list)   // recompute
  ↓
[Cursor enters a window]
  snap_target = Some(rect)
  → render: dim outside rect, blue 2px outline, magnifier still follows cursor
  ↓
[MouseDown]
  press_pos = cursor
  state stays Idle (we don't yet know if it's a click or a drag)
  ↓
[CursorMoved with mouse held + |Δ| ≥ 4 px]
  state = Dragging { start: press_pos, end: cursor }
  snap_target ignored — fall through to existing manual-drag logic
  ↓
[MouseUp without ever entering Dragging]
  if snap_target.is_some():
    state = Adjusting { rect: snap_target, edit: None }
  else:
    state = Idle (no-op click on empty area)
```

`MouseUp` from `Dragging` keeps existing Iter 2a behavior: normalize the start/end into a rect and enter `Adjusting`.

### Visual preview

When `state == Idle && snap_target.is_some()`, render:

1. **Dim outside the snap rect** — same `apply_dim` logic Iter 2a uses for the manual selection rect, just sourced from `snap_target` instead of a drag rect.
2. **Blue 2-px outline** at `#007AFF` (matches `Color::Blue` from Iter 5b for visual consistency).
3. **Magnifier** at cursor — keeps existing Iter 2a behavior. The dim+outline don't replace the magnifier; both show.
4. **No size label** — the manual-drag size label is for "while dragging." Snap targets are pre-determined; the user doesn't need a live size readout. (The label can come back in `Adjusting` after the snap commits.)

When `snap_target.is_none()` (cursor over the desktop or in a region not owned by any window), render exactly as today's `Idle` state: full dim, magnifier follows cursor.

### Detection algorithm

`enumerate_windows(monitor_geom, my_pid)`:

1. Call `CGWindowListCopyWindowInfo(.optionOnScreenOnly | .optionExcludeDesktopElements, kCGNullWindowID)`.
2. For each entry:
   - Skip if `kCGWindowOwnerPID == my_pid` (our own overlay).
   - Skip if `kCGWindowLayer ≤ 0` (Dock 20 is fine; wallpaper -2147483624 is not).
   - Skip if `kCGWindowLayer == 1500` (defensive — our overlay shouldn't be there yet but the order between create-overlay and enumerate matters).
   - Read `kCGWindowBounds` (a `CGRect` dictionary with `X`/`Y`/`Width`/`Height`), translate from screen-space to overlay-window-space by subtracting `monitor_geom.x / .y`.
   - Skip if width < 50 or height < 50 (notification bubbles, menu items, badges).
3. Return the surviving entries in z-order (front-to-back, which is `CGWindowListCopyWindowInfo`'s native order).

`window_under_cursor(cursor: (i32, i32), entries: &[WindowEntry]) -> Option<Rect>`:

- Iterate `entries` in z-order, return the first one whose `bounds.contains(cursor)`.
- No "is this window obscured by a higher one" check — z-order already encodes that. The first hit *is* the topmost.

### Mouse-press routing

The existing Iter 2a `handle_left_press` `Idle` arm calls `state::on_mouse_down_idle(self.cursor)` which transitions to `Dragging { start, end: start }`. We replace that arm:

```rust
OverlayState::Idle => {
    self.press_pos = Some(self.cursor);
    self.snap_at_press = self.snap_target;
    // Don't transition state yet — wait to see if cursor moves.
    Outcome::Continue
}
```

The `Overlay` struct gains:

```rust
pub(crate) snap_target: Option<Rect>,
pub(crate) press_pos: Option<(i32, i32)>,
pub(crate) snap_at_press: Option<Rect>,
window_list: Vec<WindowEntry>,
```

In `CursorMoved`, after the existing `Dragging` / `Adjusting` handling, insert:

```rust
OverlayState::Idle => {
    if let Some(start) = self.press_pos {
        // Mouse is held — check drag threshold.
        let dx = self.cursor.0 - start.0;
        let dy = self.cursor.1 - start.1;
        if dx.abs() + dy.abs() >= 4 {
            // Promote to Dragging using press position as start.
            self.state = OverlayState::Dragging { start, end: self.cursor };
            self.window.request_redraw();
            self.press_pos = None;
            self.snap_at_press = None;
        }
    } else {
        // Hover-snap: recompute snap_target from cursor.
        let new_target = window_under_cursor(self.cursor, &self.window_list);
        if new_target != self.snap_target {
            self.snap_target = new_target;
            self.window.request_redraw();
        }
        self.request_redraw_throttled();   // existing magnifier tick
    }
}
```

In `handle_left_release`:

```rust
if let Some(_press) = self.press_pos.take() {
    // Mouse-up without ever crossing the drag threshold → it's a click.
    if let Some(rect) = self.snap_at_press.take() {
        self.state = OverlayState::Adjusting { rect, edit: None };
        self.window.request_redraw();
    }
    // Else: clicked on empty area (no window under cursor at press) — stay Idle.
    return Outcome::Continue;
}
// ... existing Dragging / Adjusting release handling
```

### Edge cases

- **Cursor leaves the overlay window's bounds:** winit emits `CursorLeft`. Set `snap_target = None`; redraw to clear the highlight.
- **Window list is empty (no apps open or all filtered out):** `snap_target` is always `None`; behavior degrades to today's manual-drag flow. No errors.
- **Cursor over a window whose bounds extend beyond the monitor edge:** clamp the rendered rect to overlay bounds at draw time. The full rect is still stored and used as the `Adjusting` rect.
- **Click inside the overlay but on the dim area (no window under cursor):** falls into the manual-drag flow naturally. The user still has to drag — same as today's `Idle` behavior.

### Keyboard shortcuts

No new shortcuts. The existing Iter 5b set (`M/A/R/E/B/P/T`, `1234`, `[/]`, `h`, `Cmd+Z`, `Esc`, `Enter`) all stay valid in their existing states. Snap mode IS Idle, so none of those apply until the user has selected a region.

---

## Implementation

### File structure

```
src/overlay/
├── snap.rs              (new — WindowEntry, enumerate_windows, window_under_cursor)
├── render.rs            (modified — apply_dim already supports a rect mask; reuse for snap preview)
├── mod.rs               (modified — fields, CursorMoved snap path, press/release routing, redraw)
└── ...other files unchanged

src/macos_objc.rs        (extended — add bindings for CGWindowListCopyWindowInfo + CFArray walk)
```

### `src/overlay/snap.rs`

```rust
//! Window enumeration + cursor-to-window hit testing for smart snap mode.
//!
//! macOS uses CGWindowListCopyWindowInfo (already permitted by the existing
//! Screen Recording grant — no new permission prompt). Windows / Linux paths
//! are stubbed and return empty lists.

use super::state::Rect;
use crate::capture::MonitorGeom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowEntry {
    /// In overlay-window coordinates (monitor-local).
    pub bounds: Rect,
    pub layer: i32,
}

#[cfg(target_os = "macos")]
pub fn enumerate_windows(monitor_geom: &MonitorGeom, my_pid: u32) -> Vec<WindowEntry> {
    /* ... CFArray walk over CGWindowListCopyWindowInfo ... */
}

#[cfg(not(target_os = "macos"))]
pub fn enumerate_windows(_monitor_geom: &MonitorGeom, _my_pid: u32) -> Vec<WindowEntry> {
    Vec::new()
}

/// Return the bounds of the topmost window whose rect contains `cursor`,
/// or None if none does. `entries` must already be in z-order (front to back).
pub fn window_under_cursor(cursor: (i32, i32), entries: &[WindowEntry]) -> Option<Rect> {
    entries.iter().find(|e| e.bounds.contains(cursor)).map(|e| e.bounds)
}
```

`Rect::contains` already exists (Iter 2a) — verify the signature matches `(cursor: (i32, i32)) -> bool`.

### `src/macos_objc.rs` additions

Need bindings for:
- `CGWindowListCopyWindowInfo(option: u32, window_id: u32) -> CFArrayRef`
- `CFArrayGetCount`, `CFArrayGetValueAtIndex`
- `CFDictionaryGetValueIfPresent` for keys: `kCGWindowBounds`, `kCGWindowOwnerPID`, `kCGWindowLayer`
- `CFNumberGetValue` to extract i32/u32 from CF numbers
- `CFStringGetCStringPtr` for window-name (optional, debug only)
- `CFRelease` for cleanup

These all live in CoreFoundation + CoreGraphics, both already linked (no Cargo.toml change needed; existing `core-graphics` dep in Cargo.toml provides ScreenCaptureAccess and we can pull `core-foundation` if needed). Use `extern "C"` declarations + `link_name` aliasing — same pattern as the existing objc_msgSend bindings (Iter 5b's `macos_objc.rs`).

`kCGWindowBounds` returns a CFDictionary with keys `"X"`, `"Y"`, `"Width"`, `"Height"`, all CFNumbers (doubles). Convert to i32 via floor; the cursor is in i32 anyway.

The `option` flag combination is `kCGWindowListOptionOnScreenOnly (0x01) | kCGWindowListOptionExcludeDesktopElements (0x10)` = `0x11`. Pass `kCGNullWindowID = 0` for the second arg.

### `src/overlay/mod.rs` integration

Constructor:

```rust
pub fn create(...) -> Result<Self> {
    let window = ...; // existing
    let monitor_geom = ...; // already in scope
    let my_pid = std::process::id();
    let window_list = snap::enumerate_windows(monitor_geom, my_pid);
    Ok(Self {
        // ... existing fields ...
        snap_target: None,
        press_pos: None,
        snap_at_press: None,
        window_list,
    })
}
```

`CursorMoved` handler — additions detailed in UX section above.
`handle_left_press` — Idle arm replaced as detailed.
`handle_left_release` — pre-existing-Dragging early return added as detailed.

`redraw` — when `state == Idle && snap_target.is_some()`, render the dim+outline preview in the same code path that today renders the manual selection rect. The existing `render::apply_dim(buf, w, h, sel_tuple)` and `render::draw_selection_outline(...)` helpers should accept `snap_target` directly. Use blue (`#007AFF`) instead of white for the outline:

```rust
let preview_rect = match self.state {
    OverlayState::Idle => self.snap_target,
    OverlayState::Dragging { start, end } => Some(Rect::normalize(start, end)),
    OverlayState::Adjusting { rect, .. } => Some(rect),
};
if let Some(r) = preview_rect {
    let tup = r.as_tuple_u32();
    render::apply_dim(buf, w, h, Some(tup));
    let outline_color = match self.state {
        OverlayState::Idle => 0x00007AFF,    // Color::Blue argb
        _                  => 0x00FFFFFF,    // existing white
    };
    render::draw_selection_outline(buf, w, h, tup, outline_color);
}
```

The existing `draw_selection_outline` takes a `color_argb` parameter (Iter 2a) — verify it accepts arbitrary ARGB. If it's hardcoded white, parameterize.

### Testing

Pure-types tests in `snap.rs`:

```rust
#[test]
fn window_under_cursor_returns_topmost() {
    let entries = vec![
        WindowEntry { bounds: Rect { x: 0, y: 0, w: 200, h: 200 }, layer: 0 },
        WindowEntry { bounds: Rect { x: 50, y: 50, w: 200, h: 200 }, layer: 0 },
    ];
    // Both contain (75, 75); first (topmost) wins.
    assert_eq!(window_under_cursor((75, 75), &entries),
               Some(Rect { x: 0, y: 0, w: 200, h: 200 }));
}

#[test]
fn window_under_cursor_returns_none_when_no_match() {
    let entries = vec![WindowEntry { bounds: Rect { x: 0, y: 0, w: 100, h: 100 }, layer: 0 }];
    assert_eq!(window_under_cursor((500, 500), &entries), None);
}

#[test]
fn window_under_cursor_skips_empty_list() {
    assert_eq!(window_under_cursor((0, 0), &[]), None);
}

#[test]
#[cfg(target_os = "macos")]
fn enumerate_windows_returns_some_windows_on_macos() {
    let geom = MonitorGeom { x: 0, y: 0, width: 1920, height: 1080 };
    let entries = enumerate_windows(&geom, std::process::id());
    // On any Mac running this test there should be at least one user window
    // (Terminal, Finder, IDE…). Smoke test only.
    assert!(!entries.is_empty(), "expected at least one window in the list");
}
```

Drag-threshold logic — unit-testable via a pure helper:

```rust
pub(crate) fn drag_threshold_exceeded(start: (i32, i32), now: (i32, i32)) -> bool {
    (now.0 - start.0).abs() + (now.1 - start.1).abs() >= 4
}

#[test]
fn drag_threshold() {
    assert!(!drag_threshold_exceeded((0, 0), (1, 1)));
    assert!(!drag_threshold_exceeded((0, 0), (3, 0)));
    assert!( drag_threshold_exceeded((0, 0), (4, 0)));
    assert!( drag_threshold_exceeded((10, 10), (12, 12)));
}
```

Integration smoke (manual, Iter 6a Task N): run the binary, hover over Finder / Terminal, confirm the window outlines correctly; click → enters Adjusting with the right rect; drag mid-window → falls back to manual rect drag; drag-then-snap-back → final rect is the manual one (no leak from snap_at_press).

---

## Risks

- **CFArray / CFDictionary walking via raw bindings is fiddly.** Use the existing `macos_objc.rs` pattern (declare each `CFFooBar` with `extern "C"` + `#[link_name]`); fontdue's pre-Iter-5b ABI bug from the variadic objc_msgSend declaration is a precedent — be careful about return types and pointer sizes. Test on both x86_64 and aarch64 if the implementer can; otherwise just aarch64 since that's the dev machine.

- **Window list staleness during a capture.** If a notification or app launch happens between hotkey-press and cursor-move, the new window won't appear in `window_list`. Live with it — captures are sub-second; missing one fresh window for one frame won't hurt.

- **Layer filter false negatives.** Some apps (Logi Options, certain VPN clients) put their main window at unusual layers. Start permissive (filter only `layer == 0` baseline + our own PID), add specific filters only if a user reports a bogus snap target.

- **Click at edge of window.** A click 1 px inside a window edge rapidly drifting outside (say, dock-edge anti-windows) produces a jittery snap_target. The drag-threshold check (≥ 4 px) absorbs most of this.

---

## Out-of-spec items worth noting in the implementation plan

- The `Rect` type currently doesn't expose a `contains((i32, i32)) -> bool` method explicitly — verify by reading `state.rs`. If missing, add the obvious 4-line implementation.
- `apply_dim` currently takes `Option<(u32, u32, u32, u32)>` — ensure passing `Some(snap_target.as_tuple_u32())` doesn't choke on negative coordinates that snap-target rects can carry when a window extends beyond the monitor edge.
- `draw_selection_outline` color parameter — verify it's parameterized (Iter 2a). If hardcoded, parameterize and propagate the white default to existing call sites.
