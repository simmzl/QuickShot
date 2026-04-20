# quickshot Iter 5a — Annotation Tools (Design Spec)

**Date:** 2026-04-20
**Status:** Design approved (auto via user delegation); ready for plan.
**Predecessor:** Iter 4.1 (merge at `20d3d86`, tag `v0.5.1-iter4.1`) — bundle packaging + tray polish.
**Successor:** Iter 5b (text tool, color/thickness picker) or Iter 2c polish.

## Goal

After the user releases a region-drag, they now enter the `Adjusting` state where anchors let them fine-tune the selection. Iter 5a adds a parallel capability in the same state: **drawing annotations on top of the captured image** before copying. Four tools (Arrow, Rectangle, Ellipse, Mosaic), undo/redo, and a small inline toolbar. Enter flattens annotations into the PNG that lands on clipboard + optional file save.

## Non-Goals (explicitly deferred)

- **Text tool** — requires a text-input state machine (caret, cursor, commit on Enter, ESC to discard just this text). Iter 5b.
- **Color picker / thickness slider UI** — hardcoded red `#FF3B30`, 3px thickness. Iter 5b.
- **Per-annotation selection / drag-to-move / delete individually** — once drawn, annotations are immutable. Only undo/redo. Iter 5b.
- **Annotation persistence** — not saved between sessions; every capture starts with a clean slate.
- **Shape-snapping / grid / alignment guides** — free-form only.
- **Fullscreen capture annotation** — Iter 5a annotations only work in Region-capture flow (Cmd+Shift+A → drag → release → toolbar). Full-screen capture still goes straight to clipboard like today.
- **Pin-to-desktop** — was alternative Iter 5 option ①. Not part of this spec.
- **Iter 2c polish items** — TTF subset, coord helper extraction, `estimate_text_width` advance query; still deferred.

---

## UX Specification

### Tool model

During the `Adjusting` state (post-drag), the user has a current tool. Tools:

| Key | Tool | Behavior on mouse-drag inside selection |
|-----|------|------------------------------------------|
| `M` | Move (default) | Translate the selection (current Iter 2a behavior) |
| `A` | Arrow | Draw a red arrow from drag-start to drag-end |
| `R` | Rectangle | Draw a red rectangle outline between drag-start and drag-end |
| `E` | Ellipse | Draw a red ellipse outline fit in the drag-defined bounding box |
| `B` | Mosaic | Apply pixelation to the drag-defined rectangle (replace area with 8×8-block averaged pixels) |

Keyboard shortcuts:
- `M`, `A`, `R`, `E`, `B` — switch tool
- `Cmd+Z` — undo last annotation
- `Cmd+Shift+Z` — redo last undone annotation
- Enter — confirm: flatten annotations into the cropped PNG, copy to clipboard (+ save if config enabled), close overlay
- ESC — cancel everything: discard annotations, close overlay, clipboard untouched
- Double-click inside selection — same as Enter (existing Iter 2a shortcut)

Anchor resize and click-outside-to-clear are **disabled when a drawing tool is active** (A/R/E/B). Only `Move` tool preserves all existing Iter 2a behaviors. This makes the UX predictable: if you're drawing, mouse-drag always draws; if you're moving, mouse-drag always moves.

When the user switches from a drawing tool back to Move (presses `M`), anchors reappear and existing adjust behavior resumes. Any already-drawn annotations persist until Enter or ESC.

### Toolbar UI

Mini horizontal toolbar rendered below the selection rectangle (flips above when no room below — same logic as size label):

```
┌──────────────────────────────────────────┐
│ [M] [A] [R] [E] [B]  │  [↶] [↷]         │
└──────────────────────────────────────────┘
```

