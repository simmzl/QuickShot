# quickshot Iter 5b — Annotation Toolkit Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the in-overlay annotation surface to parity with WeChat / CleanShot — 4 colors, 3 stroke widths, Pen (free-form), Text (inline editing) — while keeping all existing Iter 5a behaviors intact.

**Architecture:** Extend, don't restructure. The existing `annotate.rs` / `annotate_render.rs` / `toolbar.rs` modules grow:
- `annotate.rs` — `Color`, `Stroke`, `AnnotationStyle` enums; new `Pen` and `Text` variants; `PendingDraw` becomes an enum (`Shape` vs `Pen`) so the live-preview path can carry a point list.
- `annotate_render.rs` — every render site reads `style.color.argb()` / `style.stroke.px()` instead of the file-scope constants. Two new paint helpers: pen path + text rasterizer (delegating to the existing `text.rs` font).
- `toolbar.rs` — single-row layout becomes two-row; new `ToolbarHit::Color(Color)` / `Stroke(Stroke)` variants; new Pen / Text icons.
- `overlay/mod.rs` — `Overlay` gains `current_style: AnnotationStyle` and `text_edit: Option<TextEdit>`; the `ANNOTATION_ARGB` file-scope constant is deleted; keyboard handler picks up `P / T / 1..4 / [ / ]`; mouse handlers route Pen and Text clicks.

**Tech Stack:** Existing only. No new crates.

**Spec:** `docs/superpowers/specs/2026-05-09-quickshot-iter5b-design.md`

**Scope for this plan:**
- 4 colors: Red `#FF3B30` / Yellow `#FFCC00` / Green `#34C759` / Blue `#007AFF`
- 3 stroke widths: Thin (2 px) / Medium (4 px, default) / Thick (6 px)
- Pen tool with raw connect-the-dots, 1 px sample dedup
- Text tool with point-and-type, multi-line, blinking caret
- Two-row toolbar with selection rings; row 2 greys out under Mosaic
- Keyboard: `P / T` switch tool, `1..4` color, `[ / ]` stroke step

**Not in this plan:** smart window snapping, pin-to-desktop, long screenshot, OCR, per-annotation selection — see the spec's Non-Goals section.

---

## File Structure

```
src/overlay/
├── annotate.rs              (modified — add Color/Stroke/AnnotationStyle, Pen+Text variants, PendingDraw → enum)
├── annotate_render.rs       (modified — read style; add paint_pen_*, paint_text_*)
├── toolbar.rs               (modified — Tool::Pen+Text, ToolbarHit::Color+Stroke, two-row layout, new icons)
├── mod.rs                   (modified — current_style field, text_edit state, key/click routing, delete ANNOTATION_ARGB)
├── state.rs                 (unchanged)
├── hit.rs                   (unchanged)
└── render.rs                (unchanged)

src/text.rs                  (unchanged — used by paint_text_*)
```

---

## Task 1: Color, Stroke, AnnotationStyle pure types

Pure-types task. No wiring yet — just the new enums + helpers + tests.

**Files:**
- Modify: `src/overlay/annotate.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src/overlay/annotate.rs` inside `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn color_argb_values() {
        assert_eq!(Color::Red.argb(),    0x00_FF_3B_30);
        assert_eq!(Color::Yellow.argb(), 0x00_FF_CC_00);
        assert_eq!(Color::Green.argb(),  0x00_34_C7_59);
        assert_eq!(Color::Blue.argb(),   0x00_00_7A_FF);
    }

    #[test]
    fn color_rgba_values() {
        assert_eq!(Color::Red.rgba(),    [0xFF, 0x3B, 0x30, 0xFF]);
        assert_eq!(Color::Blue.rgba(),   [0x00, 0x7A, 0xFF, 0xFF]);
    }

    #[test]
    fn stroke_px_values() {
        assert_eq!(Stroke::Thin.px(),    2);
        assert_eq!(Stroke::Medium.px(),  4);
        assert_eq!(Stroke::Thick.px(),   6);
    }

    #[test]
    fn stroke_font_px_values() {
        assert_eq!(Stroke::Thin.font_px(),    14);
        assert_eq!(Stroke::Medium.font_px(),  20);
        assert_eq!(Stroke::Thick.font_px(),   28);
    }

    #[test]
    fn stroke_step() {
        assert_eq!(Stroke::Thin.step_up(),   Stroke::Medium);
        assert_eq!(Stroke::Medium.step_up(), Stroke::Thick);
        assert_eq!(Stroke::Thick.step_up(),  Stroke::Thick);  // clamped
        assert_eq!(Stroke::Thick.step_down(),  Stroke::Medium);
        assert_eq!(Stroke::Medium.step_down(), Stroke::Thin);
        assert_eq!(Stroke::Thin.step_down(),   Stroke::Thin); // clamped
    }

    #[test]
    fn annotation_style_default_is_red_medium() {
        let s = AnnotationStyle::default();
        assert_eq!(s.color,  Color::Red);
        assert_eq!(s.stroke, Stroke::Medium);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p quickshot --lib annotate::tests::color_argb_values
```

Expected: FAIL — `Color` not found.

- [ ] **Step 3: Implement the new types**

Insert into `src/overlay/annotate.rs`, **before** `pub enum Annotation`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Yellow,
    Green,
    Blue,
}

