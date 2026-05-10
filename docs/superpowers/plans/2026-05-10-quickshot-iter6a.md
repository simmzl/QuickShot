# quickshot Iter 6a — Smart Window Snapping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Default Cmd+Shift+A behavior changes from "drag a rectangle" to "hover a window → click to select." Mouse drag ≥ 4 px falls back to existing manual rect drag. macOS only.

**Architecture:** New `src/overlay/snap.rs` module enumerates visible windows via `CGWindowListCopyWindowInfo` at overlay creation. `Overlay` gains `snap_target / press_pos / snap_at_press / window_list` fields. The `OverlayState::Idle` arm of `CursorMoved` recomputes `snap_target` from cursor position; the `handle_left_press` Idle arm defers state transition to release time so we can detect drag-vs-click.

**Tech Stack:** Existing crates only. Adds `core-foundation = "0.10"` (transitive companion to existing `core-graphics`) for safe CFArray/CFDictionary walking. No new third-party logic crates.

**Spec:** `docs/superpowers/specs/2026-05-10-quickshot-iter6a-design.md`

**Scope for this plan:**
- 1 new module: `src/overlay/snap.rs`
- 1 dep added: `core-foundation = "0.10"` (macOS only)
- `Overlay` gains 4 fields + their initializers
- `CursorMoved`, `handle_left_press`, `handle_left_release` get new logic
- `redraw` uses blue outline color when snap-previewing (state == Idle && snap_target.is_some())
- `WindowEvent::CursorLeft` handler clears snap_target

**Not in this plan:** pin-to-desktop (Iter 6b), Windows / Linux implementation, sub-region snapping, modifier-toggle to disable snap.

---

## File Structure

```
src/overlay/
├── snap.rs                 (new — WindowEntry, enumerate_windows, window_under_cursor, drag_threshold_exceeded)
├── mod.rs                  (modified — fields, CursorMoved, press/release, redraw, CursorLeft)
└── ... others unchanged

Cargo.toml                  (modified — add core-foundation under macOS deps)
```

`apply_dim` (in `render.rs`) and `draw_selection_outline` are already parameterized — no changes there.

---

## Task 1: Add `core-foundation` dep

Pure config change. Verifies the build still works with the new crate before any code uses it.

**Files:**
- Modify: `Cargo.toml`

- [ ] **Step 1: Add the dep**

In `Cargo.toml`, find the existing macOS-only deps block:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
core-graphics = "0.23"
```

Replace with:

```toml
[target.'cfg(target_os = "macos")'.dependencies]
core-graphics = "0.23"
core-foundation = "0.10"
```

- [ ] **Step 2: Verify build**

```
cargo build
```

Expected: clean compile (only pre-existing `clashing_extern_declarations` warnings from `macos_objc.rs`).

- [ ] **Step 3: Commit**

```
git add Cargo.toml Cargo.lock
git commit -m "chore: add core-foundation dep for CFArray/CFDictionary walks

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Pure types + `window_under_cursor` (TDD)

Sets up `snap.rs` module with no platform code yet. Pure logic only — testable on any host. `enumerate_windows` is stubbed to `Vec::new()` for now; Task 3 fills in the macOS implementation.

**Files:**
- Create: `src/overlay/snap.rs`
- Modify: `src/overlay/mod.rs` (add `pub(crate) mod snap;`)

- [ ] **Step 1: Add module declaration**

In `src/overlay/mod.rs`, near the existing `pub(crate) mod` lines (top of file):

```rust
pub(crate) mod snap;
```

Place it alphabetically (after `render`, before `state`) to match the existing style.

- [ ] **Step 2: Write the failing tests**

Create `src/overlay/snap.rs`:

