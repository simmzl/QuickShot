//! Floating annotation toolbar shown in the Adjusting state.
//!
//! Visual design (all sizes in points, multiplied by the display scale):
//!   * Main bar — 42pt tall, 12pt corner radius, near-black translucent
//!     surface with a hairline border and soft drop shadow. Groups:
//!     [7 tools] | [undo redo] | [pin save] | [cancel  confirm].
//!   * Style bar — a second, smaller pill under the main bar holding the
//!     color swatches and stroke sizes. Hidden for the Move tool (no style
//!     applies); colors dim under Mosaic (stroke = block size there).
//!   * 32pt hit targets, 18pt Lucide icons, hover highlight, accent-tinted
//!     active tool, and an accent-filled primary "confirm" button.
//!
//! Layout is pure (testable); drawing paints straight into softbuffer.

use super::annotate::{AnnotationStyle, Color, Stroke, Tool};
use super::state::Rect;

// ---- metrics (points) -------------------------------------------------------
const BAR_PAD: f32 = 5.0;
const BTN: f32 = 32.0;
const BTN_GAP: f32 = 2.0;
const SEP_W: f32 = 13.0;
const BAR_RADIUS: f32 = 12.0;
const BTN_RADIUS: f32 = 8.0;
const ICON_PT: f32 = 18.0;
const SUB_H: f32 = 34.0;
const SUB_GAP: f32 = 6.0;
const SUB_CELL: f32 = 24.0;
const SUB_CELL_GAP: f32 = 4.0;
const SWATCH_R: f32 = 8.0;
const EDGE_GAP: f32 = 8.0; // distance from the selection edge
const SCREEN_MARGIN: f32 = 6.0;

// ---- colors (0x00RRGGBB) ------------------------------------------------------
/// Brand/theme color: selection outline, active tool, primary button.
pub const THEME_COLOR: u32 = 0x00_FF_6B_35;
/// Lighter coral, used for hover / preview state (window snap, etc.).
pub const SNAP_PREVIEW_COLOR: u32 = 0x00_FF_A8_88;

const SURFACE: u32 = 0x00_1C_1C_1F;
const SURFACE_ALPHA: f32 = 0.94;
const BORDER: u32 = 0x00_FF_FF_FF;
const BORDER_ALPHA: f32 = 0.10;
const ICON: u32 = 0x00_E8_E8_ED;
const ICON_HOVER: u32 = 0x00_FF_FF_FF;
const ICON_DISABLED: u32 = 0x00_5C_5C_62;
const ICON_ACTIVE: u32 = 0x00_FF_9A_70;
const ACCENT_HOVER: u32 = 0x00_FF_82_52;
const SEP_COLOR: u32 = 0x00_FF_FF_FF;
const STROKE_IDLE: u32 = 0x00_9C_9C_A3;

// Lucide icon codepoints (see assets/fonts/lucide.ttf subset).
const ICON_MOVE: char = '\u{e121}'; // move
const ICON_ARROW: char = '\u{e493}'; // move-up-right
const ICON_SQUARE: char = '\u{e167}'; // square
const ICON_CIRCLE: char = '\u{e076}'; // circle
const ICON_PENCIL: char = '\u{e1f9}'; // pencil
const ICON_TYPE: char = '\u{e198}'; // type
const ICON_GRID_2X2: char = '\u{e4ff}'; // grid-2x2
const ICON_UNDO: char = '\u{e2a1}'; // undo-2
const ICON_REDO: char = '\u{e2a0}'; // redo-2
const ICON_PIN: char = '\u{e259}'; // pin
const ICON_SAVE: char = '\u{e14d}'; // save

pub struct Toolbar {
    /// Bounding box of both bars (layout tests / future callers).
    #[allow(dead_code)]
    pub origin: (i32, i32),
    #[allow(dead_code)]
    pub size: (i32, i32),
    /// Main bar rect (origin, size).
    pub main: ((i32, i32), (i32, i32)),
    /// Style bar rect (origin, size).
    pub sub: ((i32, i32), (i32, i32)),
    /// x-centers of the separators in the main bar.
    pub main_seps: Vec<i32>,
    pub sub_sep: i32,
    pub tool_buttons: Vec<ToolButton>,
    pub undo_button: IconButton,
    pub redo_button: IconButton,
    pub pin_button: IconButton,
    pub save_button: IconButton,
    pub cancel_button: IconButton,
    pub confirm_button: IconButton,
    pub color_buttons: Vec<ColorButton>,
    pub stroke_buttons: Vec<StrokeButton>,
    pub scale: f32,
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
    Pin,
    Save,
    Cancel,
    Confirm,
    Color(Color),
    Stroke(Stroke),
    None,
}

