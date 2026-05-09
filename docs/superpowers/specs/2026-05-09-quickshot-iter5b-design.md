# quickshot Iter 5b — Annotation Toolkit Completion (Design Spec)

**Date:** 2026-05-09
**Status:** Design approved; ready for implementation plan.
**Predecessor:** Iter 5a (commit `788d782`) — annotation tools (Arrow / Rect / Ellipse / Mosaic), red-only, fixed thickness.
**Successor:** Iter 6 — Smart window snapping + Pin-to-desktop (per the brainstorming roadmap).

## Goal

Bring quickshot's in-overlay annotation experience up to par with WeChat / CleanShot X for the **annotation surface** (color, stroke width, freehand pen, text). After this iter the user can:

- Pick from 4 colors (red / yellow / green / blue) for any drawing tool.
- Pick from 3 stroke widths (2 / 4 / 6 px) for any drawing tool.
- Draw freehand curves (Pen).
- Place text labels on the captured region (Text).

All four colors and three widths are persistent across tool switches within a single capture (global, not per-tool). They reset to the default red + medium on every new capture.

## Non-Goals (explicitly deferred)

- **Smart window snapping** — Iter 6.
- **Pin-to-desktop** (floating screenshot windows) — Iter 6.
- **Long screenshot / scroll capture** — Iter 7+.
- **OCR / text recognition / translation** — Iter 7+.
- **Per-annotation selection, drag-to-move, individual delete** — still only undo/redo as in Iter 5a.
- **Annotation persistence across captures** — every capture starts blank.
- **Custom color picker / hex input / eyedropper** — only the 4 fixed swatches.
- **Custom font / system font picker** — keep using the embedded font from `text.rs`.
- **Pressure-sensitive pen** — explicitly rejected; conflicts with the discrete 3-stroke model.
- **Smoothing for the Pen tool** (Catmull-Rom / Bézier) — raw connect-the-dots is sufficient; can be revisited if user reports rough lines.
- **Fullscreen capture annotation** — annotations remain Region-only (same as Iter 5a).

---

## UX Specification

### Tool model (extension of Iter 5a)

Tools (key → behavior, drag inside selection unless noted):

| Key | Tool | Behavior |
|-----|------|----------|
| `M` | Move | Translate the selection (Iter 2a) |
| `A` | Arrow | Draw an arrow from drag-start to drag-end |
| `R` | Rectangle | Draw a rectangle outline |
| `E` | Ellipse | Draw an ellipse outline |
| `P` | **Pen (new)** | Free-form curve following cursor |
| `T` | **Text (new)** | Click → place caret → type → Enter to commit |
| `B` | Mosaic | Pixelate the drag rectangle |

The Iter 5a invariant is preserved: anchor resize / click-outside-to-clear are only active when `Tool == Move`. Switching to Pen or Text disables anchors the same way as Arrow/Rect/Ellipse already do.

### Color & stroke

Two **global** settings (not per-tool) shared by all drawing tools that consume them:

- **Color** — one of `Red(#FF3B30) / Yellow(#FFCC00) / Green(#34C759) / Blue(#007AFF)`.
- **Stroke** — one of `Thin(2 px) / Medium(4 px) / Thick(6 px)`.