impl Color {
    /// softbuffer ARGB layout: 0x00RRGGBB.
    pub fn argb(self) -> u32 {
        match self {
            Color::Red    => 0x00_FF_3B_30,
            Color::Yellow => 0x00_FF_CC_00,
            Color::Green  => 0x00_34_C7_59,
            Color::Blue   => 0x00_00_7A_FF,
        }
    }
    /// `image::Rgba<u8>` payload.
    pub fn rgba(self) -> [u8; 4] {
        let argb = self.argb();
        [
            ((argb >> 16) & 0xFF) as u8,
            ((argb >> 8)  & 0xFF) as u8,
            ( argb        & 0xFF) as u8,
            0xFF,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Thin,
    Medium,
    Thick,
}

impl Stroke {
    pub fn px(self) -> i32 {
        match self { Self::Thin => 2, Self::Medium => 4, Self::Thick => 6 }
    }
    pub fn font_px(self) -> f32 {
        match self { Self::Thin => 14.0, Self::Medium => 20.0, Self::Thick => 28.0 }
    }
    pub fn step_up(self) -> Self {
        match self { Self::Thin => Self::Medium, Self::Medium => Self::Thick, Self::Thick => Self::Thick }
    }
    pub fn step_down(self) -> Self {
        match self { Self::Thick => Self::Medium, Self::Medium => Self::Thin, Self::Thin => Self::Thin }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationStyle {
    pub color: Color,
    pub stroke: Stroke,
}

impl Default for AnnotationStyle {
    fn default() -> Self {
        Self { color: Color::Red, stroke: Stroke::Medium }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p quickshot --lib annotate::tests
```

Expected: all tests in `annotate::tests` PASS (existing + 6 new).

- [ ] **Step 5: Commit**

```
git add src/overlay/annotate.rs
git commit -m "feat(annotate): Color/Stroke/AnnotationStyle pure types"
```

---

## Task 2: Annotation migration — add `style` field, drop `Copy`, switch renderers to `&Annotation`

Structural refactor. Each existing variant gets a `style: AnnotationStyle` field. `Annotation` was `Copy`; that property is preserved (`Color`, `Stroke`, `AnnotationStyle` are all `Copy`). `PendingDraw` likewise gains `style` and stays `Copy` for now (Pen variant comes in Task 3, that's when we drop Copy from `PendingDraw`). All Iter 5a tests must continue to pass after this task.

**Files:**
- Modify: `src/overlay/annotate.rs`
- Modify: `src/overlay/annotate_render.rs`
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Update `Annotation` and `PendingDraw`**

In `src/overlay/annotate.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Arrow   { from: (i32, i32), to: (i32, i32), style: AnnotationStyle },
    Rect    { rect: Rect, style: AnnotationStyle },
    Ellipse { rect: Rect, style: AnnotationStyle },
    Mosaic  { rect: Rect, block_size: u32 },               // unchanged — Mosaic ignores style
}

#[derive(Debug, Clone, Copy)]
pub struct PendingDraw {
    pub tool: Tool,
    pub from_frame: (i32, i32),
    pub to_frame: (i32, i32),
    pub style: AnnotationStyle,
}

impl PendingDraw {
    pub fn finalize(self) -> Option<Annotation> {
        match self.tool {
            Tool::Move => None,
            Tool::Arrow => Some(Annotation::Arrow {
                from: self.from_frame, to: self.to_frame, style: self.style,
            }),
            Tool::Rect => Some(Annotation::Rect {
                rect: Rect::normalize(self.from_frame, self.to_frame),
                style: self.style,
            }),
            Tool::Ellipse => Some(Annotation::Ellipse {
                rect: Rect::normalize(self.from_frame, self.to_frame),
                style: self.style,
            }),
            Tool::Mosaic => Some(Annotation::Mosaic {
                rect: Rect::normalize(self.from_frame, self.to_frame),
                block_size: 8,
            }),
        }
    }
}
```

- [ ] **Step 2: Update existing tests in `annotate.rs`**

Every existing test that constructs an `Annotation::Arrow / Rect / Ellipse` or a `PendingDraw` needs the new field. Replace each affected literal:

`Annotation::Arrow { from, to }` → `Annotation::Arrow { from, to, style: AnnotationStyle::default() }`

`Annotation::Rect { rect }` → `Annotation::Rect { rect, style: AnnotationStyle::default() }`

`Annotation::Ellipse { rect }` → `Annotation::Ellipse { rect, style: AnnotationStyle::default() }`

`PendingDraw { tool, from_frame, to_frame }` → `PendingDraw { tool, from_frame, to_frame, style: AnnotationStyle::default() }`

Update the `finalize_arrow`, `finalize_rect_normalizes`, `finalize_ellipse_normalizes` assertions to expect the new variants with `style: AnnotationStyle::default()`.

- [ ] **Step 3: Update `annotate_render.rs` to read `style`**

Delete the file-scope `ANNOTATION_COLOR_RGBA` and `ANNOTATION_THICKNESS` constants (`src/overlay/annotate_render.rs:11-13`). Replace `paint_on_cropped` to dispatch with the per-annotation style:

```rust
pub fn paint_on_cropped(img: &mut RgbaImage, ann: Annotation, crop_offset: (i32, i32)) {
    let (ox, oy) = crop_offset;
    match ann {
        Annotation::Arrow { from, to, style } => {
            draw_arrow_on_image(
                img,
                (from.0 - ox, from.1 - oy),
                (to.0 - ox, to.1 - oy),
                Rgba(style.color.rgba()),
                style.stroke.px(),
            );
        }
        Annotation::Rect { rect, style } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_rect_outline_on_image(img, local, Rgba(style.color.rgba()), style.stroke.px());
        }
        Annotation::Ellipse { rect, style } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse_outline_on_image(img, local, Rgba(style.color.rgba()), style.stroke.px());
        }
        Annotation::Mosaic { rect, block_size } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            apply_mosaic_on_image(img, local, block_size);
        }
    }
}
```

In `draw_annotation_on_buf` (around line 319), drop the `color_argb` parameter and read color + thickness from the variant:

```rust
pub fn draw_annotation_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    ann: Annotation,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    match ann {
        Annotation::Arrow { from, to, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let f = frame_to_window(from, frame_size, window_size);
            let t = frame_to_window(to, frame_size, window_size);
            let shaft_end = shaft_end_point(f, t, ARROWHEAD_LEN as f64);
            draw_line_thick_buf(buf, win_w, win_h, f, shaft_end, style.color.argb(), thickness_win);
            draw_arrowhead_buf(buf, win_w, win_h, f, t, style.color.argb());
        }
        Annotation::Rect { rect, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window((rect.x + rect.w - 1, rect.y + rect.h - 1), frame_size, window_size);
            draw_rect_outline_buf(buf, win_w, win_h, tl, br, style.color.argb(), thickness_win);
        }
        Annotation::Ellipse { rect, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window((rect.x + rect.w - 1, rect.y + rect.h - 1), frame_size, window_size);
            draw_ellipse_outline_buf(buf, win_w, win_h, tl, br, style.color.argb(), thickness_win);
        }
        Annotation::Mosaic { rect, block_size } => {
            apply_mosaic_on_buf(buf, win_w, win_h, frame, rect, block_size);
        }
    }
}

pub fn draw_pending_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    pending: PendingDraw,
) {
    if let Some(ann) = pending.finalize() {
        draw_annotation_on_buf(buf, win_w, win_h, frame, ann);
    }
}
```

The `ARROWHEAD_LEN` constant stays — it's geometry, not style. Drop the `pub const ANNOTATION_THICKNESS: i32 = 6;` and `pub const ANNOTATION_COLOR_RGBA` lines completely.

- [ ] **Step 4: Update `mod.rs` callers + delete `ANNOTATION_ARGB`**

In `src/overlay/mod.rs:22`, delete `const ANNOTATION_ARGB: u32 = 0x00_FF_3B_30;`.

Around line 467, the redraw loop:

```rust
for ann in self.history.current() {
    annotate_render::draw_annotation_on_buf(
        &mut buf, w, h, frame_ref, *ann,
    );
}
if let Some(pending) = self.pending_draw {
    annotate_render::draw_pending_on_buf(
        &mut buf, w, h, frame_ref, pending,
    );
}
```

(removed the trailing `ANNOTATION_ARGB` argument; everything else identical).

In `handle_left_press` around line 267, the `PendingDraw` constructor needs the `style` field. Use a placeholder default for now — Task 8 wires it to `self.current_style`:

```rust
self.pending_draw = Some(PendingDraw {
    tool: self.tool,
    from_frame: fp,
    to_frame: fp,
    style: AnnotationStyle::default(),
});
```

Add `use annotate::AnnotationStyle;` near the top of `mod.rs` if not already imported; the existing import line is `use annotate::PendingDraw;`.

- [ ] **Step 5: Run the full test suite**

```
cargo test -p quickshot --lib
```

Expected: all tests pass — old behavior preserved.

- [ ] **Step 6: Commit**

```
git add src/overlay/
git commit -m "refactor(annotate): per-annotation AnnotationStyle (color + stroke)"
```

---

## Task 3: `Annotation::Pen` + `PendingDraw` becomes an enum

`Pen` carries a `Vec<(i32, i32)>` so `Annotation` can no longer be `Copy`; downstream borrows need to switch from value-copy to reference. `PendingDraw` similarly drops `Copy` and becomes an enum (Shape vs Pen).

**Files:**
- Modify: `src/overlay/annotate.rs`
- Modify: `src/overlay/annotate_render.rs`
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Write failing tests for Pen finalize and pending extension**

Append to `annotate::tests`:

```rust
    #[test]
    fn pen_finalize_returns_pen_with_points() {
        let mut p = PendingDraw::pen(AnnotationStyle::default(), (10, 20));
        p.push_point((11, 21));
        p.push_point((12, 22));
        let a = p.finalize().expect("pen finalize");
        match a {
            Annotation::Pen { ref points, style } => {
                assert_eq!(points, &vec![(10, 20), (11, 21), (12, 22)]);
                assert_eq!(style, AnnotationStyle::default());
            }
            _ => panic!("expected Pen"),
        }
    }

    #[test]
    fn pen_dedup_filters_subpixel() {
        let mut p = PendingDraw::pen(AnnotationStyle::default(), (10, 10));
        p.push_point((10, 10));   // exact dup, dropped
        p.push_point((10, 11));   // 1 px Δ kept
        p.push_point((10, 11));   // dup dropped
        let a = p.finalize().unwrap();
        match a {
            Annotation::Pen { points, .. } => assert_eq!(points, vec![(10, 10), (10, 11)]),
            _ => panic!(),
        }
    }

    #[test]
    fn pen_finalize_single_point_returns_none() {
        // No drag (mouse press-release without movement) — discard.
        let p = PendingDraw::pen(AnnotationStyle::default(), (10, 10));
        assert!(p.finalize().is_none());
    }

    #[test]
    fn shape_finalize_unchanged_after_enum_conversion() {
        let p = PendingDraw::shape(Tool::Rect, AnnotationStyle::default(), (5, 5), (15, 25));
        match p.finalize().unwrap() {
            Annotation::Rect { rect, .. } => {
                assert_eq!(rect, Rect { x: 5, y: 5, w: 10, h: 20 });
            }
            _ => panic!(),
        }
    }
```

- [ ] **Step 2: Run tests, expect FAIL** (no `PendingDraw::pen` constructor, no `Annotation::Pen`).

```
cargo test -p quickshot --lib annotate::tests::pen_finalize_returns_pen_with_points
```

Expected: FAIL.

- [ ] **Step 3: Replace `Annotation` and `PendingDraw` definitions**

In `src/overlay/annotate.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Arrow   { from: (i32, i32), to: (i32, i32), style: AnnotationStyle },
    Rect    { rect: Rect, style: AnnotationStyle },
    Ellipse { rect: Rect, style: AnnotationStyle },
    Mosaic  { rect: Rect, block_size: u32 },
    Pen     { points: Vec<(i32, i32)>, style: AnnotationStyle },
}

