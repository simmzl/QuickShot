//! Mini toolbar rendered during the Adjusting state. Pure-ish: layout is
//! pure; drawing is straight softbuffer paints.

use super::annotate::{AnnotationStyle, Color, Stroke, Tool};
use super::state::Rect;

/// All toolbar lengths/sizes are in physical pixels at this scale.
pub const UI_SCALE: i32 = 3;

pub const TOOLBAR_H: i32 = 30 * UI_SCALE;
pub const TOOLBAR_GAP: i32 = 6 * UI_SCALE; // distance from selection edge
pub const ICON_SIZE: i32 = 22 * UI_SCALE;
pub const ICON_PAD: i32 = 4 * UI_SCALE;
pub const SEP_WIDTH: i32 = 8 * UI_SCALE;
pub const ROW_GAP: i32 = 4 * UI_SCALE;
pub const SWATCH_SIZE: i32 = 14 * UI_SCALE;
pub const SWATCH_PAD: i32 = 6 * UI_SCALE;
pub const STROKE_DOT_AREA: i32 = 18 * UI_SCALE;
const PILL_RADIUS: i32 = 4 * UI_SCALE;

/// Stroke thickness (pixels) used by all icons. Scales with UI.
const STROKE: i32 = 4;

pub struct Toolbar {
    pub origin: (i32, i32),
    pub size: (i32, i32),
    pub tool_buttons: Vec<ToolButton>,
    pub undo_button: IconButton,
    pub redo_button: IconButton,
    pub pin_button: IconButton,
    pub color_buttons: Vec<ColorButton>,
    pub stroke_buttons: Vec<StrokeButton>,
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

impl Toolbar {
    pub fn layout(selection: Rect, window_size: (u32, u32)) -> Toolbar {
        // Row 1: tools + sep + undo + redo (existing math, but TOOL_ORDER is now 7).
        let tools_w = TOOL_ORDER.len() as i32 * ICON_SIZE
            + (TOOL_ORDER.len() as i32 - 1) * ICON_PAD;
        let row1_content_w = tools_w
            + SEP_WIDTH
            + (ICON_SIZE + ICON_PAD) + ICON_SIZE
            + ICON_PAD + ICON_SIZE;
        let row1_w = row1_content_w + 2 * ICON_PAD;

        // Row 2: 4 swatches + sep + 3 stroke dots.
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
        if bar_x < 4 {
            bar_x = 4;
        }
        if bar_x + bar_w > wwi - 4 {
            bar_x = (wwi - 4 - bar_w).max(4);
        }

        let bar_y = if below_y + bar_h <= whi - 4 {
            below_y
        } else {
            let above = selection.y - TOOLBAR_GAP - bar_h;
            if above >= 4 {
                above
            } else {
                (whi - bar_h - 4).max(4)
            }
        };

        // Row 1 — tools + undo/redo.
        let row1_x_start = bar_x + (bar_w - row1_content_w) / 2;
        let mut x = row1_x_start;
        let row1_y = bar_y + (TOOLBAR_H - ICON_SIZE) / 2;
        let mut tool_buttons = Vec::with_capacity(TOOL_ORDER.len());
        for &t in TOOL_ORDER.iter() {
            tool_buttons.push(ToolButton {
                tool: t,
                origin: (x, row1_y),
                size: (ICON_SIZE, ICON_SIZE),
            });
            x += ICON_SIZE + ICON_PAD;
        }
        x += SEP_WIDTH;
        let undo_button = IconButton {
            origin: (x, row1_y),
            size: (ICON_SIZE, ICON_SIZE),
        };
        x += ICON_SIZE + ICON_PAD;
        let redo_button = IconButton {
            origin: (x, row1_y),
            size: (ICON_SIZE, ICON_SIZE),
        };
        x += ICON_SIZE + ICON_PAD;
        let pin_button = IconButton {
            origin: (x, row1_y),
            size: (ICON_SIZE, ICON_SIZE),
        };

        // Row 2 — colors + strokes.
        let row2_x_start = bar_x + (bar_w - row2_content_w) / 2;
        let row2_top = bar_y + TOOLBAR_H + ROW_GAP;
        let row2_y = row2_top + (TOOLBAR_H - SWATCH_SIZE) / 2;
        let mut x = row2_x_start;
        let mut color_buttons = Vec::with_capacity(COLOR_ORDER.len());
        for &c in COLOR_ORDER.iter() {
            color_buttons.push(ColorButton {
                color: c,
                origin: (x, row2_y),
                size: (SWATCH_SIZE, SWATCH_SIZE),
            });
            x += SWATCH_SIZE + SWATCH_PAD;
        }
        x += SEP_WIDTH;
        let stroke_y = row2_top + (TOOLBAR_H - STROKE_DOT_AREA) / 2;
        let mut stroke_buttons = Vec::with_capacity(STROKE_ORDER.len());
        for &s in STROKE_ORDER.iter() {
            stroke_buttons.push(StrokeButton {
                stroke: s,
                origin: (x, stroke_y),
                size: (STROKE_DOT_AREA, STROKE_DOT_AREA),
            });
            x += STROKE_DOT_AREA + SWATCH_PAD;
        }

        Toolbar {
            origin: (bar_x, bar_y),
            size: (bar_w, bar_h),
            tool_buttons,
            undo_button,
            redo_button,
            pin_button,
            color_buttons,
            stroke_buttons,
        }
    }

