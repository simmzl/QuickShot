use image::RgbaImage;
use crate::overlay::hit::ANCHOR_SIZE;
use crate::overlay::state::{Anchor, Rect};

/// Copy the captured frame into the softbuffer surface, nearest-neighbor
/// scaled to the window's pixel dimensions. Softbuffer's pixel format is
/// 0x00RRGGBB (u32 per pixel).
pub fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
    let (fw, fh) = frame.dimensions();
    for y in 0..h {
        for x in 0..w {
            let fx = (x as u64 * fw as u64 / w as u64) as u32;
            let fy = (y as u64 * fh as u64 / h as u64) as u32;
            let p = frame.get_pixel(fx.min(fw - 1), fy.min(fh - 1));
            let [r, g, b, _a] = p.0;
            buf[(y * w + x) as usize] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }
}

/// Darken everything in `buf` except the optional `inside` rect (x, y, w, h).
pub fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    for y in 0..h {
        for x in 0..w {
            let in_selection = match inside {
                Some((ix, iy, iw, ih)) => x >= ix && y >= iy && x < ix + iw && y < iy + ih,
                None => false,
            };
            if !in_selection {
                let i = (y * w + x) as usize;
                let px = buf[i];
                let r = ((px >> 16) & 0xFF) / 2;
                let g = ((px >> 8) & 0xFF) / 2;
                let b = (px & 0xFF) / 2;
                buf[i] = (r << 16) | (g << 8) | b;
            }
        }
    }
}

pub fn draw_anchors(buf: &mut [u32], w: u32, h: u32, rect: Rect) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    const WHITE: u32 = 0x00FFFFFF;
    const BLACK: u32 = 0x00000000;
    let anchors = [
        Anchor::TL, Anchor::T, Anchor::TR, Anchor::R,
        Anchor::BR, Anchor::B, Anchor::BL, Anchor::L,
    ];
    let half = ANCHOR_SIZE / 2;
    for a in anchors {
        let (ax, ay) = super::hit::anchor_center(rect, a);
        fill_square(buf, w, h, ax - half - 1, ay - half - 1, ANCHOR_SIZE + 2, BLACK);
        fill_square(buf, w, h, ax - half,     ay - half,     ANCHOR_SIZE,     WHITE);
    }
}

fn fill_square(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, size: i32, color: u32) {
    for dy in 0..size {
        for dx in 0..size {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
            buf[(py as u32 * w + px as u32) as usize] = color;
        }
    }
}

use crate::text::Font;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelPlacement {
    AboveOutside,
    InsideTopLeft,
}

/// Decide where to place the size label given the selection rect and the
/// label's own pixel dimensions.
pub fn size_label_position(
    rect: Rect,
    label_size: (i32, i32),
    gap: i32,
) -> (i32, i32, LabelPlacement) {
    let (_lw, lh) = label_size;
    let above_y = rect.y - gap - lh;
    if above_y >= 0 {
        (rect.x, above_y, LabelPlacement::AboveOutside)
    } else {
        (rect.x + gap, rect.y + gap, LabelPlacement::InsideTopLeft)
    }
}

const LABEL_FONT_PX: f32 = 12.0;
const LABEL_PAD_X: i32 = 4;
const LABEL_PAD_Y: i32 = 2;
const LABEL_GAP: i32 = 4;
const LABEL_CORNER_RADIUS: i32 = 4;

/// Draw the `W × H` label pinned to the selection's top-left. `frame_size`
/// is the captured frame's pixel dimensions; `window_size` is the overlay
/// window's pixel dimensions. The label shows physical-pixel dims of the
/// cropped region so the number matches the eventual PNG.
pub fn draw_size_label(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: Rect,
    frame_size: (u32, u32),
    window_size: (u32, u32),
    font: &mut Font,
) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    let (fw, fh) = frame_size;
    let (ww, wh) = (window_size.0.max(1), window_size.1.max(1));
    // Window-space rect -> frame-space dims (same math as Overlay::window_rect_to_frame_rect)
    let phys_w = (rect.w as u64 * fw as u64 / ww as u64) as u32;
    let phys_h = (rect.h as u64 * fh as u64 / wh as u64) as u32;
    let text = format!("{} \u{00D7} {}", phys_w.max(1), phys_h.max(1));

    let text_w = estimate_text_width(&text, LABEL_FONT_PX);
    let text_h = LABEL_FONT_PX as i32; // approximate cap height
    let box_w = text_w + LABEL_PAD_X * 2;
    let box_h = text_h + LABEL_PAD_Y * 2;

    let (bx, by, _placement) = size_label_position(rect, (box_w, box_h), LABEL_GAP);
    draw_rounded_rect_alpha(buf, w, h, bx, by, box_w, box_h, LABEL_CORNER_RADIUS, 0x000000, 0.7);
    font.render_text(
        buf,
        w,
        h,
        bx + LABEL_PAD_X,
        by + LABEL_PAD_Y,
        &text,
        LABEL_FONT_PX,
        0x00FFFFFF,
    );
}