#[derive(Debug, Clone)]
pub enum PendingDraw {
    Shape {
        tool: Tool,
        from_frame: (i32, i32),
        to_frame: (i32, i32),
        style: AnnotationStyle,
    },
    Pen {
        points: Vec<(i32, i32)>,
        style: AnnotationStyle,
    },
}

impl PendingDraw {
    pub fn shape(tool: Tool, style: AnnotationStyle, from: (i32, i32), to: (i32, i32)) -> Self {
        Self::Shape { tool, from_frame: from, to_frame: to, style }
    }

    pub fn pen(style: AnnotationStyle, first: (i32, i32)) -> Self {
        Self::Pen { points: vec![first], style }
    }

    /// For Shape: replaces `to_frame`. For Pen: pushes the point if it's at
    /// least 1 px (Manhattan) from the last sample (drops sub-pixel dupes).
    pub fn extend_to(&mut self, p: (i32, i32)) {
        match self {
            PendingDraw::Shape { to_frame, .. } => *to_frame = p,
            PendingDraw::Pen   { points, .. } => {
                let keep = points.last().is_none_or(|&last| {
                    (last.0 - p.0).abs() + (last.1 - p.1).abs() >= 1
                });
                if keep { points.push(p); }
            }
        }
    }

    pub fn push_point(&mut self, p: (i32, i32)) { self.extend_to(p) }

    pub fn finalize(self) -> Option<Annotation> {
        match self {
            PendingDraw::Shape { tool, from_frame, to_frame, style } => match tool {
                Tool::Move | Tool::Pen | Tool::Text => None,
                Tool::Arrow => Some(Annotation::Arrow { from: from_frame, to: to_frame, style }),
                Tool::Rect => Some(Annotation::Rect {
                    rect: Rect::normalize(from_frame, to_frame), style,
                }),
                Tool::Ellipse => Some(Annotation::Ellipse {
                    rect: Rect::normalize(from_frame, to_frame), style,
                }),
                Tool::Mosaic => Some(Annotation::Mosaic {
                    rect: Rect::normalize(from_frame, to_frame), block_size: 8,
                }),
            },
            PendingDraw::Pen { points, style } => {
                if points.len() < 2 { None } else { Some(Annotation::Pen { points, style }) }
            }
        }
    }
}
```

(Note: this introduces `Tool::Pen` and `Tool::Text` as match arms in `finalize`; add them now to the `Tool` enum:)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move, Arrow, Rect, Ellipse, Mosaic, Pen, Text,
}

impl Tool {
    pub fn is_drawing(self) -> bool {
        !matches!(self, Tool::Move)
    }
}
```

- [ ] **Step 4: Update existing Iter 5a tests in `annotate.rs`**

The Iter 5a tests `finalize_move_returns_none`, `finalize_arrow`, `finalize_rect_normalizes`, `finalize_ellipse_normalizes`, `finalize_mosaic_has_block_size_8` all build `PendingDraw` literals — switch them to `PendingDraw::shape(tool, AnnotationStyle::default(), from, to)`.

The `is_drawing_variants` test should now also assert `Tool::Pen.is_drawing()` and `Tool::Text.is_drawing()`.

- [ ] **Step 5: Update `annotate_render.rs` for the enum + Annotation owning Vec**

Change `paint_on_cropped` and `draw_annotation_on_buf` to take `&Annotation` (not by value), since `Annotation` is no longer `Copy`. The `Pen` arm panics-`unimplemented!()` for now — Task 4 wires it.

`paint_on_cropped` signature:

```rust
pub fn paint_on_cropped(img: &mut RgbaImage, ann: &Annotation, crop_offset: (i32, i32)) {
    // ... match on `ann` instead of consuming ...
    // For Pen: `unimplemented!("Pen render in Task 4")`
}
```

`draw_annotation_on_buf` signature changes to `ann: &Annotation`. Same `unimplemented!` for the Pen arm.

`draw_pending_on_buf` takes `pending: &PendingDraw`. It dispatches by variant:

```rust
pub fn draw_pending_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32, frame: &RgbaImage,
    pending: &PendingDraw,
) {
    match pending {
        PendingDraw::Shape { .. } => {
            // Re-use finalize → Annotation, then draw. Cheap clone for Shape.
            if let Some(ann) = pending.clone().finalize() {
                draw_annotation_on_buf(buf, win_w, win_h, frame, &ann);
            }
        }
        PendingDraw::Pen { .. } => {
            // Wired in Task 4.
            unimplemented!("Pen pending render in Task 4");
        }
    }
}
```

- [ ] **Step 6: Update `mod.rs` callers**

`src/overlay/mod.rs` line ~146 (CursorMoved handler updating pending draw):

```rust
if self.pending_draw.is_some() {
    let fp = self.window_point_to_frame_point(self.cursor);
    if let Some(p) = self.pending_draw.as_mut() {
        p.extend_to(fp);
    }
    self.request_redraw_throttled();
    return Outcome::Continue;
}
```

Line ~267 (Pen-tool branch — still using Shape until Task 9 actually handles Pen flow; for now route Pen the same as Shape, Task 9 will swap):

```rust
self.pending_draw = Some(PendingDraw::shape(
    self.tool, AnnotationStyle::default(), fp, fp,
));
```

(Note: when `self.tool == Tool::Pen`, Shape::finalize returns `None` — no annotation is committed. Task 9 fixes this.)

Lines ~466 + ~476 (redraw loop): switch to references:

```rust
for ann in self.history.current() {
    annotate_render::draw_annotation_on_buf(&mut buf, w, h, frame_ref, ann);
}
if let Some(pending) = self.pending_draw.as_ref() {
    annotate_render::draw_pending_on_buf(&mut buf, w, h, frame_ref, pending);
}
```

`self.history.current()` already returns `&[Annotation]`, so `ann` is now `&Annotation` automatically.

Line ~298 (`handle_left_release` taking pending): change `if let Some(pending) = self.pending_draw.take()` — already correct since we own it via `take()`.

Line ~410 (`flatten_for_export`):

```rust
for ann in self.history.current() {
    annotate_render::paint_on_cropped(&mut cropped, ann, offset);
}
```

(Drop the `*ann` deref since `paint_on_cropped` now takes `&Annotation`.)

- [ ] **Step 7: `cargo build` to confirm compile-clean (Pen/Text branches still `unimplemented!`)**

```
cargo build
```

Expected: success. Pen pending will panic at runtime if reached, but the existing flow (Arrow/Rect/Ellipse/Mosaic) doesn't hit Pen branches yet.

- [ ] **Step 8: Run all tests**

```
cargo test -p quickshot --lib
```

Expected: PASS — Pen unit tests now satisfied; existing tests adapted to new constructor.

- [ ] **Step 9: Commit**

```
git add src/overlay/
git commit -m "feat(annotate): Annotation::Pen + PendingDraw enum (Shape | Pen)"
```

---

## Task 4: Pen render — `paint_pen_on_image` + `draw_pen_on_buf` + pending preview

**Files:**
- Modify: `src/overlay/annotate_render.rs`

- [ ] **Step 1: Write failing tests**

Append to the existing `#[cfg(test)] mod tests` in `annotate_render.rs`:

```rust
    #[test]
    fn pen_paints_visible_pixels_on_image() {
        let mut img = RgbaImage::new(50, 50);
        let style = AnnotationStyle::default();
        let points = vec![(5, 25), (45, 25)];
        paint_pen_on_image(&mut img, &points, style);
        // The horizontal line should mark pixels along y=25 with the red color.
        let any_red = (5..=45).any(|x| {
            let p = img.get_pixel(x, 25);
            p[0] >= 0xF0 && p[1] < 0x80 && p[2] < 0x80
        });
        assert!(any_red, "expected red pixels along the pen path");
    }

    #[test]
    fn pen_with_under_two_points_is_noop() {
        let mut img = RgbaImage::new(20, 20);
        let style = AnnotationStyle::default();
        paint_pen_on_image(&mut img, &[(10, 10)], style);
        for p in img.pixels() {
            assert_eq!(*p, Rgba([0, 0, 0, 0]), "single-point pen should not paint");
        }
    }
```

- [ ] **Step 2: Run, expect FAIL**

```
cargo test -p quickshot --lib annotate_render::tests::pen
```

Expected: FAIL — `paint_pen_on_image` not found.

- [ ] **Step 3: Implement Pen helpers**

In `src/overlay/annotate_render.rs`, add public helpers:

```rust
pub fn paint_pen_on_image(img: &mut RgbaImage, points: &[(i32, i32)], style: AnnotationStyle) {
    if points.len() < 2 { return; }
    let color = Rgba(style.color.rgba());
    let thickness = style.stroke.px();
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        draw_segment_on_image(img, a, b, color, thickness);
    }
}

fn draw_segment_on_image(
    img: &mut RgbaImage, a: (i32, i32), b: (i32, i32),
    color: Rgba<u8>, thickness: i32,
) {
    // Reuse `draw_arrow_on_image`'s line algorithm by carving out a
    // segment-only helper. If you don't want to refactor draw_arrow_on_image,
    // duplicate its inner loop here — both forms are short.
    let (w, h) = (img.width() as i32, img.height() as i32);
    let (fx, fy) = (a.0 as f64, a.1 as f64);
    let (tx, ty) = (b.0 as f64, b.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let cx = fx + dx * t;
        let cy = fy + dy * t;
        // Disk fill at (cx, cy) with radius r, into the RgbaImage.
        let r_ceil = r.ceil() as i32;
        let r2 = r * r;
        for ddy in -r_ceil..=r_ceil {
            for ddx in -r_ceil..=r_ceil {
                let fxd = ddx as f64;
                let fyd = ddy as f64;
                if fxd*fxd + fyd*fyd <= r2 {
                    let x = cx as i32 + ddx;
                    let y = cy as i32 + ddy;
                    if x >= 0 && y >= 0 && x < w && y < h {
                        img.put_pixel(x as u32, y as u32, color);
                    }
                }
            }
        }
    }
}
```

For the buf path:

```rust
pub fn draw_pen_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32,
    frame: &RgbaImage, points: &[(i32, i32)], style: AnnotationStyle,
) {
    if points.len() < 2 { return; }
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
    let argb = style.color.argb();
    for w in points.windows(2) {
        let a = frame_to_window(w[0], frame_size, window_size);
        let b = frame_to_window(w[1], frame_size, window_size);
        draw_line_thick_buf(buf, win_w, win_h, a, b, argb, thickness_win);
    }
}
```

Wire the dispatch in `paint_on_cropped` and `draw_annotation_on_buf`: replace each `Annotation::Pen { .. } => unimplemented!(...)` with the appropriate helper call:

```rust
// in paint_on_cropped:
Annotation::Pen { points, style } => {
    let translated: Vec<(i32, i32)> =
        points.iter().map(|&(x, y)| (x - ox, y - oy)).collect();
    paint_pen_on_image(img, &translated, *style);
}

// in draw_annotation_on_buf:
Annotation::Pen { points, style } => {
    draw_pen_on_buf(buf, win_w, win_h, frame, points, *style);
}
```

In `draw_pending_on_buf`, swap the `unimplemented!` for the Pen branch with:

```rust
PendingDraw::Pen { points, style } => {
    draw_pen_on_buf(buf, win_w, win_h, frame, points, *style);
}
```

- [ ] **Step 4: Run, expect PASS**

```
cargo test -p quickshot --lib annotate_render
```

- [ ] **Step 5: Commit**

```
git add src/overlay/annotate_render.rs
git commit -m "feat(annotate_render): paint_pen_on_image + draw_pen_on_buf"
```

---

## Task 5: `Annotation::Text` + paint_text helpers

The Font lives in `src/text.rs`. Annotation::Text owns the `String`. Render path delegates rasterization to `Font::render_text` for the buf path. For the image path we replicate the blit (the `RgbaImage` is the export target — paint at the `style.color` ARGB, alpha-blend over existing pixels).

**Files:**
- Modify: `src/overlay/annotate.rs`
- Modify: `src/overlay/annotate_render.rs`

- [ ] **Step 1: Write failing test**

Append to `annotate::tests`:

```rust
    #[test]
    fn text_annotation_round_trip() {
        let mut h = History::new();
        let a = Annotation::Text {
            origin: (10, 20),
            content: "hi".to_string(),
            style: AnnotationStyle::default(),
        };
        h.push(a.clone());
        assert_eq!(h.current(), &[a]);
    }
```

- [ ] **Step 2: Run, expect FAIL** (`Annotation::Text` not found).

- [ ] **Step 3: Add `Annotation::Text` variant**

In `src/overlay/annotate.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Arrow   { from: (i32, i32), to: (i32, i32), style: AnnotationStyle },
    Rect    { rect: Rect, style: AnnotationStyle },
    Ellipse { rect: Rect, style: AnnotationStyle },
    Mosaic  { rect: Rect, block_size: u32 },
    Pen     { points: Vec<(i32, i32)>, style: AnnotationStyle },
    Text    { origin: (i32, i32), content: String, style: AnnotationStyle },
}
```

Update `PendingDraw::Shape::finalize` (`Tool::Text` arm already returns `None` from Task 3 — no PendingDraw involvement for Text; commits go through TextEdit in Task 10).

In `annotate_render.rs`, add Text arms to `paint_on_cropped` and `draw_annotation_on_buf` calling helpers we'll write in Step 4:

```rust
// paint_on_cropped:
Annotation::Text { origin, content, style } => {
    paint_text_on_image(img, (origin.0 - ox, origin.1 - oy), content, *style);
}
// draw_annotation_on_buf:
Annotation::Text { origin, content, style } => {
    draw_text_on_buf(buf, win_w, win_h, frame, *origin, content, *style);
}
```

- [ ] **Step 4: Implement `paint_text_on_image` + `draw_text_on_buf`**

`draw_text_on_buf` is the easy one — there's already a `Font` per overlay. Pass it in:

```rust
// signature changes — caller (mod.rs redraw) supplies the &mut Font.
// Add to top of annotate_render.rs: use crate::text::Font;

pub fn draw_text_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32,
    frame: &RgbaImage, origin_frame: (i32, i32), content: &str,
    style: AnnotationStyle, font: &mut Font,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let (px, py) = frame_to_window(origin_frame, frame_size, window_size);
    // Scale font size to window space (frame may be larger than window).
    let scale_y = window_size.1 as f32 / frame_size.1 as f32;
    let px_size = style.stroke.font_px() * scale_y;
    let argb = style.color.argb();
    // Multi-line: split on \n and stack vertically by line height (~1.2× px_size).
    let line_height = (px_size * 1.2).round() as i32;
    for (i, line) in content.split('\n').enumerate() {
        let ly = py + i as i32 * line_height;
        font.render_text(buf, win_w, win_h, px, ly, line, px_size, argb);
    }
}
```

For `paint_text_on_image`, use the same Font but render into the cropped RgbaImage by converting the rasterized glyphs to RGBA pixels. The simplest path: render into a temporary `Vec<u32>` softbuffer-style, then blit. To avoid allocation overhead, do a direct draw with a small per-glyph RGBA helper:

```rust
pub fn paint_text_on_image(
    img: &mut RgbaImage, origin: (i32, i32), content: &str,
    style: AnnotationStyle, font: &mut Font,
) {
    let px_size = style.stroke.font_px();
    let line_height = (px_size * 1.2).round() as i32;
    let color_rgba = style.color.rgba();
    for (i, line) in content.split('\n').enumerate() {
        let ly = origin.1 + i as i32 * line_height;
        font.render_text_rgba(img, origin.0, ly, line, px_size, color_rgba);
    }
}
```

`Font::render_text_rgba` doesn't exist yet — add it in `src/text.rs`:

```rust
#[allow(clippy::too_many_arguments)]
pub fn render_text_rgba(
    &mut self, img: &mut RgbaImage, x: i32, y: i32,
    text: &str, px_size: f32, color: [u8; 4],
) {
    if self.inner.is_none() { return; }
    let mut pen_x = x as f32;
    let (w, h) = (img.width() as i32, img.height() as i32);
    for ch in text.chars() {
        let Some((metrics, bitmap)) = self.rasterize(ch, px_size).cloned() else { continue };
        let gx = pen_x.round() as i32 + metrics.xmin;
        let ascent = (px_size * 0.8) as i32;
        let gy = y + ascent - metrics.height as i32 - metrics.ymin;
        for ggy in 0..metrics.height as i32 {
            for ggx in 0..metrics.width as i32 {
                let alpha = bitmap[(ggy * metrics.width as i32 + ggx) as usize];
                if alpha == 0 { continue; }
                let px = gx + ggx;
                let py = gy + ggy;
                if px < 0 || py < 0 || px >= w || py >= h { continue; }
                let bg = img.get_pixel_mut(px as u32, py as u32);
                let a = alpha as u16;
                let inv = (255 - alpha) as u16;
                bg[0] = ((color[0] as u16 * a + bg[0] as u16 * inv) / 255) as u8;
                bg[1] = ((color[1] as u16 * a + bg[1] as u16 * inv) / 255) as u8;
                bg[2] = ((color[2] as u16 * a + bg[2] as u16 * inv) / 255) as u8;
                bg[3] = 0xFF;
            }
        }
        pen_x += metrics.advance_width;
    }
}
```