const TOOL_ORDER: [Tool; 7] = [
    Tool::Move, Tool::Arrow, Tool::Rect, Tool::Ellipse,
    Tool::Pen, Tool::Text, Tool::Mosaic,
];
const COLOR_ORDER: [Color; 4] = [Color::Red, Color::Yellow, Color::Green, Color::Blue];
const STROKE_ORDER: [Stroke; 3] = [Stroke::Thin, Stroke::Medium, Stroke::Thick];

/// Whether the style bar applies to `tool`.
fn style_bar_visible(tool: Tool) -> bool {
    tool != Tool::Move
}

impl Toolbar {
    /// `scale` = display backing scale (physical px per point).
    pub fn layout(selection: Rect, window_size: (u32, u32), scale: f32) -> Toolbar {
        let s = scale.max(0.5);
        let px = |v: f32| (v * s).round() as i32;
        let btn = px(BTN);
        let gap = px(BTN_GAP);
        let sep = px(SEP_W);
        let pad = px(BAR_PAD);
        let bar_h = btn + 2 * pad;

        // ---- main bar width
        let groups = [TOOL_ORDER.len() as i32, 2, 2, 2];
        let mut main_w = 2 * pad;
        for (i, &n) in groups.iter().enumerate() {
            main_w += n * btn + (n - 1) * gap;
            if i + 1 < groups.len() {
                main_w += sep;
            }
        }
        // ---- style bar width
        let cell = px(SUB_CELL);
        let cgap = px(SUB_CELL_GAP);
        let sub_pad = px(BAR_PAD);
        let sub_h = px(SUB_H);
        let n_c = COLOR_ORDER.len() as i32;
        let n_s = STROKE_ORDER.len() as i32;
        let sub_w = 2 * sub_pad + n_c * cell + (n_c - 1) * cgap + sep + n_s * cell + (n_s - 1) * cgap;

        let total_w = main_w.max(sub_w);
        let total_h = bar_h + px(SUB_GAP) + sub_h;

        // ---- placement: centered under the selection, flip above if needed
        let (ww, wh) = (window_size.0 as i32, window_size.1 as i32);
        let margin = px(SCREEN_MARGIN);
        let edge = px(EDGE_GAP);
        let mut x = selection.x + selection.w / 2 - main_w / 2;
        x = x.min(ww - margin - main_w).max(margin);
        let below = selection.y + selection.h + edge;
        let y = if below + total_h <= wh - margin {
            below
        } else {
            let above = selection.y - edge - total_h;
            if above >= margin { above } else { (wh - total_h - margin).max(margin) }
        };

        // ---- main bar buttons
        let by = y + pad;
        let mut cx = x + pad;
        let mut main_seps = Vec::new();
        let mut tool_buttons = Vec::with_capacity(TOOL_ORDER.len());
        for (i, &t) in TOOL_ORDER.iter().enumerate() {
            if i > 0 { cx += gap; }
            tool_buttons.push(ToolButton { tool: t, origin: (cx, by), size: (btn, btn) });
            cx += btn;
        }
        let mut next = |cx: &mut i32, first: bool| -> IconButton {
            if first {
                main_seps.push(*cx + sep / 2);
                *cx += sep;
            } else {
                *cx += gap;
            }
            let b = IconButton { origin: (*cx, by), size: (btn, btn) };
            *cx += btn;
            b
        };
        let undo_button = next(&mut cx, true);
        let redo_button = next(&mut cx, false);
        let pin_button = next(&mut cx, true);
        let save_button = next(&mut cx, false);
        let cancel_button = next(&mut cx, true);
        let confirm_button = next(&mut cx, false);

        // ---- style bar (centered under the main bar)
        let sx = x + (main_w - sub_w) / 2;
        let sy = y + bar_h + px(SUB_GAP);
        let cy = sy + (sub_h - cell) / 2;
        let mut cx = sx + sub_pad;
        let mut color_buttons = Vec::with_capacity(COLOR_ORDER.len());
        for (i, &c) in COLOR_ORDER.iter().enumerate() {
            if i > 0 { cx += cgap; }
            color_buttons.push(ColorButton { color: c, origin: (cx, cy), size: (cell, cell) });
            cx += cell;
        }
        let sub_sep = cx + sep / 2;
        cx += sep;
        let mut stroke_buttons = Vec::with_capacity(STROKE_ORDER.len());
        for (i, &st) in STROKE_ORDER.iter().enumerate() {
            if i > 0 { cx += cgap; }
            stroke_buttons.push(StrokeButton { stroke: st, origin: (cx, cy), size: (cell, cell) });
            cx += cell;
        }

        Toolbar {
            origin: (x, y),
            size: (total_w, total_h),
            main: ((x, y), (main_w, bar_h)),
            sub: ((sx, sy), (sub_w, sub_h)),
            main_seps,
            sub_sep,
            tool_buttons,
            undo_button,
            redo_button,
            pin_button,
            save_button,
            cancel_button,
            confirm_button,
            color_buttons,
            stroke_buttons,
            scale: s,
        }
    }