```rust
//! Window enumeration + cursor-to-window hit testing for smart snap mode.
//!
//! macOS uses CGWindowListCopyWindowInfo (already permitted by the existing
//! Screen Recording grant — no new permission prompt). Windows / Linux paths
//! return empty lists for now.

use super::state::Rect;
use crate::capture::MonitorGeom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowEntry {
    /// In overlay-window coordinates (monitor-local — already translated from
    /// CG screen-space by subtracting the monitor's origin).
    pub bounds: Rect,
    pub layer: i32,
}

/// Return the bounds of the topmost window whose rect contains `cursor`,
/// or None if none does. `entries` must already be in z-order (front to back).
pub fn window_under_cursor(cursor: (i32, i32), entries: &[WindowEntry]) -> Option<Rect> {
    entries.iter().find(|e| e.bounds.contains(cursor)).map(|e| e.bounds)
}

#[cfg(target_os = "macos")]
pub fn enumerate_windows(_monitor_geom: &MonitorGeom, _my_pid: u32) -> Vec<WindowEntry> {
    // Filled in by Task 3.
    Vec::new()
}

#[cfg(not(target_os = "macos"))]
pub fn enumerate_windows(_monitor_geom: &MonitorGeom, _my_pid: u32) -> Vec<WindowEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(x: i32, y: i32, w: i32, h: i32, layer: i32) -> WindowEntry {
        WindowEntry { bounds: Rect { x, y, w, h }, layer }
    }

    #[test]
    fn window_under_cursor_returns_topmost() {
        // Both entries contain (75, 75); the first (topmost in z-order) wins.
        let entries = vec![
            entry(0, 0, 200, 200, 0),
            entry(50, 50, 200, 200, 0),
        ];
        assert_eq!(
            window_under_cursor((75, 75), &entries),
            Some(Rect { x: 0, y: 0, w: 200, h: 200 })
        );
    }

    #[test]
    fn window_under_cursor_returns_none_when_no_match() {
        let entries = vec![entry(0, 0, 100, 100, 0)];
        assert_eq!(window_under_cursor((500, 500), &entries), None);
    }

    #[test]
    fn window_under_cursor_skips_empty_list() {
        assert_eq!(window_under_cursor((0, 0), &[]), None);
    }

    #[test]
    fn window_under_cursor_finds_only_match() {
        let entries = vec![
            entry(0, 0, 100, 100, 0),
            entry(200, 200, 100, 100, 0),
        ];
        assert_eq!(
            window_under_cursor((250, 250), &entries),
            Some(Rect { x: 200, y: 200, w: 100, h: 100 })
        );
    }
}
```

- [ ] **Step 3: Run tests**

```
cargo test -p quickshot snap::tests
```

Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```
git add src/overlay/mod.rs src/overlay/snap.rs
git commit -m "feat(snap): pure types + window_under_cursor hit test

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: `enumerate_windows` macOS implementation

Walks `CGWindowListCopyWindowInfo` and returns window entries in z-order. Uses the existing `core-graphics` crate's constants and `core-foundation`'s safe CFArray/CFDictionary wrappers (added in Task 1).

**Files:**
- Modify: `src/overlay/snap.rs`

- [ ] **Step 1: Replace the macOS stub with the real implementation**

In `src/overlay/snap.rs`, replace the `#[cfg(target_os = "macos")] pub fn enumerate_windows(...)` body:

```rust
#[cfg(target_os = "macos")]
pub fn enumerate_windows(monitor_geom: &MonitorGeom, my_pid: u32) -> Vec<WindowEntry> {
    use core_foundation::array::CFArray;
    use core_foundation::base::{CFType, TCFType};
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;

    // CGWindowListOption flags. core-graphics 0.23 doesn't always re-export
    // these as constants, so define them locally.
    const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0; // 0x01
    const KCG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4; // 0x10
    const KCG_NULL_WINDOW_ID: u32 = 0;

    // CFArrayRef CGWindowListCopyWindowInfo(uint32_t option, uint32_t window)
    extern "C" {
        fn CGWindowListCopyWindowInfo(
            option: u32,
            relative_to_window: u32,
        ) -> core_foundation::array::CFArrayRef;
    }

    let option = KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY
        | KCG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS;
    let array_ref = unsafe { CGWindowListCopyWindowInfo(option, KCG_NULL_WINDOW_ID) };
    if array_ref.is_null() {
        return Vec::new();
    }

    // SAFETY: CGWindowListCopyWindowInfo returns a +1 retained CFArray; wrap
    // under create rule so it's released on Drop.
    let array: CFArray<CFDictionary<CFString, CFType>> =
        unsafe { CFArray::wrap_under_create_rule(array_ref) };

    let mut out = Vec::with_capacity(array.len() as usize);
    for dict_ref in array.iter() {
        let dict: &CFDictionary<CFString, CFType> = &*dict_ref;

        // PID filter: skip our own overlay window.
        let pid = read_i64(dict, "kCGWindowOwnerPID").unwrap_or(0);
        if pid as u32 == my_pid {
            continue;
        }

        // Layer filter: skip dock background, wallpaper, etc.
        // Keep layer 0 (normal app windows). Status-bar items live above (25),
        // dock at 20 — those are valid hover targets, but most users don't
        // want to "snap" the dock or menu bar. Filter to layer == 0 only for
        // a tight, predictable result.
        let layer = read_i64(dict, "kCGWindowLayer").unwrap_or(i64::MIN) as i32;
        if layer != 0 {
            continue;
        }

        // Bounds: CFDictionary { X, Y, Width, Height } → Rect.
        let Some(bounds_dict) = read_dict(dict, "kCGWindowBounds") else { continue };
        let cg_x = read_f64(&bounds_dict, "X").unwrap_or(0.0);
        let cg_y = read_f64(&bounds_dict, "Y").unwrap_or(0.0);
        let cg_w = read_f64(&bounds_dict, "Width").unwrap_or(0.0);
        let cg_h = read_f64(&bounds_dict, "Height").unwrap_or(0.0);

        // Skip pathological / decorative tiny windows (notification dots, badges).
        if cg_w < 50.0 || cg_h < 50.0 {
            continue;
        }

        // Translate CG-screen coords (origin at top-left of primary display) to
        // overlay-window coords (origin at the captured monitor's top-left).
        let local_x = (cg_x as i32) - monitor_geom.x;
        let local_y = (cg_y as i32) - monitor_geom.y;

        out.push(WindowEntry {
            bounds: Rect {
                x: local_x,
                y: local_y,
                w: cg_w as i32,
                h: cg_h as i32,
            },
            layer,
        });
    }

    out
}

#[cfg(target_os = "macos")]
fn read_i64(
    dict: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
    key: &str,
) -> Option<i64> {
    use core_foundation::base::TCFType;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    let key = CFString::new(key);
    let val = dict.find(&key)?;
    let num = val.downcast::<CFNumber>()?;
    num.to_i64()
}

#[cfg(target_os = "macos")]
fn read_f64(
    dict: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
    key: &str,
) -> Option<f64> {
    use core_foundation::base::TCFType;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    let key = CFString::new(key);
    let val = dict.find(&key)?;
    let num = val.downcast::<CFNumber>()?;
    num.to_f64()
}

#[cfg(target_os = "macos")]
fn read_dict(
    dict: &core_foundation::dictionary::CFDictionary<
        core_foundation::string::CFString,
        core_foundation::base::CFType,
    >,
    key: &str,
) -> Option<core_foundation::dictionary::CFDictionary<
    core_foundation::string::CFString,
    core_foundation::base::CFType,
>> {
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    let key = CFString::new(key);
    let val = dict.find(&key)?;
    val.downcast::<CFDictionary<CFString, core_foundation::base::CFType>>()
}
```

