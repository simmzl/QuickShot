# quickshot Iter 5a — Annotation Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Arrow / Rectangle / Ellipse / Mosaic annotation tools to the region-capture flow, with undo/redo and an inline toolbar. Enter flattens the annotations into the PNG that lands on clipboard + optional file save. ESC discards.

**Architecture:** Three new modules under `src/overlay/`:
- `annotate.rs` — pure types (Tool, Annotation, PendingDraw) + undo/redo history. No IO, no rendering.
- `annotate_render.rs` — drawing helpers. Two code paths: `*_on_image` for flattening into an `RgbaImage` (export), `*_on_buf` for live softbuffer preview.
- `toolbar.rs` — toolbar layout (below selection, flip above at edge), icon rendering, hit testing.

`Overlay` gains three fields: `tool: Tool`, `history: History`, `pending_draw: Option<PendingDraw>`. Keyboard keys M/A/R/E/B switch tool, Cmd+Z / Cmd+Shift+Z undo/redo. Drawing tools: mouse-drag inside selection paints a shape; anchors are hidden while a drawing tool is active. `app.rs::confirm` uses `overlay.flatten_for_export(rect)` to get the annotated cropped image.

**Tech Stack:** Existing only. No new crates.

**Spec:** `docs/superpowers/specs/2026-04-20-quickshot-iter5a-design.md`

**Scope for this plan:**
- Four tools: Arrow, Rectangle, Ellipse, Mosaic
- Default Move tool (keeps Iter 2a anchor-resize + click-to-translate + click-outside-clear)
- Hardcoded red `#FF3B30`, 3-px thickness
- Undo (Cmd+Z) / Redo (Cmd+Shift+Z); redo stack cleared on new annotation
- Mini toolbar below the selection with 7 buttons (M/A/R/E/B/Undo/Redo)
- Enter flattens annotations into the PNG → clipboard + save

**Not in this plan:**
- Text tool (Iter 5b)
- Color / thickness picker (Iter 5b)
- Per-annotation selection or delete (Iter 5b)
- Annotation on full-screen capture flow
- Iter 2c polish items

---

## File Structure

```
src/overlay/
├── annotate.rs              (new — Tool, Annotation, PendingDraw, History)
├── annotate_render.rs       (new — image + buf drawing helpers)
├── toolbar.rs               (new — layout, icons, hit test)
├── mod.rs                   (modified — fields, keyboard, mouse, redraw)
├── state.rs                 (unchanged)
├── hit.rs                   (unchanged)
└── render.rs                (unchanged)

src/app.rs                   (modified — confirm uses flatten_for_export)
```

---

## Task 1: `annotate.rs` types + History

Pure logic + unit tests. No wiring into overlay yet.

**Files:**
- Create: `src/overlay/annotate.rs`
- Modify: `src/overlay/mod.rs` (declare `pub(crate) mod annotate;`)

- [ ] **Step 1: Write `src/overlay/annotate.rs`**

Create `src/overlay/annotate.rs`:
```rust
//! Pure types + undo/redo for annotation state. No IO, no drawing.

use super::state::Rect;

/// A single placed annotation, in FRAME-space coordinates (physical pixels
/// of the captured image, matching what the PNG contains).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Arrow { from: (i32, i32), to: (i32, i32) },
    Rect { rect: Rect },
    Ellipse { rect: Rect },
    Mosaic { rect: Rect, block_size: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Arrow,
    Rect,
    Ellipse,
    Mosaic,
}

impl Tool {
    pub fn is_drawing(self) -> bool {
        !matches!(self, Tool::Move)
    }
}

/// In-flight drawing: user has mouse-pressed while a drawing tool is active.
#[derive(Debug, Clone, Copy)]
pub struct PendingDraw {
    pub tool: Tool,
    pub from_frame: (i32, i32),
    pub to_frame: (i32, i32),
}

impl PendingDraw {
    /// Produce the Annotation that this pending draw represents.
    /// Returns None if the tool is not drawing-capable (shouldn't happen in
    /// normal flow but protects against misuse).
    pub fn finalize(self) -> Option<Annotation> {
        match self.tool {
            Tool::Move => None,
            Tool::Arrow => Some(Annotation::Arrow {
                from: self.from_frame,
                to: self.to_frame,
            }),
            Tool::Rect => Some(Annotation::Rect {
                rect: Rect::normalize(self.from_frame, self.to_frame),
            }),
            Tool::Ellipse => Some(Annotation::Ellipse {
                rect: Rect::normalize(self.from_frame, self.to_frame),
            }),
            Tool::Mosaic => Some(Annotation::Mosaic {
                rect: Rect::normalize(self.from_frame, self.to_frame),
                block_size: 8,
            }),
        }
    }
}

/// Undo/redo stack. Each completed annotation is pushed onto `undo_stack`;
/// Undo moves the top annotation onto `redo_stack`. Any new push clears
/// the redo stack.
pub struct History {
    undo_stack: Vec<Annotation>,
    redo_stack: Vec<Annotation>,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn push(&mut self, a: Annotation) {
        self.undo_stack.push(a);
        self.redo_stack.clear();
    }

    /// Undo the top annotation. Returns true if a change happened.
    pub fn undo(&mut self) -> bool {
        if let Some(a) = self.undo_stack.pop() {
            self.redo_stack.push(a);
            true
        } else {
            false
        }
    }

    /// Redo the most-recently-undone annotation. Returns true if a change happened.
    pub fn redo(&mut self) -> bool {
        if let Some(a) = self.redo_stack.pop() {
            self.undo_stack.push(a);
            true
        } else {
            false
        }
    }

    pub fn current(&self) -> &[Annotation] {
        &self.undo_stack
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_drawing_variants() {
        assert!(!Tool::Move.is_drawing());
        assert!(Tool::Arrow.is_drawing());
        assert!(Tool::Rect.is_drawing());
        assert!(Tool::Ellipse.is_drawing());
        assert!(Tool::Mosaic.is_drawing());
    }

    #[test]
    fn finalize_move_returns_none() {
        let p = PendingDraw {
            tool: Tool::Move,
            from_frame: (0, 0),
            to_frame: (10, 10),
        };
        assert!(p.finalize().is_none());
    }

    #[test]
    fn finalize_arrow() {
        let p = PendingDraw {
            tool: Tool::Arrow,
            from_frame: (10, 20),
            to_frame: (50, 80),
        };
        let a = p.finalize().unwrap();
        assert_eq!(
            a,
            Annotation::Arrow {
                from: (10, 20),
                to: (50, 80)
            }
        );
    }

    #[test]
    fn finalize_rect_normalizes() {
        let p = PendingDraw {
            tool: Tool::Rect,
            from_frame: (80, 60),
            to_frame: (20, 10),
        };
        match p.finalize().unwrap() {
            Annotation::Rect { rect } => {
                assert_eq!(rect, Rect { x: 20, y: 10, w: 60, h: 50 });
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn finalize_ellipse_normalizes() {
        let p = PendingDraw {
            tool: Tool::Ellipse,
            from_frame: (0, 0),
            to_frame: (30, 40),
        };
        match p.finalize().unwrap() {
            Annotation::Ellipse { rect } => {
                assert_eq!(rect, Rect { x: 0, y: 0, w: 30, h: 40 });
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn finalize_mosaic_has_block_size_8() {
        let p = PendingDraw {
            tool: Tool::Mosaic,
            from_frame: (5, 5),
            to_frame: (50, 50),
        };
        match p.finalize().unwrap() {
            Annotation::Mosaic { rect, block_size } => {
                assert_eq!(rect, Rect { x: 5, y: 5, w: 45, h: 45 });
                assert_eq!(block_size, 8);
            }
            _ => panic!("expected Mosaic"),
        }
    }

    #[test]
    fn history_empty() {
        let h = History::new();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 0);
    }

    #[test]
    fn history_push() {
        let mut h = History::new();
        h.push(Annotation::Arrow {
            from: (0, 0),
            to: (10, 10),
        });
        assert!(h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 1);
    }

    #[test]
    fn history_undo_moves_to_redo() {
        let mut h = History::new();
        h.push(Annotation::Rect {
            rect: Rect { x: 0, y: 0, w: 10, h: 10 },
        });
        assert!(h.undo());
        assert!(!h.can_undo());
        assert!(h.can_redo());
        assert_eq!(h.current().len(), 0);
    }

    #[test]
    fn history_redo_restores() {
        let mut h = History::new();
        let a = Annotation::Ellipse {
            rect: Rect { x: 5, y: 5, w: 20, h: 20 },
        };
        h.push(a);
        h.undo();
        assert!(h.redo());
        assert_eq!(h.current(), &[a]);
        assert!(!h.can_redo());
    }

    #[test]
    fn push_after_undo_clears_redo() {
        let mut h = History::new();
        h.push(Annotation::Arrow { from: (0, 0), to: (5, 5) });
        h.undo();
        assert!(h.can_redo());
        h.push(Annotation::Arrow { from: (10, 10), to: (20, 20) });
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 1);
    }

    #[test]
    fn undo_empty_returns_false() {
        let mut h = History::new();
        assert!(!h.undo());
        assert!(!h.redo());
    }

    #[test]
    fn multiple_undo_redo() {
        let mut h = History::new();
        let a = Annotation::Arrow { from: (0, 0), to: (1, 1) };
        let b = Annotation::Arrow { from: (2, 2), to: (3, 3) };
        let c = Annotation::Arrow { from: (4, 4), to: (5, 5) };
        h.push(a);
        h.push(b);
        h.push(c);
        assert_eq!(h.current().len(), 3);
        h.undo();
        h.undo();
        assert_eq!(h.current(), &[a]);
        h.redo();
        assert_eq!(h.current(), &[a, b]);
    }
}
```