- Icon size: 22×22 logical px each, 4 px inner padding between
- Total toolbar: ~170×30 logical px
- Background: black 70% opaque, 4px rounded corners (same motif as size label)
- Active tool: white-filled 20×20 square background behind its icon
- Hover: subtle white 30%-opacity highlight
- Click any icon → switch to that tool (or run undo/redo)
- Disabled state: Undo icon 40%-opacity when stack is empty; same for Redo
- Icons drawn programmatically (no external asset) — simple glyphs composed from lines/shapes

Toolbar does NOT react to mouse-drag — only clicks. Clicking on the toolbar does not count as drawing.

### Drawing flow (Arrow example)

1. User presses `Cmd+Shift+A` → overlay opens
2. User drags a 400×300 region → releases → `Adjusting` state, anchors visible, Move tool active
3. User presses `A` → tool becomes Arrow, anchors disappear, toolbar shows Arrow highlighted
4. User mouse-drags inside the selection from point P1 to P2 → a red arrow from P1 to P2 appears and stays
5. User presses `A` again (or clicks Rectangle) → draw another shape, or…
6. User presses `M` → anchors reappear, selection can be adjusted again, existing annotations stay
7. Enter → annotations flattened into the cropped RGBA image → clipboard write → overlay closes

### Undo/redo semantics

- Undo stack: push on each completed annotation (mouse-up for drag tools).
- Redo stack: cleared whenever a new annotation is created. Populated by undos.
- Undo with empty stack: no-op.
- Redo with empty stack: no-op.
- Undo-redo works across tool switches: drawing an arrow, then switching to rect, then drawing a rect, then pressing `Cmd+Z` undoes the rect; again undoes the arrow.

### Visual style

