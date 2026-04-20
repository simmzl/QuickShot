//! Rendering helpers for annotations. Two code paths:
//!   * `*_on_image`: paint into a `RgbaImage` (used by flatten_for_export).
//!   * `*_on_buf`: paint into a softbuffer `&mut [u32]` (used by live preview;
//!     implemented in Task 3).

use image::{Rgba, RgbaImage};

use super::annotate::Annotation;
use super::state::Rect;

pub const ANNOTATION_COLOR_RGBA: [u8; 4] = [0xFF, 0x3B, 0x30, 0xFF]; // #FF3B30
pub const ANNOTATION_THICKNESS: i32 = 6; // 2× the original 3
pub const ARROWHEAD_LEN: f32 = 28.0; // 2× the original 14

/// Apply an annotation to a CROPPED image. `crop_offset` is the frame-space
/// coordinate corresponding to the cropped image's (0, 0). Annotations are
/// stored in frame coords; we translate into crop-local coords here.
pub fn paint_on_cropped(img: &mut RgbaImage, ann: Annotation, crop_offset: (i32, i32)) {
    let (ox, oy) = crop_offset;
    match ann {
        Annotation::Arrow { from, to } => {
            draw_arrow_on_image(
                img,
                (from.0 - ox, from.1 - oy),
                (to.0 - ox, to.1 - oy),
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Rect { rect } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_rect_outline_on_image(
                img,
                local,
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Ellipse { rect } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse_outline_on_image(
                img,
                local,
                Rgba(ANNOTATION_COLOR_RGBA),
                ANNOTATION_THICKNESS,
            );
        }
        Annotation::Mosaic { rect, block_size } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            apply_mosaic_on_image(img, local, block_size);
        }
    }
}

pub fn draw_rect_outline_on_image(
    img: &mut RgbaImage,
    rect: Rect,
    color: Rgba<u8>,
    thickness: i32,
) {
    if rect.w <= 0 || rect.h <= 0 || thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let half = thickness / 2;
    for ty in 0..thickness {
        let y1 = rect.y - half + ty;
        let y2 = rect.y + rect.h - 1 - half + ty;
        for x in rect.x..(rect.x + rect.w) {
            put_clamped(img, x, y1, w, h, color);
            put_clamped(img, x, y2, w, h, color);
        }
    }
    for tx in 0..thickness {
        let x1 = rect.x - half + tx;
        let x2 = rect.x + rect.w - 1 - half + tx;
        for y in rect.y..(rect.y + rect.h) {
            put_clamped(img, x1, y, w, h, color);
            put_clamped(img, x2, y, w, h, color);
        }
    }
}

pub fn draw_ellipse_outline_on_image(
    img: &mut RgbaImage,
    rect: Rect,
    color: Rgba<u8>,
    thickness: i32,
) {
    if rect.w <= 2 || rect.h <= 2 || thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    let cx = rect.x as f64 + rect.w as f64 / 2.0;
    let cy = rect.y as f64 + rect.h as f64 / 2.0;
    let rx = rect.w as f64 / 2.0;
    let ry = rect.h as f64 / 2.0;
    let half_thick = thickness as f64 / 2.0;
    let pad = (thickness + 2).max(2);
    for y in (rect.y - pad)..=(rect.y + rect.h + pad) {
        for x in (rect.x - pad)..=(rect.x + rect.w + pad) {
            let nx = (x as f64 + 0.5 - cx) / rx;
            let ny = (y as f64 + 0.5 - cy) / ry;
            let r = (nx * nx + ny * ny).sqrt();
            let band = half_thick / rx.min(ry);
            if (r - 1.0).abs() <= band {
                put_clamped(img, x, y, w, h, color);
            }
        }
    }
}

pub fn draw_arrow_on_image(
    img: &mut RgbaImage,
    from: (i32, i32),
    to: (i32, i32),
    color: Rgba<u8>,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    // Stop the shaft at the arrowhead's base so it doesn't poke past the tip.
    let shaft_end = shaft_end_point(from, to, ARROWHEAD_LEN as f64);
    draw_line_thick(img, from, shaft_end, color, thickness, w, h);
    draw_arrowhead(img, from, to, color, w, h);
}

/// Given a line from `from` to `to` (the tip) and `head_len` arrowhead length,
/// returns the point where the shaft should end (the arrowhead's base).
/// If the arrow is shorter than the head, returns `from` (no visible shaft).
fn shaft_end_point(from: (i32, i32), to: (i32, i32), head_len: f64) -> (i32, i32) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt();
    if len <= head_len {
        return from;
    }
    let ux = dx / len;
    let uy = dy / len;
    ((tx - ux * head_len) as i32, (ty - uy * head_len) as i32)
}

