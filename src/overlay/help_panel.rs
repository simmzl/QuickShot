//! Translucent keyboard-shortcut cheatsheet shown when the user presses `h`.
//! Layout is computed in window-pixel space; rendering uses the same softbuffer
//! ARGB primitives the toolbar uses.

use super::state::Rect;
use crate::text::Font;

const PADDING: i32 = 18;
const LINE_HEIGHT_PX: f32 = 20.0; // ~1.4× font_px
const FONT_PX: f32 = 14.0;
const PANEL_BG_RGB: u32 = 0x000000;
const PANEL_BG_ALPHA: f32 = 0.78;
const TEXT_COLOR: u32 = 0x00FFFFFF;
const TITLE_COLOR: u32 = 0x00FFCC00; // accent yellow for headers
const PILL_RADIUS: i32 = 8;

/// One line of the cheatsheet. Title rows render in TITLE_COLOR; rest in TEXT_COLOR.
struct Row<'a> {
    text: &'a str,
    is_title: bool,
}

fn rows() -> &'static [Row<'static>] {
    use std::sync::OnceLock;
    static ROWS: OnceLock<Vec<Row>> = OnceLock::new();
    ROWS.get_or_init(|| vec![
        Row { text: "quickshot — keyboard shortcuts", is_title: true },
        Row { text: "",                               is_title: false },
        Row { text: "Tools",                          is_title: true },
        Row { text: "  M  Move        A  Arrow",      is_title: false },
        Row { text: "  R  Rect        E  Ellipse",    is_title: false },
        Row { text: "  P  Pen         T  Text",       is_title: false },
        Row { text: "  B  Mosaic",                    is_title: false },
        Row { text: "",                               is_title: false },
        Row { text: "Color & stroke",                 is_title: true },
        Row { text: "  1 Red  2 Yellow  3 Green  4 Blue", is_title: false },
        Row { text: "  [  thinner       ]  thicker",  is_title: false },
        Row { text: "",                               is_title: false },
        Row { text: "Actions",                        is_title: true },
        Row { text: "  Cmd+Z       undo",             is_title: false },
        Row { text: "  Cmd+Shift+Z redo",             is_title: false },
        Row { text: "  Enter       copy & save",      is_title: false },
        Row { text: "  Esc         cancel",           is_title: false },
        Row { text: "",                               is_title: false },
        Row { text: "  h           toggle this help", is_title: false },
    ])
}

pub struct PanelLayout {
    pub origin: (i32, i32),
    pub size: (i32, i32),
}

/// Compute a panel position. Prefers bottom-right of `selection` when present,
/// otherwise bottom-right of the window. Clamps to fit on-screen.
pub fn layout(selection: Option<Rect>, window_size: (u32, u32)) -> PanelLayout {
    let row_count = rows().len() as i32;
    // Approximate width: longest row is ~36 chars at 14px monospace ≈ 14 × 0.6 = 8.4 px/char.
    // Use a generous fixed width; the font is monospace, so this is stable.
    let width = 360;
    let height = PADDING * 2 + (LINE_HEIGHT_PX as i32) * row_count;

    let (ww, wh) = (window_size.0 as i32, window_size.1 as i32);
    let (mut x, mut y) = if let Some(s) = selection {
        // Bottom-right of the selection, with a small gap.
        (s.x + s.w as i32 + 8, s.y + s.h as i32 + 8 - height)
    } else {
        // Bottom-right of the window.
        (ww - width - 16, wh - height - 16)
    };

    // Clamp to window bounds.
    if x + width > ww - 8 { x = ww - width - 8; }
    if x < 8 { x = 8; }
    if y + height > wh - 8 { y = wh - height - 8; }
    if y < 8 { y = 8; }

    PanelLayout { origin: (x, y), size: (width, height) }
}

pub fn draw(
    buf: &mut [u32], win_w: u32, win_h: u32,
    layout: &PanelLayout, font: &mut Font,
) {
    draw_pill(buf, win_w, win_h, layout.origin, layout.size, PILL_RADIUS, PANEL_BG_RGB, PANEL_BG_ALPHA);

    let (x, y) = layout.origin;
    let mut cursor_y = y + PADDING;
    for row in rows().iter() {
        let color = if row.is_title { TITLE_COLOR } else { TEXT_COLOR };
        font.render_text(buf, win_w, win_h, x + PADDING, cursor_y, row.text, FONT_PX, color);
        cursor_y += LINE_HEIGHT_PX as i32;
    }
}

// --- primitives (mirrors toolbar.rs's draw_pill) ---

#[allow(clippy::too_many_arguments)]
fn draw_pill(
    buf: &mut [u32], w: u32, h: u32,
    origin: (i32, i32), size: (i32, i32), radius: i32,
    color_rgb: u32, alpha: f32,
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
                if d2 > radius * radius { continue; }
            }
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
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

    #[test]
    fn layout_with_selection_anchors_below_right() {
        let sel = Rect { x: 100, y: 100, w: 400, h: 300 };
        let l = layout(Some(sel), (1440, 900));
        // Origin should be a sane on-screen rect
        assert!(l.origin.0 >= 8);
        assert!(l.origin.1 >= 8);
        assert!(l.origin.0 + l.size.0 <= 1440 - 8);
        assert!(l.origin.1 + l.size.1 <= 900 - 8);
    }

    #[test]
    fn layout_no_selection_uses_window_corner() {
        let l = layout(None, (1440, 900));
        // Bottom-right corner area
        assert!(l.origin.0 + l.size.0 <= 1440 - 8);
        assert!(l.origin.1 + l.size.1 <= 900 - 8);
        assert!(l.origin.0 > 1440 / 2);
    }

    #[test]
    fn layout_clamps_when_selection_at_edge() {
        let sel = Rect { x: 1300, y: 800, w: 100, h: 50 };
        let l = layout(Some(sel), (1440, 900));
        // Must still fit on screen
        assert!(l.origin.0 + l.size.0 <= 1440 - 8);
        assert!(l.origin.1 + l.size.1 <= 900 - 8);
    }
}