    pub fn hit_with_tool(&self, cursor: (i32, i32), active_tool: Tool) -> ToolbarHit {
        for btn in &self.tool_buttons {
            if point_in(cursor, btn.origin, btn.size) {
                return ToolbarHit::Tool(btn.tool);
            }
        }
        let singles = [
            (&self.undo_button, ToolbarHit::Undo),
            (&self.redo_button, ToolbarHit::Redo),
            (&self.pin_button, ToolbarHit::Pin),
            (&self.save_button, ToolbarHit::Save),
            (&self.cancel_button, ToolbarHit::Cancel),
            (&self.confirm_button, ToolbarHit::Confirm),
        ];
        for (b, hit) in singles {
            if point_in(cursor, b.origin, b.size) {
                return hit;
            }
        }
        if !style_bar_visible(active_tool) {
            return ToolbarHit::None;
        }
        // Colors are disabled under Mosaic (no color concept for mosaic).
        if active_tool != Tool::Mosaic {
            for btn in &self.color_buttons {
                if point_in(cursor, btn.origin, btn.size) {
                    return ToolbarHit::Color(btn.color);
                }
            }
        }
        // Stroke stays interactive under Mosaic (controls block size).
        for btn in &self.stroke_buttons {
            if point_in(cursor, btn.origin, btn.size) {
                return ToolbarHit::Stroke(btn.stroke);
            }
        }
        ToolbarHit::None
    }

    /// Whether `cursor` is over any visible toolbar surface (incl. padding).
    pub fn contains(&self, cursor: (i32, i32), active_tool: Tool) -> bool {
        point_in(cursor, self.main.0, self.main.1)
            || (style_bar_visible(active_tool) && point_in(cursor, self.sub.0, self.sub.1))
    }
}

fn point_in(p: (i32, i32), origin: (i32, i32), size: (i32, i32)) -> bool {
    p.0 >= origin.0 && p.1 >= origin.1 && p.0 < origin.0 + size.0 && p.1 < origin.1 + size.1
}

