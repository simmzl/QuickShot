//! Rendering helpers for annotations. Two code paths:
//!   * `*_on_image`: paint into a `RgbaImage` (used by flatten_for_export).
//!   * `*_on_buf`: paint into a softbuffer `&mut [u32]` (used by live preview;
//!     implemented in Task 3).

use image::{Rgba, RgbaImage};

use super::annotate::{Annotation, AnnotationStyle};
use super::state::Rect;

pub const ARROWHEAD_LEN: f32 = 28.0; // 2× the original 14

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
    let pad = (thickness + 2).max(2);
    let (iw, ih) = (img.width() as i32, img.height() as i32);
    let y_min = (rect.y - pad).max(0);
    let y_max = (rect.y + rect.h + pad).min(ih - 1);
    let x_min = (rect.x - pad).max(0);
    let x_max = (rect.x + rect.w + pad).min(iw - 1);
    // Cheap reject: points whose normalized radius is far from 1 can't be
    // within the stroke. The band is sized for the *major* radius so it's
    // conservative everywhere on the ring.
    let band = (half_t + 1.0) / rx.min(ry);
    let (r_lo, r_hi) = ((1.0 - band).max(0.0), 1.0 + band);
    let (r_lo2, r_hi2) = (r_lo * r_lo, r_hi * r_hi);
    for y in y_min..=y_max {
        for x in x_min..=x_max {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let nx = (px - cx) / rx;
            let ny = (py - cy) / ry;
            let rn2 = nx * nx + ny * ny;
            if rn2 < r_lo2 || rn2 > r_hi2 {
                continue;
            }
            let dist = ellipse_outline_distance(px, py, cx, cy, rx, ry);
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

/// Hit-test annotations in reverse z-order (topmost first). Returns the
/// index of the first annotation whose footprint contains `cursor_frame`,
/// or None. Useful when the user clicks to grab an annotation.
///
/// Footprint per variant (frame-space coords):
/// - Arrow: tolerance band around the line segment (width = stroke.px() * 2).
/// - Rect: outline OR interior (so a click anywhere inside the rect grabs it).
/// - Ellipse: outline OR interior.
/// - Mosaic: interior (the masked rect).
/// - Pen: tolerance band around any segment of the path.
/// - Text: rendered bbox.
pub fn annotation_at_cursor(
    cursor_frame: (i32, i32),
    history: &[Annotation],
    font: &mut crate::text::Font,
) -> Option<usize> {
    history
        .iter()
        .enumerate()
        .rev()
        .find(|(_, ann)| annotation_contains_point(ann, cursor_frame, font))
        .map(|(i, _)| i)
}

fn annotation_contains_point(
    ann: &Annotation,
    p: (i32, i32),
    font: &mut crate::text::Font,
) -> bool {
    match ann {
        Annotation::Arrow { from, to, style } => {
            point_near_segment(p, *from, *to, (style.stroke.px() * 2).max(8))
        }
        Annotation::Rect { rect, .. } | Annotation::Mosaic { rect, .. } => rect.contains(p),
        Annotation::Ellipse { rect, .. } => rect.contains(p),
        Annotation::Pen { points, style } => {
            let tol = (style.stroke.px() * 2).max(8);
            points.windows(2).any(|win| point_near_segment(p, win[0], win[1], tol))
        }
        Annotation::Text { .. } => text_bbox(ann, font)
            .is_some_and(|bb| bb.contains(p)),
    }
}

/// Manhattan-ish perpendicular distance from a point to a segment, with a
/// tolerance threshold. Returns true if the point is within `tol_px` of the
/// segment (treats it as a "fat line" with rounded endpoints).
fn point_near_segment(p: (i32, i32), a: (i32, i32), b: (i32, i32), tol_px: i32) -> bool {
    let px = p.0 as f64;
    let py = p.1 as f64;
    let ax = a.0 as f64;
    let ay = a.1 as f64;
    let bx = b.0 as f64;
    let by = b.1 as f64;
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    if len2 < 0.0001 {
        // Degenerate segment — treat as a point.
        let d = ((px - ax).powi(2) + (py - ay).powi(2)).sqrt();
        return d <= tol_px as f64;
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / len2;
    let tc = t.clamp(0.0, 1.0);
    let proj_x = ax + tc * dx;
    let proj_y = ay + tc * dy;
    let dist = ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt();
    dist <= tol_px as f64
}

/// Frame-space bounding rectangle of an annotation, padded slightly to
/// accommodate stroke width. Used by the Move-tool hover/drag highlight.
pub fn annotation_bbox(
    ann: &Annotation,
    font: &mut crate::text::Font,
) -> Rect {
    match ann {
        Annotation::Arrow { from, to, style } => {
            let min_x = from.0.min(to.0);
            let max_x = from.0.max(to.0);
            let min_y = from.1.min(to.1);
            let max_y = from.1.max(to.1);
            let pad = style.stroke.px() + 4;
            Rect {
                x: min_x - pad,
                y: min_y - pad,
                w: (max_x - min_x) + pad * 2,
                h: (max_y - min_y) + pad * 2,
            }
        }
        Annotation::Rect { rect, style } | Annotation::Ellipse { rect, style } => {
            let pad = style.stroke.px() + 2;
            Rect {
                x: rect.x - pad,
                y: rect.y - pad,
                w: rect.w + pad * 2,
                h: rect.h + pad * 2,
            }
        }
        Annotation::Mosaic { rect, .. } => Rect {
            x: rect.x - 2,
            y: rect.y - 2,
            w: rect.w + 4,
            h: rect.h + 4,
        },
        Annotation::Pen { points, style } => {
            if points.is_empty() {
                return Rect { x: 0, y: 0, w: 0, h: 0 };
            }
            let min_x = points.iter().map(|p| p.0).min().unwrap();
            let max_x = points.iter().map(|p| p.0).max().unwrap();
            let min_y = points.iter().map(|p| p.1).min().unwrap();
            let max_y = points.iter().map(|p| p.1).max().unwrap();
            let pad = style.stroke.px() + 4;
            Rect {
                x: min_x - pad,
                y: min_y - pad,
                w: (max_x - min_x) + pad * 2,
                h: (max_y - min_y) + pad * 2,
            }
        }
        Annotation::Text { .. } => text_bbox(ann, font).unwrap_or(Rect { x: 0, y: 0, w: 0, h: 0 }),
    }
}

/// Map a frame-space rectangle to a window-space tuple `(x, y, w, h)`.
/// Returns `None` for degenerate input (w or h ≤ 0). Used by the Move-tool
/// hover/drag highlight to convert `annotation_bbox` output to the
/// `draw_selection_outline` input shape.
pub fn frame_rect_to_window_rect(
    rect: Rect,
    frame_size: (u32, u32),
    window_size: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    if rect.w <= 0 || rect.h <= 0 {
        return None;
    }
    let tl = frame_to_window((rect.x, rect.y), frame_size, window_size);
    let br = frame_to_window(
        (rect.x + rect.w - 1, rect.y + rect.h - 1),
        frame_size,
        window_size,
    );
    let x = tl.0.max(0) as u32;
    let y = tl.1.max(0) as u32;
    let w = (br.0 - tl.0 + 1).max(1) as u32;
    let h = (br.1 - tl.1 + 1).max(1) as u32;
    Some((x, y, w, h))
}

/// Compute the rendered bounding box (in frame-space) of a Text annotation.
/// Returns None for non-Text variants or empty content.
///
/// Used by Tool::Text click handling to test whether the user clicked on an
/// existing text annotation (so it can be lifted back into edit mode).
pub fn text_bbox(
    ann: &Annotation,
    font: &mut crate::text::Font,
) -> Option<Rect> {
    let Annotation::Text { origin, content, style } = ann else { return None };
    if content.is_empty() {
        return None;
    }
    let (w, h) = font.measure_annotation(content, style.stroke.font_px(), style.color.argb());
    Some(Rect {
        x: origin.0,
        y: origin.1,
        w: (w as i32).max(1),
        h: (h as i32).max(1),
    })
}

/// Paint text into an `RgbaImage` (export path). `origin` is the crop-local
/// top-left of the first line box. Rasterized by the platform text stack
/// (same font as the native editor) so the export matches what was typed.
pub fn paint_text_on_image(
    img: &mut RgbaImage,
    origin: (i32, i32),
    content: &str,
    style: AnnotationStyle,
    font: &mut crate::text::Font,
) {
    let mask = font.text_mask(content, style.stroke.font_px(), style.color.argb());
    crate::text::blit_mask_rgba(img, origin.0, origin.1, mask);
}

/// Paint text into the softbuffer (live preview). `origin_frame` is in frame
/// coords; the frame-space mask is scaled by the frame→window ratio so the
/// on-screen text matches the eventual export.
#[allow(clippy::too_many_arguments)]
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
    let scale = window_size.1 as f32 / frame_size.1.max(1) as f32;
    let mask = font.text_mask(content, style.stroke.font_px(), style.color.argb());
    crate::text::blit_mask_buf(buf, win_w, win_h, px, py, mask, scale);
}

pub fn draw_annotation_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    ann: &Annotation,
    font: &mut crate::text::Font,
    mosaic_cache: Option<&mut MosaicCache>,
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
            apply_mosaic_on_buf(buf, win_w, win_h, frame, *rect, *block_size, mosaic_cache);
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
            // No mosaic cache: an in-flight mosaic's rect changes every frame,
            // so memoizing would only accumulate stale entries.
            if let Some(ann) = pending.clone().finalize() {
                draw_annotation_on_buf(buf, win_w, win_h, frame, &ann, font, None);
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

/// Thick anti-aliased segment with round caps (live preview).
///
/// Hot path while dragging an arrow / pen stroke. Instead of testing every
/// pixel of the segment's axis-aligned bounding box (a diagonal 3000 px
/// arrow touched ~9 M pixels, mostly empty), each scanline only visits the
/// x-span where the capsule can have coverage, and the `sqrt` is skipped
/// for pixels that are fully inside or fully outside.
fn draw_line_thick_buf(
    buf: &mut [u32],
    w: u32,
    h: u32,
    from: (i32, i32),
    to: (i32, i32),
    color: u32,
    thickness: i32,
) {
    if thickness <= 0 {
        return;
    }
    let half_t = thickness as f64 / 2.0;
    let (x0, y0) = (from.0 as f64, from.1 as f64);
    let (x1, y1) = (to.0 as f64, to.1 as f64);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    // Coverage reaches 0 at distance half_t + 0.5 from the centerline.
    let reach = half_t + 0.5;
    let reach2 = reach * reach;
    let inner = (half_t - 0.5).max(0.0);
    let inner2 = inner * inner;

    let y_lo = ((y0.min(y1) - reach).floor() as i32).max(0);
    let y_hi = ((y0.max(y1) + reach).ceil() as i32).min(h as i32 - 1);
    for y in y_lo..=y_hi {
        let py = y as f64 + 0.5;
        // Exact x-range of the capsule (segment ⊕ disk of radius `reach`)
        // on this scanline: union of both end-cap circles and the band
        // between the two offset lines.
        let (mut xa, mut xb) = (f64::INFINITY, f64::NEG_INFINITY);
        for (cx, cy) in [(x0, y0), (x1, y1)] {
            let ddy = py - cy;
            if ddy.abs() < reach {
                let hw = (reach2 - ddy * ddy).sqrt();
                xa = xa.min(cx - hw);
                xb = xb.max(cx + hw);
            }
        }
        if len2 > 1e-6 && dy.abs() > 1e-9 {
            // Band between the caps: on row `py` the infinite thick line
            // occupies x ∈ [xc - hw, xc + hw] with xc the centerline's x at
            // this row and hw = reach·len/|dy|. The part of it within the
            // segment's t∈[0,1] slab lies between the end caps' x-centers,
            // widened by hw — a superset of the true band that stays tight.
            let len = len2.sqrt();
            let xc = x0 + (py - y0) * dx / dy;
            let hw = reach * len / dy.abs();
            let lo = (xc - hw).max(x0.min(x1) - reach);
            let hi = (xc + hw).min(x0.max(x1) + reach);
            // Only rows inside the segment's vertical extent (± reach) can
            // hit the band; the caps above already cover the ends.
            if py >= y0.min(y1) - reach && py <= y0.max(y1) + reach && lo <= hi {
                xa = xa.min(lo);
                xb = xb.max(hi);
            }
        } else if len2 > 1e-6 && (py - y0).abs() < reach {
            // Horizontal segment: band spans the whole segment.
            xa = xa.min(x0.min(x1));
            xb = xb.max(x0.max(x1));
        }
        if xa > xb {
            continue;
        }
        // xa/xb bound pixel *centers*; pad one pixel so floor/ceil can't
        // drop an edge pixel. The exact distance test below decides.
        let x_lo = ((xa - 1.0).floor() as i32).max(0);
        let x_hi = ((xb + 1.0).ceil() as i32).min(w as i32 - 1);
        let row = (y as u32 * w) as usize;
        for x in x_lo..=x_hi {
            let px = x as f64 + 0.5;
            let tc = if len2 < 1e-6 {
                0.0
            } else {
                (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0)
            };
            let ex = px - (x0 + tc * dx);
            let ey = py - (y0 + tc * dy);
            let d2 = ex * ex + ey * ey;
            if d2 >= reach2 {
                continue;
            }
            let idx = row + x as usize;
            if d2 <= inner2 {
                buf[idx] = color;
            } else {
                let cov = (reach - d2.sqrt()).clamp(0.0, 1.0) as f32;
                blend_px(&mut buf[idx], color, cov);
            }
        }
    }
}

/// Approximate Euclidean distance from (px, py) to the ellipse outline with
/// center (cx, cy) and radii (rx, ry).
///
/// Uses the first-order estimate |F| / |∇F| on F(x, y) = √(x²/rx² + y²/ry²) − 1,
/// which stays accurate for very eccentric ellipses. The previous estimate
/// `|F| · min(rx, ry)` treated every point as if it lay on the minor axis, so
/// on a wide ellipse the stroke near the narrow ends came out rx/ry times
/// thicker than requested.
#[inline(always)]
pub(crate) fn ellipse_outline_distance(px: f64, py: f64, cx: f64, cy: f64, rx: f64, ry: f64) -> f64 {
    let (nx, ny) = ((px - cx) / rx, (py - cy) / ry);
    let r = (nx * nx + ny * ny).sqrt();
    if r < 1e-9 {
        // Exactly at the center: distance to the nearest vertex.
        return rx.min(ry);
    }
    // ∇F = (nx / (rx·r), ny / (ry·r)) → |∇F| = √((nx/rx)² + (ny/ry)²) / r
    let gx = nx / rx;
    let gy = ny / ry;
    let grad = (gx * gx + gy * gy).sqrt() / r;
    (r - 1.0).abs() / grad
}

/// Blend `color` over `*px` by `cov` (0..=1), 0x00RRGGBB.
#[inline(always)]
fn blend_px(px: &mut u32, color: u32, cov: f32) {
    let a = (cov * 256.0) as u32;
    let inv = 256 - a;
    let bg = *px;
    let rb = (((color & 0x00FF_00FF) * a + (bg & 0x00FF_00FF) * inv) >> 8) & 0x00FF_00FF;
    let g = (((color & 0x0000_FF00) * a + (bg & 0x0000_FF00) * inv) >> 8) & 0x0000_FF00;
    *px = rb | g;
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

/// Anti-aliased ellipse outline (live preview). Scanline-based: each row
/// only visits the two short x-spans the ring actually crosses, instead of
/// the full bounding box (a full-screen ellipse was ~7 M pixel tests per
/// frame).
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
    let half_t = thickness as f64 / 2.0;
    let min_r = rx.min(ry);
    // Normalized-radius band where coverage > 0: |r_norm - 1| * min_r < half_t + 0.5.
    let band = (half_t + 0.5) / min_r;
    let (r_out, r_in) = (1.0 + band, (1.0 - band).max(0.0));
    let oy = ry * r_out;

    let y_lo = ((cy - oy).floor() as i32).max(0);
    let y_hi = ((cy + oy).ceil() as i32).min(h as i32 - 1);
    for y in y_lo..=y_hi {
        let py = y as f64 + 0.5;
        let ny = (py - cy) / ry;
        // Outer and inner x half-extents of the ring on this row.
        let outer2 = r_out * r_out - ny * ny;
        if outer2 <= 0.0 {
            continue;
        }
        let xo = rx * outer2.sqrt() + 1.0;
        let inner2 = r_in * r_in - ny * ny;
        let xi = if inner2 > 0.0 { (rx * inner2.sqrt() - 1.0).max(0.0) } else { 0.0 };
        let row = (y as u32 * w) as usize;
        let spans = [(cx - xo, cx - xi), (cx + xi, cx + xo)];
        for (i, (a, b)) in spans.iter().enumerate() {
            // When the row passes through the ring's top/bottom cap the two
            // spans meet; draw the merged span once.
            if i == 1 && xi == 0.0 {
                break;
            }
            let (a, b) = if xi == 0.0 { (cx - xo, cx + xo) } else { (*a, *b) };
            let x_lo = (a.floor() as i32).max(0);
            let x_hi = (b.ceil() as i32).min(w as i32 - 1);
            for x in x_lo..=x_hi {
                let px = x as f64 + 0.5;
                let dist = ellipse_outline_distance(px, py, cx, cy, rx, ry);
                let cov = (half_t + 0.5 - dist).clamp(0.0, 1.0) as f32;
                if cov > 0.0 {
                    let idx = row + x as usize;
                    if cov >= 1.0 {
                        buf[idx] = color;
                    } else {
                        blend_px(&mut buf[idx], color, cov);
                    }
                }
            }
        }
    }
}

/// Averaged ARGB (0x00RRGGBB) per mosaic block, keyed by the mosaic's
/// frame-space rect + block size. The frame is fixed for the overlay's
/// lifetime, so these averages are constant for a committed mosaic — we compute
/// them once and reuse across redraws instead of re-summing the source pixels
/// (the expensive part) every frame.
pub type MosaicCache = std::collections::HashMap<(i32, i32, i32, i32, u32), Vec<u32>>;

/// Compute the per-block averaged colors for a mosaic, in row-major block order
/// (outer `by`, inner `bx`). This is the expensive pass (reads every source
/// pixel); `paint_mosaic_averages` consumes the result.
fn compute_mosaic_averages(frame: &RgbaImage, rect_frame: Rect, block_size: u32) -> Vec<u32> {
    let (iw, ih) = {
        let (fw, fh) = frame.dimensions();
        (fw as i32, fh as i32)
    };
    let bs = block_size as i32;
    let x0 = rect_frame.x.max(0);
    let y0 = rect_frame.y.max(0);
    let x1 = (rect_frame.x + rect_frame.w).min(iw);
    let y1 = (rect_frame.y + rect_frame.h).min(ih);

    let mut out = Vec::new();
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
            // count is always ≥ 1 here (block_x1 > bx, block_y1 > by), but stay
            // defensive to match the original guard's intent.
            let avg_argb = if count == 0 {
                0
            } else {
                ((sum[0] / count) as u32) << 16
                    | ((sum[1] / count) as u32) << 8
                    | ((sum[2] / count) as u32)
            };
            out.push(avg_argb);
            bx += bs;
        }
        by += bs;
    }
    out
}

/// Paint precomputed block averages into the softbuffer. Walks the block grid
/// in the identical order `compute_mosaic_averages` produced, so `averages`
/// indexes 1:1. Cheap (fills window pixels), so it runs every frame.
fn paint_mosaic_averages(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame_size: (u32, u32),
    rect_frame: Rect,
    block_size: u32,
    averages: &[u32],
) {
    let (iw, ih) = (frame_size.0 as i32, frame_size.1 as i32);
    let bs = block_size as i32;
    let x0 = rect_frame.x.max(0);
    let y0 = rect_frame.y.max(0);
    let x1 = (rect_frame.x + rect_frame.w).min(iw);
    let y1 = (rect_frame.y + rect_frame.h).min(ih);

    let mut i = 0usize;
    let mut by = y0;
    while by < y1 {
        let mut bx = x0;
        while bx < x1 {
            let block_x1 = (bx + bs).min(x1);
            let block_y1 = (by + bs).min(y1);
            let Some(&avg_argb) = averages.get(i) else { return };
            i += 1;
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

/// `cache: Some(_)` memoizes the block averages (committed mosaics, whose rect
/// is stable); `None` recomputes fresh (an in-flight mosaic drag, whose rect
/// changes every frame — caching it would just churn stale entries).
fn apply_mosaic_on_buf(
    buf: &mut [u32],
    win_w: u32,
    win_h: u32,
    frame: &RgbaImage,
    rect_frame: Rect,
    block_size: u32,
    cache: Option<&mut MosaicCache>,
) {
    if rect_frame.w <= 0 || rect_frame.h <= 0 || block_size == 0 {
        return;
    }
    let frame_size = frame.dimensions();
    match cache {
        Some(c) => {
            let key = (
                rect_frame.x,
                rect_frame.y,
                rect_frame.w,
                rect_frame.h,
                block_size,
            );
            let averages = c
                .entry(key)
                .or_insert_with(|| compute_mosaic_averages(frame, rect_frame, block_size));
            paint_mosaic_averages(
                buf, win_w, win_h, frame_size, rect_frame, block_size, averages,
            );
        }
        None => {
            let averages = compute_mosaic_averages(frame, rect_frame, block_size);
            paint_mosaic_averages(
                buf, win_w, win_h, frame_size, rect_frame, block_size, &averages,
            );
        }
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
    fn mosaic_cache_matches_uncached_buf() {
        // The cached buf path (Some) must be bit-for-bit identical to the
        // fresh-compute path (None), including on a repeat call that hits the
        // populated cache.
        let mut frame = RgbaImage::new(40, 30);
        for y in 0..30u32 {
            for x in 0..40u32 {
                frame.put_pixel(x, y, Rgba([(x * 6) as u8, (y * 8) as u8, (x + y) as u8, 255]));
            }
        }
        let (win_w, win_h) = (40u32, 30u32);
        let rect = Rect { x: 3, y: 2, w: 25, h: 20 };
        let block = 7u32;

        let mut uncached = vec![0u32; (win_w * win_h) as usize];
        apply_mosaic_on_buf(&mut uncached, win_w, win_h, &frame, rect, block, None);

        let mut cache = MosaicCache::new();
        let mut first = vec![0u32; (win_w * win_h) as usize];
        apply_mosaic_on_buf(&mut first, win_w, win_h, &frame, rect, block, Some(&mut cache));
        assert_eq!(uncached, first, "first cached render must match uncached");
        assert_eq!(cache.len(), 1, "one mosaic key should be memoized");

        let mut second = vec![0u32; (win_w * win_h) as usize];
        apply_mosaic_on_buf(&mut second, win_w, win_h, &frame, rect, block, Some(&mut cache));
        assert_eq!(uncached, second, "cache-hit render must match uncached");
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

#[cfg(test)]
mod fast_raster_tests {
    use super::*;

    /// Brute-force reference: coverage test on every pixel of the bbox.
    fn ref_line(buf: &mut [u32], w: u32, h: u32, a: (i32, i32), b: (i32, i32), t: i32) {
        let half = t as f64 / 2.0;
        let (x0, y0, x1, y1) = (a.0 as f64, a.1 as f64, b.0 as f64, b.1 as f64);
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len2 = dx * dx + dy * dy;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                let tc = if len2 < 1e-6 { 0.0 } else { (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0) };
                let d = ((px - x0 - tc * dx).powi(2) + (py - y0 - tc * dy).powi(2)).sqrt();
                if half + 0.5 - d > 0.0 {
                    buf[(y as u32 * w + x as u32) as usize] = 1;
                }
            }
        }
    }

    #[test]
    fn fast_line_covers_exactly_the_reference_pixels() {
        let (w, h) = (120u32, 90u32);
        let cases = [
            ((10, 10), (100, 70), 6),
            ((100, 5), (8, 80), 4),
            ((20, 40), (100, 40), 5),
            ((60, 5), (60, 85), 3),
            ((50, 50), (50, 50), 8),
            ((5, 80), (115, 78), 2),
            ((30, 10), (34, 85), 7),
        ];
        for (a, b, t) in cases {
            let mut fast = vec![0u32; (w * h) as usize];
            draw_line_thick_buf(&mut fast, w, h, a, b, 0x00FF_FFFF, t);
            let mut reference = vec![0u32; (w * h) as usize];
            ref_line(&mut reference, w, h, a, b, t);
            // Every pixel the reference paints with visible coverage (> 1/64)
            // must be painted, and nothing outside the reference may be.
            let half = t as f64 / 2.0;
            for i in 0..fast.len() {
                if fast[i] != 0 {
                    assert!(reference[i] != 0, "case {a:?}->{b:?} t={t}: extra px {i}");
                }
            }
            let mut strong = vec![0u32; (w * h) as usize];
            ref_line(&mut strong, w, h, a, b, ((half - 1.0 / 64.0) * 2.0).max(0.0) as i32);
            let missing = (0..fast.len()).filter(|&i| strong[i] != 0 && fast[i] == 0).count();
            assert_eq!(missing, 0, "case {a:?}->{b:?} t={t}: {missing} missing px");
        }
    }

    #[test]
    fn fast_ellipse_has_no_gaps() {
        let (w, h) = (200u32, 140u32);
        let mut buf = vec![0u32; (w * h) as usize];
        draw_ellipse_outline_buf(&mut buf, w, h, (10, 10), (190, 130), 0x00FF_FFFF, 4);
        // Every row crossing the ellipse must have painted pixels on both sides.
        for y in 14..126u32 {
            let row = &buf[(y * w) as usize..((y + 1) * w) as usize];
            assert!(row[..100].iter().any(|&p| p != 0), "left gap at row {y}");
            assert!(row[100..].iter().any(|&p| p != 0), "right gap at row {y}");
        }
        // Center stays empty (outline, not fill).
        assert_eq!(buf[(70 * w + 100) as usize], 0);
    }
}


#[cfg(test)]
mod ellipse_width_tests {
    use super::*;

    /// Count painted pixels (cov > ~50%) along a horizontal row through the
    /// ellipse's left end (narrow side) and along a vertical column through
    /// its top (wide side). For a uniform stroke both must be ~thickness.
    fn widths(buf: &[u32], w: u32, cx: u32, cy: u32, left_x: u32, top_y: u32) -> (usize, usize) {
        let row = (cy * w) as usize;
        let horiz = (left_x.saturating_sub(20)..left_x + 40)
            .filter(|&x| (buf[row + x as usize] >> 16) & 0xFF > 0x80)
            .count();
        let vert = (top_y.saturating_sub(20)..top_y + 40)
            .filter(|&y| (buf[(y * w + cx) as usize] >> 16) & 0xFF > 0x80)
            .count();
        (horiz, vert)
    }

    #[test]
    fn wide_ellipse_stroke_is_uniform_live() {
        // 10:1 ellipse — the user-reported case.
        let (w, h) = (1100u32, 200u32);
        let mut buf = vec![0u32; (w * h) as usize];
        draw_ellipse_outline_buf(&mut buf, w, h, (50, 50), (1050, 150), 0x00FF_0000, 6);
        let (hz, vt) = widths(&buf, w, 550, 100, 50, 50);
        assert!((5..=8).contains(&hz), "end thickness {hz}px (expected ~6)");
        assert!((5..=8).contains(&vt), "side thickness {vt}px (expected ~6)");
    }

    #[test]
    fn wide_ellipse_stroke_is_uniform_export() {
        let mut img = RgbaImage::from_pixel(1100, 200, Rgba([0, 0, 0, 255]));
        let rect = Rect { x: 50, y: 50, w: 1000, h: 100 };
        draw_ellipse_outline_on_image(&mut img, rect, Rgba([255, 0, 0, 255]), 6);
        let buf: Vec<u32> = img.pixels().map(|p| (p[0] as u32) << 16).collect();
        let (hz, vt) = widths(&buf, 1100, 550, 100, 50, 50);
        assert!((5..=8).contains(&hz), "end thickness {hz}px (expected ~6)");
        assert!((5..=8).contains(&vt), "side thickness {vt}px (expected ~6)");
    }

    #[test]
    fn circle_unchanged() {
        // For a circle the new distance equals the old |r-1|*r exactly.
        let d_new = ellipse_outline_distance(130.5, 100.5, 100.0, 100.0, 50.0, 50.0);
        let r = ((30.5f64).powi(2) + 0.25).sqrt() / 50.0;
        let d_old = (r - 1.0).abs() * 50.0;
        assert!((d_new - d_old).abs() < 1e-9);
    }
}