fn draw_line_thick(
    img: &mut RgbaImage,
    from: (i32, i32),
    to: (i32, i32),
    color: Rgba<u8>,
    thickness: i32,
    w: i32,
    h: i32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = fx + dx * t;
        let y = fy + dy * t;
        fill_disk(img, x, y, r, color, w, h);
    }
}

fn fill_disk(img: &mut RgbaImage, cx: f64, cy: f64, r: f64, color: Rgba<u8>, w: i32, h: i32) {
    let r_ceil = r.ceil() as i32;
    let r2 = r * r;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64;
            let fy = dy as f64;
            if fx * fx + fy * fy <= r2 {
                put_clamped(img, cx as i32 + dx, cy as i32 + dy, w, h, color);
            }
        }
    }
}

fn draw_arrowhead(img: &mut RgbaImage, from: (i32, i32), to: (i32, i32), color: Rgba<u8>, w: i32, h: i32) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let base_x = tx - ux * ARROWHEAD_LEN as f64;
    let base_y = ty - uy * ARROWHEAD_LEN as f64;
    let px = -uy;
    let py = ux;
    let half_w = ARROWHEAD_LEN as f64 * 0.577; // 60° tip: half-angle 30° → tan(30°)
    let a = (base_x + px * half_w, base_y + py * half_w);
    let b = (base_x - px * half_w, base_y - py * half_w);
    let tip = (tx, ty);
    fill_triangle(img, tip, a, b, color, w, h);
}

fn fill_triangle(
    img: &mut RgbaImage,
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    color: Rgba<u8>,
    w: i32,
    h: i32,
) {
    let min_x = p0.0.min(p1.0).min(p2.0).floor() as i32;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil() as i32;
    let min_y = p0.1.min(p1.1).min(p2.1).floor() as i32;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil() as i32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            if point_in_triangle((px, py), p0, p1, p2) {
                put_clamped(img, x, y, w, h, color);
            }
        }
    }
}

pub fn point_in_triangle(p: (f64, f64), a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> bool {
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let neg = (d1 < 0.0) || (d2 < 0.0) || (d3 < 0.0);
    let pos = (d1 > 0.0) || (d2 > 0.0) || (d3 > 0.0);
    !(neg && pos)
}

fn sign(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    (p.0 - b.0) * (a.1 - b.1) - (a.0 - b.0) * (p.1 - b.1)
}

pub fn apply_mosaic_on_image(img: &mut RgbaImage, rect: Rect, block_size: u32) {
    if rect.w <= 0 || rect.h <= 0 || block_size == 0 {
        return;
    }
    let bs = block_size as i32;
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let x0 = rect.x.max(0);
    let y0 = rect.y.max(0);
    let x1 = (rect.x + rect.w).min(iw);
    let y1 = (rect.y + rect.h).min(ih);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let block_x1 = (bx + bs).min(x1);
            let block_y1 = (by + bs).min(y1);
            let mut sum = [0u64; 4];
            let mut count: u64 = 0;
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    let p = img.get_pixel(xx as u32, yy as u32).0;
                    sum[0] += p[0] as u64;
                    sum[1] += p[1] as u64;
                    sum[2] += p[2] as u64;
                    sum[3] += p[3] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                bx += bs;
                continue;
            }
            let avg = Rgba([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ]);
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    img.put_pixel(xx as u32, yy as u32, avg);
                }
            }
            bx += bs;
        }
        by += bs;
    }
}

fn put_clamped(img: &mut RgbaImage, x: i32, y: i32, w: i32, h: i32, color: Rgba<u8>) {
    if x >= 0 && y >= 0 && x < w && y < h {
        img.put_pixel(x as u32, y as u32, color);
    }
}

// --- buf-path helpers (live softbuffer preview) ---

use super::annotate::PendingDraw;

/// Map a frame-space point to a window-space point.
fn frame_to_window(p: (i32, i32), frame_size: (u32, u32), window_size: (u32, u32)) -> (i32, i32) {
    let (fw, fh) = (frame_size.0.max(1) as i64, frame_size.1.max(1) as i64);
    let (ww, wh) = (window_size.0.max(1) as i64, window_size.1.max(1) as i64);
    let x = (p.0 as i64 * ww / fw) as i32;
    let y = (p.1 as i64 * wh / fh) as i32;
    (x, y)
}

fn window_thickness(frame_thickness: i32, frame_size: (u32, u32), window_size: (u32, u32)) -> i32 {
    let ratio_w = window_size.0 as f64 / frame_size.0.max(1) as f64;
    let ratio_h = window_size.1 as f64 / frame_size.1.max(1) as f64;
    ((frame_thickness as f64) * ratio_w.min(ratio_h)).round().max(1.0) as i32
}

