//! Mini toolbar rendered during the Adjusting state. Pure-ish: layout is
//! pure; drawing is straight softbuffer paints.

use super::annotate::Tool;
use super::state::Rect;

/// All toolbar lengths/sizes are multiplied by this factor.
/// Icons are drawn into a 22×22 virtual canvas and nearest-neighbor upscaled.
pub const UI_SCALE: i32 = 3;

const SOURCE_ICON_SIZE: i32 = 22;

pub const TOOLBAR_H: i32 = 30 * UI_SCALE;
pub const TOOLBAR_GAP: i32 = 6 * UI_SCALE; // distance from selection edge
pub const ICON_SIZE: i32 = SOURCE_ICON_SIZE * UI_SCALE;
pub const ICON_PAD: i32 = 4 * UI_SCALE;
pub const SEP_WIDTH: i32 = 8 * UI_SCALE;
const PILL_RADIUS: i32 = 4 * UI_SCALE;

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
        draw_tool_icon_scaled(buf, win_w, win_h, btn.tool, btn.origin, 0xFFFFFF);
    }

    let undo_color = if can_undo { 0xFFFFFF } else { 0x888888 };
    draw_icon_scaled(buf, win_w, win_h, toolbar.undo_button.origin, undo_color, draw_icon_undo);

    let redo_color = if can_redo { 0xFFFFFF } else { 0x888888 };
    draw_icon_scaled(buf, win_w, win_h, toolbar.redo_button.origin, redo_color, draw_icon_redo);
}

/// Render an icon function's output into a 22×22 temp buffer, then
/// nearest-neighbor upscale each source pixel into a UI_SCALE×UI_SCALE block
/// in the real softbuffer. Background (0x00000000) is treated as transparent
/// and skipped, so the tool-button pill underneath shows through.
fn draw_icon_scaled(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    origin: (i32, i32),
    color: u32,
    paint: fn(&mut [u32], u32, u32, (i32, i32), u32),
) {
    let s = SOURCE_ICON_SIZE;
    let mut tmp: Vec<u32> = vec![0u32; (s * s) as usize];
    paint(&mut tmp, s as u32, s as u32, (0, 0), color);
    for sy in 0..s {
        for sx in 0..s {
            let idx = (sy * s + sx) as usize;
            let px = tmp[idx];
            if px == 0 {
                continue;
            }
            for dy in 0..UI_SCALE {
                for dx in 0..UI_SCALE {
                    put(
                        buf,
                        win_w,
                        win_h,
                        origin.0 + sx * UI_SCALE + dx,
                        origin.1 + sy * UI_SCALE + dy,
                        px,
                    );
                }
            }
        }
    }
}

fn draw_tool_icon_scaled(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    tool: Tool,
    origin: (i32, i32),
    color: u32,
) {
    let paint: fn(&mut [u32], u32, u32, (i32, i32), u32) = match tool {
        Tool::Move => draw_icon_move,
        Tool::Arrow => draw_icon_arrow,
        Tool::Rect => draw_icon_rect,
        Tool::Ellipse => draw_icon_ellipse,
        Tool::Mosaic => draw_icon_mosaic,
    };
    draw_icon_scaled(buf, win_w, win_h, origin, color, paint);
}

fn draw_icon_move(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    let cx = o.0 + SOURCE_ICON_SIZE / 2;
    let cy = o.1 + SOURCE_ICON_SIZE / 2;
    for d in -5..=5 {
        put(buf, w, h, cx + d, cy, color);
        put(buf, w, h, cx, cy + d, color);
    }
}

fn draw_icon_arrow(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
    let (x0, y0) = (o.0 + 4, o.1 + 16);
    let (x1, y1) = (o.0 + 16, o.1 + 4);
    draw_line_2px(buf, w, h, x0, y0, x1, y1, color);
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
    let cx = o.0 + SOURCE_ICON_SIZE / 2;
    let cy = o.1 + SOURCE_ICON_SIZE / 2;
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
    let cx = o.0 + 11;
    let cy = o.1 + 11;
    for deg in 90..=270 {
        let r = 6.0f64;
        let rad = (deg as f64).to_radians();
        let x = cx + (r * rad.cos()) as i32;
        let y = cy + (r * rad.sin()) as i32;
        put(buf, w, h, x, y, color);
    }
    put(buf, w, h, cx - 6, cy - 1, color);
    put(buf, w, h, cx - 5, cy - 2, color);
    put(buf, w, h, cx - 5, cy, color);
    put(buf, w, h, cx - 7, cy - 1, color);
    put(buf, w, h, cx - 6, cy + 1, color);
}

fn draw_icon_redo(buf: &mut [u32], w: u32, h: u32, o: (i32, i32), color: u32) {
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

fn put(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
}

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
fn draw_line_2px(buf: &mut [u32], w: u32, h: u32, x0: i32, y0: i32, x1: i32, y1: i32, color: u32) {
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
