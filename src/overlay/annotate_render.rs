//! Rendering helpers for annotations. Two code paths:
//!   * `*_on_image`: paint into a `RgbaImage` (used by flatten_for_export).
//!   * `*_on_buf`: paint into a softbuffer `&mut [u32]` (used by live preview;
//!     implemented in Task 3).

use image::{Rgba, RgbaImage};

use super::annotate::{Annotation, AnnotationStyle};
use super::state::Rect;

pub const ARROWHEAD_LEN: f32 = 28.0; // 2× the original 14

/// Multi-line text leading factor (line_height / px_size).
const LINE_HEIGHT_RATIO: f32 = 1.2;

/// Apply an annotation to a CROPPED image. `crop_offset` is the frame-space
/// coordinate corresponding to the cropped image's (0, 0). Annotations are
/// stored in frame coords; we translate into crop-local coords here.
pub fn paint_on_cropped(
    img: &mut RgbaImage,
    ann: &Annotation,
    crop_offset: (i32, i32),
    font: &mut crate::text::Font,
) {
    let (ox, oy) = crop_offset;
    match ann {
        Annotation::Arrow { from, to, style } => {
            draw_arrow_on_image(
                img,
                (from.0 - ox, from.1 - oy),
                (to.0 - ox, to.1 - oy),
                Rgba(style.color.rgba()),
                style.stroke.px(),
            );
        }
        Annotation::Rect { rect, style } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_rect_outline_on_image(img, local, Rgba(style.color.rgba()), style.stroke.px());
        }
        Annotation::Ellipse { rect, style } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            draw_ellipse_outline_on_image(img, local, Rgba(style.color.rgba()), style.stroke.px());
        }
        Annotation::Mosaic { rect, block_size } => {
            let local = Rect { x: rect.x - ox, y: rect.y - oy, w: rect.w, h: rect.h };
            apply_mosaic_on_image(img, local, *block_size);
        }
        Annotation::Pen { points, style } => {
            let translated: Vec<(i32, i32)> =
                points.iter().map(|&(x, y)| (x - ox, y - oy)).collect();
            paint_pen_on_image(img, &translated, *style);
        }
        Annotation::Text { origin, content, style } => {
            paint_text_on_image(
                img,
                (origin.0 - ox, origin.1 - oy),
                content,
                *style,
                font,
            );
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
    // Per-pixel distance-coverage AA (image variant). Single blend pass per
    // pixel with proper coverage — no overlap accumulation.
    let cx = rect.x as f64 + rect.w as f64 / 2.0;
    let cy = rect.y as f64 + rect.h as f64 / 2.0;
    let rx = rect.w as f64 / 2.0;
    let ry = rect.h as f64 / 2.0;
    let half_t = thickness as f64 / 2.0;
    let min_r = rx.min(ry);
    let pad = (thickness + 2).max(2);
    let y_min = rect.y - pad;
    let y_max = rect.y + rect.h + pad;
    let x_min = rect.x - pad;
    let x_max = rect.x + rect.w + pad;
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            let r_norm = (nx * nx + ny * ny).sqrt();
            // Approximate Euclidean distance to perimeter via min-radius scale.
            let dist = (r_norm - 1.0).abs() * min_r;
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend_image(img, x, y, color, coverage);
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
    _w: i32,
    _h: i32,
) {
    // Per-pixel distance-coverage AA (image variant). Avoids speckling that
    // arises from stamping AA disks along a path. _w/_h are kept for callsite
    // compatibility; `put_blend_image` does its own bounds check.
    if thickness <= 0 {
        return;
    }
    let half_t = thickness as f64 / 2.0;
    let fx0 = from.0 as f64;
    let fy0 = from.1 as f64;
    let fx1 = to.0 as f64;
    let fy1 = to.1 as f64;
    let dx = fx1 - fx0;
    let dy = fy1 - fy0;
    let len2 = dx * dx + dy * dy;
    let bound_min_x = (fx0.min(fx1) - half_t - 1.0).floor() as i32;
    let bound_max_x = (fx0.max(fx1) + half_t + 1.0).ceil() as i32;
    let bound_min_y = (fy0.min(fy1) - half_t - 1.0).floor() as i32;
    let bound_max_y = (fy0.max(fy1) + half_t + 1.0).ceil() as i32;

    for y in bound_min_y..=bound_max_y {
        for x in bound_min_x..=bound_max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            // Project pixel center onto segment; clamp [0,1] for rounded caps.
            // For degenerate (len2≈0) lines, force tc=0 so we just compute
            // distance to the start point (round dot).
            let tc = if len2 < 0.0001 {
                0.0
            } else {
                (((px - fx0) * dx + (py - fy0) * dy) / len2).clamp(0.0, 1.0)
            };
            let proj_x = fx0 + tc * dx;
            let proj_y = fy0 + tc * dy;
            let ddx = px - proj_x;
            let ddy = py - proj_y;
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend_image(img, x, y, color, coverage);
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
    _w: i32,
    _h: i32,
) {
    // 4×4 supersample AA — matches toolbar.rs's fill_triangle. Coverage is
    // hits / (N*N), blended once per pixel.
    let min_x = p0.0.min(p1.0).min(p2.0).floor() as i32;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil() as i32;
    let min_y = p0.1.min(p1.1).min(p2.1).floor() as i32;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil() as i32;
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
                put_blend_image(img, x, y, color, coverage);
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

/// AA pixel writer for softbuffer u32 path.
/// Blends `color` (0x00RRGGBB) into the buffer at (x, y) by `coverage`
/// in a single pass (no overlap accumulation).
fn put_blend_buf(buf: &mut [u32], w: u32, h: u32, x: i32, y: i32, color: u32, coverage: f32) {
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
    buf[idx] = (((fr * a + br * inv) as u32) << 16)
        | (((fg * a + bgc * inv) as u32) << 8)
        | ((fb * a + bb * inv) as u32);
}

/// AA pixel writer for image::RgbaImage path. Preserves destination alpha so
/// overlapping text annotations can still blend correctly (Iter 5b decision).
fn put_blend_image(img: &mut RgbaImage, x: i32, y: i32, color: Rgba<u8>, coverage: f32) {
    if coverage <= 0.0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return;
    }
    let a = coverage.clamp(0.0, 1.0);
    let inv = 1.0 - a;
    let bg = img.get_pixel(x as u32, y as u32);
    let r = (color[0] as f32 * a + bg[0] as f32 * inv) as u8;
    let g = (color[1] as f32 * a + bg[1] as f32 * inv) as u8;
    let b = (color[2] as f32 * a + bg[2] as f32 * inv) as u8;
    img.put_pixel(x as u32, y as u32, Rgba([r, g, b, bg[3]]));
}

// --- buf-path helpers (live softbuffer preview) ---

use super::annotate::PendingDraw;

/// Map a frame-space point to a window-space point.
pub(crate) fn frame_to_window(p: (i32, i32), frame_size: (u32, u32), window_size: (u32, u32)) -> (i32, i32) {
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

pub fn paint_pen_on_image(img: &mut RgbaImage, points: &[(i32, i32)], style: AnnotationStyle) {
    if points.len() < 2 { return; }
    let color = Rgba(style.color.rgba());
    let thickness = style.stroke.px();
    let (w, h) = (img.width() as i32, img.height() as i32);
    for win in points.windows(2) {
        draw_line_thick(img, win[0], win[1], color, thickness, w, h);
    }
}

pub fn draw_pen_on_buf(
    buf: &mut [u32], win_w: u32, win_h: u32,
    frame: &RgbaImage, points: &[(i32, i32)], style: AnnotationStyle,
) {
    if points.len() < 2 { return; }
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
    let argb = style.color.argb();
    for w in points.windows(2) {
        let a = frame_to_window(w[0], frame_size, window_size);
        let b = frame_to_window(w[1], frame_size, window_size);
        draw_line_thick_buf(buf, win_w, win_h, a, b, argb, thickness_win);
    }
}

/// Paint multi-line text into an `RgbaImage` (export path). `origin` is the
/// crop-local pen position of the first line; subsequent lines step down by
/// `1.2 * px_size`. Color/font size come from `style`.
pub fn paint_text_on_image(
    img: &mut RgbaImage,
    origin: (i32, i32),
    content: &str,
    style: AnnotationStyle,
    font: &mut crate::text::Font,
) {
    let px_size = style.stroke.font_px();
    let line_height = (px_size * LINE_HEIGHT_RATIO).round() as i32;
    let color_rgba = style.color.rgba();
    // Normalize Windows / classic-Mac line endings before splitting on '\n'.
    // Order matters: replace `\r\n` first, then lone `\r`. Otherwise `\r\n`
    // would become `\n\n`.
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    for (i, line) in normalized.split('\n').enumerate() {
        let ly = origin.1 + i as i32 * line_height;
        font.render_text_rgba(img, origin.0, ly, line, px_size, color_rgba);
    }
}

/// Paint multi-line text into the softbuffer (live preview). `origin_frame` is
/// in frame coords; we map it to window coords and scale `px_size` by the
/// vertical frame→window ratio so on-screen text matches the eventual export.
pub fn draw_text_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    origin_frame: (i32, i32),
    content: &str,
    style: AnnotationStyle,
    font: &mut crate::text::Font,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    let (px, py) = frame_to_window(origin_frame, frame_size, window_size);
    let scale_y = window_size.1 as f32 / frame_size.1.max(1) as f32;
    let px_size = style.stroke.font_px() * scale_y;
    let argb = style.color.argb();
    let line_height = (px_size * LINE_HEIGHT_RATIO).round() as i32;
    // Normalize Windows / classic-Mac line endings before splitting on '\n'.
    // Order matters: replace `\r\n` first, then lone `\r`. Otherwise `\r\n`
    // would become `\n\n`.
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    for (i, line) in normalized.split('\n').enumerate() {
        let ly = py + i as i32 * line_height;
        font.render_text(buf, win_w, win_h, px, ly, line, px_size, argb);
    }
}

pub fn draw_annotation_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    ann: &Annotation,
    font: &mut crate::text::Font,
) {
    let frame_size = frame.dimensions();
    let window_size = (win_w, win_h);
    match ann {
        Annotation::Arrow { from, to, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let f = frame_to_window(*from, frame_size, window_size);
            let t = frame_to_window(*to, frame_size, window_size);
            let shaft_end = shaft_end_point(f, t, ARROWHEAD_LEN as f64);
            draw_line_thick_buf(buf, win_w, win_h, f, shaft_end, style.color.argb(), thickness_win);
            draw_arrowhead_buf(buf, win_w, win_h, f, t, style.color.argb());
        }
        Annotation::Rect { rect, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_rect_outline_buf(buf, win_w, win_h, tl, br, style.color.argb(), thickness_win);
        }
        Annotation::Ellipse { rect, style } => {
            let thickness_win = window_thickness(style.stroke.px(), frame_size, window_size);
            let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
            let br = frame_to_window(
                (rect.x + rect.w - 1, rect.y + rect.h - 1),
                frame_size,
                window_size,
            );
            draw_ellipse_outline_buf(buf, win_w, win_h, tl, br, style.color.argb(), thickness_win);
        }
        Annotation::Mosaic { rect, block_size } => {
            apply_mosaic_on_buf(buf, win_w, win_h, frame, *rect, *block_size);
        }
        Annotation::Pen { points, style } => {
            draw_pen_on_buf(buf, win_w, win_h, frame, points, *style);
        }
        Annotation::Text { origin, content, style } => {
            draw_text_on_buf(buf, win_w, win_h, frame, *origin, content, *style, font);
        }
    }
}

