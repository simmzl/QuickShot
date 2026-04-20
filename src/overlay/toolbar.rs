//! Mini toolbar rendered during the Adjusting state. Pure-ish: layout is
//! pure; drawing is straight softbuffer paints.

use super::annotate::Tool;
use super::state::Rect;

/// All toolbar lengths/sizes are in physical pixels at this scale.
pub const UI_SCALE: i32 = 3;

pub const TOOLBAR_H: i32 = 30 * UI_SCALE;
pub const TOOLBAR_GAP: i32 = 6 * UI_SCALE; // distance from selection edge
pub const ICON_SIZE: i32 = 22 * UI_SCALE;
pub const ICON_PAD: i32 = 4 * UI_SCALE;
pub const SEP_WIDTH: i32 = 8 * UI_SCALE;
const PILL_RADIUS: i32 = 4 * UI_SCALE;

/// Stroke thickness (pixels) used by all icons. Scales with UI.
const STROKE: i32 = 4;

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
    pub fn layout(selection: Rect, window_size: (u32, u32)) -> Toolbar {
        let tools_w = TOOL_ORDER.len() as i32 * ICON_SIZE
            + (TOOL_ORDER.len() as i32 - 1) * ICON_PAD;
        let content_w =
            tools_w + SEP_WIDTH + (ICON_SIZE + ICON_PAD) + ICON_SIZE;
        let bar_w = content_w + 2 * ICON_PAD;
        let bar_h = TOOLBAR_H;

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

    #[allow(dead_code)] // public API; retained for future callers
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
    draw_pill(buf, win_w, win_h, toolbar.origin, toolbar.size, PILL_RADIUS, 0x000000, 0.7);

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
    // Diagonal line from bottom-left to top-right with a pointy arrowhead.
    let pad = ICON_SIZE / 5; // ~13
    let (x0, y0) = (o.0 + pad, o.1 + ICON_SIZE - pad);
    let (x1, y1) = (o.0 + ICON_SIZE - pad, o.1 + pad);
    stroke_line(buf, w, h, x0, y0, x1, y1, STROKE, color);
    // Arrowhead: narrow isosceles triangle (base ≈ 0.33 × length → ~18° half-angle).
    let head_len = 20.0;
    let dx = (x1 - x0) as f64;
    let dy = (y1 - y0) as f64;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let ux = dx / len;
    let uy = dy / len;
    let base_x = x1 as f64 - ux * head_len;
    let base_y = y1 as f64 - uy * head_len;
    let px = -uy;
    let py = ux;
    let half_w = head_len * 0.33;
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

// --- primitive drawing helpers (all operate on softbuffer ARGB u32) ---

fn put(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
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
    let r_ceil = r.ceil() as i32;
    let r2 = r * r;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64;
            let fy = dy as f64;
            if fx * fx + fy * fy <= r2 {
                put(buf, w, h, cx as i32 + dx, cy as i32 + dy, color);
            }
        }
    }
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
    let (fx, fy) = (x0 as f64, y0 as f64);
    let (tx, ty) = (x1 as f64, y1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = fx + dx * t;
        let y = fy + dy * t;
        fill_disk(buf, w, h, x, y, r, color);
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
    // Parametric ellipse: sample enough points for smooth stroke.
    let perimeter = 2.0 * std::f64::consts::PI * rx.max(ry);
    let steps = (perimeter * 2.0) as i32;
    let stroke_r = thickness as f64 / 2.0;
    let (cxf, cyf) = (cx as f64, cy as f64);
    for i in 0..=steps {
        let theta = (i as f64 / steps as f64) * 2.0 * std::f64::consts::PI;
        let x = cxf + rx * theta.cos();
        let y = cyf + ry * theta.sin();
        fill_disk(buf, w, h, x, y, stroke_r, color);
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
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let pf = (x as f64 + 0.5, y as f64 + 0.5);
            if point_in_triangle(pf, p0, p1, p2) {
                put(buf, w, h, x, y, color);
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
        assert!(t.origin.1 > sel().y + sel().h);
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
        let btn = &t.tool_buttons[1];
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