pub fn draw_annotation_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    ann: Annotation,
    color_argb: u32,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let thickness_win = window_thickness(ANNOTATION_THICKNESS, frame_size, window_size);
    match ann {
        Annotation::Arrow { from, to } => {
            let f = frame_to_window(from, frame_size, window_size);
            let t = frame_to_window(to, frame_size, window_size);
            let shaft_end = shaft_end_point(f, t, ARROWHEAD_LEN as f64);
            draw_line_thick_buf(buf, win_w, win_h, f, shaft_end, color_argb, thickness_win);
            draw_arrowhead_buf(buf, win_w, win_h, f, t, color_argb);
        }
        Annotation::Rect { rect } => {
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_rect_outline_buf(buf, win_w, win_h, tl, br, color_argb, thickness_win);
        }
        Annotation::Ellipse { rect } => {
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_ellipse_outline_buf(buf, win_w, win_h, tl, br, color_argb, thickness_win);
        }
        Annotation::Mosaic { rect, block_size } => {
            apply_mosaic_on_buf(buf, win_w, win_h, frame, rect, block_size);
        }
    }
}

pub fn draw_pending_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    pending: PendingDraw,
    color_argb: u32,
) {
    if let Some(ann) = pending.finalize() {
        draw_annotation_on_buf(buf, win_w, win_h, frame, ann, color_argb);
    }
}

fn put_buf(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32) {
    if x >= 0 && y >= 0 && x < w as i32 && y < h as i32 {
        buf[(y as u32 * w + x as u32) as usize] = color;
    }
}

fn draw_line_thick_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    from: (i32, i32),
    to: (i32, i32),
    color: u32,
    thickness: i32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt().max(1.0);
    let steps = (len.ceil() as i32).max(1);
    let r = thickness as f64 / 2.0;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let x = fx + dx * t;
        let y = fy + dy * t;
        fill_disk_buf(buf, w, h, x, y, r, color);
    }
}

fn fill_disk_buf(buf: &mut [u32], w: u32, h: u32, cx: f64, cy: f64, r: f64, color: u32) {
    let r_ceil = r.ceil() as i32;
    let r2 = r * r;
    for dy in -r_ceil..=r_ceil {
        for dx in -r_ceil..=r_ceil {
            let fx = dx as f64;
            let fy = dy as f64;
            if fx * fx + fy * fy <= r2 {
                put_buf(buf, w, h, cx as i32 + dx, cy as i32 + dy, color);
            }
        }
    }
}

fn draw_arrowhead_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    from: (i32, i32),
    to: (i32, i32),
    color: u32,
) {
    let (fx, fy) = (from.0 as f64, from.1 as f64);
    let (tx, ty) = (to.0 as f64, to.1 as f64);
    let dx = tx - fx;
    let dy = ty - fy;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }
    let ux = dx / len;
    let uy = dy / len;
    let base_x = tx - ux * ARROWHEAD_LEN as f64;
    let base_y = ty - uy * ARROWHEAD_LEN as f64;
    let px = -uy;
    let py = ux;
    let half_w = ARROWHEAD_LEN as f64 * 0.577; // 60° tip: half-angle 30° → tan(30°)
    let a = (base_x + px * half_w, base_y + py * half_w);
    let b = (base_x - px * half_w, base_y - py * half_w);
    let tip = (tx, ty);
    fill_triangle_buf(buf, w, h, tip, a, b, color);
}

fn fill_triangle_buf(
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
                put_buf(buf, w, h, x, y, color);
            }
        }
    }
}

fn draw_rect_outline_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    tl: (i32, i32),
    br: (i32, i32),
    color: u32,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let (x0, y0) = tl;
    let (x1, y1) = br;
    let half = thickness / 2;
    for ty in 0..thickness {
        let yt = y0 - half + ty;
        let yb = y1 - half + ty;
        for x in x0..=x1 {
            put_buf(buf, w, h, x, yt, color);
            put_buf(buf, w, h, x, yb, color);
        }
    }
    for tx in 0..thickness {
        let xl = x0 - half + tx;
        let xr = x1 - half + tx;
        for y in y0..=y1 {
            put_buf(buf, w, h, xl, y, color);
            put_buf(buf, w, h, xr, y, color);
        }
    }
}

fn draw_ellipse_outline_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    tl: (i32, i32),
    br: (i32, i32),
    color: u32,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let cx = (tl.0 as f64 + br.0 as f64) / 2.0;
    let cy = (tl.1 as f64 + br.1 as f64) / 2.0;
    let rx = ((br.0 - tl.0) as f64).abs() / 2.0;
    let ry = ((br.1 - tl.1) as f64).abs() / 2.0;
    if rx < 1.0 || ry < 1.0 {
        return;
    }
    let half_thick = thickness as f64 / 2.0;
    let band = half_thick / rx.min(ry);
    let pad = (thickness + 2).max(2);
    let x_min = tl.0.min(br.0) - pad;
    let x_max = tl.0.max(br.0) + pad;
    let y_min = tl.1.min(br.1) - pad;
    let y_max = tl.1.max(br.1) + pad;
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let nx = (x as f64 + 0.5 - cx) / rx;
            let ny = (y as f64 + 0.5 - cy) / ry;
            let r = (nx * nx + ny * ny).sqrt();
            if (r - 1.0).abs() <= band {
                put_buf(buf, w, h, x, y, color);
            }
        }
    }
}