/// Paint the toolbar. `hover` is the button currently under the cursor.
#[allow(clippy::too_many_arguments)]
pub fn draw_toolbar(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    toolbar: &Toolbar,
    current_tool: Tool,
    current_style: AnnotationStyle,
    can_undo: bool,
    can_redo: bool,
    show_hints: bool,
    hover: ToolbarHit,
    icon_font: &mut crate::icon_font::IconFont,
    label_font: &mut crate::text::Font,
) {
    let s = toolbar.scale;
    let c = Canvas { buf, w: win_w, h: win_h };
    let mut c = c;

    // ---- surfaces
    let show_sub = style_bar_visible(current_tool);
    c.surface(toolbar.main.0, toolbar.main.1, BAR_RADIUS * s, s);
    if show_sub {
        c.surface(toolbar.sub.0, toolbar.sub.1, (BAR_RADIUS - 2.0) * s, s);
    }

    // ---- separators
    let sep_h = 18.0 * s;
    let (my, mh) = (toolbar.main.0 .1 as f32, toolbar.main.1 .1 as f32);
    for &x in &toolbar.main_seps {
        c.fill_rrect(x as f32 - 0.5 * s, my + (mh - sep_h) / 2.0, s.max(1.0), sep_h, 0.0, SEP_COLOR, 0.12);
    }
    if show_sub {
        let (sy, sh) = (toolbar.sub.0 .1 as f32, toolbar.sub.1 .1 as f32);
        let h = 16.0 * s;
        c.fill_rrect(toolbar.sub_sep as f32 - 0.5 * s, sy + (sh - h) / 2.0, s.max(1.0), h, 0.0, SEP_COLOR, 0.12);
    }

    // ---- tool buttons
    for btn in &toolbar.tool_buttons {
        let active = btn.tool == current_tool;
        let hovered = hover == ToolbarHit::Tool(btn.tool);
        let color = if active {
            c.button_bg(btn.origin, btn.size, s, THEME_COLOR, 0.26);
            ICON_ACTIVE
        } else if hovered {
            c.button_bg(btn.origin, btn.size, s, 0xFFFFFF, 0.10);
            ICON_HOVER
        } else {
            ICON
        };
        c.icon(icon_font, btn.origin, btn.size, s, tool_icon_codepoint(btn.tool), color);
    }

    // ---- simple icon buttons
    let simple = [
        (&toolbar.undo_button, ToolbarHit::Undo, ICON_UNDO, can_undo),
        (&toolbar.redo_button, ToolbarHit::Redo, ICON_REDO, can_redo),
        (&toolbar.pin_button, ToolbarHit::Pin, ICON_PIN, true),
        (&toolbar.save_button, ToolbarHit::Save, ICON_SAVE, true),
    ];
    for (b, hit, ch, enabled) in simple {
        let color = if !enabled {
            ICON_DISABLED
        } else if hover == hit {
            c.button_bg(b.origin, b.size, s, 0xFFFFFF, 0.10);
            ICON_HOVER
        } else {
            ICON
        };
        c.icon(icon_font, b.origin, b.size, s, ch, color);
    }

    // ---- cancel (×)
    {
        let b = &toolbar.cancel_button;
        let color = if hover == ToolbarHit::Cancel {
            c.button_bg(b.origin, b.size, s, 0xFFFFFF, 0.10);
            ICON_HOVER
        } else {
            ICON
        };
        let (ox, oy) = (b.origin.0 as f32, b.origin.1 as f32);
        let k = b.size.0 as f32 / BTN;
        let t = 2.0 * s;
        c.line(ox + 11.0 * k, oy + 11.0 * k, ox + 21.0 * k, oy + 21.0 * k, t, color);
        c.line(ox + 21.0 * k, oy + 11.0 * k, ox + 11.0 * k, oy + 21.0 * k, t, color);
    }
    // ---- confirm (✓) — primary action
    {
        let b = &toolbar.confirm_button;
        let bg = if hover == ToolbarHit::Confirm { ACCENT_HOVER } else { THEME_COLOR };
        c.button_bg(b.origin, b.size, s, bg, 1.0);
        let (ox, oy) = (b.origin.0 as f32, b.origin.1 as f32);
        let k = b.size.0 as f32 / BTN;
        let t = 2.4 * s;
        c.line(ox + 10.0 * k, oy + 16.5 * k, ox + 14.2 * k, oy + 20.7 * k, t, 0xFFFFFF);
        c.line(ox + 14.2 * k, oy + 20.7 * k, ox + 22.0 * k, oy + 12.0 * k, t, 0xFFFFFF);
    }

    // ---- style bar
    if show_sub {
        let colors_disabled = current_tool == Tool::Mosaic;
        for btn in &toolbar.color_buttons {
            // Snap centers + radii to whole pixels: fractional geometry
            // smears every edge across two pixels and reads as blurry.
            let (cx, cy) = snap_center(btn.origin, btn.size);
            let r = (SWATCH_R * s).round();
            let alpha = if colors_disabled { 0.3 } else { 1.0 };
            let selected = !colors_disabled && btn.color == current_style.color;
            let hovered = !colors_disabled && hover == ToolbarHit::Color(btn.color);
            if selected {
                // iOS-style: white ring with a dark gap separating it from
                // the swatch, so the selected color stays fully visible.
                c.disk(cx, cy, r + 3.5 * s, 0xFFFFFF, 1.0);
                c.disk(cx, cy, r + 1.5 * s, SURFACE, 1.0);
            } else if hovered {
                c.disk(cx, cy, r + 3.5 * s, 0xFFFFFF, 0.28);
                c.disk(cx, cy, r + 1.5 * s, SURFACE, 1.0);
            }
            c.disk(cx, cy, r, btn.color.argb(), alpha);
        }
        for btn in &toolbar.stroke_buttons {
            let selected = btn.stroke == current_style.stroke;
            let hovered = hover == ToolbarHit::Stroke(btn.stroke);
            if selected {
                c.button_bg(btn.origin, btn.size, s, 0xFFFFFF, 0.14);
            } else if hovered {
                c.button_bg(btn.origin, btn.size, s, 0xFFFFFF, 0.08);
            }
            let (cx, cy) = snap_center(btn.origin, btn.size);
            let r = (match btn.stroke {
                Stroke::Thin => 2.0,
                Stroke::Medium => 3.5,
                Stroke::Thick => 5.5,
            } * s)
                .round();
            let col = if selected || hovered { 0xFFFFFF } else { STROKE_IDLE };
            c.disk(cx, cy, r, col, 1.0);
        }
    }

    // ---- keyboard hint badges
    if show_hints {
        for btn in &toolbar.tool_buttons {
            c.hint(label_font, btn.origin, btn.size, s, tool_hint_char(btn.tool));
        }
        if show_sub && current_tool != Tool::Mosaic {
            for (i, btn) in toolbar.color_buttons.iter().enumerate() {
                c.hint(label_font, btn.origin, btn.size, s, char::from(b'1' + i as u8));
            }
        }
    }
}

