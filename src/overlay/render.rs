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
    let (l, t) = (rect.x, rect.y);
    let (r, b) = (rect.x + rect.w - 1, rect.y + rect.h - 1);
    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    for a in anchors {
        let (ax, ay) = match a {
            Anchor::TL => (l, t),
            Anchor::T  => (cx, t),
            Anchor::TR => (r, t),
            Anchor::R  => (r, cy),
            Anchor::BR => (r, b),
            Anchor::B  => (cx, b),
            Anchor::BL => (l, b),
            Anchor::L  => (l, cy),
        };
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