fn apply_mosaic_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    rect_frame: Rect,
    block_size: u32,
) {
    if rect_frame.w <= 0 || rect_frame.h <= 0 || block_size == 0 {
        return;
    }
    let frame_size = frame.dimensions();
    let bs = block_size as i32;
    let (iw, ih) = (frame_size.0 as i32, frame_size.1 as i32);
    let x0 = rect_frame.x.max(0);
    let y0 = rect_frame.y.max(0);
    let x1 = (rect_frame.x + rect_frame.w).min(iw);
    let y1 = (rect_frame.y + rect_frame.h).min(ih);

    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let block_x1 = (bx + bs).min(x1);
            let block_y1 = (by + bs).min(y1);
            let mut sum = [0u64; 3];
            let mut count: u64 = 0;
            for yy in by..block_y1 {
                for xx in bx..block_x1 {
                    let px = frame.get_pixel(xx as u32, yy as u32).0;
                    sum[0] += px[0] as u64;
                    sum[1] += px[1] as u64;
                    sum[2] += px[2] as u64;
                    count += 1;
                }
            }
            if count == 0 {
                bx += bs;
                continue;
            }
            let avg_argb = ((sum[0] / count) as u32) << 16
                | ((sum[1] / count) as u32) << 8
                | ((sum[2] / count) as u32);
            let win_tl = frame_to_window((bx, by), frame_size, (win_w, win_h));
            let win_br = frame_to_window(
                (block_x1 - 1, block_y1 - 1),
                frame_size,
                (win_w, win_h),
            );
            for y in win_tl.1..=win_br.1 {
                for x in win_tl.0..=win_br.0 {
                    put_buf(buf, win_w, win_h, x, y, avg_argb);
                }
            }
            bx += bs;
        }
        by += bs;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn red() -> Rgba<u8> { Rgba([255, 59, 48, 255]) }

    #[test]
    fn draw_rect_outline_basic() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0u8, 0, 0, 255]));
        let rect = Rect { x: 5, y: 5, w: 10, h: 10 };
        draw_rect_outline_on_image(&mut img, rect, red(), 1);
        assert_eq!(img.get_pixel(5, 5).0, [255, 59, 48, 255]);
        assert_eq!(img.get_pixel(10, 10).0, [0, 0, 0, 255]);
    }

    #[test]
    fn draw_rect_outline_zero_dims_noop() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([0u8, 0, 0, 255]));
        let rect = Rect { x: 5, y: 5, w: 0, h: 0 };
        draw_rect_outline_on_image(&mut img, rect, red(), 1);
        for px in img.pixels() {
            assert_eq!(px.0, [0, 0, 0, 255]);
        }
    }

    #[test]
    fn mosaic_averages_block() {
        let mut img = RgbaImage::new(8, 8);
        for y in 0..8 {
            for x in 0..8 {
                let c = if x < 4 {
                    Rgba([255u8, 0, 0, 255])
                } else {
                    Rgba([0u8, 0, 255, 255])
                };
                img.put_pixel(x, y, c);
            }
        }
        apply_mosaic_on_image(&mut img, Rect { x: 0, y: 0, w: 8, h: 8 }, 8);
        let expected = [127u8, 0, 127, 255];
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(img.get_pixel(x, y).0, expected, "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn mosaic_blocks_are_independent() {
        let mut img = RgbaImage::new(16, 8);
        for y in 0..8 {
            for x in 0..16 {
                let c = if x < 8 {
                    Rgba([255u8, 0, 0, 255])
                } else {
                    Rgba([0u8, 0, 255, 255])
                };
                img.put_pixel(x, y, c);
            }
        }
        apply_mosaic_on_image(&mut img, Rect { x: 0, y: 0, w: 16, h: 8 }, 8);
        assert_eq!(img.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(7, 7).0, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(8, 0).0, [0, 0, 255, 255]);
        assert_eq!(img.get_pixel(15, 7).0, [0, 0, 255, 255]);
    }

    #[test]
    fn paint_on_cropped_translates() {
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0u8, 0, 0, 255]));
        paint_on_cropped(
            &mut img,
            Annotation::Rect { rect: Rect { x: 15, y: 15, w: 5, h: 5 } },
            (10, 10),
        );
        assert_eq!(img.get_pixel(5, 5).0, [255, 59, 48, 255]);
    }
}