- [ ] **Step 2: Declare the module in `src/overlay/mod.rs`**

Edit `src/overlay/mod.rs`. Find the top-level submodule declarations (around line 1-3):
```rust
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;
```
Change to:
```rust
pub(crate) mod annotate;
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;
```
(Alphabetical order; `annotate` goes first.)

- [ ] **Step 3: Build + test**

```bash
cargo test overlay::annotate::tests
```
Expected: 12 tests pass.

```bash
cargo test
```
Expected: 71 + 12 = 83 passed / 0 failed / 2 ignored.

```bash
cargo build --release
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. `History`, `PendingDraw`, `Annotation`, `Tool` all have a dead-code smell (not yet wired); add `#[allow(dead_code)]` on the enum/struct/impl blocks if clippy flags any. Specifically:
- `History::can_undo`, `History::can_redo`, `History::current` may warn "never used" — add `#[allow(dead_code)]` on the `impl History` block (with a comment "wired in Task 5") OR on each method.
- Variants of `Annotation` may warn unused — add `#[allow(dead_code)]` on the enum.

Minimal targeted `#[allow(dead_code)]`:
```rust
#[allow(dead_code)]  // wired in Task 5+6
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation { ... }

#[allow(dead_code)]  // wired in Task 5+6
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool { ... }
```
And similar for `PendingDraw`, `History`. These allows will be removed in later tasks when the items get used.

- [ ] **Step 4: Commit**

```bash
git add src/overlay/annotate.rs src/overlay/mod.rs
git commit -m "feat(annotate): Tool/Annotation/History pure types with undo/redo"
```

---

## Task 2: `annotate_render.rs` image-path helpers

Drawing functions that mutate an `RgbaImage`. Used by the flatten-for-export path. `_on_buf` variants (softbuffer preview) come in Task 3.

**Files:**
- Create: `src/overlay/annotate_render.rs`
- Modify: `src/overlay/mod.rs` (declare `pub(crate) mod annotate_render;`)

- [ ] **Step 1: Write `src/overlay/annotate_render.rs`**