**Signature propagation (mechanical but spelled out so you don't miss it):**

In `annotate_render.rs`, both public render entry points gain a `font: &mut crate::text::Font` parameter:

```rust
pub fn paint_on_cropped(
    img: &mut RgbaImage, ann: &Annotation, crop_offset: (i32, i32),
    font: &mut crate::text::Font,
) { /* match arms unchanged for non-Text; Text arm uses the new font */ }

pub fn draw_annotation_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32, frame: &RgbaImage,
    ann: &Annotation, font: &mut crate::text::Font,
) { /* same — Text arm uses font */ }

pub fn draw_pending_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32, frame: &RgbaImage,
    pending: &PendingDraw, font: &mut crate::text::Font,
) { /* Text never lands in PendingDraw, but accept the param uniformly */ }
```

In `src/overlay/mod.rs`:

- Change `pub fn flatten_for_export(&self, rect: Rect)` to `pub fn flatten_for_export(&mut self, rect: Rect)` so we can pass `&mut self.font`. App.rs already owns the overlay by value at the call site (`let final_image = overlay.flatten_for_export(rect);`), so `&mut` works without further changes.
- Update the loop inside `flatten_for_export`:
  ```rust
  for ann in self.history.current() {
      annotate_render::paint_on_cropped(&mut cropped, ann, offset, &mut self.font);
  }
  ```
  Wait — `self.history.current()` borrows `self` immutably while `&mut self.font` is mutable. Restructure:
  ```rust
  let offset = (frame_rect.0 as i32, frame_rect.1 as i32);
  let history_clone: Vec<Annotation> = self.history.current().to_vec();
  for ann in &history_clone {
      annotate_render::paint_on_cropped(&mut cropped, ann, offset, &mut self.font);
  }
  ```
  The clone is one-shot at export time — non-hot path, acceptable.
- Update the redraw loop at line ~466:
  ```rust
  for ann in self.history.current() {
      annotate_render::draw_annotation_on_buf(&mut buf, w, h, frame_ref, ann, font);
  }
  if let Some(pending) = self.pending_draw.as_ref() {
      annotate_render::draw_pending_on_buf(&mut buf, w, h, frame_ref, pending, font);
  }
  ```
  `font` is already `&mut self.font` in scope from line ~437.

- [ ] **Step 5: Add a smoke test for `paint_text_on_image`**

```rust
    #[test]
    fn paint_text_paints_some_pixels() {
        let mut img = RgbaImage::new(200, 60);
        let mut font = crate::text::Font::embedded();
        let style = AnnotationStyle::default();
        paint_text_on_image(&mut img, (4, 4), "Hi", style, &mut font);
        let any_painted = img.pixels().any(|p| p[3] != 0);
        assert!(any_painted, "expected some painted pixels for 'Hi'");
    }
```

- [ ] **Step 6: Run, expect PASS**

```
cargo test -p quickshot --lib
```

- [ ] **Step 7: Commit**

```
git add src/overlay/annotate.rs src/overlay/annotate_render.rs src/text.rs
git commit -m "feat(annotate): Annotation::Text + paint_text helpers"
```

---

## Task 6: Toolbar layout — `Tool::Pen` + `Tool::Text` + row 2 (color + stroke)

**Files:**
- Modify: `src/overlay/toolbar.rs`

- [ ] **Step 1: Failing tests for new layout + hits**

Append to `toolbar::tests`:

```rust
    #[test]
    fn tool_order_includes_pen_and_text() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let tools: Vec<Tool> = t.tool_buttons.iter().map(|b| b.tool).collect();
        assert_eq!(tools, vec![
            Tool::Move, Tool::Arrow, Tool::Rect, Tool::Ellipse,
            Tool::Pen, Tool::Text, Tool::Mosaic,
        ]);
    }

    #[test]
    fn layout_has_color_and_stroke_buttons() {
        let t = Toolbar::layout(sel(), (1440, 900));
        assert_eq!(t.color_buttons.len(), 4);
        assert_eq!(t.stroke_buttons.len(), 3);
        // Row 2 sits below row 1.
        let row1_y = t.tool_buttons[0].origin.1;
        let row2_y = t.color_buttons[0].origin.1;
        assert!(row2_y > row1_y);
    }

    #[test]
    fn hit_returns_color_and_stroke() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let yellow = &t.color_buttons[1];
        let c = (yellow.origin.0 + 4, yellow.origin.1 + 4);
        assert_eq!(t.hit_with_tool(c, Tool::Arrow), ToolbarHit::Color(Color::Yellow));
        let thick = &t.stroke_buttons[2];
        let s = (thick.origin.0 + 4, thick.origin.1 + 4);
        assert_eq!(t.hit_with_tool(s, Tool::Arrow), ToolbarHit::Stroke(Stroke::Thick));
    }

    #[test]
    fn hit_row2_returns_none_when_mosaic_active() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let yellow = &t.color_buttons[1];
        let c = (yellow.origin.0 + 4, yellow.origin.1 + 4);
        assert_eq!(t.hit_with_tool(c, Tool::Mosaic), ToolbarHit::None);
    }
```

- [ ] **Step 2: Run, expect FAIL** (Color/Stroke not in `ToolbarHit`, no `color_buttons`).

- [ ] **Step 3: Implement layout extension**

In `src/overlay/toolbar.rs`:

```rust
use super::annotate::{Color, Stroke, Tool};

const TOOL_ORDER: [Tool; 7] = [
    Tool::Move, Tool::Arrow, Tool::Rect, Tool::Ellipse,
    Tool::Pen, Tool::Text, Tool::Mosaic,
];

const COLOR_ORDER:  [Color;  4] = [Color::Red, Color::Yellow, Color::Green, Color::Blue];
const STROKE_ORDER: [Stroke; 3] = [Stroke::Thin, Stroke::Medium, Stroke::Thick];

pub const ROW_GAP: i32 = 4 * UI_SCALE;
pub const SWATCH_SIZE: i32 = 14 * UI_SCALE;
pub const SWATCH_PAD:  i32 = 6  * UI_SCALE;
pub const STROKE_DOT_AREA: i32 = 18 * UI_SCALE;

pub struct Toolbar {
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub tool_buttons: Vec<ToolButton>,
    pub undo_button: IconButton,
    pub redo_button: IconButton,
    pub color_buttons:  Vec<ColorButton>,
    pub stroke_buttons: Vec<StrokeButton>,
}

pub struct ColorButton {
    pub color: Color,
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

pub struct StrokeButton {
    pub stroke: Stroke,
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    Tool(Tool),
    Undo,
    Redo,
    Color(Color),
    Stroke(Stroke),
    None,
}
```

Layout logic (rewrite `Toolbar::layout`):

```rust
impl Toolbar {
    pub fn layout(selection: Rect, window_size: (u32, u32)) -> Toolbar {
        // Row 1: tools + sep + undo + redo (existing math, but TOOL_ORDER is now 7).
        let tools_w = TOOL_ORDER.len() as i32 * ICON_SIZE
            + (TOOL_ORDER.len() as i32 - 1) * ICON_PAD;
        let row1_content_w = tools_w + SEP_WIDTH + (ICON_SIZE + ICON_PAD) + ICON_SIZE;
        let row1_w = row1_content_w + 2 * ICON_PAD;

        // Row 2: 4 swatches + gap + 3 stroke dots.
        let swatches_w = COLOR_ORDER.len() as i32 * SWATCH_SIZE
            + (COLOR_ORDER.len() as i32 - 1) * SWATCH_PAD;
        let strokes_w = STROKE_ORDER.len() as i32 * STROKE_DOT_AREA
            + (STROKE_ORDER.len() as i32 - 1) * SWATCH_PAD;
        let row2_content_w = swatches_w + SEP_WIDTH + strokes_w;
        let row2_w = row2_content_w + 2 * ICON_PAD;

        let bar_w = row1_w.max(row2_w);
        let bar_h = TOOLBAR_H * 2 + ROW_GAP;

        let mut bar_x = selection.x + selection.w / 2 - bar_w / 2;
        let below_y = selection.y + selection.h + TOOLBAR_GAP;

        let wwi = window_size.0 as i32;
        let whi = window_size.1 as i32;
        if bar_x < 4 { bar_x = 4; }
        if bar_x + bar_w > wwi - 4 { bar_x = (wwi - 4 - bar_w).max(4); }

        let bar_y = if below_y + bar_h <= whi - 4 {
            below_y
        } else {
            let above = selection.y - TOOLBAR_GAP - bar_h;
            if above >= 4 { above } else { (whi - bar_h - 4).max(4) }
        };

        // Row 1 — tools + undo/redo.
        let row1_x_start = bar_x + (bar_w - row1_content_w) / 2;
        let mut x = row1_x_start;
        let row1_y = bar_y + (TOOLBAR_H - ICON_SIZE) / 2;
        let mut tool_buttons = Vec::with_capacity(TOOL_ORDER.len());
        for &t in TOOL_ORDER.iter() {
            tool_buttons.push(ToolButton {
                tool: t, origin: (x, row1_y), size: (ICON_SIZE, ICON_SIZE),
            });
            x += ICON_SIZE + ICON_PAD;
        }
        x += SEP_WIDTH;
        let undo_button = IconButton { origin: (x, row1_y), size: (ICON_SIZE, ICON_SIZE) };
        x += ICON_SIZE + ICON_PAD;
        let redo_button = IconButton { origin: (x, row1_y), size: (ICON_SIZE, ICON_SIZE) };

        // Row 2 — colors + strokes.
        let row2_x_start = bar_x + (bar_w - row2_content_w) / 2;
        let row2_top = bar_y + TOOLBAR_H + ROW_GAP;
        let row2_y = row2_top + (TOOLBAR_H - SWATCH_SIZE) / 2;
        let mut x = row2_x_start;
        let mut color_buttons = Vec::with_capacity(COLOR_ORDER.len());
        for &c in COLOR_ORDER.iter() {
            color_buttons.push(ColorButton {
                color: c, origin: (x, row2_y), size: (SWATCH_SIZE, SWATCH_SIZE),
            });
            x += SWATCH_SIZE + SWATCH_PAD;
        }
        x += SEP_WIDTH;
        let stroke_y = row2_top + (TOOLBAR_H - STROKE_DOT_AREA) / 2;
        let mut stroke_buttons = Vec::with_capacity(STROKE_ORDER.len());
        for &s in STROKE_ORDER.iter() {
            stroke_buttons.push(StrokeButton {
                stroke: s, origin: (x, stroke_y), size: (STROKE_DOT_AREA, STROKE_DOT_AREA),
            });
            x += STROKE_DOT_AREA + SWATCH_PAD;
        }

        Toolbar {
            origin: (bar_x, bar_y),
            size: (bar_w, bar_h),
            tool_buttons, undo_button, redo_button,
            color_buttons, stroke_buttons,
        }
    }

    pub fn hit(&self, cursor: (i32, i32)) -> ToolbarHit {
        // Backwards-compat: doesn't know the active tool. Rarely the right call now.
        self.hit_with_tool(cursor, Tool::Move)
    }

    pub fn hit_with_tool(&self, cursor: (i32, i32), active_tool: Tool) -> ToolbarHit {
        for btn in &self.tool_buttons {
            if point_in(cursor, btn.origin, btn.size) { return ToolbarHit::Tool(btn.tool); }
        }
        if point_in(cursor, self.undo_button.origin, self.undo_button.size) { return ToolbarHit::Undo; }
        if point_in(cursor, self.redo_button.origin, self.redo_button.size) { return ToolbarHit::Redo; }
        // Row 2 disabled under Mosaic (style isn't applied).
        if active_tool == Tool::Mosaic { return ToolbarHit::None; }
        for btn in &self.color_buttons {
            if point_in(cursor, btn.origin, btn.size) { return ToolbarHit::Color(btn.color); }
        }
        for btn in &self.stroke_buttons {
            if point_in(cursor, btn.origin, btn.size) { return ToolbarHit::Stroke(btn.stroke); }
        }
        ToolbarHit::None
    }
}
```

- [ ] **Step 4: Run, expect PASS**

```
cargo test -p quickshot --lib toolbar
```

(Iter 5a tests call `t.hit(...)`. Backwards-compat is preserved: `hit()` delegates to `hit_with_tool(c, Tool::Move)` and the Iter 5a tests probe row 1 / outside-bar coordinates, so they keep passing untouched.)

- [ ] **Step 5: Commit**

```
git add src/overlay/toolbar.rs
git commit -m "feat(toolbar): two-row layout + Color/Stroke hits"
```

---

## Task 7: Toolbar drawing — Pen/Text icons + color swatches + stroke dots + selection rings + greyed row 2

Smoke test only — pixel layout details are easier to validate visually.

**Files:**
- Modify: `src/overlay/toolbar.rs`
- Modify: `src/overlay/mod.rs` (the call to `draw_toolbar` gains `current_style: AnnotationStyle`)

- [ ] **Step 1: Implement new icon helpers**

In `src/overlay/toolbar.rs`:

```rust
fn draw_icon_pen(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Diagonal nib: solid line from top-right to bottom-left, with a small
    // triangle (filled) on the bottom-left tip.
    let pad = ICON_SIZE / 5;
    let (x0, y0) = (o.0 + ICON_SIZE - pad, o.1 + pad);
    let (x1, y1) = (o.0 + pad, o.1 + ICON_SIZE - pad);
    stroke_line(buf, w, h, x0, y0, x1, y1, STROKE, color);
    // Small filled triangle (nib) at (x1, y1).
    let nib = 4 * UI_SCALE;
    fill_triangle(
        buf, w, h,
        (x1 as f64, y1 as f64),
        ((x1 + nib) as f64, y1 as f64),
        (x1 as f64, (y1 - nib) as f64),
        color,
    );
}

fn draw_icon_text(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Capital "T" — horizontal bar on top + vertical bar centered.
    let pad = ICON_SIZE / 5;
    let bar_w = ICON_SIZE - 2 * pad;
    fill_rect(buf, w, h, o.0 + pad, o.1 + pad, bar_w, STROKE, color);
    let cx = o.0 + ICON_SIZE / 2;
    fill_rect(buf, w, h, cx - STROKE / 2, o.1 + pad, STROKE, ICON_SIZE - 2 * pad, color);
}
```

Update `draw_tool_icon` to dispatch `Tool::Pen` and `Tool::Text` to the new helpers.

- [ ] **Step 2: Update `draw_toolbar` to render row 2**

```rust
pub fn draw_toolbar(
    buf: &mut [u32], win_w: u32, win_h: u32,
    toolbar: &Toolbar,
    current_tool: Tool, current_style: AnnotationStyle,
    can_undo: bool, can_redo: bool,
) {
    draw_pill(buf, win_w, win_h, toolbar.origin, toolbar.size, PILL_RADIUS, 0x000000, 0.7);

    // Row 1.
    for btn in &toolbar.tool_buttons {
        if btn.tool == current_tool {
            draw_pill(buf, win_w, win_h, btn.origin, btn.size, PILL_RADIUS, 0xFFFFFF, 0.25);
        }
        draw_tool_icon(buf, win_w, win_h, btn.tool, btn.origin, 0xFFFFFF);
    }
    let undo_color = if can_undo { 0xFFFFFF } else { 0x888888 };
    draw_icon_undo(buf, win_w, win_h, toolbar.undo_button.origin, undo_color);
    let redo_color = if can_redo { 0xFFFFFF } else { 0x888888 };
    draw_icon_redo(buf, win_w, win_h, toolbar.redo_button.origin, redo_color);

    // Row 2 — only painted in full opacity when not Mosaic.
    let row2_alpha: f32 = if current_tool == Tool::Mosaic { 0.4 } else { 1.0 };
    for btn in &toolbar.color_buttons {
        let argb = btn.color.argb();
        let cx = btn.origin.0 + btn.size.0 / 2;
        let cy = btn.origin.1 + btn.size.1 / 2;
        let radius = btn.size.0 as f64 / 2.0;
        // Filled disk (color), faded if Mosaic active.
        let argb_alpha = mix_argb(argb, 0x000000, 1.0 - row2_alpha);
        fill_disk(buf, win_w, win_h, cx as f64, cy as f64, radius, argb_alpha);
        if btn.color == current_style.color && current_tool != Tool::Mosaic {
            // Active ring: white circle outlining the swatch.
            stroke_disk(buf, win_w, win_h, cx as f64, cy as f64, radius + 3.0, 2, 0xFFFFFF);
        }
    }
    for btn in &toolbar.stroke_buttons {
        let cx = btn.origin.0 + btn.size.0 / 2;
        let cy = btn.origin.1 + btn.size.1 / 2;
        let dot_r = (btn.stroke.px() * UI_SCALE) as f64 / 2.0;
        let argb = if current_tool == Tool::Mosaic { 0x666666 } else { 0xFFFFFF };
        fill_disk(buf, win_w, win_h, cx as f64, cy as f64, dot_r, argb);
        if btn.stroke == current_style.stroke && current_tool != Tool::Mosaic {
            stroke_disk(buf, win_w, win_h, cx as f64, cy as f64, dot_r + 4.0, 2, 0xFFFFFF);
        }
    }
}

// Add small helpers next to the existing primitives:

fn stroke_disk(buf: &mut [u32], w: u32, h: u32, cx: f64, cy: f64, r: f64, thickness: i32, color: u32) {
    let r_outer = r + thickness as f64 / 2.0;
    let r_inner = (r - thickness as f64 / 2.0).max(0.0);
    let r2_o = r_outer * r_outer;
    let r2_i = r_inner * r_inner;
    let r_ceil = r_outer.ceil() as i32;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let d2 = (dx * dx + dy * dy) as f64;
            if d2 <= r2_o && d2 >= r2_i {
                put(buf, w, h, cx as i32 + dx, cy as i32 + dy, color);
            }
        }
    }
}

fn mix_argb(fg: u32, bg: u32, alpha_to_bg: f32) -> u32 {
    let lerp = |a: u32, b: u32| {
        let af = a as f32 * (1.0 - alpha_to_bg);
        let bf = b as f32 * alpha_to_bg;
        (af + bf) as u32
    };
    let r = lerp((fg >> 16) & 0xFF, (bg >> 16) & 0xFF);
    let g = lerp((fg >>  8) & 0xFF, (bg >>  8) & 0xFF);
    let b = lerp( fg        & 0xFF,  bg        & 0xFF);
    (r << 16) | (g << 8) | b
}
```

- [ ] **Step 3: Update mod.rs call site to keep the build green**

`draw_toolbar`'s signature now requires `current_style: AnnotationStyle`. Until Task 8 wires the field on `Overlay`, pass a temporary default at the call site in `src/overlay/mod.rs` (around line 500):

```rust
toolbar::draw_toolbar(
    &mut buf, w, h, &tb,
    self.tool,
    annotate::AnnotationStyle::default(),     // TEMPORARY — Task 8 swaps for self.current_style
    self.history.can_undo(),
    self.history.can_redo(),
);
```

Add `use annotate::AnnotationStyle;` at the top of `mod.rs` if Task 2 hasn't already added it.

```
cargo build
```

Expected: clean.

- [ ] **Step 4: Manual smoke**

```
cargo run --release
```

Press Cmd+Shift+A, drag a region — toolbar shows two rows; row 2 dimmed when Mosaic is active.

- [ ] **Step 5: Commit**

```
git add src/overlay/toolbar.rs src/overlay/mod.rs
git commit -m "feat(toolbar): row 2 swatches + stroke dots + Pen/Text icons"
```

---

## Task 8: Wire `current_style` into `Overlay`; route Color/Stroke clicks; keyboard shortcuts

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add field + plumb into render**

In `src/overlay/mod.rs`:

```rust
// Around the Overlay struct definition (~line 38):
pub struct Overlay {
    // ... existing fields ...
    pub(crate) tool: annotate::Tool,
    pub(crate) history: annotate::History,
    pub(crate) pending_draw: Option<PendingDraw>,
    pub(crate) current_style: annotate::AnnotationStyle,   // NEW
    modifiers: ModifiersState,
}
```

In `Overlay::create`'s constructor (around line 132):

```rust
current_style: annotate::AnnotationStyle::default(),
```

Replace the `AnnotationStyle::default()` placeholder in `handle_left_press` (Task 2, Step 4) with `self.current_style`:

```rust
self.pending_draw = Some(PendingDraw::shape(
    self.tool, self.current_style, fp, fp,
));
```

In `redraw`, the `draw_toolbar(...)` call passes `self.current_style`:

```rust
toolbar::draw_toolbar(
    &mut buf, w, h, &tb, self.tool, self.current_style,
    self.history.can_undo(), self.history.can_redo(),
);
```

- [ ] **Step 2: Route color/stroke clicks in `handle_left_press`**

In the existing match around `ToolbarHit`:

```rust
match tb.hit_with_tool(self.cursor, self.tool) {
    toolbar::ToolbarHit::Tool(t) => { ... }     // existing
    toolbar::ToolbarHit::Undo   => { ... }       // existing
    toolbar::ToolbarHit::Redo   => { ... }       // existing
    toolbar::ToolbarHit::Color(c) => {
        self.current_style.color = c;
        self.window.request_redraw();
        return Outcome::Continue;
    }
    toolbar::ToolbarHit::Stroke(s) => {
        self.current_style.stroke = s;
        self.window.request_redraw();
        return Outcome::Continue;
    }
    toolbar::ToolbarHit::None => {}
}
```

- [ ] **Step 3: Keyboard shortcuts — write failing test**

Add a test of pure key→style mapping (factor the logic into a helper for testability):

In `src/overlay/mod.rs` add (above `impl Overlay`):

```rust
pub(crate) fn key_to_color(c: char) -> Option<annotate::Color> {
    match c {
        '1' => Some(annotate::Color::Red),
        '2' => Some(annotate::Color::Yellow),
        '3' => Some(annotate::Color::Green),
        '4' => Some(annotate::Color::Blue),
        _ => None,
    }
}
```

Add to `mod tests` at the bottom of `mod.rs` (create `#[cfg(test)] mod tests` if absent):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use annotate::Color;

    #[test]
    fn key_to_color_mapping() {
        assert_eq!(key_to_color('1'), Some(Color::Red));
        assert_eq!(key_to_color('2'), Some(Color::Yellow));
        assert_eq!(key_to_color('3'), Some(Color::Green));
        assert_eq!(key_to_color('4'), Some(Color::Blue));
        assert_eq!(key_to_color('5'), None);
        assert_eq!(key_to_color('a'), None);
    }
}
```

```
cargo test -p quickshot --lib overlay::tests::key_to_color_mapping
```

Expected: PASS once the helper compiles.

- [ ] **Step 4: Wire keys in `handle_key`**

Within the existing tool-shortcut block (around line 339), extend the match to add Pen + Text:

```rust
let new_tool = match ch {
    'm' => Some(annotate::Tool::Move),
    'a' => Some(annotate::Tool::Arrow),
    'r' => Some(annotate::Tool::Rect),
    'e' => Some(annotate::Tool::Ellipse),
    'b' => Some(annotate::Tool::Mosaic),
    'p' => Some(annotate::Tool::Pen),
    't' => Some(annotate::Tool::Text),
    _ => None,
};
```

After that block, add color + stroke shortcuts (only when modifiers/super not held — the spec says shortcuts work in `Adjusting` only):

```rust
if matches!(self.state, OverlayState::Adjusting { .. }) && !self.modifiers.super_key() {
    if let Key::Character(s) = &key {
        let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
        if let Some(c) = key_to_color(ch) {
            self.current_style.color = c;
            self.window.request_redraw();
            return Outcome::Continue;
        }
        if ch == '[' { self.current_style.stroke = self.current_style.stroke.step_down(); self.window.request_redraw(); return Outcome::Continue; }
        if ch == ']' { self.current_style.stroke = self.current_style.stroke.step_up();   self.window.request_redraw(); return Outcome::Continue; }
    }
}
```

- [ ] **Step 5: `cargo build` + `cargo test`**

Expected: clean build, all tests pass.

- [ ] **Step 6: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): current_style field + Color/Stroke clicks + 1234 [ ] keys"
```