If the `downcast::<CFDictionary<CFString, CFType>>()` doesn't compile due to generic-parameter inference, fall back to `downcast::<core_foundation::dictionary::CFDictionary<core_foundation::base::CFType, core_foundation::base::CFType>>()` and adjust subsequent reads accordingly. The exact API shape of `core-foundation` 0.10 may differ slightly — read `cargo doc -p core-foundation --open` if needed and adapt the type parameters. The CFArray walk and key-string lookups are stable; only the type witness in `downcast` is what shifts.

- [ ] **Step 2: Add the macOS smoke test**

Append to `src/overlay/snap.rs::tests`:

```rust
    #[test]
    #[cfg(target_os = "macos")]
    fn enumerate_windows_returns_some_windows_on_macos() {
        // On any Mac running this test there should be at least one user
        // window (Finder, Terminal, IDE…). Smoke test only — exact contents
        // depend on the test machine.
        let geom = MonitorGeom { x: 0, y: 0, width: 1920, height: 1080 };
        let entries = enumerate_windows(&geom, std::process::id());
        assert!(
            !entries.is_empty(),
            "expected at least one window in the list on macOS",
        );
    }
```

- [ ] **Step 3: Run tests**

```
cargo test -p quickshot snap
```

Expected: 5 tests pass on macOS (4 pure + 1 macOS smoke). On Windows/Linux the smoke test is skipped, only 4 pass.

- [ ] **Step 4: Verify build**

```
cargo build --release
```

Expected: clean release build. Universal-build path (`scripts/package.sh`) is not exercised by this task — Task 11 covers that.

- [ ] **Step 5: Commit**

```
git add src/overlay/snap.rs
git commit -m "feat(snap): enumerate visible windows via CGWindowListCopyWindowInfo

Filters by layer == 0 and PID != self. Translates CG screen coords to
monitor-local. Drops < 50px windows (badges, notifications). Returned
in z-order so window_under_cursor's first-hit is correct.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: `drag_threshold_exceeded` helper (TDD)

A 4-pixel Manhattan distance check on press_pos vs current cursor — used in CursorMoved to decide "click → snap" vs "drag → manual rect."

**Files:**
- Modify: `src/overlay/snap.rs`

- [ ] **Step 1: Failing tests**

Append to `src/overlay/snap.rs` (above the `#[cfg(test)] mod tests`):

```rust
/// Returns true if the cursor has moved at least 4 pixels (Manhattan
/// distance) from the press position. Used to disambiguate "click on a
/// window to snap" from "drag a rectangle".
pub fn drag_threshold_exceeded(start: (i32, i32), now: (i32, i32)) -> bool {
    (now.0 - start.0).abs() + (now.1 - start.1).abs() >= 4
}
```

In the existing `tests` mod:

```rust
    #[test]
    fn drag_threshold_at_zero_is_false() {
        assert!(!drag_threshold_exceeded((0, 0), (0, 0)));
    }

    #[test]
    fn drag_threshold_under_4_is_false() {
        assert!(!drag_threshold_exceeded((0, 0), (1, 1)));
        assert!(!drag_threshold_exceeded((0, 0), (3, 0)));
        assert!(!drag_threshold_exceeded((0, 0), (0, 3)));
        assert!(!drag_threshold_exceeded((0, 0), (1, 2)));
    }

    #[test]
    fn drag_threshold_at_or_over_4_is_true() {
        assert!(drag_threshold_exceeded((0, 0), (4, 0)));
        assert!(drag_threshold_exceeded((0, 0), (0, 4)));
        assert!(drag_threshold_exceeded((0, 0), (2, 2)));
        assert!(drag_threshold_exceeded((10, 10), (12, 12)));
    }

    #[test]
    fn drag_threshold_works_with_negative_motion() {
        // |Δx| + |Δy| uses .abs(), so cursor moving up-left also counts.
        assert!(drag_threshold_exceeded((10, 10), (6, 10)));
        assert!(drag_threshold_exceeded((10, 10), (10, 6)));
        assert!(drag_threshold_exceeded((10, 10), (8, 8)));
        assert!(!drag_threshold_exceeded((10, 10), (9, 9)));
    }
```