Create `src/overlay/annotate_render.rs`:
```rust
//! Rendering helpers for annotations. Two code paths:
//!   * `*_on_image`: paint into a `RgbaImage` (used by flatten_for_export).
//!   * `*_on_buf`: paint into a softbuffer `&mut [u32]` (used by live preview;
//!     implemented in Task 3).

use image::{Rgba, RgbaImage};

use super::annotate::Annotation;
use super::state::Rect;

pub const ANNOTATION_COLOR_RGBA: [u8; 4] = [0xFF, 0x3B, 0x30, 0xFF]; // #FF3B30
pub const ANNOTATION_THICKNESS: i32 = 3;
pub const ARROWHEAD_LEN: f32 = 14.0;

/// Apply an annotation to a CROPPED image. `crop_offset` is the frame-space
/// coordinate corresponding to the cropped image's (0, 0). Annotations are
/// stored in frame coords; we translate into crop-local coords here.
pub fn paint_on_cropped(img: &mut RgbaImage, ann: Annotation, crop_offset: (i32, i32)) {
    let (ox, oy) = crop_offset;
    match ann {
        Annotation::Arrow { from, to } => {
            draw_arrow_on_image(
                img,
                (from.0 - ox, from.1 - oy),
                (to.0 - ox, to.1 - oy),
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Rect { rect } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_rect_outline_on_image(
                img,
                local,
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Ellipse { rect } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse_outline_on_image(
                img,
                local,
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Mosaic { rect, block_size } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            apply_mosaic_on_image(img, local, block_size);
        }
    }
}

pub fn draw_rect_outline_on_image(
    img: &mut RgbaImage,
    rect: Rect,
    color: Rgba<u8>,
    thickness: i32,
) {
    if rect.w <= 0 || rect.h <= 0 || thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let half = thickness / 2;
    // Top + bottom strips
    for ty in 0..thickness {
        let y1 = rect.y - half + ty;
        let y2 = rect.y + rect.h - 1 - half + ty;
        for x in rect.x..(rect.x + rect.w) {
            put_clamped(img, x, y1, w, h, color);
            put_clamped(img, x, y2, w, h, color);
        }
    }
    // Left + right strips (avoid re-painting corners — harmless but wasteful)
    for tx in 0..thickness {
        let x1 = rect.x - half + tx;
        let x2 = rect.x + rect.w - 1 - half + tx;
        for y in rect.y..(rect.y + rect.h) {
            put_clamped(img, x1, y, w, h, color);
            put_clamped(img, x2, y, w, h, color);
        }
    }
}

pub fn draw_ellipse_outline_on_image(
    img: &mut RgbaImage,
    rect: Rect,
    color: Rgba<u8>,
    thickness: i32,
) {
    if rect.w <= 2 || rect.h <= 2 || thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let cx = rect.x as f64 + rect.w as f64 / 2.0;
    let cy = rect.y as f64 + rect.h as f64 / 2.0;
    let rx = rect.w as f64 / 2.0;
    let ry = rect.h as f64 / 2.0;
    let half_thick = thickness as f64 / 2.0;

    // Iterate over the bounding box and keep pixels whose normalized radius
    // is within the stroke band.
    let pad = (thickness + 2).max(2);
    for y in (rect.y - pad)..=(rect.y + rect.h + pad) {
        for x in (rect.x - pad)..=(rect.x + rect.w + pad) {
            let nx = (x as f64 + 0.5 - cx) / rx;
            let ny = (y as f64 + 0.5 - cy) / ry;
            let r = (nx * nx + ny * ny).sqrt();
            // Convert stroke-band half-thickness from pixels to normalized radius.
            // Approx: use the smaller of rx/ry to keep ellipse anisotropy reasonable.
            let band = half_thick / rx.min(ry);
            if (r - 1.0).abs() <= band {
                put_clamped(img, x, y, w, h, color);
            }
        }
    }
}

pub fn draw_arrow_on_image(
    img: &mut RgbaImage,
    from: (i32, i32),
    to: (i32, i32),
    color: Rgba<u8>,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    draw_line_thick(img, from, to, color, thickness, w, h);
    draw_arrowhead(img, from, to, color, w, h);
}

/// Fat line via a series of circles — simple and correct, adequate at our
/// small thickness values.
fn draw_line_thick(
    img: &mut RgbaImage,
    from: (i32, i32),
    to: (i32, i32),
    color: Rgba<u8>,
    thickness: i32,
    w: i32,
    h: i32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = fx + dx * t;
        let y = fy + dy * t;
        fill_disk(img, x, y, r, color, w, h);
    }
}

fn fill_disk(img: &mut RgbaImage, cx: f64, cy: f64, r: f64, color: Rgba<u8>, w: i32, h: i32) {
    let r_ceil = r.ceil() as i32;
    let r2 = r * r;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64;
            let fy = dy as f64;
            if fx * fx + fy * fy <= r2 {
                put_clamped(img, cx as i32 + dx, cy as i32 + dy, w, h, color);
            }
        }
    }
}

fn draw_arrowhead(img: &mut RgbaImage, from: (i32, i32), to: (i32, i32), color: Rgba<u8>, w: i32, h: i32) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    // Two base points of the arrowhead triangle: perpendicular to (ux, uy)
    // at distance ARROWHEAD_LEN behind the tip.
    let base_x = tx - ux * ARROWHEAD_LEN as f64;
    let base_y = ty - uy * ARROWHEAD_LEN as f64;
    let px = -uy;  // perpendicular unit vector
    let py = ux;
    let half_w = ARROWHEAD_LEN as f64 * 0.4;
    let a = (base_x + px * half_w, base_y + py * half_w);
    let b = (base_x - px * half_w, base_y - py * half_w);
    let tip = (tx, ty);
    // Scanline fill of the triangle.
    fill_triangle(img, tip, a, b, color, w, h);
}

fn fill_triangle(
    img: &mut RgbaImage,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    color: Rgba<u8>,
    w: i32,
    h: i32,
) {
    let min_x = p0.0.min(p1.0).min(p2.0).floor() as i32;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil() as i32;
    let min_y = p0.1.min(p1.1).min(p2.1).floor() as i32;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            if point_in_triangle((px, py), p0, p1, p2) {
                put_clamped(img, x, y, w, h, color);
            }
        }
    }
}

fn point_in_triangle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(neg && pos)
}

fn sign(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (p.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (p.1 - b.1)
}

pub fn apply_mosaic_on_image(img: &mut RgbaImage, rect: Rect, block_size: u32) {
    if rect.w <= 0 || rect.h <= 0 || block_size == 0 {
        return;
    }
    let bs = block_size as i32;
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(iw);
    let y1 = (rect.y + rect.h).min(ih);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let block_x1 = (bx + bs).min(x1);
            let block_y1 = (by + bs).min(y1);
            // Average the block
            let mut sum = [0u64; 4];
            let mut count: u64 = 0;
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    let p = img.get_pixel(xx as u32, yy as u32).0;
                    sum[0] += p[0] as u64;
                    sum[1] += p[1] as u64;
                    sum[2] += p[2] as u64;
                    sum[3] += p[3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                bx += bs;
                continue;
            }
            let avg = Rgba([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ]);
            // Paint the block
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    img.put_pixel(xx as u32, yy as u32, avg);
                }
            }
            bx += bs;
        }
        by += bs;
    }
}

fn put_clamped(img: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && x < w && y < h {
        img.put_pixel(x as u32, y as u32, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Rgba<u8> { Rgba([255, 59, 48, 255]) }

    #[test]
    fn draw_rect_outline_basic() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0u8, 0, 0, 255]));
        let rect = Rect { x: 5, y: 5, w: 10, h: 10 };
        draw_rect_outline_on_image(&mut img, rect, red(), 1);
        // Top edge pixel should be red
        assert_eq!(img.get_pixel(5, 5).0, [255, 59, 48, 255]);
        // Interior should still be black
        assert_eq!(img.get_pixel(10, 10).0, [0, 0, 0, 255]);
    }

    #[test]
    fn draw_rect_outline_zero_dims_noop() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([0u8, 0, 0, 255]));
        let rect = Rect { x: 5, y: 5, w: 0, h: 0 };
        draw_rect_outline_on_image(&mut img, rect, red(), 1);
        // No pixels should have changed.
        for px in img.pixels() {
            assert_eq!(px.0, [0, 0, 0, 255]);
        }
    }

    #[test]
    fn mosaic_averages_block() {
        let mut img = RgbaImage::new(8, 8);
        // Half red, half blue — the 8×8 block average should be (128, 0, 128, 255).
        for y in 0..8 {
            for x in 0..8 {
                let c = if x < 4 {
                    Rgba([255u8, 0, 0, 255])
                } else {
                    Rgba([0u8, 0, 255, 255])
                };
                img.put_pixel(x, y, c);
            }
        }
        apply_mosaic_on_image(&mut img, Rect { x: 0, y: 0, w: 8, h: 8 }, 8);
        let expected = [127u8, 0, 127, 255]; // 2040/16 = 127 (int div)
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(img.get_pixel(x, y).0, expected, "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn mosaic_blocks_are_independent() {
        let mut img = RgbaImage::new(16, 8);
        // Left 8×8 solid red, right 8×8 solid blue.
        for y in 0..8 {
            for x in 0..16 {
                let c = if x < 8 {
                    Rgba([255u8, 0, 0, 255])
                } else {
                    Rgba([0u8, 0, 255, 255])
                };
                img.put_pixel(x, y, c);
            }
        }
        apply_mosaic_on_image(&mut img, Rect { x: 0, y: 0, w: 16, h: 8 }, 8);
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(7, 7).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(8, 0).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(15, 7).0, [0, 0, 255, 255]);
    }

    #[test]
    fn paint_on_cropped_translates() {
        // 20×20 black image; simulate a crop at offset (10, 10) — so a Rect
        // annotation at frame (15, 15, 5, 5) should land at crop-local (5, 5, 5, 5).
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0u8, 0, 0, 255]));
        paint_on_cropped(
            &mut img,
            Annotation::Rect { rect: Rect { x: 15, y: 15, w: 5, h: 5 } },
            (10, 10),
        );
        // Pixel at (15-10, 15-10) = (5, 5) should be red after translation.
        assert_eq!(img.get_pixel(5, 5).0, [255, 59, 48, 255]);
    }
}
```

- [ ] **Step 2: Declare the module**

Edit `src/overlay/mod.rs`. Add `pub(crate) mod annotate_render;` alphabetically (between `annotate` and `hit`). After:
```rust
pub(crate) mod annotate;
pub(crate) mod annotate_render;
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;
```

- [ ] **Step 3: Build + test**

```bash
cargo test overlay::annotate_render::tests
```
Expected: 4 tests pass.

```bash
cargo test
```
Expected: 83 + 4 = 87 passed / 0 failed / 2 ignored.

```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. Public functions will be dead-code; add `#![allow(dead_code)]` at the top of the file with a comment "wired in Tasks 5–7". Remove in Task 7.

- [ ] **Step 4: Commit**

```bash
git add src/overlay/annotate_render.rs src/overlay/mod.rs
git commit -m "feat(annotate_render): image-path helpers for arrow/rect/ellipse/mosaic"
```

---

## Task 3: `annotate_render.rs` buf-path helpers (live preview)

Add softbuffer-targeted variants for live preview during Adjusting state. These map frame-space annotation coords into window-space then paint directly into the u32 softbuffer.

**Files:**
- Modify: `src/overlay/annotate_render.rs`

- [ ] **Step 1: Append buf-path helpers**

Append to the end of `src/overlay/annotate_render.rs` (after the last `}` of `apply_mosaic_on_image` but before `fn put_clamped`):