Defaults on every new capture: `Red` + `Medium` (matches today's hardcoded behavior — current users see no change unless they actively switch).

Mosaic ignores both color and stroke (the second toolbar row greys out when Mosaic is the active tool).

For the Text tool, stroke maps to font size:

| Stroke | Font size (px) |
|--------|----------------|
| Thin   | 14 |
| Medium | 20 |
| Thick  | 28 |

This avoids adding a separate font-size control. Users who want a bigger label switch stroke; users who want a thicker arrow do the same gesture.

### Toolbar UI (two rows)

```
┌─────────────────────────────────────────────────┐
│  M   ↗   □   ○   ✎   T   ▣   ⤺   ⤻             │  ← row 1: tools + undo/redo
│  🔴  🟡  🟢  🔵      •   •   ⬤                  │  ← row 2: color swatches + 3 stroke dots
└─────────────────────────────────────────────────┘
```

- **Row 1** (tool row): existing Iter 5a icons in their current order, plus `Pen` (✎) and `Text` (T) inserted after Ellipse and before Mosaic. `⤺ ⤻` (undo/redo) keep their existing positions. There is no toolbar-level "confirm" button; commit is via Enter / double-click as in Iter 5a.
- **Row 2** (style row): four color swatches on the left, a small gap, three stroke-width dots on the right. The dots are filled circles drawn at the literal stroke px width so the choice is WYSIWYG.

**Selected-state visual cues:**
- Active tool: filled rounded-rect background under the icon (existing Iter 5a behavior).
- Active color: thick white ring around the swatch.
- Active stroke: filled accent ring around the dot.

**Disabled state:** When the active tool is `Mosaic`, row 2 renders at 40 % alpha and ignores hits.

**Layout positioning:** Same flip-above-when-no-room-below logic as Iter 5a's single-row toolbar; the entire two-row toolbar is treated as one block by the layout engine.

### Keyboard shortcuts

Existing (preserved):
- `M / A / R / E / B` — switch tool
- `Cmd+Z` — undo
- `Cmd+Shift+Z` — redo
- `Enter` / double-click — confirm
- `Esc` — cancel

New:
- `P` — switch to Pen
- `T` — switch to Text
- `1 / 2 / 3 / 4` — select color (Red / Yellow / Green / Blue)
- `[` — stroke down (thicker → medium → thin clamped at thin)
- `]` — stroke up (thin → medium → thicker clamped at thick)

When Text is the active tool **and** the user is currently composing text (caret visible), all of the above shortcuts are suppressed except `Esc` (discard text) and `Enter` (commit text). Color and stroke shortcuts re-enable after the text is committed or cancelled.

### Text tool input flow

1. User selects T (toolbar click or `T` key).
2. User clicks any point inside the selection rectangle.
3. State transitions to `OverlayState::TextEditing { origin, buffer: String }`.
4. Subsequent `KeyboardInput` events accumulate into `buffer`. Backspace removes one char. Other named keys (Tab, etc.) are ignored. Cursor is rendered as a blinking `|` glyph at the end of the current text run.
5. `Enter` commits `Annotation::Text { origin, content: buffer, style: current_style }` to history; state returns to `OverlayState::Adjusting`.
6. `Esc` discards `buffer`; state returns to `OverlayState::Adjusting`.
7. Clicking elsewhere (inside or outside selection) **also** commits the current text and starts a new edit at the click location (only if Text is still the active tool; otherwise just commits).

Multi-line is supported: `Shift+Enter` inserts a literal newline, `Enter` alone commits. (Rationale: matches WeChat behavior.)

### Pen tool input flow

1. User selects ✎ (Pen).
2. mouse press inside selection → `PendingDraw::Pen { points: vec![p0] }`.
3. mouse move → if `distance(last_point, current) >= 1 px` → push current to `points`. (Prevents 240 Hz cursor sampling from blowing up `points`.)
4. mouse release → finalize as `Annotation::Pen { points, style: current_style }`.

A pen stroke with fewer than 2 points (single click without drag) is discarded — no zero-length annotation in history.

---

## Implementation

### Data model (`src/overlay/annotate.rs`)

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Tool { Move, Arrow, Rect, Ellipse, Mosaic, Pen, Text }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Color { Red, Yellow, Green, Blue }

impl Color {
    pub fn argb(self) -> u32 {
        match self {
            Color::Red    => 0x00_FF_3B_30,
            Color::Yellow => 0x00_FF_CC_00,
            Color::Green  => 0x00_34_C7_59,
            Color::Blue   => 0x00_00_7A_FF,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Stroke { Thin, Medium, Thick }

impl Stroke {
    pub fn px(self) -> u32 {
        match self { Self::Thin => 2, Self::Medium => 4, Self::Thick => 6 }
    }
    pub fn font_px(self) -> u32 {
        match self { Self::Thin => 14, Self::Medium => 20, Self::Thick => 28 }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct AnnotationStyle { pub color: Color, pub stroke: Stroke }

impl Default for AnnotationStyle {
    fn default() -> Self { Self { color: Color::Red, stroke: Stroke::Medium } }
}

#[derive(Clone, Debug)]
pub enum Annotation {
    Arrow   { from: (i32, i32), to: (i32, i32), style: AnnotationStyle },
    Rect    { rect: (u32, u32, u32, u32),       style: AnnotationStyle },
    Ellipse { rect: (u32, u32, u32, u32),       style: AnnotationStyle },
    Mosaic  { rect: (u32, u32, u32, u32) },
    Pen     { points: Vec<(i32, i32)>,           style: AnnotationStyle },
    Text    { origin: (i32, i32), content: String, style: AnnotationStyle },
}
```

`Annotation` variants for Arrow/Rect/Ellipse gain a `style` field — this is a breaking change to existing pattern matches in `annotate_render.rs` and `History`. Migration: replace the hardcoded `ANNOTATION_ARGB` constant in `overlay/mod.rs` (line 22) with per-annotation `style.color.argb()` reads at the rendering site.

### Overlay state (`src/overlay/mod.rs`)

`Overlay` gains:

```rust
pub(crate) current_style: AnnotationStyle,    // default red+medium
pub(crate) text_edit: Option<TextEdit>,        // Some when composing
```

```rust
pub(crate) struct TextEdit {
    pub origin_frame: (i32, i32),
    pub buffer: String,
    pub last_blink: std::time::Instant,
    pub cursor_visible: bool,
}
```

`OverlayState` does **not** gain a new variant — text editing is layered onto `Adjusting` via `text_edit: Option<TextEdit>`. This keeps the state machine flat and the existing toolbar/anchor logic unchanged.

### Toolbar (`src/overlay/toolbar.rs`)

Hit type extended:

```rust
pub enum ToolbarHit {
    Tool(Tool),
    Undo,
    Redo,
    Color(Color),                 // new
    Stroke(Stroke),               // new
    None,
}
```

`Toolbar::layout` returns a struct describing both rows; `Toolbar::hit` walks both rows. Layout math:
- Row 1 width = (existing tool count + 2 new) × icon_size + dividers + undo/redo + confirm.
- Row 2 width: 4 swatches + gap + 3 dots, sized to fit under row 1 (right-aligned within row 1's bounding box).
- Row 2 is rendered at 40 % alpha + ignored by `hit` when `active_tool == Mosaic`.

`draw_toolbar` renders both rows; selection-cue renderers (color ring, stroke accent) are added.

### Renderer (`src/overlay/annotate_render.rs`)

- Replace every read of the old `ANNOTATION_ARGB` constant with `style.color.argb()`.
- Replace hardcoded line-thickness constants with `style.stroke.px()`.
- `paint_pen` (new): iterate `points` pairwise, call existing `stroke_line` with stroke px.
- `paint_text` (new): rasterize `content` through `text.rs`'s embedded font at `style.stroke.font_px()`, blit at `origin` with `style.color.argb()`.
- `flatten_for_export` (in `mod.rs`): no logic change; just dispatches the new variants through the new helpers.

### Input handling (`src/overlay/mod.rs::handle_event`)

Three additions to `handle_key`:

1. `'p'` and `'t'` join the existing tool-shortcut switch (M/A/R/E/B).
2. `'1'..='4'` set `current_style.color`.
3. `'['` and `']'` step `current_style.stroke`.

When `text_edit.is_some()`, `handle_key` first routes the event through `text_edit_handle_key`, which handles printable chars, Backspace, Enter, Esc, Shift+Enter, and **swallows everything else**. The shortcut block above only runs when `text_edit.is_none()`.

`handle_left_press` adds two cases inside `Adjusting`:
- `Tool::Pen` + click inside rect → start `PendingDraw::Pen`.
- `Tool::Text` + click inside rect → if `text_edit.is_some()` commit current, then start new `TextEdit { origin: …, buffer: String::new(), … }`.

### Cursor blink

Driven by the existing `request_redraw_throttled` clock: every redraw, if `text_edit.is_some()` and `now - last_blink >= 530 ms`, toggle `cursor_visible` and reset. No new timer infrastructure needed.

---

## Testing

Pure-types unit tests (no winit / no rendering):

- `Color::argb` returns expected ARGB for each variant.
- `Stroke::px` and `Stroke::font_px` return expected values.
- `AnnotationStyle::default()` is `Red + Medium`.
- Stroke-step helpers: `step_up(Thin) = Medium, step_up(Thick) = Thick`, etc.

Toolbar layout / hit:

- Row 2 layout produces correct rects for 4 colors + 3 strokes given a row 1 width.
- `Toolbar::hit` returns `Color(Yellow)` for a point inside the yellow swatch's rect.
- `Toolbar::hit` returns `Stroke(Thick)` for a point inside the thick-dot rect.
- When `active_tool == Mosaic`, hits inside row 2 return `None`.

Pen flow (mock the cursor stream):

- A run of 5 mouse-move events with 0.5 px deltas produces a single push (sub-pixel filtered).
- A run of 5 mouse-move events with 2 px deltas produces 5 pushes.
- Mouse-press → mouse-release without intermediate move (single click) does **not** push an Annotation::Pen to history.

Text flow (mock the keyboard stream):

- Type `"hi"` then Enter: history has one `Annotation::Text { content: "hi", … }`.
- Type `"hi"` then Esc: history is empty.
- Type `"hi"` then click elsewhere: history has one Text annotation; new TextEdit started at click position.
- Backspace on empty buffer: no-op (no panic).
- Shift+Enter inserts `\n`; subsequent Enter commits the multi-line buffer.

Integration smoke (manual, not automated): screenshot a region → switch through every color × every stroke × every drawing tool → confirm exported PNG matches expectation.

---

## Out-of-spec but worth flagging during implementation

- The current Iter 5a `ANNOTATION_ARGB` constant in `overlay/mod.rs:22` becomes dead after the migration — delete it, don't leave it as a fallback.
- `History` already stores `Annotation` by value, so adding new variants is a clean enum extension; no signature changes there.
- The toolbar's existing icon-rendering pipeline (commit `8a66c5c`, draws icons natively at 3× scale) generalizes to the two new icons (Pen / Text); no DPI work needed.
