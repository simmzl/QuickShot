//! Rendering helpers for annotations. Two code paths:
//!   * `*_on_image`: paint into a `RgbaImage` (used by flatten_for_export).
//!   * `*_on_buf`: paint into a softbuffer `&mut [u32]` (used by live preview;
//!     implemented in Task 3).

#![allow(dead_code)] // wired in Tasks 5–7

use image::{Rgba, RgbaImage};

use super::annotate::Annotation;
use super::state::Rect;

pub const ANNOTATION_COLOR_RGBA: [u8; 4] = [0xFF, 0x3B, 0x30, 0xFF]; // #FF3B30
pub const ANNOTATION_THICKNESS: i32 = 3;
pub const ARROWHEAD_LEN: f32 = 14.0;

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
    draw_line_thick(img, from, to, color, thickness, w, h);
    draw_arrowhead(img, from, to, color, w, h);
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
    let half_w = ARROWHEAD_LEN as f64 * 0.4;
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