```rust
// --- buf-path helpers (live softbuffer preview) ---

use image::GenericImageView;

/// Map a frame-space point to a window-space point.
fn frame_to_window(p: (i32, i32), frame_size: (u32, u32), window_size: (u32, u32)) -> (i32, i32) {
    let (fw, fh) = (frame_size.0.max(1) as i64, frame_size.1.max(1) as i64);
    let (ww, wh) = (window_size.0.max(1) as i64, window_size.1.max(1) as i64);
    let x = (p.0 as i64 * ww / fw) as i32;
    let y = (p.1 as i64 * wh / fh) as i32;
    (x, y)
}

fn window_thickness(frame_thickness: i32, frame_size: (u32, u32), window_size: (u32, u32)) -> i32 {
    // Prefer the smaller ratio so thickness doesn't look asymmetric on
    // non-square pixel mappings.
    let ratio_w = window_size.0 as f64 / frame_size.0.max(1) as f64;
    let ratio_h = window_size.1 as f64 / frame_size.1.max(1) as f64;
    ((frame_thickness as f64) * ratio_w.min(ratio_h)).round().max(1.0) as i32
}

pub fn draw_annotation_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    ann: Annotation,
    color_argb: u32,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let thickness_win = window_thickness(ANNOTATION_THICKNESS, frame_size, window_size);
    match ann {
        Annotation::Arrow { from, to } => {
            let f = frame_to_window(from, frame_size, window_size);
            let t = frame_to_window(to, frame_size, window_size);
            draw_line_thick_buf(buf, win_w, win_h, f, t, color_argb, thickness_win);
            draw_arrowhead_buf(buf, win_w, win_h, f, t, color_argb);
        }
        Annotation::Rect { rect } => {
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_rect_outline_buf(buf, win_w, win_h, tl, br, color_argb, thickness_win);
        }
        Annotation::Ellipse { rect } => {
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_ellipse_outline_buf(buf, win_w, win_h, tl, br, color_argb, thickness_win);
        }
        Annotation::Mosaic { rect, block_size } => {
            apply_mosaic_on_buf(buf, win_w, win_h, frame, rect, block_size);
        }
    }
}

/// Render the in-flight pending draw (same routine; uses PendingDraw conversion).
pub fn draw_pending_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    pending: super::annotate::PendingDraw,
    color_argb: u32,
) {
    if let Some(ann) = pending.finalize() {
        draw_annotation_on_buf(buf, win_w, win_h, frame, ann, color_argb);
    }
}

fn put_buf(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
}

fn draw_line_thick_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    from: (i32, i32),
    to: (i32, i32),
    color: u32,
    thickness: i32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = fx + dx * t;
        let y = fy + dy * t;
        fill_disk_buf(buf, w, h, x, y, r, color);
    }
}

fn fill_disk_buf(buf: &mut [u32], w: u32, h: u32, cx: f64, cy: f64, r: f64, color: u32) {
    let r_ceil = r.ceil() as i32;
    let r2 = r * r;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64;
            let fy = dy as f64;
            if fx * fx + fy * fy <= r2 {
                put_buf(buf, w, h, cx as i32 + dx, cy as i32 + dy, color);
            }
        }
    }
}

fn draw_arrowhead_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    from: (i32, i32),
    to: (i32, i32),
    color: u32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let base_x = tx - ux * ARROWHEAD_LEN as f64;
    let base_y = ty - uy * ARROWHEAD_LEN as f64;
    let px = -uy;
    let py = ux;
    let half_w = ARROWHEAD_LEN as f64 * 0.4;
    let a = (base_x + px * half_w, base_y + py * half_w);
    let b = (base_x - px * half_w, base_y - py * half_w);
    let tip = (tx, ty);
    fill_triangle_buf(buf, w, h, tip, a, b, color);
}

fn fill_triangle_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    color: u32,
) {
    let min_x = p0.0.min(p1.0).min(p2.0).floor() as i32;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil() as i32;
    let min_y = p0.1.min(p1.1).min(p2.1).floor() as i32;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let pf = (x as f64 + 0.5, y as f64 + 0.5);
            if point_in_triangle(pf, p0, p1, p2) {
                put_buf(buf, w, h, x, y, color);
            }
        }
    }
}

fn draw_rect_outline_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    tl: (i32, i32),
    br: (i32, i32),
    color: u32,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let (x0, y0) = tl;
    let (x1, y1) = br;
    let half = thickness / 2;
    for ty in 0..thickness {
        let yt = y0 - half + ty;
        let yb = y1 - half + ty;
        for x in x0..=x1 {
            put_buf(buf, w, h, x, yt, color);
            put_buf(buf, w, h, x, yb, color);
        }
    }
    for tx in 0..thickness {
        let xl = x0 - half + tx;
        let xr = x1 - half + tx;
        for y in y0..=y1 {
            put_buf(buf, w, h, xl, y, color);
            put_buf(buf, w, h, xr, y, color);
        }
    }
}

fn draw_ellipse_outline_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    tl: (i32, i32),
    br: (i32, i32),
    color: u32,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let cx = (tl.0 as f64 + br.0 as f64) / 2.0;
    let cy = (tl.1 as f64 + br.1 as f64) / 2.0;
    let rx = ((br.0 - tl.0) as f64).abs() / 2.0;
    let ry = ((br.1 - tl.1) as f64).abs() / 2.0;
    if rx < 1.0 || ry < 1.0 {
        return;
    }
    let half_thick = thickness as f64 / 2.0;
    let band = half_thick / rx.min(ry);
    let pad = (thickness + 2).max(2);
    let x_min = tl.0.min(br.0) - pad;
    let x_max = tl.0.max(br.0) + pad;
    let y_min = tl.1.min(br.1) - pad;
    let y_max = tl.1.max(br.1) + pad;
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let nx = (x as f64 + 0.5 - cx) / rx;
            let ny = (y as f64 + 0.5 - cy) / ry;
            let r = (nx * nx + ny * ny).sqrt();
            if (r - 1.0).abs() <= band {
                put_buf(buf, w, h, x, y, color);
            }
        }
    }
}

fn apply_mosaic_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    rect_frame: Rect,
    block_size: u32,
) {
    if rect_frame.w <= 0 || rect_frame.h <= 0 || block_size == 0 {
        return;
    }
    let frame_size = frame.dimensions();
    let bs = block_size as i32;
    let (iw, ih) = (frame_size.0 as i32, frame_size.1 as i32);
    let x0 = rect_frame.x.max(0);
    let y0 = rect_frame.y.max(0);
    let x1 = (rect_frame.x + rect_frame.w).min(iw);
    let y1 = (rect_frame.y + rect_frame.h).min(ih);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let block_x1 = (bx + bs).min(x1);
            let block_y1 = (by + bs).min(y1);
            // Average the block in frame space
            let mut sum = [0u64; 3];
            let mut count: u64 = 0;
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    let p = frame.view(xx as u32, yy as u32, 1, 1).to_image();
                    let px = p.get_pixel(0, 0).0;
                    sum[0] += px[0] as u64;
                    sum[1] += px[1] as u64;
                    sum[2] += px[2] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                bx += bs;
                continue;
            }
            let avg_argb = ((sum[0] / count) as u32) << 16
                | ((sum[1] / count) as u32) << 8
                | ((sum[2] / count) as u32);
            // Paint the block in window space: map each block corner to window coords
            let win_tl = frame_to_window((bx, by), frame_size, (win_w, win_h));
            let win_br = frame_to_window(
                (block_x1 - 1, block_y1 - 1),
                frame_size,
                (win_w, win_h),
            );
            for y in win_tl.1..=win_br.1 {
                for x in win_tl.0..=win_br.0 {
                    put_buf(buf, win_w, win_h, x, y, avg_argb);
                }
            }
            bx += bs;
        }
        by += bs;
    }
}
```

Note: the `use image::GenericImageView;` at the top of the new block can be consolidated with the existing `use image::{Rgba, RgbaImage};` at the top of the file. Prefer consolidation.

Actually, we don't need `GenericImageView` — `frame.get_pixel(x, y)` works directly on `RgbaImage`. Simplify:

Replace the `apply_mosaic_on_buf` block's `frame.view(...).to_image()` lookup with:
```rust
let px = frame.get_pixel(xx as u32, yy as u32).0;
```
And remove the `use image::GenericImageView;` import entirely.

- [ ] **Step 2: Verify build and clippy**

```bash
cargo build --release
cargo clippy --release --all-targets -- -D warnings
cargo test
```
Expected: 87 tests still pass; clippy clean (dead-code allows from Task 2 still in place).

- [ ] **Step 3: Commit**

```bash
git add src/overlay/annotate_render.rs
git commit -m "feat(annotate_render): buf-path helpers for live softbuffer preview"
```

---

## Task 4: `toolbar.rs` — layout + icons + hit test

**Files:**
- Create: `src/overlay/toolbar.rs`
- Modify: `src/overlay/mod.rs` (declare `pub(crate) mod toolbar;`)

- [ ] **Step 1: Write `src/overlay/toolbar.rs`**