- [ ] **Step 2: Run, expect PASS**

```
cargo test -p quickshot snap::tests::drag_threshold
```

Expected: 4 tests PASS.

- [ ] **Step 3: Commit**

```
git add src/overlay/snap.rs
git commit -m "feat(snap): drag_threshold_exceeded helper (4 px Manhattan)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: `Overlay` struct fields + constructor

Adds `snap_target / press_pos / snap_at_press / window_list` fields. Constructor enumerates windows once. No behavior change yet (subsequent tasks wire them).

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add fields to `Overlay`**

In `src/overlay/mod.rs`, find the `pub struct Overlay { ... }` definition (around line 31). Add four fields at the bottom of the struct, just before the closing brace:

```rust
pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub(crate) state: OverlayState,
    pub(crate) cursor: (i32, i32),
    last_click: Option<std::time::Instant>,
    last_redraw: Option<std::time::Instant>,
    font: crate::text::Font,
    scale_factor: f32,
    pub(crate) tool: annotate::Tool,
    pub(crate) history: annotate::History,
    pub(crate) pending_draw: Option<PendingDraw>,
    pub(crate) current_style: AnnotationStyle,
    pub(crate) text_edit: Option<TextEdit>,
    modifiers: ModifiersState,
    pub(crate) show_hints: bool,
    // Iter 6a: smart window snapping state.
    pub(crate) snap_target: Option<Rect>,
    pub(crate) press_pos: Option<(i32, i32)>,
    pub(crate) snap_at_press: Option<Rect>,
    window_list: Vec<snap::WindowEntry>,
}
```

- [ ] **Step 2: Initialize in `Overlay::create`**

Find `Ok(Self { ... })` in `Overlay::create` (around line 180). Add the four new field initializers after `show_hints: false,`:

```rust
        Ok(Self {
            window,
            surface,
            frame,
            state: OverlayState::Idle,
            cursor: (0, 0),
            last_click: None,
            last_redraw: None,
            font: crate::text::Font::embedded(),
            scale_factor,
            tool: annotate::Tool::Move,
            history: annotate::History::new(),
            pending_draw: None,
            current_style: AnnotationStyle::default(),
            text_edit: None,
            modifiers: ModifiersState::default(),
            show_hints: false,
            // Iter 6a: enumerate windows once at overlay creation. Captures
            // are sub-second so window topology is effectively frozen for
            // the duration; no need to refresh.
            snap_target: None,
            press_pos: None,
            snap_at_press: None,
            window_list: snap::enumerate_windows(monitor_geom, std::process::id()),
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

Expected: same count as before this task (no new tests; no existing tests should break).

- [ ] **Step 5: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): snap state fields on Overlay (no behavior change)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: CursorMoved hover-snap + drag-threshold transition

The Idle arm of `CursorMoved` recomputes `snap_target` from cursor position when no mouse button is held; when `press_pos` is set, watches for the 4-px drag threshold and promotes Idle → Dragging.

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Update the Idle arm of CursorMoved**

Find the `WindowEvent::CursorMoved` block in `Overlay::handle_event` (around line 204). The match-on-`self.state` inside has `OverlayState::Idle` returning a magnifier-tick `request_redraw_throttled()` (around line 231-235). Replace the Idle arm with:

```rust
                    OverlayState::Idle => {
                        if let Some(start) = self.press_pos {
                            // Mouse held — check drag threshold to decide
                            // click-to-snap vs drag-to-rect.
                            if snap::drag_threshold_exceeded(start, self.cursor) {
                                // Promote: use press position as the rect's start.
                                self.state = OverlayState::Dragging {
                                    start,
                                    end: self.cursor,
                                };
                                self.snap_target = None;
                                self.press_pos = None;
                                self.snap_at_press = None;
                                self.window.request_redraw();
                            }
                        } else {
                            // Hover-snap: recompute snap_target from cursor.
                            let new_target = snap::window_under_cursor(
                                self.cursor,
                                &self.window_list,
                            );
                            if new_target != self.snap_target {
                                self.snap_target = new_target;
                                self.window.request_redraw();
                            }
                            // Magnifier tick — preserves Iter 2a behavior.
                            self.request_redraw_throttled();
                        }
                    }
```

The `OverlayState::Dragging` and `OverlayState::Adjusting` arms below this stay unchanged.

- [ ] **Step 2: Build + test**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile. Test count unchanged. No behavior visible yet because `press_pos` is never set to `Some` (Task 7 wires the press handler). Hover-snap is now active in Idle; manual smoke at Task 11.

- [ ] **Step 3: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): CursorMoved Idle hover-snap + drag-threshold detection

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Mouse press/release routing