---

## Task 9: Pen mouse flow — start on press, dedup on move, finalize on release

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Update `handle_left_press` to route Pen**

In `src/overlay/mod.rs` around line 264 (the `if self.tool.is_drawing() && rect.contains(self.cursor)` branch):

```rust
if self.tool.is_drawing() && rect.contains(self.cursor) {
    let fp = self.window_point_to_frame_point(self.cursor);
    self.pending_draw = Some(match self.tool {
        annotate::Tool::Pen => PendingDraw::pen(self.current_style, fp),
        _                   => PendingDraw::shape(self.tool, self.current_style, fp, fp),
    });
    self.window.request_redraw();
    return Outcome::Continue;
}
```

Note: `Tool::Text` is **not** a drawing tool from `is_drawing()`'s POV in the spec (`is_drawing` returns true today for everything except Move). To keep Text out of this branch, change `is_drawing` to:

```rust
impl Tool {
    pub fn is_drawing(self) -> bool {
        !matches!(self, Tool::Move | Tool::Text)
    }
}
```

This is a small but important behavior tweak — Text uses TextEdit (Task 10), not PendingDraw.

- [ ] **Step 2: Verify CursorMoved already routes to `extend_to`**

`extend_to` was added in Task 3. Confirm `mod.rs` line ~146 calls `p.extend_to(fp)`. No change needed.