Create `src/overlay/toolbar.rs`:
```rust
//! Mini toolbar rendered during the Adjusting state. Pure-ish: layout is
//! pure, drawing is straight softbuffer paints.

use super::annotate::Tool;
use super::state::Rect;

pub const TOOLBAR_H: i32 = 30;
pub const TOOLBAR_GAP: i32 = 6;   // distance from selection edge
pub const ICON_SIZE: i32 = 22;
pub const ICON_PAD: i32 = 4;
pub const SEP_WIDTH: i32 = 8;

/// Layout model — all positions in window-space pixels.
pub struct Toolbar {
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub tool_buttons: Vec<ToolButton>,
    pub undo_button: IconButton,
    pub redo_button: IconButton,
}

pub struct ToolButton {
    pub tool: Tool,
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

pub struct IconButton {
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarHit {
    Tool(Tool),
    Undo,
    Redo,
    None,
}

const TOOL_ORDER: [Tool; 5] = [
    Tool::Move,
    Tool::Arrow,
    Tool::Rect,
    Tool::Ellipse,
    Tool::Mosaic,
];

impl Toolbar {
    /// Compute toolbar layout given the selection rect in window coords
    /// and the window size for edge-flip.
    pub fn layout(selection: Rect, window_size: (u32, u32)) -> Toolbar {
        let tools_w =
            TOOL_ORDER.len() as i32 * (ICON_SIZE + ICON_PAD) - ICON_PAD;
        let total_w =
            tools_w + SEP_WIDTH + 2 * (ICON_SIZE + ICON_PAD) - ICON_PAD + 2 * ICON_PAD;
        // Clamp: total_w is icons + padding + separator + undo + redo.
        // Recompute tidily:
        let content_w = tools_w + SEP_WIDTH + (ICON_SIZE + ICON_PAD) + ICON_SIZE;
        let bar_w = content_w + 2 * ICON_PAD;
        let bar_h = TOOLBAR_H;

        // Default: below selection, horizontally centered under the selection.
        let mut bar_x = selection.x + selection.w / 2 - bar_w / 2;
        let below_y = selection.y + selection.h + TOOLBAR_GAP;

        // Clamp horizontally to window.
        let wwi = window_size.0 as i32;
        let whi = window_size.1 as i32;
        if bar_x < 4 {
            bar_x = 4;
        }
        if bar_x + bar_w > wwi - 4 {
            bar_x = (wwi - 4 - bar_w).max(4);
        }

        // Flip above if no room below.
        let bar_y = if below_y + bar_h <= whi - 4 {
            below_y
        } else {
            let above = selection.y - TOOLBAR_GAP - bar_h;
            if above >= 4 {
                above
            } else {
                // No room either way: place at the bottom with a small margin.
                (whi - bar_h - 4).max(4)
            }
        };

        // Build tool buttons left-to-right.
        let mut x = bar_x + ICON_PAD;
        let y = bar_y + (bar_h - ICON_SIZE) / 2;
        let mut tool_buttons = Vec::with_capacity(TOOL_ORDER.len());
        for &t in TOOL_ORDER.iter() {
            tool_buttons.push(ToolButton {
                tool: t,
                origin: (x, y),
                size: (ICON_SIZE, ICON_SIZE),
            });
            x += ICON_SIZE + ICON_PAD;
        }
        // Separator width (just spacing)
        x += SEP_WIDTH;

        let undo_button = IconButton {
            origin: (x, y),
            size: (ICON_SIZE, ICON_SIZE),
        };
        x += ICON_SIZE + ICON_PAD;
        let redo_button = IconButton {
            origin: (x, y),
            size: (ICON_SIZE, ICON_SIZE),
        };

        Toolbar {
            origin: (bar_x, bar_y),
            size: (bar_w, bar_h),
            tool_buttons,
            undo_button,
            redo_button,
        }
    }

    pub fn hit(&self, cursor: (i32, i32)) -> ToolbarHit {
        for btn in &self.tool_buttons {
            if point_in(cursor, btn.origin, btn.size) {
                return ToolbarHit::Tool(btn.tool);
            }
        }
        if point_in(cursor, self.undo_button.origin, self.undo_button.size) {
            return ToolbarHit::Undo;
        }
        if point_in(cursor, self.redo_button.origin, self.redo_button.size) {
            return ToolbarHit::Redo;
        }
        ToolbarHit::None
    }

    pub fn contains(&self, cursor: (i32, i32)) -> bool {
        point_in(cursor, self.origin, self.size)
    }
}

fn point_in(p: (i32, i32), origin: (i32, i32), size: (i32, i32)) -> bool {
    p.0 >= origin.0 && p.1 >= origin.1 && p.0 < origin.0 + size.0 && p.1 < origin.1 + size.1
}

pub fn draw_toolbar(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    toolbar: &Toolbar,
    current_tool: Tool,
    can_undo: bool,
    can_redo: bool,
) {
    // Background pill: black 70% opaque, 4px rounded.
    draw_pill(
        buf,
        win_w,
        win_h,
        toolbar.origin,
        toolbar.size,
        4,
        0x000000,
        0.7,
    );

    // Tool buttons
    for btn in &toolbar.tool_buttons {
        if btn.tool == current_tool {
            // Highlight background: white 20% opaque behind the icon
            draw_pill(buf, win_w, win_h, btn.origin, btn.size, 4, 0xFFFFFF, 0.25);
        }
        draw_tool_icon(buf, win_w, win_h, btn.tool, btn.origin, 0xFFFFFF);
    }

    // Undo
    let undo_color = if can_undo { 0xFFFFFF } else { 0x888888 };
    draw_icon_undo(buf, win_w, win_h, toolbar.undo_button.origin, undo_color);

    // Redo
    let redo_color = if can_redo { 0xFFFFFF } else { 0x888888 };
    draw_icon_redo(buf, win_w, win_h, toolbar.redo_button.origin, redo_color);
}

fn draw_tool_icon(buf: &mut [u32], w: u32, h: u32, tool: Tool, origin: (i32, i32), color: u32) {
    match tool {
        Tool::Move => draw_icon_move(buf, w, h, origin, color),
        Tool::Arrow => draw_icon_arrow(buf, w, h, origin, color),
        Tool::Rect => draw_icon_rect(buf, w, h, origin, color),
        Tool::Ellipse => draw_icon_ellipse(buf, w, h, origin, color),
        Tool::Mosaic => draw_icon_mosaic(buf, w, h, origin, color),
    }
}

// --- icons (simple glyphs inside a 22x22 button) ---

fn draw_icon_move(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Plus sign at center
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    for d in -5..=5 {
        put(buf, w, h, cx + d, cy, color);
        put(buf, w, h, cx, cy + d, color);
    }
    for d in -1..=1 {
        put(buf, w, h, cx + d, cy, color);
        put(buf, w, h, cx, cy + d, color);
    }
}

fn draw_icon_arrow(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Diagonal arrow from (4,16) to (16,4) with arrowhead
    let (x0, y0) = (o.0 + 4, o.1 + 16);
    let (x1, y1) = (o.0 + 16, o.1 + 4);
    draw_line_2px(buf, w, h, x0, y0, x1, y1, color);
    // Arrowhead: three pixels near tip
    put(buf, w, h, x1 - 3, y1, color);
    put(buf, w, h, x1 - 2, y1, color);
    put(buf, w, h, x1, y1 + 2, color);
    put(buf, w, h, x1, y1 + 3, color);
    put(buf, w, h, x1 - 1, y1 + 1, color);
}

fn draw_icon_rect(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    rect_outline(buf, w, h, o.0 + 4, o.1 + 5, 14, 12, color);
}

fn draw_icon_ellipse(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Simple midpoint-ellipse outline in an 14x12 box
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    let rx = 7.0f64;
    let ry = 6.0f64;
    for y in -7..=7 {
        for x in -7..=7 {
            let nx = x as f64 / rx;
            let ny = y as f64 / ry;
            let r = (nx * nx + ny * ny).sqrt();
            if (r - 1.0).abs() < 0.12 {
                put(buf, w, h, cx + x, cy + y, color);
            }
        }
    }
}

fn draw_icon_mosaic(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // 3×3 checker inside 12×12 box
    for row in 0..3 {
        for col in 0..3 {
            if (row + col) % 2 == 0 {
                let x = o.0 + 5 + col * 4;
                let y = o.1 + 5 + row * 4;
                for dy in 0..3 {
                    for dx in 0..3 {
                        put(buf, w, h, x + dx, y + dy, color);
                    }
                }
            }
        }
    }
}

fn draw_icon_undo(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Arc from (14,6) curling left to (6,12), with arrowhead pointing down-left.
    let cx = o.0 + 11;
    let cy = o.1 + 11;
    for deg in 90..=270 {
        let r = 6.0f64;
        let rad = (deg as f64).to_radians();
        let x = cx + (r * rad.cos()) as i32;
        let y = cy + (r * rad.sin()) as i32;
        put(buf, w, h, x, y, color);
    }
    // Arrowhead
    put(buf, w, h, cx - 6, cy - 1, color);
    put(buf, w, h, cx - 5, cy - 2, color);
    put(buf, w, h, cx - 5, cy, color);
    put(buf, w, h, cx - 7, cy - 1, color);
    put(buf, w, h, cx - 6, cy + 1, color);
}

fn draw_icon_redo(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Mirror of undo.
    let cx = o.0 + 11;
    let cy = o.1 + 11;
    for deg in -90..=90 {
        let r = 6.0f64;
        let rad = (deg as f64).to_radians();
        let x = cx + (r * rad.cos()) as i32;
        let y = cy + (r * rad.sin()) as i32;
        put(buf, w, h, x, y, color);
    }
    put(buf, w, h, cx + 6, cy - 1, color);
    put(buf, w, h, cx + 5, cy - 2, color);
    put(buf, w, h, cx + 5, cy, color);
    put(buf, w, h, cx + 7, cy - 1, color);
    put(buf, w, h, cx + 6, cy + 1, color);
}

// --- primitive helpers ---

fn put(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
}

fn rect_outline(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, rw: i32, rh: i32, color: u32) {
    for dx in 0..rw {
        put(buf, w, h, x + dx, y, color);
        put(buf, w, h, x + dx, y + rh - 1, color);
    }
    for dy in 0..rh {
        put(buf, w, h, x, y + dy, color);
        put(buf, w, h, x + rw - 1, y + dy, color);
    }
}

fn draw_line_2px(buf: &mut [u32], w: u32, h: u32, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
    // Bresenham-ish, with 2 px width via offset duplication.
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut x = x0;
    let mut y = y0;
    loop {
        put(buf, w, h, x, y, color);
        put(buf, w, h, x + 1, y, color);
        put(buf, w, h, x, y + 1, color);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Alpha-blended rounded rectangle (background pill).
fn draw_pill(
    buf: &mut [u32],
    w: u32,
    h: u32,
    origin: (i32, i32),
    size: (i32, i32),
    radius: i32,
    color_rgb: u32,
    alpha: f32,
) {
    let (x, y) = origin;
    let (rw, rh) = size;
    let fr = ((color_rgb >> 16) & 0xFF) as f32;
    let fg = ((color_rgb >> 8) & 0xFF) as f32;
    let fb = (color_rgb & 0xFF) as f32;
    for dy in 0..rh {
        for dx in 0..rw {
            // Corner mask
            let in_corner_tl = dx < radius && dy < radius;
            let in_corner_tr = dx >= rw - radius && dy < radius;
            let in_corner_bl = dx < radius && dy >= rh - radius;
            let in_corner_br = dx >= rw - radius && dy >= rh - radius;
            if in_corner_tl || in_corner_tr || in_corner_bl || in_corner_br {
                let cx = if dx < radius { radius } else { rw - radius - 1 };
                let cy = if dy < radius { radius } else { rh - radius - 1 };
                let d2 = (dx - cx).pow(2) + (dy - cy).pow(2);
                if d2 > radius * radius {
                    continue;
                }
            }
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 {
                continue;
            }
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

    fn sel() -> Rect { Rect { x: 200, y: 200, w: 400, h: 300 } }

    #[test]
    fn layout_below_default() {
        let t = Toolbar::layout(sel(), (1440, 900));
        // Toolbar should be below the selection
        assert!(t.origin.1 > sel().y + sel().h);
        // 5 tool buttons + undo + redo
        assert_eq!(t.tool_buttons.len(), 5);
        assert_eq!(t.tool_buttons[0].tool, Tool::Move);
        assert_eq!(t.tool_buttons[4].tool, Tool::Mosaic);
    }

    #[test]
    fn layout_flips_above_when_low() {
        let s = Rect { x: 100, y: 800, w: 300, h: 90 };
        let t = Toolbar::layout(s, (1440, 900));
        assert!(t.origin.1 < s.y);
    }

    #[test]
    fn hit_tool_button() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let btn = &t.tool_buttons[1]; // Arrow
        let c = (btn.origin.0 + 5, btn.origin.1 + 5);
        assert_eq!(t.hit(c), ToolbarHit::Tool(Tool::Arrow));
    }

    #[test]
    fn hit_undo() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let u = &t.undo_button;
        let c = (u.origin.0 + 3, u.origin.1 + 3);
        assert_eq!(t.hit(c), ToolbarHit::Undo);
    }

    #[test]
    fn hit_none_outside() {
        let t = Toolbar::layout(sel(), (1440, 900));
        assert_eq!(t.hit((10, 10)), ToolbarHit::None);
    }

    #[test]
    fn contains_inside_bar() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let c = (t.origin.0 + t.size.0 / 2, t.origin.1 + t.size.1 / 2);
        assert!(t.contains(c));
        assert!(!t.contains((10, 10)));
    }
}
```