The Idle arm of `handle_left_press` no longer transitions to `Dragging` on press. Instead it captures `press_pos` and `snap_at_press`. `handle_left_release` checks for that captured state and either commits the snap to `Adjusting` or returns to true Idle. If the cursor moved past 4 px during press (CursorMoved promoted to Dragging in Task 6), release falls through to existing Dragging logic.

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Replace the Idle arm of `handle_left_press`**

Find `handle_left_press` in `src/overlay/mod.rs` (around line 316). The body has a `match self.state` with `OverlayState::Idle` calling `state::on_mouse_down_idle` (line 335-339). Replace that arm:

```rust
            OverlayState::Idle => {
                // Defer state transition until release. If the cursor moves
                // ≥ 4 px before release, CursorMoved promotes to Dragging
                // (Task 6). If release happens with no movement, we treat it
                // as a click — commit snap_target to Adjusting (Step 2 below).
                self.press_pos = Some(self.cursor);
                self.snap_at_press = self.snap_target;
                Outcome::Continue
            }
```

- [ ] **Step 2: Update `handle_left_release`**

Find `handle_left_release` (around line 444). Insert a snap-commit block AFTER the `pending_draw` early-return and BEFORE the `match self.state { ... }`:

```rust
    fn handle_left_release(&mut self) -> Outcome {
        // Commit an in-flight annotation draw first.
        if let Some(pending) = self.pending_draw.take() {
            if let Some(ann) = pending.finalize() {
                self.history.push(ann);
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }

        // Snap-click commit: if a press happened in Idle and CursorMoved never
        // promoted to Dragging (drag threshold not exceeded), treat this as a
        // click. Snap to the window the cursor was over at press time.
        if let Some(_press) = self.press_pos.take() {
            if let Some(rect) = self.snap_at_press.take() {
                self.state = OverlayState::Adjusting { rect, edit: None };
                self.snap_target = None;  // clear stale hover highlight
                self.window.request_redraw();
            }
            // Else: clicked on dim area (no window under cursor at press time)
            // — stay Idle, no transition.
            return Outcome::Continue;
        }

        match self.state {
            // ... existing Dragging / Adjusting arms unchanged ...
```

(Keep the existing match block below this insertion as-is — it handles releases from `Dragging` and `Adjusting` whose `press_pos` is already cleared.)

- [ ] **Step 3: Build + test**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass. Behavior is now functionally complete — manual smoke at Task 11.

- [ ] **Step 4: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): mouse press defers to release; snap-click commits to Adjusting

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Snap preview rendering (blue outline + dim)

When `state == Idle && snap_target.is_some()`, render the snap target rect using `apply_dim` (already exists) and `draw_selection_outline` (already exists). Use blue (`#007AFF`) instead of white to distinguish snap preview from confirmed selection.

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Compute the preview rect + outline color**

Find the `redraw` function (around line 685). The existing code computes `sel_tuple` and `sel_rect` from `current_selection_rect()` (lines 691-692), which return `None` when state is `Idle`. We need to override that for the snap-preview case.