    pub fn hit_with_tool(&self, cursor: (i32, i32), active_tool: Tool) -> ToolbarHit {
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
        // Pin is row 1 (always clickable, even under Mosaic).
        if point_in(cursor, self.pin_button.origin, self.pin_button.size) {
            return ToolbarHit::Pin;
        }
        // Row 2 disabled under Mosaic (style isn't applied).
        if active_tool == Tool::Mosaic {
            return ToolbarHit::None;
        }
        for btn in &self.color_buttons {
            if point_in(cursor, btn.origin, btn.size) {
                return ToolbarHit::Color(btn.color);
            }
        }
        for btn in &self.stroke_buttons {
            if point_in(cursor, btn.origin, btn.size) {
                return ToolbarHit::Stroke(btn.stroke);
            }
        }
        ToolbarHit::None
    }

    #[allow(dead_code)] // public API; retained for future callers
    pub fn contains(&self, cursor: (i32, i32)) -> bool {
        point_in(cursor, self.origin, self.size)
    }
}

fn point_in(p: (i32, i32), origin: (i32, i32), size: (i32, i32)) -> bool {
    p.0 >= origin.0 && p.1 >= origin.1 && p.0 < origin.0 + size.0 && p.1 < origin.1 + size.1
}

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
    // Pin icon is always white (no enabled/disabled state).
    draw_icon_pin_thumbtack(buf, win_w, win_h, toolbar.pin_button.origin, 0xFFFFFF);

    // Row 2 — disabled (faded) when Mosaic is active.
    let row2_alpha: f32 = if current_tool == Tool::Mosaic { 0.4 } else { 1.0 };
    let row2_disabled = current_tool == Tool::Mosaic;
    for btn in &toolbar.color_buttons {
        let argb = btn.color.argb();
        let cx = btn.origin.0 + btn.size.0 / 2;
        let cy = btn.origin.1 + btn.size.1 / 2;
        let radius = btn.size.0 as f64 / 2.0;
        // Filled disk (color), faded if Mosaic active.
        let argb_alpha = mix_argb(argb, 0x000000, 1.0 - row2_alpha);
        fill_disk(buf, win_w, win_h, cx as f64, cy as f64, radius, argb_alpha);
        if btn.color == current_style.color && !row2_disabled {
            // Active ring: white circle outlining the swatch.
            stroke_ellipse(buf, win_w, win_h, cx, cy, radius + 3.0, radius + 3.0, 2, 0xFFFFFF);
        }
    }
    for btn in &toolbar.stroke_buttons {
        let cx = btn.origin.0 + btn.size.0 / 2;
        let cy = btn.origin.1 + btn.size.1 / 2;
        let dot_r = (btn.stroke.px() * UI_SCALE) as f64 / 2.0;
        let argb = mix_argb(0xFFFFFF, 0x000000, 1.0 - row2_alpha);
        fill_disk(buf, win_w, win_h, cx as f64, cy as f64, dot_r, argb);
        if btn.stroke == current_style.stroke && !row2_disabled {
            stroke_ellipse(buf, win_w, win_h, cx, cy, dot_r + 4.0, dot_r + 4.0, 2, 0xFFFFFF);
        }
    }