/// Center of a button, placed on a pixel center (x.5) when the shape's
/// pixel-rounded radius makes that the crisp position — for our integer
/// radii that's the pixel boundary, i.e. whole-number coordinates.
fn snap_center(origin: (i32, i32), size: (i32, i32)) -> (f32, f32) {
    (
        (origin.0 as f32 + size.0 as f32 / 2.0).round(),
        (origin.1 as f32 + size.1 as f32 / 2.0).round(),
    )
}

fn tool_hint_char(tool: Tool) -> char {
    match tool {
        Tool::Move => 'M',
        Tool::Arrow => 'A',
        Tool::Rect => 'R',
        Tool::Ellipse => 'E',
        Tool::Pen => 'P',
        Tool::Text => 'T',
        Tool::Mosaic => 'B',
    }
}

/// Map a `Tool` to the Lucide glyph codepoint that represents it.
fn tool_icon_codepoint(tool: Tool) -> char {
    match tool {
        Tool::Move => ICON_MOVE,
        Tool::Arrow => ICON_ARROW,
        Tool::Rect => ICON_SQUARE,
        Tool::Ellipse => ICON_CIRCLE,
        Tool::Pen => ICON_PENCIL,
        Tool::Text => ICON_TYPE,
        Tool::Mosaic => ICON_GRID_2X2,
    }
}

// ---- drawing primitives -------------------------------------------------------

pub(crate) struct Canvas<'a> {
    pub buf: &'a mut [u32],
    pub w: u32,
    pub h: u32,
}

/// Signed distance from (px, py) to a rounded rect (negative = inside).
#[inline]
fn sdf_rrect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let r = r.min(hw).min(hh).max(0.0);
    let qx = (px - (x + hw)).abs() - (hw - r);
    let qy = (py - (y + hh)).abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