- [ ] **Step 3: Verify `handle_left_release` finalizes Pen**

`handle_left_release` already takes `self.pending_draw.take()` and calls `finalize()`. With `PendingDraw::Pen`, `finalize()` returns `Some(Annotation::Pen { ... })` if ≥ 2 points, else `None` — which means a single-click pen attempt is silently discarded. No change needed.

- [ ] **Step 4: Manual smoke**

```
cargo run --release
```

Press Cmd+Shift+A, drag region, press `P`, click-drag to draw a curve, release. Press Enter — verify the curve is in the saved PNG.

- [ ] **Step 5: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): Pen mouse flow (press/drag/release)"
```

---

## Task 10: TextEdit state — click flow, key compose flow, blinking caret

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add TextEdit struct**

In `src/overlay/mod.rs`:

```rust
pub(crate) struct TextEdit {
    pub origin_frame: (i32, i32),
    pub buffer: String,
    pub last_blink: std::time::Instant,
    pub cursor_visible: bool,
}

impl TextEdit {
    pub fn new(origin_frame: (i32, i32)) -> Self {
        Self {
            origin_frame,
            buffer: String::new(),
            last_blink: std::time::Instant::now(),
            cursor_visible: true,
        }
    }

    /// Returns true if the buffer changed (and a redraw is warranted).
    pub fn handle_char(&mut self, ch: char) -> bool {
        if ch == '\u{7f}' || ch == '\u{8}' {
            // delete / backspace — handled separately in caller
            return false;
        }
        if !ch.is_control() || ch == '\n' {
            self.buffer.push(ch);
            return true;
        }
        false
    }