/// Live preview of an in-flight `PendingDraw`. Currently only Shape and Pen
/// variants exist; `font` is plumbed through for signature uniformity with
/// `draw_annotation_on_buf` and reserved for a future text-edit pending path.
pub fn draw_pending_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    pending: &PendingDraw,
    font: &mut crate::text::Font,
) {
    match pending {
        PendingDraw::Shape { .. } => {
            // Reuse finalize → Annotation, then draw. Cheap clone for Shape variants.
            if let Some(ann) = pending.clone().finalize() {
                draw_annotation_on_buf(buf, win_w, win_h, frame, &ann, font);
            }
        }
        PendingDraw::Pen { points, style } => {
            draw_pen_on_buf(buf, win_w, win_h, frame, points, *style);
        }
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
    // Per-pixel distance-coverage AA (buf variant). One blend pass per pixel.
    if thickness <= 0 {
        return;
    }
    let half_t = thickness as f64 / 2.0;
    let fx0 = from.0 as f64;
    let fy0 = from.1 as f64;
    let fx1 = to.0 as f64;
    let fy1 = to.1 as f64;
    let dx = fx1 - fx0;
    let dy = fy1 - fy0;
    let len2 = dx * dx + dy * dy;
    let bound_min_x = (fx0.min(fx1) - half_t - 1.0).floor() as i32;
    let bound_max_x = (fx0.max(fx1) + half_t + 1.0).ceil() as i32;
    let bound_min_y = (fy0.min(fy1) - half_t - 1.0).floor() as i32;
    let bound_max_y = (fy0.max(fy1) + half_t + 1.0).ceil() as i32;

    for y in bound_min_y..=bound_max_y {
        for x in bound_min_x..=bound_max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            // For degenerate (len2≈0) lines, force tc=0 → distance to start.
            let tc = if len2 < 0.0001 {
                0.0
            } else {
                (((px - fx0) * dx + (py - fy0) * dy) / len2).clamp(0.0, 1.0)
            };
            let proj_x = fx0 + tc * dx;
            let proj_y = fy0 + tc * dy;
            let ddx = px - proj_x;
            let ddy = py - proj_y;
            let dist = (ddx * ddx + ddy * ddy).sqrt();
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend_buf(buf, w, h, x, y, color, coverage);
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
    // 4×4 supersample AA — matches toolbar.rs's fill_triangle.
    let min_x = p0.0.min(p1.0).min(p2.0).floor() as i32;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil() as i32;
    let min_y = p0.1.min(p1.1).min(p2.1).floor() as i32;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil() as i32;
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
                put_blend_buf(buf, w, h, x, y, color, coverage);
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
    // Per-pixel distance-coverage AA (buf variant). One pass per pixel.
    let half_t = thickness as f64 / 2.0;
    let min_r = rx.min(ry);
    let pad = (thickness + 2).max(2);
    let x_min = tl.0.min(br.0) - pad;
    let x_max = tl.0.max(br.0) + pad;
    let y_min = tl.1.min(br.1) - pad;
    let y_max = tl.1.max(br.1) + pad;
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            let r_norm = (nx * nx + ny * ny).sqrt();
            let dist = (r_norm - 1.0).abs() * min_r;
            let coverage = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
            if coverage > 0.0 {
                put_blend_buf(buf, w, h, x, y, color, coverage);
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
        use super::super::annotate::AnnotationStyle;
        let mut img = RgbaImage::from_pixel(20, 20, Rgba([0u8, 0, 0, 255]));
        let mut font = crate::text::Font::embedded();
        paint_on_cropped(
            &mut img,
            &Annotation::Rect {
                rect: Rect { x: 15, y: 15, w: 5, h: 5 },
                style: AnnotationStyle::default(),
            },
            (10, 10),
            &mut font,
        );
        assert_eq!(img.get_pixel(5, 5).0, [255, 59, 48, 255]);
    }

    #[test]
    fn pen_paints_visible_pixels_on_image() {
        use super::super::annotate::AnnotationStyle;
        let mut img = RgbaImage::new(50, 50);
        let style = AnnotationStyle::default();
        let points = vec![(5, 25), (45, 25)];
        paint_pen_on_image(&mut img, &points, style);
        // The horizontal line should mark pixels along y=25 with the red color.
        let any_red = (5..=45).any(|x| {
            let p = img.get_pixel(x, 25);
            p[0] >= 0xF0 && p[1] < 0x80 && p[2] < 0x80
        });
        assert!(any_red, "expected red pixels along the pen path");
    }

    #[test]
    fn pen_with_under_two_points_is_noop() {
        use super::super::annotate::AnnotationStyle;
        let mut img = RgbaImage::new(20, 20);
        let style = AnnotationStyle::default();
        paint_pen_on_image(&mut img, &[(10, 10)], style);
        for p in img.pixels() {
            assert_eq!(*p, Rgba([0, 0, 0, 0]), "single-point pen should not paint");
        }
    }

    #[test]
    fn paint_text_paints_some_pixels() {
        // Use an opaque black destination — matches the real export path
        // (flatten_for_export always paints onto an opaque crop). Since
        // render_text_rgba correctly preserves destination alpha (Porter-Duff
        // source-over), we detect "painted" by an RGB change, not by alpha.
        let mut img = RgbaImage::from_pixel(200, 60, Rgba([0u8, 0, 0, 255]));
        let mut font = crate::text::Font::embedded();
        let style = AnnotationStyle::default();
        paint_text_on_image(&mut img, (4, 4), "Hi", style, &mut font);
        let any_painted = img.pixels().any(|p| p[0] != 0 || p[1] != 0 || p[2] != 0);
        assert!(any_painted, "expected some painted pixels for 'Hi'");
    }
}