    if show_hints {
        // Single-letter / digit hints anchored to bottom-right corner of each
        // button. Renders as a tiny rounded-rect badge with monospace text.
        for btn in &toolbar.tool_buttons {
            let label = tool_hint_char(btn.tool);
            draw_hint_badge(buf, win_w, win_h, btn.origin, btn.size, label);
        }
        // Row 2 hints — only when row 2 is enabled (not Mosaic).
        if current_tool != Tool::Mosaic {
            for (i, btn) in toolbar.color_buttons.iter().enumerate() {
                let digit = match i { 0 => '1', 1 => '2', 2 => '3', _ => '4' };
                draw_hint_badge(buf, win_w, win_h, btn.origin, btn.size, digit);
            }
        }
    }
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

fn draw_hint_badge(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    button_origin: (i32, i32),
    button_size: (i32, i32),
    label: char,
) {
    // Badge size scales with UI but stays compact.
    let badge_w = (10 * UI_SCALE).max(20);
    let badge_h = (10 * UI_SCALE).max(20);
    // Anchor at the button's bottom-right corner.
    let bx = button_origin.0 + button_size.0 - badge_w;
    let by = button_origin.1 + button_size.1 - badge_h;

    // Semi-transparent dark background, rounded.
    draw_pill(
        buf, win_w, win_h,
        (bx, by), (badge_w, badge_h),
        2 * UI_SCALE,
        0x000000,
        0.7,
    );

    // Letter is hand-rendered using the same primitives the toolbar uses
    // for its own icons (no font, no rasterizer dependency, fast).
    draw_hint_glyph(buf, win_w, win_h, (bx, by), (badge_w, badge_h), label);
}

fn draw_hint_glyph(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    badge_origin: (i32, i32),
    badge_size: (i32, i32),
    label: char,
) {
    let pad = 4;
    let cw = badge_size.0 - pad * 2;
    let ch = badge_size.1 - pad * 2;
    let x = badge_origin.0 + pad;
    let y = badge_origin.1 + pad;
    let s = 2; // stroke
    let color = 0xFFFFFF;

    match label {
        'M' => {
            // Two verticals + a V in the middle.
            fill_rect(buf, win_w, win_h, x, y, s, ch, color);
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch, color);
            stroke_line(buf, win_w, win_h, x, y, x + cw / 2, y + ch / 2, s, color);
            stroke_line(buf, win_w, win_h, x + cw / 2, y + ch / 2, x + cw, y, s, color);
        }
        'A' => {
            // Triangle outline + crossbar.
            stroke_line(buf, win_w, win_h, x + cw / 2, y, x, y + ch, s, color);
            stroke_line(buf, win_w, win_h, x + cw / 2, y, x + cw, y + ch, s, color);
            fill_rect(buf, win_w, win_h, x + cw / 4, y + ch * 2 / 3, cw / 2, s, color);
        }
        'R' => {
            // Vertical + top-half rect outline + leg.
            fill_rect(buf, win_w, win_h, x, y, s, ch, color);
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch / 2, color);          // right (top half)
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw, s, color);      // middle
            stroke_line(buf, win_w, win_h, x + cw / 2, y + ch / 2, x + cw, y + ch, s, color); // leg
        }
        'E' => {
            fill_rect(buf, win_w, win_h, x, y, s, ch, color);                       // vertical
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw * 3 / 4, s, color); // middle
            fill_rect(buf, win_w, win_h, x, y + ch - s, cw, s, color);              // bottom
        }
        'P' => {
            fill_rect(buf, win_w, win_h, x, y, s, ch, color);                       // vertical
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch / 2, color);          // right (top half)
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw, s, color);      // middle
        }
        'T' => {
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top bar
            fill_rect(buf, win_w, win_h, x + cw / 2 - s / 2, y, s, ch, color);      // vertical
        }
        'B' => {
            fill_rect(buf, win_w, win_h, x, y, s, ch, color);                       // vertical
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch / 2, color);          // right top
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw, s, color);      // middle
            fill_rect(buf, win_w, win_h, x + cw - s, y + ch / 2, s, ch / 2, color); // right bottom
            fill_rect(buf, win_w, win_h, x, y + ch - s, cw, s, color);              // bottom
        }
        '1' => {
            fill_rect(buf, win_w, win_h, x + cw / 2 - s / 2, y, s, ch, color);
            // small foot
            fill_rect(buf, win_w, win_h, x + cw / 4, y + ch - s, cw / 2, s, color);
            // top serif
            stroke_line(buf, win_w, win_h, x + cw / 4, y + ch / 4, x + cw / 2, y, s, color);
        }
        '2' => {
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch / 2, color);          // right top
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw, s, color);      // middle
            fill_rect(buf, win_w, win_h, x, y + ch / 2, s, ch / 2, color);          // left bottom
            fill_rect(buf, win_w, win_h, x, y + ch - s, cw, s, color);              // bottom
        }
        '3' => {
            fill_rect(buf, win_w, win_h, x, y, cw, s, color);                       // top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch, color);              // right
            fill_rect(buf, win_w, win_h, x + cw / 4, y + ch / 2 - s / 2, cw * 3 / 4, s, color); // middle
            fill_rect(buf, win_w, win_h, x, y + ch - s, cw, s, color);              // bottom
        }
        '4' => {
            fill_rect(buf, win_w, win_h, x, y, s, ch / 2 + s, color);               // left top
            fill_rect(buf, win_w, win_h, x + cw - s, y, s, ch, color);              // right
            fill_rect(buf, win_w, win_h, x, y + ch / 2 - s / 2, cw, s, color);      // middle
        }
        _ => {} // unknown label — skip
    }
}