impl Canvas<'_> {
    #[inline]
    pub(crate) fn blend(&mut self, x: i32, y: i32, color: u32, a: f32) {
        if a <= 0.0 || x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let idx = (y as u32 * self.w + x as u32) as usize;
        let bg = self.buf[idx];
        let a = a.min(1.0);
        let inv = 1.0 - a;
        let ch = |sh: u32| {
            let f = ((color >> sh) & 0xFF) as f32;
            let b = ((bg >> sh) & 0xFF) as f32;
            ((f * a + b * inv) as u32).min(255)
        };
        self.buf[idx] = (ch(16) << 16) | (ch(8) << 8) | ch(0);
    }

    /// Iterate pixels in a float rect's bounding box, calling `f(px, py)`
    /// with the pixel-center coordinates.
    fn for_box(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, mut f: impl FnMut(&mut Self, i32, i32, f32, f32)) {
        let xa = (x0.floor() as i32).max(0);
        let ya = (y0.floor() as i32).max(0);
        let xb = (x1.ceil() as i32).min(self.w as i32 - 1);
        let yb = (y1.ceil() as i32).min(self.h as i32 - 1);
        for y in ya..=yb {
            for x in xa..=xb {
                f(self, x, y, x as f32 + 0.5, y as f32 + 0.5);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fill_rrect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32, alpha: f32) {
        self.for_box(x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0, |c, ix, iy, px, py| {
            let d = sdf_rrect(px, py, x, y, w, h, r);
            let cov = (0.5 - d).clamp(0.0, 1.0);
            c.blend(ix, iy, color, cov * alpha);
        });
    }

    /// Bar surface: soft drop shadow, translucent fill, hairline border.
    pub(crate) fn surface(&mut self, origin: (i32, i32), size: (i32, i32), r: f32, s: f32) {
        let (x, y, w, h) = (origin.0 as f32, origin.1 as f32, size.0 as f32, size.1 as f32);
        // Shadow: offset down, quadratic falloff over `blur`.
        let blur = 14.0 * s;
        let oy = 4.0 * s;
        self.for_box(x - blur, y - blur + oy, x + w + blur, y + h + blur + oy, |c, ix, iy, px, py| {
            let d = sdf_rrect(px, py - oy, x, y, w, h, r);
            if d >= blur {
                return;
            }
            let t = ((d + blur * 0.25) / (blur * 1.25)).clamp(0.0, 1.0);
            let a = 0.45 * (1.0 - t) * (1.0 - t);
            c.blend(ix, iy, 0x000000, a);
        });
        // Fill + 1pt inner border in one pass.
        let bw = s.max(1.0);
        self.for_box(x - 1.0, y - 1.0, x + w + 1.0, y + h + 1.0, |c, ix, iy, px, py| {
            let d = sdf_rrect(px, py, x, y, w, h, r);
            let fill = (0.5 - d).clamp(0.0, 1.0);
            if fill <= 0.0 {
                return;
            }
            c.blend(ix, iy, SURFACE, fill * SURFACE_ALPHA);
            let border = fill - (0.5 - (d + bw)).clamp(0.0, 1.0);
            c.blend(ix, iy, BORDER, border * BORDER_ALPHA);
        });
    }

    fn button_bg(&mut self, origin: (i32, i32), size: (i32, i32), s: f32, color: u32, alpha: f32) {
        self.fill_rrect(
            origin.0 as f32,
            origin.1 as f32,
            size.0 as f32,
            size.1 as f32,
            BTN_RADIUS * s * (size.0 as f32 / (BTN * s)).min(1.0),
            color,
            alpha,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn icon(
        &mut self,
        font: &mut crate::icon_font::IconFont,
        origin: (i32, i32),
        size: (i32, i32),
        s: f32,
        ch: char,
        color: u32,
    ) {
        // render_centered draws the glyph at 85% of the box it's given.
        let ib = (ICON_PT * s / 0.85).round() as i32;
        let o = (origin.0 + (size.0 - ib) / 2, origin.1 + (size.1 - ib) / 2);
        font.render_centered(self.buf, self.w, self.h, o, ib, ch, color);
    }

    pub(crate) fn disk(&mut self, cx: f32, cy: f32, r: f32, color: u32, alpha: f32) {
        self.for_box(cx - r - 1.0, cy - r - 1.0, cx + r + 1.0, cy + r + 1.0, |c, ix, iy, px, py| {
            let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt() - r;
            c.blend(ix, iy, color, (0.5 - d).clamp(0.0, 1.0) * alpha);
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: u32) {
        let hw = width / 2.0;
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = (dx * dx + dy * dy).max(1e-6);
        self.for_box(
            x0.min(x1) - hw - 1.0,
            y0.min(y1) - hw - 1.0,
            x0.max(x1) + hw + 1.0,
            y0.max(y1) + hw + 1.0,
            |c, ix, iy, px, py| {
                let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
                let (qx, qy) = (x0 + t * dx - px, y0 + t * dy - py);
                let d = (qx * qx + qy * qy).sqrt() - hw;
                c.blend(ix, iy, color, (0.5 - d).clamp(0.0, 1.0));
            },
        );
    }

    /// Small accent badge with a keyboard-shortcut glyph at the button's
    /// bottom-right corner.
    fn hint(&mut self, font: &mut crate::text::Font, origin: (i32, i32), size: (i32, i32), s: f32, ch: char) {
        let bw = 14.0 * s;
        let bx = (origin.0 + size.0) as f32 - bw + 2.0 * s;
        let by = (origin.1 + size.1) as f32 - bw + 2.0 * s;
        self.fill_rrect(bx, by, bw, bw, 4.0 * s, 0x000000, 0.35);
        self.fill_rrect(bx + 0.5 * s, by + 0.5 * s, bw - s, bw - s, 3.5 * s, THEME_COLOR, 1.0);
        let fpx = 10.0 * s;
        let mut tmp = [0u8; 4];
        let text = ch.encode_utf8(&mut tmp);
        let tw = font.measure_label(text, fpx);
        let tx = (bx + (bw - tw) / 2.0).round() as i32;
        let ty = (by + (bw - fpx) / 2.0 - 0.5 * s).round() as i32;
        font.render_text(self.buf, self.w, self.h, tx, ty, text, fpx, 0xFFFFFF);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel() -> Rect { Rect { x: 200, y: 200, w: 400, h: 300 } }

    #[test]
    fn layout_below_default() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        assert!(t.origin.1 > sel().y + sel().h);
        assert_eq!(t.tool_buttons.len(), 7);
        assert_eq!(t.tool_buttons[0].tool, Tool::Move);
        assert_eq!(t.tool_buttons[6].tool, Tool::Mosaic);
    }

    #[test]
    fn layout_flips_above_when_low() {
        let s = Rect { x: 100, y: 800, w: 300, h: 90 };
        let t = Toolbar::layout(s, (1440, 900), 1.0);
        assert!(t.origin.1 < s.y);
    }

    #[test]
    fn hit_tool_button() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let btn = &t.tool_buttons[1];
        let c = (btn.origin.0 + 5, btn.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Tool(Tool::Arrow));
    }

    #[test]
    fn hit_undo() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let u = &t.undo_button;
        let c = (u.origin.0 + 3, u.origin.1 + 3);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Undo);
    }

    #[test]
    fn hit_none_outside() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        assert_eq!(t.hit_with_tool((10, 10), Tool::Move), ToolbarHit::None);
    }

    #[test]
    fn contains_inside_bar() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let c = (t.origin.0 + t.size.0 / 2, t.origin.1 + t.size.1 / 2);
        assert!(t.contains(c, Tool::Arrow));
        assert!(!t.contains((10, 10), Tool::Arrow));
    }

    #[test]
    fn tool_order_includes_pen_and_text() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let tools: Vec<Tool> = t.tool_buttons.iter().map(|b| b.tool).collect();
        assert_eq!(tools, vec![
            Tool::Move, Tool::Arrow, Tool::Rect, Tool::Ellipse,
            Tool::Pen, Tool::Text, Tool::Mosaic,
        ]);
    }

    #[test]
    fn layout_has_color_and_stroke_buttons() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        assert_eq!(t.color_buttons.len(), 4);
        assert_eq!(t.stroke_buttons.len(), 3);
        // Row 2 sits below row 1.
        let row1_y = t.tool_buttons[0].origin.1;
        let row2_y = t.color_buttons[0].origin.1;
        assert!(row2_y > row1_y, "row 2 should sit below row 1");
    }

    #[test]
    fn style_bar_hidden_for_move_tool() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let red = &t.color_buttons[0];
        let c = (red.origin.0 + 4, red.origin.1 + 4);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::None);
        assert!(!t.contains(c, Tool::Move));
        assert_eq!(t.hit_with_tool(c, Tool::Rect), ToolbarHit::Color(Color::Red));
    }

    #[test]
    fn cancel_and_confirm_are_last() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        assert!(t.cancel_button.origin.0 > t.save_button.origin.0);
        assert!(t.confirm_button.origin.0 > t.cancel_button.origin.0);
        let c = (t.confirm_button.origin.0 + 3, t.confirm_button.origin.1 + 3);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Confirm);
        let x = (t.cancel_button.origin.0 + 3, t.cancel_button.origin.1 + 3);
        assert_eq!(t.hit_with_tool(x, Tool::Move), ToolbarHit::Cancel);
    }

    #[test]
    fn layout_scales_with_display() {
        let a = Toolbar::layout(sel(), (4000, 3000), 1.0);
        let b = Toolbar::layout(sel(), (4000, 3000), 2.0);
        assert!((b.main.1 .0 - 2 * a.main.1 .0).abs() <= 6);
        assert!((b.main.1 .1 - 2 * a.main.1 .1).abs() <= 2);
    }

    #[test]
    fn hit_returns_color_and_stroke() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let yellow = &t.color_buttons[1];
        let c = (yellow.origin.0 + 4, yellow.origin.1 + 4);
        assert_eq!(t.hit_with_tool(c, Tool::Arrow), ToolbarHit::Color(Color::Yellow));
        let thick = &t.stroke_buttons[2];
        let s = (thick.origin.0 + 4, thick.origin.1 + 4);
        assert_eq!(t.hit_with_tool(s, Tool::Arrow), ToolbarHit::Stroke(Stroke::Thick));
    }

    #[test]
    fn hit_row2_color_returns_none_when_mosaic_active() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let yellow = &t.color_buttons[1];
        let c = (yellow.origin.0 + 4, yellow.origin.1 + 4);
        assert_eq!(t.hit_with_tool(c, Tool::Mosaic), ToolbarHit::None);
    }

    #[test]
    fn hit_stroke_under_mosaic_returns_stroke() {
        // Stroke section is interactive under Mosaic (controls block size).
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let thick = &t.stroke_buttons[2];
        let s = (thick.origin.0 + 4, thick.origin.1 + 4);
        assert_eq!(t.hit_with_tool(s, Tool::Mosaic), ToolbarHit::Stroke(Stroke::Thick));
    }

    #[test]
    fn toolbar_has_pin_button() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        // Pin button sits to the right of redo.
        assert!(t.pin_button.origin.0 > t.redo_button.origin.0);
        // Same vertical center as the rest of row 1.
        assert_eq!(t.pin_button.origin.1, t.redo_button.origin.1);
        // Same size as other icons.
        assert_eq!(t.pin_button.size, t.undo_button.size);
    }

    #[test]
    fn hit_pin_button() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let p = &t.pin_button;
        let c = (p.origin.0 + 5, p.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Pin);
    }

    #[test]
    fn hit_pin_button_works_under_mosaic() {
        // Row 2 is greyed under Mosaic, but row 1 (incl. pin) stays clickable.
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let p = &t.pin_button;
        let c = (p.origin.0 + 5, p.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Mosaic), ToolbarHit::Pin);
    }

    #[test]
    fn toolbar_has_save_button() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        // Save sits to the right of pin on row 1.
        assert!(t.save_button.origin.0 > t.pin_button.origin.0);
        assert_eq!(t.save_button.origin.1, t.pin_button.origin.1);
        assert_eq!(t.save_button.size, t.pin_button.size);
    }

    #[test]
    fn hit_save_button() {
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let s = &t.save_button;
        let c = (s.origin.0 + 5, s.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Save);
    }

    #[test]
    fn hit_save_button_works_under_mosaic() {
        // Save (row 1) stays clickable even under Mosaic.
        let t = Toolbar::layout(sel(), (1440, 900), 1.0);
        let s = &t.save_button;
        let c = (s.origin.0 + 5, s.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Mosaic), ToolbarHit::Save);
    }
}