After the existing `let sel_rect = self.current_selection_rect();` line (line 692), add:

```rust
        // Snap preview: when Idle and hovering a window, render that window's
        // bounds the same way as a confirmed selection but with a blue outline.
        let (sel_rect, sel_tuple, outline_color) = match (sel_rect, &self.state, self.snap_target) {
            (None, OverlayState::Idle, Some(snap)) => {
                let clamped = snap.clamp_to((w, h));
                if clamped.w == 0 || clamped.h == 0 {
                    (None, None, 0x00FFFFFF)
                } else {
                    let tup = clamped.as_tuple_u32();
                    (Some(clamped), Some(tup), 0x00007AFF) // Color::Blue
                }
            }
            (existing, _, _) => (existing, sel_tuple, 0x00FFFFFF),
        };
```

This shadows the original `sel_rect` and `sel_tuple` to also account for the snap-preview case. `outline_color` is new.

- [ ] **Step 2: Use `outline_color` in `draw_selection_outline`**

Find the existing `draw_selection_outline` call (around line 716):

```rust
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }
```

Replace `0x00FFFFFF` with `outline_color`:

```rust
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, outline_color);
        }
```

- [ ] **Step 3: Build + test**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 4: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): blue outline + dim for snap preview in Idle state

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: CursorLeft handler

When the cursor leaves the overlay window, clear `snap_target` so the highlight goes away. (winit fires `WindowEvent::CursorLeft` on macOS when the cursor exits the window's bounds.)

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add a `WindowEvent::CursorLeft` arm**

In `Overlay::handle_event`, find the existing `match event { ... }` block. Add a new arm BEFORE the catch-all `_ => Outcome::Continue`:

```rust
            WindowEvent::CursorLeft { .. } => {
                if self.snap_target.is_some() {
                    self.snap_target = None;
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
```

Place it near `WindowEvent::CursorMoved` for readability — they're both cursor-position events.

- [ ] **Step 2: Build + test**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 3: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): clear snap_target when cursor leaves window

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 10: Cleanup snap state on transitions back to Idle

When `Outcome::Cancelled` fires from Esc or close, the overlay tears down — no cleanup needed. But two existing transitions revert state to `OverlayState::Idle` mid-capture and would leave `snap_target / press_pos / snap_at_press` set with stale values:

- `handle_left_release` Dragging arm with `rect.w == 0 || rect.h == 0` → resets to Idle (line 460)
- Various `Adjusting` arms reset to `Idle` on click-outside (line 435)

These existing resets need to also clear the snap state.

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Find the two reset sites and clear snap state**

In `handle_left_release`, around line 460:

```rust
            OverlayState::Dragging { start, end } => {
                let rect = Rect::normalize(start, end);
                if rect.w > 0 && rect.h > 0 {
                    self.state = OverlayState::Adjusting { rect, edit: None };
                } else {
                    self.state = OverlayState::Idle;
                    // Snap state could have lingered if Dragging was promoted
                    // mid-press; clear so hover-snap restarts cleanly.
                    self.snap_target = None;
                }
                self.window.request_redraw();
            }
```

In `handle_left_press` `Adjusting` arm (around line 435 — search for `self.state = OverlayState::Idle;` inside the Adjusting branch):

```rust
                    hit::HitZone::Outside => {
                        self.state = OverlayState::Idle;
                        self.snap_target = None;
                        self.press_pos = None;
                        self.snap_at_press = None;
                        self.window.request_redraw();
                    }
```

Note: there may be other places that reassign to `OverlayState::Idle`. Search for `state = OverlayState::Idle` and add the same triple-clear pattern at each site that doesn't already have it. The implementer should grep:

```
grep -n "state = OverlayState::Idle" src/overlay/mod.rs
```

For each hit not already covered, add `self.snap_target = None; self.press_pos = None; self.snap_at_press = None;` (three lines) before the `self.window.request_redraw();`.