- [ ] **Step 2: Declare the module**

Edit `src/overlay/mod.rs`. Add `pub(crate) mod toolbar;` alphabetically after `state`:
```rust
pub(crate) mod annotate;
pub(crate) mod annotate_render;
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;
pub(crate) mod toolbar;
```

- [ ] **Step 3: Build + test**

```bash
cargo test overlay::toolbar::tests
```
Expected: 6 tests pass.

```bash
cargo test
```
Expected: 87 + 6 = 93 passed / 0 failed / 2 ignored.

```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. Add `#![allow(dead_code)]` at top of `toolbar.rs` with a comment "wired in Task 5" — removed in Task 5.

- [ ] **Step 4: Commit**

```bash
git add src/overlay/toolbar.rs src/overlay/mod.rs
git commit -m "feat(toolbar): layout, icons, and hit testing for annotation toolbar"
```

---

## Task 5: Wire tool state + keyboard + toolbar display

Fields on `Overlay`; keyboard handlers for tool switch + undo/redo; toolbar rendered in Adjusting state; mouse behavior NOT yet changed (Task 6).

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add fields to `Overlay`**

Edit `src/overlay/mod.rs`. Find the `Overlay` struct definition. Add three fields at the bottom (after existing `last_redraw` and `font`, before the closing brace):

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
    // NEW — Iter 5a
    pub(crate) tool: annotate::Tool,
    pub(crate) history: annotate::History,
    pub(crate) pending_draw: Option<annotate::PendingDraw>,
}
```

Initialize in `create`:
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
            // NEW
            tool: annotate::Tool::Move,
            history: annotate::History::new(),
            pending_draw: None,
        })
```

Add an import at the top (with other `use super::` imports or the module sibling `use` imports):
```rust
use self::annotate::{self, PendingDraw};
```

(If there's already a `use state::{...};` line, group similarly. Match prevailing import style.)

- [ ] **Step 2: Add keyboard handlers for tools + undo/redo**

Find `fn handle_key(&mut self, key: Key) -> Outcome`. Extend the match:

Current body (approximately):
```rust
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

This is what we had. Replace with a version that also handles tool shortcuts + undo/redo. Keyboard accelerators need modifier-aware parsing. We don't have `Cmd/Ctrl` state tracked from previous Key events; we can use `winit::keyboard::ModifiersState` passed to `KeyboardInput` via the WindowEvent. However, the current `handle_event` strips modifiers:

```rust
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
```

Extend this to also pass the current `modifiers`. winit 0.30 exposes modifiers via `ModifiersChanged` event separately OR via `KeyEvent::logical_key.text()`. Simpler: maintain a `modifiers: ModifiersState` field on Overlay, updated on `ModifiersChanged`.

**Add this field to `Overlay`:**
```rust
    modifiers: winit::keyboard::ModifiersState,
```

Initialize:
```rust
    modifiers: winit::keyboard::ModifiersState::default(),
```

**Handle `ModifiersChanged`:**
In `handle_event`, after the existing match arms, add a new arm:
```rust
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
                Outcome::Continue
            }
