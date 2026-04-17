# quickshot Iter 2a — Selection Interaction Enhancements (Design Spec)

**Date:** 2026-04-17
**Status:** Design approved (per prior brainstorm); ready for implementation plan
**Predecessor:** `docs/superpowers/plans/2026-04-16-quickshot-mvp.md` (Iter 1 MVP, shipped)
**Successor:** Iter 2b (system integration — separate spec)

## Goal

Upgrade the overlay selection UX from the MVP "drag-and-release" model to a Snipaste-style two-phase interaction: drag to draft a region, then refine it via 8 resize anchors / internal drag before confirming with Enter or double-click. Add a magnifier + crosshair during pointing/dragging and a live size label during selection.

These changes are self-contained to the overlay rendering and input layer. No OS-level features, no capture-layer changes, no clipboard/crop changes.

## Non-Goals (explicitly deferred)

- **Cross-screen selection** (a single screenshot spanning multiple monitors) — not worth the complexity; users don't do this.
- **Shift-to-constrain aspect ratio** — future polish.
- **Configurable magnifier / size label** — hardcode for now.
- **All Iter 2b features:** full-screen hotkey `Cmd/Ctrl+Shift+S`, system notifications, tray icon.
- **All Iter 3 features:** settings window, file saving, config persistence, autostart.

---

## UX Specification

### State machine (user-facing)

```
      Hotkey
        │
        ▼
     ┌──────┐   MouseDown   ┌──────────┐  MouseUp  ┌────────────┐
     │ Idle │ ────────────▶ │ Dragging │ ────────▶ │ Adjusting  │
     └──────┘               └──────────┘           └────────────┘
        │                        │                       │
        │                        │                       │  Enter / dblclick inside
        │                        │                       │    → Confirmed → copy → close
        │                        │                       │
        │                        │                       │  MouseDown on anchor → resize edit
        │                        │                       │  MouseDown inside   → translate edit
        │                        │                       │  MouseDown outside  → back to Idle
        │                        │                       │
        └────────────────────────┴───────────────────────┘
                              ESC (any state) → close without copy
```

Note: "resize edit" and "translate edit" stay inside the `Adjusting` state — pressing on an anchor starts a new drag whose release produces a new `Adjusting{rect}`; the state enum doesn't need dedicated `Resizing` / `Translating` variants at the spec level (implementation may model them as a sub-field if cleaner — see Architecture).

### Anchors

- **Count:** 8 per selection (four corners + four edge midpoints).
- **Shape:** 6×6 logical-pixel white squares with a 1 px black border.
- **Visible only in `Adjusting` state.** Hidden during `Idle` and `Dragging`.
- **Hit targets:** 12×12 logical-pixel invisible hit boxes centered on each anchor, so small targets remain clickable without pixel-perfect aim.
- **Cursor:**
  - Corner anchors → diagonal resize cursor (`NWSE`/`NESW` appropriate to the corner).
  - Edge anchors → axis-aligned resize cursor (`NS` for top/bottom, `EW` for left/right).
  - Inside selection (not on an anchor) → move cursor.
  - Outside selection (in `Adjusting`) → default cursor; clicking clears the selection and returns to `Idle`.

### Confirmation

- **Enter key** → crop + copy to clipboard + close overlay.
- **Double-click inside the selection** → same as Enter.
- **ESC** → close overlay without copying (works in any state).

### Magnifier

**Snipaste-style loupe.** Follows the cursor while the user is aiming or drafting.

- **Visible in `Idle` and `Dragging` states only.** Hidden in `Adjusting` (anchors make it redundant and it occludes the preview).
- **Size:** 120×120 logical pixels.
- **Zoom:** 4× (shows a 30×30 logical-pixel source region centered on the cursor).
- **Position:** offset 20 px to the bottom-right of the cursor by default. Flip to top/left when the magnifier would otherwise clip the screen edge (vertical or horizontal independently).
- **Contents (bottom-up layer order):**
  1. 4× upscaled (nearest-neighbor) pixels from the captured frame under the cursor.
  2. A thin 1 px cross at the geometric center (one horizontal + one vertical line spanning the magnifier).
  3. A label strip across the bottom (~18 px tall, black 70 %-opaque background) showing `#RRGGBB Xpx, Ypx` — the color of the pixel exactly under the cursor (hex, uppercase) plus the cursor's physical-pixel coordinates in the captured frame (not window coords).