fn draw_tool_icon(buf: &mut [u32], w: u32, h: u32, tool: Tool, origin: (i32, i32), color: u32) {
    match tool {
        Tool::Move => draw_icon_move(buf, w, h, origin, color),
        Tool::Arrow => draw_icon_arrow(buf, w, h, origin, color),
        Tool::Rect => draw_icon_rect(buf, w, h, origin, color),
        Tool::Ellipse => draw_icon_ellipse(buf, w, h, origin, color),
        Tool::Mosaic => draw_icon_mosaic(buf, w, h, origin, color),
        Tool::Pen => draw_icon_pen(buf, w, h, origin, color),
        Tool::Text => draw_icon_text(buf, w, h, origin, color),
    }
}

fn draw_icon_pen(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Vertical pencil: eraser cap on top, ferrule gap, fat body, triangular
    // nib at the bottom. Solid fills (not thin strokes) so it doesn't read as
    // another arrow / line.
    let cx = o.0 + ICON_SIZE / 2;
    let pad = ICON_SIZE / 6;             // top/bottom inset
    let body_w = 6 * UI_SCALE;           // ~18 px on UI_SCALE=3
    let eraser_h = 5 * UI_SCALE;         // ~15 px
    let gap_h = UI_SCALE;                // ~3 px ferrule gap between eraser and body
    let nib_h = 4 * UI_SCALE;            // ~12 px triangular tip
    let body_h =
        ICON_SIZE - 2 * pad - eraser_h - gap_h - nib_h;

    let body_x = cx - body_w / 2;
    let y_top = o.1 + pad;

    // Eraser cap
    fill_rect(buf, w, h, body_x, y_top, body_w, eraser_h, color);
    // Body (skip gap_h to create the visual ferrule break)
    let body_y = y_top + eraser_h + gap_h;
    fill_rect(buf, w, h, body_x, body_y, body_w, body_h, color);
    // Nib triangle pointing down
    let nib_y_top = body_y + body_h;
    fill_triangle(
        buf, w, h,
        (body_x as f64, nib_y_top as f64),
        ((body_x + body_w) as f64, nib_y_top as f64),
        (cx as f64, (nib_y_top + nib_h) as f64),
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

// All icon drawing is done natively at ICON_SIZE (=66) using filled primitives
// (fill_disk, fill_rect, stroke_rect, stroke_arc) for smooth thick strokes.

fn draw_icon_move(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Plus sign: horizontal arm 40×STROKE, vertical arm STROKE×40, centered.
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    let arm = 20; // half-length
    fill_rect(buf, w, h, cx - arm, cy - STROKE / 2, arm * 2, STROKE, color);
    fill_rect(buf, w, h, cx - STROKE / 2, cy - arm, STROKE, arm * 2, color);
}

fn draw_icon_arrow(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Diagonal line from bottom-left to top-right with a 60° pointy arrowhead.
    let pad = ICON_SIZE / 5; // ~13
    let (x0, y0) = (o.0 + pad, o.1 + ICON_SIZE - pad);
    let (x1, y1) = (o.0 + ICON_SIZE - pad, o.1 + pad);
    let head_len = 18.0;
    let dx = (x1 - x0) as f64;
    let dy = (y1 - y0) as f64;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let base_x = x1 as f64 - ux * head_len;
    let base_y = y1 as f64 - uy * head_len;
    // Shaft stops at the arrowhead's base so it doesn't poke past the tip.
    stroke_line(buf, w, h, x0, y0, base_x as i32, base_y as i32, STROKE, color);
    // 60° arrowhead: half-angle 30° → half_w = head_len * tan(30°) ≈ 0.577.
    let px = -uy;
    let py = ux;
    let half_w = head_len * 0.577;
    let a = (base_x + px * half_w, base_y + py * half_w);
    let b = (base_x - px * half_w, base_y - py * half_w);
    fill_triangle(buf, w, h, (x1 as f64, y1 as f64), a, b, color);
}

fn draw_icon_rect(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Centered inset rectangle outline.
    let pad = ICON_SIZE / 6; // ~11
    stroke_rect(
        buf,
        w,
        h,
        o.0 + pad,
        o.1 + pad,
        ICON_SIZE - 2 * pad,
        ICON_SIZE - 2 * pad,
        STROKE,
        color,
    );
}

fn draw_icon_ellipse(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    let pad = ICON_SIZE / 6;
    let rx = ((ICON_SIZE - 2 * pad) / 2) as f64;
    let ry = rx * 0.85; // slight oval
    stroke_ellipse(buf, w, h, cx, cy, rx, ry, STROKE, color);
}

fn draw_icon_mosaic(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // 3×3 checker. Cell size computed so the whole grid fills the icon minus pad.
    let pad = ICON_SIZE / 6;
    let grid_side = ICON_SIZE - 2 * pad;
    let cell = grid_side / 3;
    for row in 0..3 {
        for col in 0..3 {
            if (row + col) % 2 == 0 {
                let x = o.0 + pad + col * cell;
                let y = o.1 + pad + row * cell;
                fill_rect(buf, w, h, x, y, cell, cell, color);
            }
        }
    }
}

fn draw_icon_undo(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // "<" chevron: two straight strokes meeting at the left-middle point.
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    let arm_x = ICON_SIZE / 4;
    let arm_y = ICON_SIZE / 3;
    let tip = (cx - arm_x, cy);
    let top = (cx + arm_x, cy - arm_y);
    let bot = (cx + arm_x, cy + arm_y);
    stroke_line(buf, w, h, top.0, top.1, tip.0, tip.1, STROKE, color);
    stroke_line(buf, w, h, tip.0, tip.1, bot.0, bot.1, STROKE, color);
}

fn draw_icon_redo(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // ">" chevron: two straight strokes meeting at the right-middle point.
    let cx = o.0 + ICON_SIZE / 2;
    let cy = o.1 + ICON_SIZE / 2;
    let arm_x = ICON_SIZE / 4;
    let arm_y = ICON_SIZE / 3;
    let tip = (cx + arm_x, cy);
    let top = (cx - arm_x, cy - arm_y);
    let bot = (cx - arm_x, cy + arm_y);
    stroke_line(buf, w, h, top.0, top.1, tip.0, tip.1, STROKE, color);
    stroke_line(buf, w, h, tip.0, tip.1, bot.0, bot.1, STROKE, color);
}

fn draw_icon_pin_thumbtack(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    // Thumbtack: a horizontal head bar at top-center, a vertical needle below.
    let pad = ICON_SIZE / 5;
    let cx = o.0 + ICON_SIZE / 2;
    // Head: a wide bar near the top.
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

// --- primitive drawing helpers (all operate on softbuffer ARGB u32) ---

fn put(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
}

/// Write `color` at (x, y) blended with the existing pixel by `coverage`
/// (0.0 = transparent, 1.0 = fully opaque). Clips to buffer bounds.
fn put_blend(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32, coverage: f32) {
    if coverage <= 0.0 || x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return;
    }
    let idx = (y as u32 * w + x as u32) as usize;
    let bg = buf[idx];
    let a = coverage.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    let fr = ((color >> 16) & 0xFF) as f32;
    let fg = ((color >> 8) & 0xFF) as f32;
    let fb = (color & 0xFF) as f32;
    let br = ((bg >> 16) & 0xFF) as f32;
    let bgc = ((bg >> 8) & 0xFF) as f32;
    let bb = (bg & 0xFF) as f32;
    let r = (fr * a + br * inv) as u32;
    let g = (fg * a + bgc * inv) as u32;
    let b = (fb * a + bb * inv) as u32;
    buf[idx] = (r << 16) | (g << 8) | b;
}

#[allow(clippy::too_many_arguments)]
fn fill_rect(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, rw: i32, rh: i32, color: u32) {
    for dy in 0..rh {
        for dx in 0..rw {
            put(buf, w, h, x + dx, y + dy, color);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stroke_rect(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    thickness: i32,
    color: u32,
) {
    // top, bottom
    fill_rect(buf, w, h, x, y, rw, thickness, color);
    fill_rect(buf, w, h, x, y + rh - thickness, rw, thickness, color);
    // left, right
    fill_rect(buf, w, h, x, y, thickness, rh, color);
    fill_rect(buf, w, h, x + rw - thickness, y, thickness, rh, color);
}

fn fill_disk(buf: &mut [u32], w: u32, h: u32, cx: f64, cy: f64, r: f64, color: u32) {
    if r <= 0.0 { return; }
    let r_ceil = (r + 1.0).ceil() as i32;
    let cx_i = cx.round() as i32;
    let cy_i = cy.round() as i32;
    let cx_f = cx - cx_i as f64;
    let cy_f = cy - cy_i as f64;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64 - cx_f;
            let fy = dy as f64 - cy_f;
            let d = (fx * fx + fy * fy).sqrt();
            // Linear coverage at the edge: 1.0 well inside, 0.0 well outside,
            // ramp over a 1-pixel band centered on the boundary.
            let coverage = (r + 0.5 - d).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend(buf, w, h, cx_i + dx, cy_i + dy, color, coverage);
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

#[allow(clippy::too_many_arguments)]
fn stroke_line(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    color: u32,
) {
    // Per-pixel distance-coverage AA: avoid the speckling/half-transparency
    // that stamping AA disks along a path produces when overlap accumulation
    // never reaches 1.0 coverage.
    let half_t = thickness as f64 / 2.0;
    let fx0 = x0 as f64;
    let fy0 = y0 as f64;
    let fx1 = x1 as f64;
    let fy1 = y1 as f64;
    let dx = fx1 - fx0;
    let dy = fy1 - fy0;
    let len2 = dx * dx + dy * dy;
    if len2 < 0.0001 {
        // Degenerate: just draw a disk at the start.
        fill_disk(buf, w, h, fx0, fy0, half_t, color);
        return;
    }
    let bound_min_x = (fx0.min(fx1) - half_t - 1.0).floor() as i32;
    let bound_max_x = (fx0.max(fx1) + half_t + 1.0).ceil() as i32;
    let bound_min_y = (fy0.min(fy1) - half_t - 1.0).floor() as i32;
    let bound_max_y = (fy0.max(fy1) + half_t + 1.0).ceil() as i32;

    for y in bound_min_y..=bound_max_y {
        for x in bound_min_x..=bound_max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            // Project pixel center onto segment; clamp to [0, 1] for rounded caps.
            let t = ((px - fx0) * dx + (py - fy0) * dy) / len2;
            let tc = t.clamp(0.0, 1.0);
            let proj_x = fx0 + tc * dx;
            let proj_y = fy0 + tc * dy;
            let ddx = px - proj_x;
            let ddy = py - proj_y;
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend(buf, w, h, x, y, color, coverage);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn stroke_ellipse(
    buf: &mut [u32],
    w: u32,
    h: u32,
    cx: i32,
    cy: i32,
    rx: f64,
    ry: f64,
    thickness: i32,
    color: u32,
) {
    // Per-pixel distance-coverage AA: each pixel contributes once with its
    // proper coverage. No path-stamping, no overlap accumulation.
    let half_t = thickness as f64 / 2.0;
    let cxf = cx as f64;
    let cyf = cy as f64;
    let min_r = rx.min(ry);
    // Bounding box: ellipse extents + half-thickness + 1 px AA bleed.
    let bound_x_min = (cxf - rx - half_t - 1.0).floor() as i32;
    let bound_x_max = (cxf + rx + half_t + 1.0).ceil() as i32;
    let bound_y_min = (cyf - ry - half_t - 1.0).floor() as i32;
    let bound_y_max = (cyf + ry + half_t + 1.0).ceil() as i32;

    for y in bound_y_min..=bound_y_max {
        for x in bound_x_min..=bound_x_max {
            // Sample at pixel center.
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            // Normalized distance to the ellipse perimeter (perimeter at r_norm == 1).
            let nx = (px - cxf) / rx;
            let ny = (py - cyf) / ry;
            let r_norm = (nx * nx + ny * ny).sqrt();
            // Approximate Euclidean distance to perimeter using min-radius scale.
            // Exact distance requires a quartic; this is visually correct for
            // mildly-eccentric ellipses (our icons + user-drawn marks).
            let dist = (r_norm - 1.0).abs() * min_r;
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend(buf, w, h, x, y, color, coverage);
            }
        }
    }
}

fn fill_triangle(
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
    // 4×4 supersample grid per pixel.
    const N: i32 = 4;
    let step = 1.0 / N as f64;
    let offset = step / 2.0;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let mut hits = 0i32;
            for sy in 0..N {
                for sx in 0..N {
                    let px = x as f64 + offset + sx as f64 * step;
                    let py = y as f64 + offset + sy as f64 * step;
                    if point_in_triangle((px, py), p0, p1, p2) {
                        hits += 1;
                    }
                }
            }
            if hits > 0 {
                let coverage = hits as f32 / (N * N) as f32;
                put_blend(buf, w, h, x, y, color, coverage);
            }
        }
    }
}

fn point_in_triangle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let d1 = sign_tri(p, a, b);
    let d2 = sign_tri(p, b, c);
    let d3 = sign_tri(p, c, a);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn sign_tri(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (p.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (p.1 - b.1)
}

#[allow(clippy::too_many_arguments)]
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
            let in_corner_tl = dx < radius && dy < radius;
            let in_corner_tr = dx >= rw - radius && dy < radius;
            let in_corner_bl = dx < radius && dy >= rh - radius;
            let in_corner_br = dx >= rw - radius && dy >= rh - radius;
            // Compute per-pixel coverage at corners; 1.0 elsewhere.
            let mut corner_coverage = 1.0_f32;
            if in_corner_tl || in_corner_tr || in_corner_bl || in_corner_br {
                let cx = if dx < radius { radius } else { rw - radius - 1 };
                let cy = if dy < radius { radius } else { rh - radius - 1 };
                let fx = (dx - cx) as f64;
                let fy = (dy - cy) as f64;
                let d = (fx * fx + fy * fy).sqrt();
                let cov = (radius as f64 + 0.5 - d).clamp(0.0, 1.0);
                if cov <= 0.0 {
                    continue;
                }
                corner_coverage = cov as f32;
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
            let effective_alpha = alpha * corner_coverage;
            let r = (fr * effective_alpha + br * (1.0 - effective_alpha)) as u32;
            let g = (fg * effective_alpha + bgc * (1.0 - effective_alpha)) as u32;
            let b = (fb * effective_alpha + bb * (1.0 - effective_alpha)) as u32;
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
        assert!(t.origin.1 > sel().y + sel().h);
        assert_eq!(t.tool_buttons.len(), 7);
        assert_eq!(t.tool_buttons[0].tool, Tool::Move);
        assert_eq!(t.tool_buttons[6].tool, Tool::Mosaic);
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
        let btn = &t.tool_buttons[1];
        let c = (btn.origin.0 + 5, btn.origin.1 + 5);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Tool(Tool::Arrow));
    }

    #[test]
    fn hit_undo() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let u = &t.undo_button;
        let c = (u.origin.0 + 3, u.origin.1 + 3);
        assert_eq!(t.hit_with_tool(c, Tool::Move), ToolbarHit::Undo);
    }

    #[test]
    fn hit_none_outside() {
        let t = Toolbar::layout(sel(), (1440, 900));
        assert_eq!(t.hit_with_tool((10, 10), Tool::Move), ToolbarHit::None);
    }

    #[test]
    fn contains_inside_bar() {
        let t = Toolbar::layout(sel(), (1440, 900));
        let c = (t.origin.0 + t.size.0 / 2, t.origin.1 + t.size.1 / 2);
        assert!(t.contains(c));
        assert!(!t.contains((10, 10)));
    }

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
        assert!(row2_y > row1_y, "row 2 should sit below row 1");
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
}