- **Color**: solid `#FF3B30` (Apple system red). Same across all shape tools.
- **Line thickness**: 3 px logical (6 px physical on 2× Retina).
- **Arrowhead**: filled triangle, side length ~14 px logical, rotated to match line direction.
- **Rectangle outline**: 3-px thick stroke; interior unchanged.
- **Ellipse outline**: 3-px thick stroke approximated by drawing on the stroke band of `|√((x−cx)² + (y−cy)²)/r − 1| < 3px/2`.
- **Mosaic**: the drag rectangle is re-sampled at 1/8 resolution (each 8×8 block replaced with its average color). Applied DESTRUCTIVELY to the captured frame copy — mosaic is a pixel-level permanent effect, unlike overlaid shapes. Undo restores the affected pixels.
- All shapes are anti-aliased visually (in v1 we accept pixel-stepped edges for simplicity — true AA requires a rasterizer we don't have; shapes at 3px thickness look fine without AA).

### Rendering order

In `Adjusting` state, `redraw` composes in this order (bottom to top):
1. Background (captured frame), dim mask, selection outline — all existing.
2. Anchors — only when tool is Move.
3. Annotations (arrows, rects, ellipses, mosaics) — only the parts inside the current selection rect. Drawn over the undimmed interior pixels.
4. Toolbar.
5. Size label (existing).
6. Magnifier — only in Idle/Dragging (existing), unchanged.

The anti-pattern here is: while the user is drag-drawing a shape, the in-flight shape is also drawn as a preview using the same rendering path (treated as a "pending" annotation at the end of the stack). On mouse-up, it commits.

### Confirmation flatten

When user presses Enter:
1. Compute `frame_rect` from current selection (existing math).
2. Crop the source frame → produce a new RgbaImage.
3. Walk the annotation stack in order, painting each onto the cropped image in its final frame-space coordinates.
4. For Mosaic annotations, the pixel-averaging has been recorded against the ORIGINAL frame coordinates; re-apply to the cropped image.
5. Push final image to clipboard; optionally save (existing path).
6. Close overlay.

This means annotations only show up in the saved PNG — the overlay's on-screen preview is a separate pipeline that includes annotations for feedback, but the "source of truth" is the annotation stack plus the pre-cropped original frame.

---

## Architecture

### File additions

```
src/
├── overlay/
│   ├── annotate.rs         (new ~180 lines — Annotation/Tool types, undo/redo)
│   ├── annotate_render.rs  (new ~200 lines — draw_arrow, draw_rect, draw_ellipse, apply_mosaic)
│   ├── toolbar.rs          (new ~150 lines — toolbar geometry, icon rendering, hit test)
│   ├── mod.rs              (modified — tool state, event dispatch, compose annotations on redraw)
│   ├── state.rs            (unchanged — OverlayState stays as-is; tool is on Overlay not in state)
│   ├── render.rs           (unchanged — existing pure draw helpers stay)
│   ├── hit.rs              (unchanged)
├── app.rs                  (modified — Enter now flattens via overlay.flatten_for_export)
```

### `src/overlay/annotate.rs`

```rust
use super::state::Rect;

/// A single placed annotation in frame-space coordinates (physical pixels of
/// the captured image, not window pixels).
#[derive(Debug, Clone, Copy)]
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

/// In-flight drawing state: user has pressed mouse-down with a drawing tool.
#[derive(Debug, Clone, Copy)]
pub struct PendingDraw {
    pub tool: Tool,
    pub from_frame: (i32, i32),
    pub to_frame: (i32, i32),
}

impl PendingDraw {
    pub fn finalize(self) -> Option<Annotation> {
        match self.tool {
            Tool::Move => None,
            Tool::Arrow => Some(Annotation::Arrow { from: self.from_frame, to: self.to_frame }),
            Tool::Rect => Some(Annotation::Rect { rect: Rect::normalize(self.from_frame, self.to_frame) }),
            Tool::Ellipse => Some(Annotation::Ellipse { rect: Rect::normalize(self.from_frame, self.to_frame) }),
            Tool::Mosaic => Some(Annotation::Mosaic { rect: Rect::normalize(self.from_frame, self.to_frame), block_size: 8 }),
        }
    }
}

/// Annotation history with undo/redo.
pub struct History {
    undo_stack: Vec<Annotation>,
    redo_stack: Vec<Annotation>,
}

impl History {
    pub fn new() -> Self { Self { undo_stack: Vec::new(), redo_stack: Vec::new() } }
    pub fn push(&mut self, a: Annotation) { self.undo_stack.push(a); self.redo_stack.clear(); }
    pub fn undo(&mut self) -> bool {
        if let Some(a) = self.undo_stack.pop() {
            self.redo_stack.push(a);
            true
        } else { false }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(a) = self.redo_stack.pop() {
            self.undo_stack.push(a);
            true
        } else { false }
    }
    pub fn current(&self) -> &[Annotation] { &self.undo_stack }
    pub fn can_undo(&self) -> bool { !self.undo_stack.is_empty() }
    pub fn can_redo(&self) -> bool { !self.redo_stack.is_empty() }
}
```

Unit tests:
- `tool::is_drawing` for each variant.
- `PendingDraw::finalize` for each tool produces expected Annotation.
- `History::push` clears redo.
- `History::undo` / `redo` wiring.
- Drawing 3, undo, undo, draw new → redo stack emptied.

### `src/overlay/annotate_render.rs`

Pure drawing functions that take a `&mut image::RgbaImage` (for flatten to final PNG) OR a `&mut [u32]` softbuffer + window→frame scale (for live preview).

To keep the code DRY and decoupled, functions operate on an `&mut image::RgbaImage` (the flatten path) since the softbuffer live-preview can render annotations by compositing into the softbuffer the same way `draw_background` already handles image → buffer. But to avoid the overhead of re-compositing for every frame, we keep a cached `preview_frame: image::RgbaImage` that is `frame.clone()` + all committed annotations burned in. Pending draw is rendered on top during redraw.

Simpler (decided): TWO rendering paths:
- **On-screen preview** in softbuffer — draw annotations directly into the `buf: &mut [u32]` after `draw_background`, by mapping frame-space coordinates to window-space. Paint each annotation as window-pixels on top of the scaled background.
- **Flatten for export** — clone the frame, apply annotations in frame-space to the clone, then crop.

Functions:
```rust
pub fn draw_arrow_on_buf(buf: &mut [u32], win_w: u32, win_h: u32,
                          frame_w: u32, frame_h: u32,
                          from_frame: (i32, i32), to_frame: (i32, i32),
                          color_rgb: u32, thickness: i32);
pub fn draw_rect_outline_on_buf(..., rect_frame: Rect, ...);
pub fn draw_ellipse_outline_on_buf(..., rect_frame: Rect, ...);
pub fn apply_mosaic_on_buf(buf: &mut [u32], win_w, win_h,
                            frame: &RgbaImage, rect_frame: Rect, block_size: u32);

// Flatten path (burn into RgbaImage):
pub fn draw_arrow_on_image(img: &mut RgbaImage, from: (i32, i32), to: (i32, i32), color: Rgba<u8>, thickness: i32);
pub fn draw_rect_outline_on_image(img: &mut RgbaImage, rect: Rect, color: Rgba<u8>, thickness: i32);
pub fn draw_ellipse_outline_on_image(img: &mut RgbaImage, rect: Rect, color: Rgba<u8>, thickness: i32);
pub fn apply_mosaic_on_image(img: &mut RgbaImage, rect: Rect, block_size: u32);
```

The _on_image functions write into the RGBA image (flatten path), the _on_buf functions write into the softbuffer (preview path; take the frame ref to compute scaling).

To avoid duplicating math, the image-path functions could be the primary implementation; buf-path uses the image functions on a transient `RgbaImage` created from the current buf contents, then blits back. But that's a memory copy per frame. Instead:

**Final design**: the `on_buf` variants are self-contained — they compute window-space positions from frame-space inputs using `(win_w, win_h, frame_w, frame_h)` and directly paint into buf pixels. Code is more repetitive but avoids per-frame allocations.

Unit tests (image-path only; buf-path hand-verified):
- `draw_rect_outline_on_image` with a 100×100 image + small rect → assert specific pixel colors at the outline and interior.
- `apply_mosaic_on_image` with a 32×32 gradient image + block_size=8 → assert each 8×8 block is a single color equal to the block's average.
- Arrow and ellipse skipped for unit tests (visual only; hand-verified).

### `src/overlay/toolbar.rs`

```rust
use super::state::Rect;

pub const TOOLBAR_H: i32 = 30;
pub const TOOLBAR_GAP: i32 = 6;   // from selection edge
pub const ICON_SIZE: i32 = 22;
pub const ICON_SPACING: i32 = 4;

pub struct Toolbar {
    pub tools: Vec<ToolButton>,
    pub undo: IconButton,
    pub redo: IconButton,
    pub origin: (i32, i32),   // top-left of toolbar in WINDOW coords
    pub size: (i32, i32),
}

pub struct ToolButton {
    pub tool: super::annotate::Tool,
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

pub struct IconButton {
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

pub enum ToolbarHit {
    Tool(super::annotate::Tool),
    Undo,
    Redo,
    None,
}

impl Toolbar {
    /// Compute toolbar layout given the selection rect in WINDOW coordinates
    /// and the window size for edge-flipping.
    pub fn layout(selection_window: Rect, window_size: (u32, u32)) -> Toolbar;
    pub fn hit(&self, cursor: (i32, i32)) -> ToolbarHit;
    pub fn contains(&self, cursor: (i32, i32)) -> bool;  // for gate: clicking toolbar ≠ drawing
}

pub fn draw_toolbar(buf: &mut [u32], win_w: u32, win_h: u32,
                   toolbar: &Toolbar, current_tool: super::annotate::Tool,
                   can_undo: bool, can_redo: bool);

pub fn draw_icon_move(buf, x, y, size, tint);
pub fn draw_icon_arrow(buf, x, y, size, tint);
pub fn draw_icon_rect(buf, x, y, size, tint);
pub fn draw_icon_ellipse(buf, x, y, size, tint);
pub fn draw_icon_mosaic(buf, x, y, size, tint);
pub fn draw_icon_undo(buf, x, y, size, tint);
pub fn draw_icon_redo(buf, x, y, size, tint);
```

Icons are tiny geometric primitives drawn with pixel loops. Example: Rectangle icon is an outlined 12×10 rect centered in the 22×22 button. Arrow icon is a small diagonal line with arrowhead. Mosaic icon is a 3×3 grid of squares.

Unit tests:
- `Toolbar::layout` with various selection positions (edge-flip).
- `Toolbar::hit` with known icon positions.

### `src/overlay/mod.rs` changes

Add fields to `Overlay`:
```rust
pub struct Overlay {
    // existing fields...
    pub(crate) tool: super::annotate::Tool,
    pub(crate) history: super::annotate::History,
    pub(crate) pending_draw: Option<super::annotate::PendingDraw>,
    pub(crate) toolbar: Option<super::toolbar::Toolbar>,  // recomputed on redraw
}
```

`handle_event` changes:
- Keyboard input for M/A/R/E/B/Cmd+Z/Cmd+Shift+Z:
  - Tool switch: update `self.tool`; if switching away from Move, cancel any in-flight resize/translate edit. request_redraw.
  - Undo/Redo: update `self.history`. request_redraw.
- `handle_left_press`:
  - If toolbar hit: switch tool or undo/redo based on `ToolbarHit`.
  - Else if in `Adjusting` and tool.is_drawing() and click is inside selection: start `PendingDraw`.
  - Else (Move tool or click outside): existing behavior.
- `handle_left_release`:
  - If `pending_draw.is_some()`: finalize it, push to history, clear `pending_draw`. request_redraw.
  - Else: existing behavior.
- `CursorMoved`:
  - If `pending_draw.is_some()`: update `pending_draw.to_frame` based on cursor mapped into frame space. request_redraw.
  - Else: existing behavior.

`redraw` changes:
- After `draw_background + apply_dim + draw_selection_outline`:
  - In `Adjusting`, after annotations/anchors:
    - If `self.tool == Move`: draw anchors (existing).
    - Compose all `self.history.current()` annotations into the buf (via `annotate_render::draw_*_on_buf`).
    - If `self.pending_draw.is_some()`: draw it as in-progress.
    - Compute toolbar layout, store in `self.toolbar`, draw it via `toolbar::draw_toolbar`.
  - Size label (existing).

New public method:
```rust
/// Produce the final cropped+annotated RGBA image for export.
pub fn flatten_for_export(&self, rect: state::Rect) -> image::RgbaImage {
    let frame_rect = self.window_rect_to_frame_rect(rect);
    let mut cropped = crop::crop_rgba(&self.frame, frame_rect);
    // Annotations are in FRAME coords; translate them so (0,0) is the crop origin.
    let offset = (frame_rect.0 as i32, frame_rect.1 as i32);
    for ann in self.history.current() {
        annotate_render::paint_on_cropped(&mut cropped, *ann, offset);
    }
    cropped
}
```

`paint_on_cropped` is a helper in `annotate_render.rs` that applies an `Annotation` to a crop that was extracted at `offset`.

### `src/app.rs` changes

`confirm(rect)` is updated to use `overlay.flatten_for_export(rect)` instead of crop+clipboard directly:
```rust
fn confirm(&mut self, rect: Rect) {
    let Some(overlay) = self.overlay.take() else { return; };
    let final_image = overlay.flatten_for_export(rect);
    match clipboard::put_image(&final_image) {
        Ok(()) => { /* existing println + save */ }
        Err(e) => { /* existing error */ }
    }
    drop(overlay);
}
```

`Overlay::window_rect_to_frame_rect` becomes `pub` if it isn't already (verify).

### Window→frame coordinate mapping

Already exists in `Overlay::window_rect_to_frame_rect`. We'll add a companion `fn window_point_to_frame_point(&self, p: (i32, i32)) -> (i32, i32)` for mapping the cursor during drawing.

---

## Data Flow

### Drawing an arrow

```
User presses 'A'
  → KeyboardInput handler → self.tool = Arrow
  → request_redraw: toolbar + anchors hidden
User MouseDown inside selection (window coords)
  → handle_left_press: tool.is_drawing() → PendingDraw::Arrow { from_frame: map(click), to_frame: map(click) }
  → request_redraw: pending arrow rendered (zero-length)
User MouseMove
  → pending_draw.to_frame = map(cursor)
  → request_redraw: arrow extends
User MouseUp
  → finalize pending_draw: history.push(Arrow { from, to })
  → pending_draw = None
  → request_redraw: arrow now committed
```

### Undo

```
User presses Cmd+Z
  → history.undo() → redo_stack.push(popped)
  → request_redraw: fewer annotations
```

### Enter (flatten + copy)

```
User presses Enter
  → App::user_event(... the existing Outcome::Confirmed(rect) path)
  → App::confirm(rect):
    → final_image = overlay.flatten_for_export(rect)
      → crop frame → cropped RgbaImage
      → for each ann in history: annotate_render::paint_on_cropped(&mut cropped, ann, frame_offset)
    → clipboard::put_image(&final_image)
    → (config.save.enabled) file_save::save_png(&final_image, ...)
```

### Tool switch cancels in-flight edit

If the user is mid-resize (dragging an anchor with Move tool) and presses `A`:
- Stop here: this cannot happen because drawing keys are only accepted when tool is drawing-capable, but we're specifically talking about pressing `A` DURING a drag. In practice, macOS delivers Key events and MouseInput events serially — the key event would arrive at a time when there's no mouse-drag in progress (or mid-mouse-drag), and we handle it by:
  - If there's a `Adjusting { edit: Some(_) }` from Move tool: commit the edit first, then switch tool.
  - If there's a `PendingDraw`: commit the pending draw first, then switch tool.

Simpler policy: if the user switches tool while any mouse-drag is active, **cancel** the in-flight operation without committing. This keeps implementation clean.

---

## Testing Strategy

### Unit tests

- `annotate.rs`:
  - `Tool::is_drawing` per variant.
  - `PendingDraw::finalize` for each tool → correct `Annotation` variant.
  - `History::push` clears redo stack.
  - `History::undo` + `redo` roundtrip.
  - Push-after-undo clears redo.

- `annotate_render.rs`:
  - `draw_rect_outline_on_image`: 8×8 image, draw 2×2 rect at (2,2) with thickness 1 → assert outline pixel colors at expected positions, interior unchanged.
  - `apply_mosaic_on_image`: 16×16 gradient image, mosaic full rect with block_size 4 → assert each 4×4 block is uniform color equal to block average.

- `toolbar.rs`:
  - `Toolbar::layout` with selection at screen center → toolbar below selection.
  - `Toolbar::layout` with selection near bottom → toolbar flips above.
  - `Toolbar::hit` at known icon centers returns correct `ToolbarHit`.
  - `Toolbar::contains` returns true inside, false outside.

### Manual verification

1. `Cmd+Shift+A` → drag region → release → Adjusting state with Move tool, anchors visible, toolbar visible below selection.
2. Press `A` → anchors vanish, Arrow tool highlighted in toolbar. Cursor inside selection shows a crosshair (we can use default).
3. Drag inside → red arrow follows cursor, arrowhead at the drag endpoint.
4. Release → arrow committed; can be dragged again from a new start point.
5. Press `R` → Rectangle tool active. Drag → red rectangle outline.
6. Press `E` → Ellipse. Drag → red ellipse outline.
7. Press `B` → Mosaic. Drag → rectangle in selection gets pixelated.
8. `Cmd+Z` → last annotation vanishes. Redraw is instant.
9. `Cmd+Shift+Z` → last undone annotation returns.
10. Press `M` → anchors reappear. Existing Iter 2a anchor-resize still works. Annotations preserved.
11. Switch back to a tool, draw more, press Enter → clipboard has the final annotated image with both the anchor-adjusted selection and all annotations flattened.
12. Paste into Preview / Photoshop → PNG contains all shapes at the right positions.
13. ESC at any point → overlay closes, clipboard unchanged from before overlay opened.
14. Click toolbar tool icon → switches tool (same as keyboard).
15. Click toolbar undo/redo → works (same as keyboard).
16. Toolbar below-flips-above when selection near bottom of screen.

Regression:
17. All existing Iter 2a anchor/magnifier/size-label behavior works when tool is Move.
18. Cmd+Shift+S full-screen capture unchanged (no annotations, goes straight to clipboard).
19. Config save + notification continue to work for annotated captures.
20. Existing 71 tests still pass; new tests bring total to ~95.

---

## Implementation order (plan-level)

1. **`annotate.rs` types + undo/redo** — Tool, Annotation, PendingDraw, History. Unit tests. No wiring.
2. **`annotate_render.rs` image-path helpers + unit tests** — draw_rect_outline_on_image, apply_mosaic_on_image with tests; draw_arrow_on_image, draw_ellipse_outline_on_image, paint_on_cropped (visual, hand-verified).
3. **`annotate_render.rs` softbuffer-path helpers** — the `_on_buf` variants for live preview.
4. **`toolbar.rs` layout + hit test + icon rendering** — pure geometry tests; icon rendering hand-verified.
5. **Wire tool state into `Overlay`** — add fields, keyboard handlers for tools + undo/redo, request_redraw on changes. No mouse handling yet; tool state changes + toolbar appears.
6. **Wire mouse drawing** — handle_left_press/release/CursorMoved for pending_draw; compose history + pending into redraw.
7. **`flatten_for_export` + `app.rs confirm` update** — Enter path produces the annotated PNG.
8. **Polish + tag** — README update, clippy, binary size check, `v0.6.0-iter5a`.

Eight tasks, matching prior iteration cadence.

---

## Risks & mitigations

- **Complex UX states** (tool × edit × pending_draw × overlay) — multiple orthogonal state dimensions risk bugs. Mitigation: unit test `annotate.rs` pure logic thoroughly; keep the overlay/mod.rs wiring small and documented.
- **Coordinate mapping bugs** — annotations are stored in frame-space but drawn in window-space during preview and in image-space during flatten. Three coordinate systems: window (DPI-physical), frame (capture physical), image (same as frame). Mitigation: a single `window_point_to_frame_point` helper; rigorously test with non-square windows + non-1:1 scale factors during manual verification.
- **Anti-aliasing absence** — at 3px thickness it's acceptable visually; if ellipse edges look too jaggy, iterate in v1.1.
- **Mosaic over already-annotated pixels** — if user draws an arrow, then applies mosaic over its path, the mosaic should operate on the ORIGINAL frame pixels, not on the arrow-painted preview. Our design stores Mosaic as `{ rect, block_size }` against the original frame; so flatten applies in order (arrow first, then mosaic), which means arrow gets mosaiced too. Alternative: mosaic always refers to original. Decision: apply in order (the user sees this during preview too; consistent with expectation). Can be refined later if users complain.
- **Performance** — redraw each frame recomposes the entire frame + dim + outline + all annotations + toolbar + size label. For a 4K display + 50 annotations, this is a lot of pixels. Mitigation: we already do full-screen redraw for magnifier; annotations add a few hundred pixels of incremental work. Should be fine.
- **Binary size** — adds ~15-25 KB of code. Negligible.

## Open questions

None blocking. Design locked.