- [ ] **Step 2: Build + test**

```
cargo build
cargo test -p quickshot
```

Expected: clean compile, all tests pass.

- [ ] **Step 3: Commit**

```
git add src/overlay/mod.rs
git commit -m "fix(overlay): clear snap state on every Idle reassignment

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 11: Final smoke + verification

Manual GUI smoke test of the full snap flow plus the existing flows to confirm no regressions.

**Files:** none (verification only).

- [ ] **Step 1: Full test suite**

```
cargo test -p quickshot
```

Expected: all green (≥ 124 tests — 4 new pure tests in snap.rs + 4 drag_threshold tests + 1 macOS smoke = 9 new; 124 prior + 9 = 133, but counts may diverge if the implementer wrote fewer / more tests).

- [ ] **Step 2: Build release**

```
cargo build --release
```

Expected: clean compile, only pre-existing `clashing_extern_declarations` warnings.

- [ ] **Step 3: Stop any old quickshot, start the new build**

```
pkill -f 'quickshot' 2>/dev/null ; sleep 1 ; /Users/simmzl/Desktop/personal/quickshot/target/release/quickshot > /tmp/quickshot.log 2>&1 &
```

- [ ] **Step 4: Manual smoke matrix**

For each, hit the hotkey, do the action, confirm visual + saved PNG behavior.

- [ ] **Hover-snap basic**: Cmd+Shift+A → don't drag. Hover over Finder, then a Terminal window, then a code editor. Each window should highlight with a blue outline as the cursor moves over it.
- [ ] **Snap-click commits**: Hover a window → click. Should land in Adjusting state with the toolbar visible at the window's bottom edge. The snap rect matches the window.
- [ ] **Manual drag still works**: Cmd+Shift+A → press and drag (not just click). After ~4 px of motion, the snap highlight disappears and a normal red selection rect appears, sized to the drag.
- [ ] **Click empty area**: Cmd+Shift+A → click on the dim area where no window is. No state change (still Idle, no toolbar).
- [ ] **Snap → annotate → confirm**: Snap to a window → switch to red Arrow (`A`) → drag an arrow → Enter. Saved PNG should show the snapped window with the red arrow overlaid.
- [ ] **Iter 5b regressions check**: Snap to a window → press `T` → click → type "你好" → Enter. Confirm CJK still renders, color/stroke shortcuts (`1234`/`[`/`]`) still work, `h` hint badges still appear.
- [ ] **Cursor leaves overlay**: Cmd+Shift+A → move cursor to top edge of screen so it leaves the overlay. The blue highlight should disappear. Move back in — highlight returns when over a window.
- [ ] **Esc cancels**: Hover a window → Esc. Overlay closes, no clipboard change.
- [ ] **Multi-monitor (if available)**: Cmd+Shift+A on monitor 2 — the overlay covers monitor 2 only, snap targets are windows on monitor 2 only.

- [ ] **Step 5: Stop the dev binary**

```
pkill -f 'target/release/quickshot' 2>/dev/null
```

- [ ] **Step 6: If all green, commit a marker (optional)**

```
git tag v0.8.0-iter6a -m "iter6a: smart window snapping"
```

(Don't push the tag without user confirmation.)

- [ ] **Step 7: If any smoke item fails**

Stop. Diagnose. Fix as a follow-up commit. Re-run the full smoke matrix before declaring done.

---

## Summary

Total tasks: 11. Estimated effort: 1.5–2.5 days.

The plan is deliberately layered so each task ends in a clean compile and a meaningful commit. Tasks 1–4 build the `snap.rs` module without touching `Overlay`. Tasks 5–7 wire `Overlay`'s state and event handling. Tasks 8–10 polish rendering and edge cases. Task 11 is human-driven smoke validation. By the final commit, every requirement in `docs/superpowers/specs/2026-05-10-quickshot-iter6a-design.md` has either an implementation site or an explicit Non-Goal entry.