    pub fn backspace(&mut self) -> bool {
        self.buffer.pop().is_some()
    }
}
```

Add `pub(crate) text_edit: Option<TextEdit>` field on `Overlay`. In `Overlay::create`'s `Ok(Self { ... })` initializer, add `text_edit: None,`.

- [ ] **Step 2: Failing tests for buffer logic**

Append to `overlay::tests`:

```rust
    #[test]
    fn text_edit_appends_printable() {
        let mut t = TextEdit::new((10, 10));
        t.handle_char('h');
        t.handle_char('i');
        assert_eq!(t.buffer, "hi");
    }

    #[test]
    fn text_edit_backspace() {
        let mut t = TextEdit::new((0, 0));
        t.buffer.push_str("abc");
        assert!(t.backspace());
        assert_eq!(t.buffer, "ab");
        t.buffer.clear();
        assert!(!t.backspace()); // empty backspace returns false
    }

    #[test]
    fn text_edit_newline() {
        let mut t = TextEdit::new((0, 0));
        t.handle_char('a');
        t.handle_char('\n');
        t.handle_char('b');
        assert_eq!(t.buffer, "a\nb");
    }
```

```
cargo test -p quickshot --lib overlay::tests::text_edit
```

Expected: PASS.

- [ ] **Step 3: Click flow in `handle_left_press`**

When `Tool::Text` is active and the click is inside the selection rect:

```rust
if self.tool == annotate::Tool::Text && rect.contains(self.cursor) {
    // Commit any in-flight TextEdit first.
    if let Some(t) = self.text_edit.take() {
        if !t.buffer.is_empty() {
            self.history.push(annotate::Annotation::Text {
                origin: t.origin_frame,
                content: t.buffer,
                style: self.current_style,
            });
        }
    }
    let fp = self.window_point_to_frame_point(self.cursor);
    self.text_edit = Some(TextEdit::new(fp));
    self.window.request_redraw();
    return Outcome::Continue;
}
```

This goes inside the `OverlayState::Adjusting` branch of `handle_left_press`, **before** the `is_drawing` check (so a Text-mode click inside the rect doesn't fall through into a Shape).

- [ ] **Step 4: Key flow in `handle_key`**

At the very top of `handle_key` (before any other handling):

```rust
if let Some(text_edit) = self.text_edit.as_mut() {
    match &key {
        Key::Named(NamedKey::Enter) => {
            if self.modifiers.shift_key() {
                text_edit.handle_char('\n');
                self.window.request_redraw();
                return Outcome::Continue;
            }
            // Plain Enter commits.
            let t = self.text_edit.take().unwrap();
            if !t.buffer.is_empty() {
                self.history.push(annotate::Annotation::Text {
                    origin: t.origin_frame,
                    content: t.buffer,
                    style: self.current_style,
                });
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }
        Key::Named(NamedKey::Escape) => {
            self.text_edit = None;
            self.window.request_redraw();
            return Outcome::Continue;
        }
        Key::Named(NamedKey::Backspace) => {
            text_edit.backspace();
            self.window.request_redraw();
            return Outcome::Continue;
        }
        Key::Character(s) => {
            for ch in s.chars() { text_edit.handle_char(ch); }
            self.window.request_redraw();
            return Outcome::Continue;
        }
        _ => return Outcome::Continue,
    }
}
```

- [ ] **Step 5: Render the live edit + blinking cursor**

In `Overlay::redraw`, after the existing history-loop and pending-draw paint, add:

```rust
if let Some(t) = self.text_edit.as_mut() {
    let now = std::time::Instant::now();
    if now.duration_since(t.last_blink) >= std::time::Duration::from_millis(530) {
        t.cursor_visible = !t.cursor_visible;
        t.last_blink = now;
    }
    let display = if t.cursor_visible {
        format!("{}|", t.buffer)
    } else {
        t.buffer.clone()
    };
    annotate_render::draw_text_on_buf(
        &mut buf, w, h, frame_ref, t.origin_frame, &display,
        self.current_style, font,
    );
}
```

(`font` is the `&mut self.font` already in scope from earlier render lines.)

To keep the cursor blinking continuously without input, request a redraw on a timer-ish basis. Simpler: in `request_redraw_throttled`, when `text_edit.is_some()` is also schedule a redraw at the next 530 ms boundary. For Iter 5b, accept that the cursor only blinks while the user is pressing keys — explicit blinking timer is YAGNI for this iter.

- [ ] **Step 6: `cargo test` + `cargo run`**

```
cargo test -p quickshot --lib
cargo run --release
```

Trigger Cmd+Shift+A → drag region → `T` → click → type "hi" → Enter. Confirm "hi" appears in the saved screenshot in the chosen color/size.

- [ ] **Step 7: Commit**

```
git add src/overlay/mod.rs
git commit -m "feat(overlay): Text tool inline editing (click + type + Enter)"
```

---

## Task 11: Final smoke + close-out commit

**Files:** none modified — verification only.

- [ ] **Step 1: Full test suite**

```
cargo test -p quickshot --lib
```

Expected: all green.

- [ ] **Step 2: Build release**

```
cargo build --release
```

Expected: warnings only from existing `clashing_extern_declarations` (macOS objc), no new warnings.

- [ ] **Step 3: Manual smoke matrix**

Run `target/release/quickshot`, press Cmd+Shift+A, drag a region, then for each combination verify the saved PNG is correct:

- [ ] Color × Tool: cycle 1/2/3/4 with Arrow, Rect, Ellipse, Pen, Text — 4 × 5 = 20 paths render in the right color.
- [ ] Stroke × Tool: cycle [/] with Arrow, Rect, Ellipse, Pen — Thin/Medium/Thick visibly differ.
- [ ] Stroke × Text: Thin/Medium/Thick produce 14/20/28 px text.
- [ ] Mosaic still works; row 2 is greyed out and ignores clicks while Mosaic is active.
- [ ] Cmd+Z undo; Cmd+Shift+Z redo for Pen + Text annotations.
- [ ] Esc cancels mid-edit Text without committing.
- [ ] Enter commits Text. Shift+Enter inserts newline.

- [ ] **Step 4: Tag the release if all green** (optional, only if user requests)

```
git log --oneline -15
git tag -a v0.8.0-iter5b -m "iter5b: annotation toolkit completion"
```

Don't push the tag automatically — confirm with the user first.

---

## Summary

Total tasks: 11. Estimated effort: 1–2 days.

The plan keeps Iter 5a behavior intact at every commit — every task either compiles cleanly + passes tests, or is the structural-refactor task (Task 2) that fails Iter 5a tests only on the test-update step. By the end of Task 11, every spec item in `docs/superpowers/specs/2026-05-09-quickshot-iter5b-design.md` has a corresponding implementation + test.