- **Border:** 1 px solid white outline around the magnifier.
- **Clipping/sampling:** when the 30×30 source window hangs off the frame edge, clamp source sampling to the frame bounds (edge pixels repeat — don't wrap).

### Size label

- **Content:** `W × H` (width and height in **physical pixels of the captured frame**, matching the eventual PNG).
  - `×` is the Unicode multiplication sign U+00D7, with one space either side.
- **Font size:** ~12 px logical.
- **Position:** attached to the selection's top-left corner, sitting **outside the rect, above it**, with a 4 px gap. If there isn't room above (selection too close to the top of the screen), flip to *inside* the selection at the top-left with the same gap.
- **Style:** black background at 70 % opacity, 4 px corner radius, white text, 4 px horizontal padding, 2 px vertical padding.
- **Visible in `Dragging` and `Adjusting` states.** Updates live every redraw.

---

## Architecture

### New/modified files

```
src/
├── app.rs             (modified — translate winit events into state transitions)
├── overlay.rs         (DELETED — content moves into overlay/ submodule below)
├── overlay/
│   ├── mod.rs         (new — Overlay struct; owns Window/Surface/state/frame;
│   │                   winit event ingress; redraw orchestration)
│   ├── state.rs       (new — OverlayState enum + pure transition helpers;
│   │                   unit-tested)
│   ├── hit.rs         (new — cursor → HitZone classification + CursorIcon;
│   │                   unit-tested)
│   └── render.rs      (new — pure draw_* functions for each layer;
│                       key math unit-tested, pixel output hand-verified)
├── text.rs            (new — fontdue glyph rasterization into the softbuffer)
├── capture.rs         (unchanged)
├── clipboard.rs       (unchanged)
├── crop.rs            (unchanged)
├── hotkey.rs          (unchanged)
├── main.rs            (unchanged)
└── permission.rs      (unchanged)
```

Rationale for the submodule split: the current `overlay.rs` would grow to ~600+ lines across rendering, hit-testing, state, and raw winit glue if we bolted Iter 2a onto it. Splitting along natural seams keeps each file focused (<200 lines target) and makes the pure pieces — state transitions, hit-testing, render math — unit-testable without a display.

### Module responsibilities

**`overlay/mod.rs`** (`pub struct Overlay`)
- Owns: winit `Window`, `softbuffer::Surface`, the captured `RgbaImage` frame, monitor geometry, cursor position, and an `OverlayState`.
- Exposes: `create(event_loop, frame, monitor_geom)`, `handle_event(event)` (translates `WindowEvent` into state transitions via `state::`), `redraw()` (orchestrates `render::` calls).
- Responsible for `request_redraw()` after every state-changing event.
- Owns cursor-icon updates (`window.set_cursor_icon(...)`) based on `hit::classify`.

**`overlay/state.rs`**

```rust
pub enum OverlayState {
    Idle,
    Dragging { start: (i32, i32), end: (i32, i32) },
    Adjusting { rect: Rect, edit: Option<Edit> },
}

pub enum Edit {
    // Active drag that will replace `rect` on release.
    Resize { anchor: Anchor, origin: Rect, from: (i32, i32) },
    Translate { origin: Rect, from: (i32, i32) },
}

pub enum Anchor { TL, T, TR, R, BR, B, BL, L }

pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
```

Pure transition helpers (all `pub fn`, no side effects, unit-testable):
- `on_mouse_down(state, cursor, hit_zone) -> OverlayState`
- `on_mouse_move(state, cursor) -> OverlayState` (updates `Dragging.end`, `Edit.*` in-flight rect)
- `on_mouse_up(state) -> OverlayState` (commits `Dragging → Adjusting`; commits `Edit` into the base rect)
- `on_double_click(state, cursor, hit_zone) -> Transition` (where `Transition = Stay(state) | Confirm(rect)`)
- `on_enter(state) -> Transition`
- `on_escape(_) -> Transition::Cancel`

`Rect` helpers:
- `normalize(start, end) -> Rect` (always positive w/h)
- `resize_from_anchor(origin, anchor, dx, dy) -> Rect` (clamps w/h to ≥1; flipping through zero swaps the anchor — acceptable since adjustment continues in the same drag)
- `translate(origin, dx, dy) -> Rect`
- `clamp_to_bounds(rect, bounds) -> Rect`

**`overlay/hit.rs`**

```rust
pub enum HitZone {
    Anchor(Anchor),
    Inside,
    Outside,
}

pub fn classify(cursor: (i32, i32), rect: Rect, anchor_size: i32, hit_pad: i32) -> HitZone;
pub fn cursor_icon_for(zone: HitZone) -> winit::window::CursorIcon;
```

Table-driven unit tests: fixed `rect`, a battery of cursor points covering each of the 9 hit regions (8 anchors + inside) plus outside, assert expected `HitZone` and `CursorIcon`.

**`overlay/render.rs`**

Pure functions taking a `&mut [u32]` softbuffer slice + dimensions + state-derived inputs. No `Overlay` reference; no `Window` calls. One function per layer:

```rust
pub fn draw_background(buf, w, h, frame);
pub fn apply_dim(buf, w, h, selection);
pub fn draw_selection_outline(buf, w, h, rect);
pub fn draw_anchors(buf, w, h, rect, anchor_size);
pub fn draw_magnifier(buf, w, h, frame, cursor, cfg: MagnifierCfg);
pub fn draw_size_label(buf, w, h, rect, frame_size, window_size, font: &Font);
```

Unit-testable geometry helpers colocated with the drawers (no pixel assertions):
- `magnifier_position(cursor, window_size, magnifier_size, offset) -> (i32, i32)` — flip-on-edge logic.
- `size_label_position(rect, label_size, gap) -> (i32, i32, LabelPlacement::{AboveOutside, InsideTopLeft})` — flip-on-top logic.

Pixel-output correctness is hand-verified during manual testing.

**`src/text.rs`**

Thin fontdue wrapper.

```rust
pub struct Font { inner: fontdue::Font, /* glyph cache */ }

impl Font {
    pub fn embedded() -> Self; // loads the baked-in font file via include_bytes!
    pub fn render_text(&mut self, buf: &mut [u32], w: u32, h: u32,
                       x: i32, y: i32, text: &str, px_size: f32, color_rgb: u32);
}
```

- Embedded font: **JetBrains Mono Regular** (OFL 1.1, permissively licensed), subset via `fonttools` to ASCII + `#` + `×` + `.` + `,` + `px` to keep the binary small (~40–100 KB).
  - Rationale over Inter: monospace means size labels and magnifier text don't jitter as digits change, and the subset is smaller.
  - The font file is committed to `assets/fonts/JetBrainsMono-Regular-subset.ttf` and loaded via `include_bytes!`.
- Glyph rasterization is alpha-blended over the existing `buf` content (softbuffer's 0x00RRGGBB format — alpha is mixed into RGB manually).
- A simple `HashMap<(char, u32 /* px_size * 10 */), Bitmap>` cache inside `Font` avoids re-rasterizing the same digits per frame; cache eviction isn't needed at our scale.
- **Fallback behavior:** if font loading fails at startup (corrupt asset, unlikely), `Font::embedded()` returns a `Font` whose `render_text` is a no-op. Overlay still works; labels just don't paint. Never panic.

### `app.rs` changes

Slim. The overlay now owns most of its logic. `App::window_event` becomes:

```rust
match event {
    CloseRequested => { self.overlay = None; }
    event => {
        if let Some(overlay) = self.overlay.as_mut() {
            match overlay.handle_event(event) {
                Outcome::Confirmed(rect) => self.finish_selection(rect),
                Outcome::Cancelled      => self.overlay = None,
                Outcome::Continue       => {}
            }
        }
    }
}
```

`Outcome` is a small enum returned by `Overlay::handle_event` so `app.rs` doesn't need to inspect `OverlayState` directly.

`finish_selection` is updated to take an explicit `Rect` (from confirmation) instead of reading `drag_start` / `drag_end` off the overlay. The rest of its logic (`crop::crop_rgba` + `clipboard::put_image`) is unchanged.

### Data flow (full cycle)

```
Hotkey ───────────────────────────────────────────────────────────────
  → capture::capture_at_cursor()
  → Overlay::create(frame, geom, font) with state=Idle
  → window.request_redraw()

MouseDown  (state=Idle) ──────────────────────────────────────────────
  → state=Dragging{start=cursor, end=cursor}
  → request_redraw

MouseMove  (state=Dragging) ──────────────────────────────────────────
  → state=Dragging{end=cursor}
  → request_redraw  (magnifier + rect + size label update)

MouseUp    (state=Dragging) ──────────────────────────────────────────
  → state=Adjusting{rect=normalize(start,end), edit=None}
  → request_redraw  (anchors appear, magnifier hides)

MouseMove  (state=Adjusting, edit=None) ──────────────────────────────
  → cursor-icon updated from hit::classify
  → no redraw unless icon changed (optional optimization)

MouseDown  (state=Adjusting, hit=Anchor(a)) ──────────────────────────
  → state=Adjusting{rect, edit=Some(Resize{anchor=a, origin=rect, from=cursor})}

MouseMove  (state=Adjusting, edit=Some(Resize)) ──────────────────────
  → update rect = resize_from_anchor(origin, anchor, cursor-from).clamped
  → request_redraw

MouseUp    (state=Adjusting, edit=Some(Resize)) ──────────────────────
  → state=Adjusting{rect, edit=None}

MouseDown  (state=Adjusting, hit=Inside) ─────────────────────────────
  → state=Adjusting{rect, edit=Some(Translate{origin=rect, from=cursor})}
  → (Move & Up follow the same Translate flow)

MouseDown  (state=Adjusting, hit=Outside) ────────────────────────────
  → state=Idle
  → request_redraw  (selection cleared, magnifier shows again)

DoubleClick (state=Adjusting, hit=Inside)  OR  Enter ─────────────────
  → confirm: frame_rect = window_rect_to_frame_rect(rect)
  → crop + clipboard + close overlay

Escape (any state) ───────────────────────────────────────────────────
  → close overlay without copying
```

### Render layer stack (bottom → top)

Every `redraw` paints these in order:

1. **Background:** `render::draw_background` — nearest-neighbor blit of the captured frame to window pixels.
2. **Dim:** `render::apply_dim` — halves RGB everywhere except inside the current selection rect (if any).
3. **Selection outline** (if `Dragging` or `Adjusting`): 1 px white rect outline.
4. **Anchors** (only in `Adjusting`): `render::draw_anchors` — 8 × (6×6 white + 1 px black border) squares.
5. **Magnifier** (only in `Idle` + `Dragging`): `render::draw_magnifier`.
6. **Size label** (only in `Dragging` + `Adjusting`): `render::draw_size_label`.

This matches the memory's layer spec. The order ensures anchors sit above the selection outline, and magnifier + label sit above everything so they're never occluded.

### Text-rendering dependency

Add to `Cargo.toml`:
```toml
fontdue = "0.9"
```

fontdue is a small, pure-Rust TrueType rasterizer. No system-font lookup, no FreeType. Expected release-binary growth: ~100–150 KB (rasterizer code + subset font blob).

---

## Testing strategy

**Unit tests (new):**

1. **`overlay/state.rs`** — exhaustive state transitions:
   - `Idle --MouseDown--> Dragging`
   - `Dragging --MouseMove--> Dragging` (end updates)
   - `Dragging --MouseUp--> Adjusting` (rect = normalized start/end)
   - `Adjusting --MouseDown@Anchor--> Adjusting{edit=Resize}`
   - `Adjusting --MouseDown@Inside--> Adjusting{edit=Translate}`
   - `Adjusting --MouseDown@Outside--> Idle`
   - `Adjusting --Enter / DblClickInside--> Confirm(rect)`
   - `Any --Escape--> Cancel`
   - `normalize_rect` (reversed drag, negative start, zero-area)
   - `resize_from_anchor` (each anchor, including drags that flip w/h through zero)
   - `translate` + `clamp_to_bounds`

2. **`overlay/hit.rs`** — table-driven:
   - Fixed selection rect, list of `(cursor_point, expected_zone, expected_cursor_icon)`
   - At minimum: each of the 8 anchor centers, each anchor corner of its 12×12 hit box, one point just outside each hit box, one point deep inside selection, one point far outside.

3. **`overlay/render.rs`** — geometry math only (pure helpers extracted alongside the draw functions):
   - `magnifier_position`: cursor in center, top-left corner, bottom-right corner, near each edge — assert flip logic.
   - `size_label_position`: rect near top of screen flips to inside; rect in middle stays above-outside.
   - No pixel-output assertions (manual verification only).

**Manual verification (every cycle, before commit):**

1. Hotkey → overlay; magnifier tracks cursor smoothly; color label updates live.
2. Drag a rect → white outline follows; size label shows live `W × H`; magnifier visible throughout drag.
3. Release → anchors appear; magnifier disappears; size label persists.
4. Hover each anchor → correct resize cursor; drag each anchor → rect edge/corner moves as expected.
5. Drag inside selection → whole rect moves; outside stays dim correctly.
6. Click outside → selection clears; back to Idle; magnifier reappears.
7. Enter → copies; paste into Preview confirms exact region.
8. Double-click inside → same as Enter.
9. ESC from any state → no copy; clipboard unchanged.
10. Re-open overlay after each case → no stale state.

**Regression checks (Iter 1 invariants must still hold):**
- Multi-display: capture follows cursor monitor.
- macOS level-1000 window still covers dock and menu bar.
- Post-capture hotkey still fires (no `ControlFlow::Wait` regression).
- Release binary size target: ≤ 700 KB (Iter 1 was ~500 KB; fontdue + font subset adds ~150 KB).

---

## Implementation order (for the plan)

1. **Scaffold overlay submodule split** without behavior change: move existing `overlay.rs` content into `overlay/mod.rs`; create empty `state.rs`, `hit.rs`, `render.rs`; extract existing draw functions into `render.rs`. Verify Iter 1 behavior still works end-to-end before touching interaction.
2. **Add `text.rs` + font asset** with a trivial `render_text` and a smoke test that writes "Hello" into a test buffer.
3. **Add `OverlayState` + transitions** behind the existing MouseDown/MouseMove/MouseUp flow (Idle → Dragging → Adjusting), keeping confirmation still tied to MouseUp temporarily so Iter 1 UX is preserved while the wiring lands.
4. **Switch confirmation** from MouseUp to Enter + double-click; wire ESC. Iter 1 behavior breaks here (expected — the interaction model is changing). This is the first commit where manual testing verifies the new UX.
5. **Add anchors + hit-testing** with cursor-icon updates and resize/translate edits.
6. **Add size label.**
7. **Add magnifier.**
8. **Polish pass:** clippy, binary size check, README update with Iter 2a status + known limitations.

Each step is independently testable and commit-able; the plan (written next) will detail steps and acceptance per task.

---

## Open questions (none blocking)

All UX and architectural decisions were resolved in the prior brainstorm. If any unknown surfaces during implementation (e.g. fontdue on specific macOS SDK versions, softbuffer alpha-blend quirks for the magnifier border), we'll resolve inline and note in the plan.