```

Add the import at the top of `mod.rs`:
```rust
use winit::keyboard::ModifiersState;
```

**Now rewrite `handle_key` to route Cmd+Z / Cmd+Shift+Z / M/A/R/E/B:**

```rust
    fn handle_key(&mut self, key: Key) -> Outcome {
        // Cmd+Shift+Z → redo; Cmd+Z → undo
        if self.modifiers.super_key() {
            if let Key::Character(s) = &key {
                let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
                if ch == 'z' {
                    let changed = if self.modifiers.shift_key() {
                        self.history.redo()
                    } else {
                        self.history.undo()
                    };
                    if changed {
                        self.window.request_redraw();
                    }
                    return Outcome::Continue;
                }
            }
        }

        // Tool shortcuts (only in Adjusting state)
        if matches!(self.state, OverlayState::Adjusting { .. }) && !self.modifiers.super_key() {
            if let Key::Character(s) = &key {
                let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
                let new_tool = match ch {
                    'm' => Some(annotate::Tool::Move),
                    'a' => Some(annotate::Tool::Arrow),
                    'r' => Some(annotate::Tool::Rect),
                    'e' => Some(annotate::Tool::Ellipse),
                    'b' => Some(annotate::Tool::Mosaic),
                    _ => None,
                };
                if let Some(t) = new_tool {
                    // Switching tool cancels any in-flight mouse drag:
                    // - commit pending draw? → NO, cancel it (user switched tools mid-draw)
                    // - cancel pending Edit in Adjusting {edit: Some} → YES
                    self.pending_draw = None;
                    if let OverlayState::Adjusting { rect, edit: _ } = self.state {
                        self.state = OverlayState::Adjusting { rect, edit: None };
                    }
                    self.tool = t;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
            }
        }

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

- [ ] **Step 3: Update redraw to display annotations + toolbar**

Find the `redraw` method. After the existing anchor-drawing block (which currently only runs when `OverlayState::Adjusting { .. }`), add annotations + toolbar rendering conditional on:
- Annotations: always in Adjusting.
- Toolbar: only in Adjusting (but NOT while pending_draw is active? Still drawn — user can click while drawing, but our pending_draw logic in Task 6 ensures clicks in toolbar aren't drawing).

Current `redraw`:
```rust
        if matches!(self.state, OverlayState::Adjusting { .. }) {
            if let Some(r) = sel_rect {
                render::draw_anchors(&mut buf, w, h, r);
            }
        }
```

Extend (anchors only when Move tool):
```rust
        if let OverlayState::Adjusting { .. } = self.state {
            // Annotations (always)
            if let Some(_r) = sel_rect {
                for ann in self.history.current() {
                    annotate_render::draw_annotation_on_buf(
                        &mut buf,
                        w,
                        h,
                        frame_ref,
                        *ann,
                        ANNOTATION_ARGB,
                    );
                }
                if let Some(pending) = self.pending_draw {
                    annotate_render::draw_pending_on_buf(
                        &mut buf,
                        w,
                        h,
                        frame_ref,
                        pending,
                        ANNOTATION_ARGB,
                    );
                }
            }

            // Anchors only when the Move tool is active.
            if self.tool == annotate::Tool::Move {
                if let Some(r) = sel_rect {
                    render::draw_anchors(&mut buf, w, h, r);
                }
            }

            // Toolbar
            if let Some(sel_win) = sel_rect {
                let tb = toolbar::Toolbar::layout(sel_win, (w, h));
                toolbar::draw_toolbar(
                    &mut buf,
                    w,
                    h,
                    &tb,
                    self.tool,
                    self.history.can_undo(),
                    self.history.can_redo(),
                );
            }
        }
```

Add constant near top of `mod.rs`:
```rust
const ANNOTATION_ARGB: u32 = 0x00_FF_3B_30; // #FF3B30 (softbuffer is 0x00RRGGBB)
```

- [ ] **Step 4: Build + test**

```bash
cargo build --release
cargo test
```
Expected: 93 passed / 0 failed / 2 ignored.

```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean. You MAY need to remove the `#![allow(dead_code)]` from `toolbar.rs` and `annotate_render.rs` now that many functions are called. If new unused warnings appear, either wire them or allow specifically.

Specifically: `annotate_render::draw_*_on_image` and `paint_on_cropped` are still dead (wired in Task 7). Keep those allowed — narrow the annotate_render.rs allow to just those functions OR wait until Task 7 to clean up.

Simplest: keep `#![allow(dead_code)]` at top of both files; Task 8 polish removes.

- [ ] **Step 5: Commit**

```bash
git add src/overlay/mod.rs
git commit -m "feat(overlay): tool state, keyboard shortcuts, toolbar render in Adjusting"
```

---

## Task 6: Wire mouse drawing

In Adjusting + drawing-tool, mouse-drag inside selection creates a PendingDraw; release commits.

**Files:**
- Modify: `src/overlay/mod.rs`

- [ ] **Step 1: Add helper for window→frame point mapping**

Add method on `impl Overlay`:
```rust
    fn window_point_to_frame_point(&self, p: (i32, i32)) -> (i32, i32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1) as i64, size.height.max(1) as i64);
        let (fw, fh) = self.frame.dimensions();
        let x = (p.0 as i64 * fw as i64 / ww) as i32;
        let y = (p.1 as i64 * fh as i64 / wh) as i32;
        (x, y)
    }
```

- [ ] **Step 2: Update `handle_left_press` to start PendingDraw or hit toolbar**

Current body handles double-click, then `OverlayState::Idle/Dragging/Adjusting`. In the Adjusting arm:
```rust
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
```

Replace with:
```rust
            OverlayState::Adjusting { rect, .. } => {
                if is_double_click {
                    match state::on_double_click_adjusting(rect, self.cursor) {
                        Transition::Confirm(r) => return Outcome::Confirmed(r),
                        Transition::Cancel => return Outcome::Cancelled,
                        Transition::Stay => {}
                    }
                }

                // Toolbar click takes priority over any selection interaction.
                let win_size = self.window.inner_size();
                let tb = toolbar::Toolbar::layout(
                    rect.clamp_to((win_size.width, win_size.height)),
                    (win_size.width, win_size.height),
                );
                match tb.hit(self.cursor) {
                    toolbar::ToolbarHit::Tool(t) => {
                        self.pending_draw = None;
                        self.tool = t;
                        // Also cancel any in-flight anchor edit
                        if let OverlayState::Adjusting { rect: r, .. } = self.state {
                            self.state = OverlayState::Adjusting { rect: r, edit: None };
                        }
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Undo => {
                        if self.history.undo() {
                            self.window.request_redraw();
                        }
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Redo => {
                        if self.history.redo() {
                            self.window.request_redraw();
                        }
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::None => {}
                }

                // Drawing tool → start a PendingDraw only when click is inside
                // the selection AND not inside the toolbar (already handled above).
                if self.tool.is_drawing() && rect.contains(self.cursor) {
                    let fp = self.window_point_to_frame_point(self.cursor);
                    self.pending_draw = Some(PendingDraw {
                        tool: self.tool,
                        from_frame: fp,
                        to_frame: fp,
                    });
                    self.window.request_redraw();
                    return Outcome::Continue;
                }

                // Move tool → existing anchor/inside/outside behavior.
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
```

Also — add the `Rect::clamp_to` call. `Toolbar::layout` wants the selection in window coords. We're passing the raw `rect` (which is already in window coords inside Adjusting) but clamp for safety. Actually let's just use `rect` directly; clamping is overkill.

Simplify the toolbar layout call:
```rust
                let tb = toolbar::Toolbar::layout(
                    rect,
                    (win_size.width, win_size.height),
                );
```

- [ ] **Step 3: Update `handle_left_release` to commit PendingDraw**

Current:
```rust
    fn handle_left_release(&mut self) -> Outcome {
        match self.state {
            OverlayState::Dragging { start, end } => {
                ...
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

Add pending_draw commit BEFORE the other arms (pending_draw works in Adjusting):
```rust
    fn handle_left_release(&mut self) -> Outcome {
        // Commit any in-flight annotation draw.
        if let Some(pending) = self.pending_draw.take() {
            if let Some(ann) = pending.finalize() {
                self.history.push(ann);
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }

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

- [ ] **Step 4: Update `CursorMoved` to update PendingDraw**

Find the `CursorMoved` branch in `handle_event`. The current Adjusting branches (`edit: Some`, `edit: None`) should have a pending_draw case added BEFORE them:

Current:
```rust
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                match self.state {
                    OverlayState::Dragging { start, .. } => {
                        self.state = state::on_mouse_move_dragging(start, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { edit: Some(_), .. } => {
                        self.state = state::update_edit(self.state, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { rect, edit: None } => {
                        let icon = hit::cursor_icon_for(hit::classify(self.cursor, rect));
                        self.window.set_cursor(icon);
                    }
                    OverlayState::Idle => {
                        self.request_redraw_throttled();
                    }
                }
                Outcome::Continue
            }
```

Add pending_draw handling (before the `OverlayState::Adjusting { edit: Some(_) }` arm):
```rust
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);

                // If a PendingDraw is active, update its endpoint.
                if self.pending_draw.is_some() {
                    let fp = self.window_point_to_frame_point(self.cursor);
                    if let Some(p) = self.pending_draw.as_mut() {
                        p.to_frame = fp;
                    }
                    self.request_redraw_throttled();
                    return Outcome::Continue;
                }

                match self.state {
                    OverlayState::Dragging { start, .. } => {
                        self.state = state::on_mouse_move_dragging(start, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { edit: Some(_), .. } => {
                        self.state = state::update_edit(self.state, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { rect, edit: None } => {
                        let icon = hit::cursor_icon_for(hit::classify(self.cursor, rect));
                        self.window.set_cursor(icon);
                    }
                    OverlayState::Idle => {
                        self.request_redraw_throttled();
                    }
                }
                Outcome::Continue
            }
```

- [ ] **Step 5: Build + test**

```bash
cargo build --release
cargo test
```
Expected: 93 passed.

```bash
cargo clippy --release --all-targets -- -D warnings
```
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/overlay/mod.rs
git commit -m "feat(overlay): mouse drawing for annotations + toolbar click handling"
```

---

## Task 7: Flatten for export + app.rs confirm update

Make Enter use `overlay.flatten_for_export(rect)` to get the cropped + annotated image. Remove lingering dead-code allows in annotate_render.rs once functions are used.

**Files:**
- Modify: `src/overlay/mod.rs`
- Modify: `src/app.rs`
- Modify: `src/overlay/annotate_render.rs` (remove file-level allow)

- [ ] **Step 1: Add `flatten_for_export` method on Overlay**

Inside `impl Overlay`, add:
```rust
    /// Produce the final cropped + annotated RGBA image for export to clipboard / file.
    pub fn flatten_for_export(&self, rect: Rect) -> image::RgbaImage {
        let frame_rect = self.window_rect_to_frame_rect(rect);
        let mut cropped = crate::crop::crop_rgba(&self.frame, frame_rect);
        let offset = (frame_rect.0 as i32, frame_rect.1 as i32);
        for ann in self.history.current() {
            annotate_render::paint_on_cropped(&mut cropped, *ann, offset);
        }
        cropped
    }
```

Add the import near the top of `mod.rs` (group with other use lines):
```rust
use self::annotate_render;
```
(if you used `use self::annotate::...` earlier, put `annotate_render` next to it).

- [ ] **Step 2: Update `App::confirm` in `src/app.rs`**

Current body:
```rust
    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        match clipboard::put_image(&cropped) {
            Ok(()) => {
                println!("copied {}x{} to clipboard", cropped.width(), cropped.height());
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &cropped,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => {
                eprintln!("clipboard error: {e:?}");
            }
        }
        drop(overlay);
    }
```

Replace the `crop::crop_rgba` + `cropped` handling with `flatten_for_export`:
```rust
    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        match clipboard::put_image(&final_image) {
            Ok(()) => {
                println!(
                    "copied {}x{} to clipboard",
                    final_image.width(),
                    final_image.height()
                );
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &final_image,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => {
                eprintln!("clipboard error: {e:?}");
            }
        }
        drop(overlay);
    }
```

If `use crate::crop;` is no longer used in `app.rs`, remove it. Double-check with `grep crop::` in app.rs after the edit.

- [ ] **Step 3: Remove dead-code allows from annotate_render.rs**

Open `src/overlay/annotate_render.rs`. If there's `#![allow(dead_code)]` at the top, remove it. Expected new state: clippy reports any genuinely-unused items. If ALL functions are now used, no further action. If some image-path helpers still aren't used (e.g., `draw_arrow_on_image` is referenced only inside `paint_on_cropped` which IS used), clippy should see them as reachable.

If specific buf-path helpers remain unused after annotations render (shouldn't — we call draw_annotation_on_buf which calls each internal helper), keep a targeted `#[allow(dead_code)]` on that specific item with a comment "reserved for future use".

- [ ] **Step 4: Remove dead-code allows from annotate.rs**

Same — remove any `#![allow(dead_code)]` or per-item allows added in Task 1 that are no longer needed. Items like `can_undo` / `can_redo` / `current` / `is_drawing` / variants of `Annotation` / `Tool` should all be used now.

- [ ] **Step 5: Remove dead-code allows from toolbar.rs**

Same cleanup.

- [ ] **Step 6: Build, test, clippy**

```bash
cargo build --release
cargo test
cargo clippy --release --all-targets -- -D warnings
```
Expected: 93 passed / 0 failed / 2 ignored. Clippy clean. If any dead-code warning remains, narrow with a targeted `#[allow(dead_code)]` + justification comment.

- [ ] **Step 7: Commit**

```bash
git add src/overlay/mod.rs src/app.rs src/overlay/annotate_render.rs src/overlay/annotate.rs src/overlay/toolbar.rs
git commit -m "feat(app): Enter flattens annotations into the final clipboard PNG"
```

---

## Task 8: Polish + package + tag

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Full test + clippy**

```bash
cargo test
cargo clippy --release --all-targets -- -D warnings
```
Expected: 93 passed / 0 failed / 2 ignored; clippy clean.

- [ ] **Step 2: Repackage**

```bash
rm -rf dist
bash scripts/package.sh
ls -lh dist/
```
Expected: `dist/quickshot.app` + `dist/quickshot-<VERSION>.dmg`. Record sizes.

- [ ] **Step 3: Update `README.md` Status section**

Find `## Status (Iter 3)` (from Iter 3 polish commit). Replace the whole section with:

```markdown
## Status (Iter 5a)

- Region capture via configurable hotkey (default `Cmd+Shift+A`) with drag → anchor-adjust → Enter/double-click confirm + ESC cancel
- **Annotation tools during region capture**: Arrow (A), Rectangle (R), Ellipse (E), Mosaic (B), Move (M) + Undo (Cmd+Z) / Redo (Cmd+Shift+Z)
- **Mini toolbar below selection**: click to switch tool or undo/redo
- Full-screen capture via configurable hotkey (default `Cmd+Shift+S`) — cursor's monitor, clipboard + optional notification
- Menu-bar tray icon with Capture Region / Capture Screen / Edit Config / Start at Login / Quit
- Configurable save-to-disk with templated filenames (`~/.config/quickshot/config.toml`)
- macOS autostart via tray menu or `quickshot --install-autostart`
- Live W × H size label (physical pixels) and 4× magnifier with crosshair + hex/coord readout during region capture
- No cross-screen selection

Release binary size on this machine: <PASTE-SIZE-HERE>.
```

Replace `<PASTE-SIZE-HERE>` with actual `ls -lh dist/quickshot.app/Contents/MacOS/quickshot` output (e.g., `3.2M`).

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs: record Iter 5a status with annotation tools"
```

- [ ] **Step 5: Tag**

```bash
git tag -a v0.6.0-iter5a -m "Iter 5a: annotation tools (arrow, rect, ellipse, mosaic) + toolbar + undo/redo"
```

---

## Manual verification checklist

After all tasks land + merge to master + reinstall from fresh DMG:

1. Hotkey → drag region → release → Adjusting state with Move tool, anchors visible, toolbar visible below selection with M highlighted.
2. Press `A` → toolbar highlights Arrow, anchors hidden.
3. Click-drag inside selection → red arrow follows cursor → release → arrow committed.
4. Press `R` → draw rectangle.
5. Press `E` → draw ellipse.
6. Press `B` → draw mosaic box (pixels inside become blocky).
7. `Cmd+Z` → last annotation disappears.
8. `Cmd+Shift+Z` → annotation returns.
9. Click each toolbar icon → tool switch works, undo/redo work.
10. Press `M` → anchors reappear, existing Iter 2a anchor-resize / click-translate / click-outside-clear all work.
11. Enter → paste into Preview → annotated region appears correctly at expected position.
12. ESC during drawing → overlay closes, clipboard unchanged.
13. Save-enabled config → saved PNG contains the annotations.
14. Full-screen capture (`Cmd+Shift+S`) → straight to clipboard, no annotation UI (unchanged).
15. Multi-display + Retina DPI: annotations render crisply, toolbar visible.
16. Binary size ≤ 1.5 MB (Iter 3 was 1.4 MB; +50-100 KB for annotation code).

Regression:
17. 71 Iter 3 tests still pass + ~22 new Iter 5a tests = 93+.
18. Tray icon appears in menu bar on bundled app launch.
19. Edit Config + Start at Login toggle still work.

---

## Out of scope

**Iter 5b (next iteration):**
- Text tool (text-input state machine)
- Color picker + thickness slider UI
- Per-annotation selection / move / delete individually
- Annotation on full-screen capture flow

**Iter 2c polish (still deferred):**
- TTF subset, coord helper extraction, `estimate_text_width` advance query