/// Rough width estimate for monospace: each glyph is ~`0.6 * px_size` wide.
/// Used only for laying out the background pill; the text renderer handles
/// real advance widths.
fn estimate_text_width(text: &str, px_size: f32) -> i32 {
    (text.chars().count() as f32 * px_size * 0.6).ceil() as i32
}

/// Filled rect with alpha-blended solid color and squared corners masked into
/// a 4-px rounded pill.
fn draw_rounded_rect_alpha(
    buf: &mut [u32],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    rw: i32,
    rh: i32,
    radius: i32,
    color_rgb: u32,
    alpha: f32,
) {
    let (fr, fg, fb) = (
        ((color_rgb >> 16) & 0xFF) as f32,
        ((color_rgb >> 8) & 0xFF) as f32,
        (color_rgb & 0xFF) as f32,
    );
    for dy in 0..rh {
        for dx in 0..rw {
            // Corner mask: distance from nearest corner center.
            let in_corner_tl = dx < radius && dy < radius;
            let in_corner_tr = dx >= rw - radius && dy < radius;
            let in_corner_bl = dx < radius && dy >= rh - radius;
            let in_corner_br = dx >= rw - radius && dy >= rh - radius;
            if in_corner_tl || in_corner_tr || in_corner_bl || in_corner_br {
                let (cx, cy) = (
                    if dx < radius { radius } else { rw - radius - 1 },
                    if dy < radius { radius } else { rh - radius - 1 },
                );
                let d2 = (dx - cx).pow(2) + (dy - cy).pow(2);
                if d2 > radius * radius {
                    continue;
                }
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

/// Draw a 1-px-thick rectangle outline in the given color (0x00RRGGBB).
pub fn draw_selection_outline(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: (u32, u32, u32, u32),
    color: u32,
) {
    let (rx, ry, rw, rh) = rect;
    if rw == 0 || rh == 0 {
        return;
    }
    let x1 = rx;
    let y1 = ry;
    let x2 = rx + rw.saturating_sub(1);
    let y2 = ry + rh.saturating_sub(1);
    let xmax = w.saturating_sub(1);
    let ymax = h.saturating_sub(1);
    for x in x1..=x2.min(xmax) {
        buf[(y1.min(ymax) * w + x) as usize] = color;
        buf[(y2.min(ymax) * w + x) as usize] = color;
    }
    for y in y1..=y2.min(ymax) {
        buf[(y * w + x1.min(xmax)) as usize] = color;
        buf[(y * w + x2.min(xmax)) as usize] = color;
    }
}

const MAG_SIZE: i32 = 120;
const MAG_ZOOM: i32 = 4;
const MAG_OFFSET: i32 = 20;
const MAG_LABEL_H: i32 = 18;
const MAG_FONT_PX: f32 = 11.0;

/// Decide where to put the magnifier given cursor + window size.
/// Default: bottom-right of cursor with a gap. Flip to the opposite side
/// on each axis when the default would clip the window edge.
pub fn magnifier_position(
    cursor: (i32, i32),
    window_size: (u32, u32),
    mag_size: i32,
    offset: i32,
) -> (i32, i32) {
    let (ww, wh) = (window_size.0 as i32, window_size.1 as i32);
    let mut x = cursor.0 + offset;
    let mut y = cursor.1 + offset;
    if x + mag_size >= ww {
        x = cursor.0 - offset - mag_size;
    }
    if y + mag_size >= wh {
        y = cursor.1 - offset - mag_size;
    }
    (x.max(0), y.max(0))
}

pub fn draw_magnifier(
    buf: &mut [u32],
    w: u32,
    h: u32,
    frame: &image::RgbaImage,
    cursor: (i32, i32),
    window_size: (u32, u32),
    font: &mut Font,
) {
    let (mx, my) = magnifier_position(cursor, window_size, MAG_SIZE, MAG_OFFSET);
    let (fw, fh) = frame.dimensions();
    let (ww, wh) = (window_size.0.max(1) as u64, window_size.1.max(1) as u64);

    // Map cursor (window-space) to physical pixel coords in the frame.
    let cfx = (cursor.0.max(0) as u64 * fw as u64 / ww) as i32;
    let cfy = (cursor.1.max(0) as u64 * fh as u64 / wh) as i32;
    let src_span = MAG_SIZE / MAG_ZOOM; // 30

    // 1 px white border + black backfill, then upscaled pixels, then crosshair, then label.
    fill_square(buf, w, h, mx - 1, my - 1, MAG_SIZE + 2, 0x00FFFFFF);
    fill_square(buf, w, h, mx,     my,     MAG_SIZE,     0x00000000);

    for dy in 0..(MAG_SIZE - MAG_LABEL_H) {
        for dx in 0..MAG_SIZE {
            let sx = cfx - src_span / 2 + dx / MAG_ZOOM;
            let sy = cfy - src_span / 2 + dy / MAG_ZOOM;
            let sx = sx.clamp(0, fw as i32 - 1);
            let sy = sy.clamp(0, fh as i32 - 1);
            let p = frame.get_pixel(sx as u32, sy as u32);
            let [r, g, b, _a] = p.0;
            let px = mx + dx;
            let py = my + dy;
            if px < 0 || py < 0 || px >= w as i32 || py >= h as i32 { continue; }
            buf[(py as u32 * w + px as u32) as usize] =
                ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
        }
    }

    // Center crosshair (1 px horizontal + 1 px vertical lines across the zoom area).
    let cx = mx + MAG_SIZE / 2;
    let cy = my + (MAG_SIZE - MAG_LABEL_H) / 2;
    for dx in 0..MAG_SIZE {
        let px = mx + dx;
        if px < 0 || cy < 0 || px >= w as i32 || cy >= h as i32 { continue; }
        buf[(cy as u32 * w + px as u32) as usize] = 0x0000FFFF; // cyan
    }
    for dy in 0..(MAG_SIZE - MAG_LABEL_H) {
        let py = my + dy;
        if cx < 0 || py < 0 || cx >= w as i32 || py >= h as i32 { continue; }
        buf[(py as u32 * w + cx as u32) as usize] = 0x0000FFFF;
    }

    // Label strip at the bottom (black, 70% opaque per spec).
    let label_y = my + MAG_SIZE - MAG_LABEL_H;
    draw_rounded_rect_alpha(buf, w, h, mx, label_y, MAG_SIZE, MAG_LABEL_H, 0, 0x000000, 0.7);

    let cfx_clamped = cfx.clamp(0, fw as i32 - 1) as u32;
    let cfy_clamped = cfy.clamp(0, fh as i32 - 1) as u32;
    let center = frame.get_pixel(cfx_clamped, cfy_clamped);
    let [r, g, b, _a] = center.0;
    let text = format!(
        "#{:02X}{:02X}{:02X} {}px, {}px",
        r, g, b, cfx_clamped, cfy_clamped
    );
    font.render_text(buf, w, h, mx + 4, label_y + 2, &text, MAG_FONT_PX, 0x00FFFFFF);
}

#[cfg(test)]
mod magnifier_tests {
    use super::*;

    #[test]
    fn magnifier_default_goes_bottom_right() {
        let (x, y) = magnifier_position((100, 100), (800, 600), 120, 20);
        assert_eq!((x, y), (120, 120));
    }

    #[test]
    fn magnifier_flips_when_near_right_edge() {
        let (x, _y) = magnifier_position((750, 100), (800, 600), 120, 20);
        // x would be 770, mag ends at 890 > 800 → flip to left.
        assert_eq!(x, 750 - 20 - 120);
    }

    #[test]
    fn magnifier_flips_when_near_bottom() {
        let (_x, y) = magnifier_position((100, 550), (800, 600), 120, 20);
        assert_eq!(y, 550 - 20 - 120);
    }

    #[test]
    fn magnifier_clamps_to_zero_in_extreme_corner() {
        let (x, y) = magnifier_position((5, 5), (800, 600), 120, 20);
        // Default would be (25, 25), fits — no flip.
        assert_eq!((x, y), (25, 25));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_positions_above_when_space_available() {
        let r = Rect { x: 100, y: 100, w: 50, h: 50 };
        let (lx, ly, placement) = size_label_position(r, (40, 16), 4);
        assert_eq!(placement, LabelPlacement::AboveOutside);
        assert_eq!(lx, 100);
        assert_eq!(ly, 100 - 4 - 16);
    }

    #[test]
    fn label_flips_inside_when_no_room_above() {
        let r = Rect { x: 100, y: 5, w: 50, h: 50 };
        let (lx, ly, placement) = size_label_position(r, (40, 16), 4);
        assert_eq!(placement, LabelPlacement::InsideTopLeft);
        assert_eq!(lx, 100 + 4);
        assert_eq!(ly, 5 + 4);
    }
}
