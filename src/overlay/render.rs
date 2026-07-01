use image::RgbaImage;
use crate::overlay::hit::ANCHOR_SIZE;
use crate::overlay::state::{Anchor, Rect};

/// Copy the captured frame into the softbuffer surface, nearest-neighbor
/// scaled to the window's pixel dimensions. Softbuffer's pixel format is
/// 0x00RRGGBB (u32 per pixel).
///
/// Hot path: called every redraw. On 4K Retina (~8M pixels) the inner loop
/// dominates the drag-redraw budget, so we lean on slice iteration to let
/// LLVM auto-vectorize the channel swizzle.
pub fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
    let (fw, fh) = frame.dimensions();
    let raw = frame.as_raw();

    if fw == w && fh == h {
        // Fast path: 1:1 pixel mapping (the common case — the window is
        // sized to match the captured frame). `iter_mut().zip(chunks_exact(4))`
        // gives the optimizer a predictable, bounds-check-free stride.
        for (dst, src) in buf.iter_mut().zip(raw.chunks_exact(4)) {
            *dst = ((src[0] as u32) << 16)
                 | ((src[1] as u32) << 8)
                 | (src[2] as u32);
        }
    } else {
        // Scaling path (rare): hoist the row index out of the inner loop
        // and avoid `get_pixel` overhead by indexing the raw buffer.
        let fw_u64 = fw as u64;
        let fh_u64 = fh as u64;
        let w_u64 = w as u64;
        let h_u64 = h as u64;
        for y in 0..h {
            let src_y = ((y as u64 * fh_u64 / h_u64) as u32).min(fh - 1);
            let src_row_start = (src_y as usize) * (fw as usize) * 4;
            let dst_row_start = (y as usize) * (w as usize);
            for x in 0..w {
                let src_x = ((x as u64 * fw_u64 / w_u64) as u32).min(fw - 1) as usize;
                let i = src_row_start + src_x * 4;
                let r = raw[i] as u32;
                let g = raw[i + 1] as u32;
                let b = raw[i + 2] as u32;
                buf[dst_row_start + x as usize] = (r << 16) | (g << 8) | b;
            }
        }
    }
}

/// Precompute the fully-dimmed background once. The overlay's frame never
/// changes for its lifetime, so we build the swizzled + halved (0x00RRGGBB,
/// each channel `/ 2`) buffer a single time at `Overlay::create` and memcpy it
/// every redraw instead of re-deriving it from the RGBA frame per frame.
///
/// The values are bit-for-bit identical to running `draw_background` followed
/// by `apply_dim(.., None)`: swizzle drops each channel to 8 bits, then the
/// `>> 1` halving matches the old `((px >> shift) & 0xFF) / 2`.
pub fn precompute_dimmed(frame: &RgbaImage) -> Vec<u32> {
    let raw = frame.as_raw();
    let (fw, fh) = frame.dimensions();
    let mut out = vec![0u32; (fw as usize) * (fh as usize)];
    for (dst, src) in out.iter_mut().zip(raw.chunks_exact(4)) {
        let r = (src[0] >> 1) as u32;
        let g = (src[1] >> 1) as u32;
        let b = (src[2] >> 1) as u32;
        *dst = (r << 16) | (g << 8) | b;
    }
    out
}

/// Hot path: paint the dimmed background with the selection region restored to
/// full brightness. Replaces the per-frame `draw_background` + `apply_dim` pair
/// with a single `memcpy` of the precomputed `dimmed` buffer plus a bright
/// re-swizzle of only the selection rect (typically far smaller than the whole
/// screen). Behavior is identical: everything dimmed, selection interior bright.
///
/// Falls back to the original two-pass path when the window doesn't match the
/// frame's pixel dimensions (the rare DPI-scaling case), where the flat memcpy
/// wouldn't align.
pub fn blit_background_dimmed(
    buf: &mut [u32],
    w: u32,
    h: u32,
    frame: &RgbaImage,
    dimmed: &[u32],
    inside: Option<(u32, u32, u32, u32)>,
) {
    let (fw, fh) = frame.dimensions();
    if fw != w || fh != h || dimmed.len() != buf.len() {
        // Scaling / mismatch fallback: exact original behavior.
        draw_background(buf, w, h, frame);
        apply_dim(buf, w, h, inside);
        return;
    }

    // Fast path: 1:1 mapping (the common case). One vectorizable memcpy…
    buf.copy_from_slice(dimmed);

    // …then re-brighten just the selection interior straight from the frame.
    if let Some((ix, iy, iw, ih)) = inside {
        if iw == 0 || ih == 0 {
            return;
        }
        let raw = frame.as_raw();
        let w_us = w as usize;
        let fw_us = fw as usize;
        let ix_us = ix.min(w) as usize;
        let ix2_us = ix.saturating_add(iw).min(w) as usize;
        let iy2 = iy.saturating_add(ih).min(h);
        for y in iy..iy2 {
            let y_us = y as usize;
            let dst = &mut buf[y_us * w_us + ix_us..y_us * w_us + ix2_us];
            let src_row = y_us * fw_us * 4;
            for (dx, px) in dst.iter_mut().enumerate() {
                let i = src_row + (ix_us + dx) * 4;
                *px = ((raw[i] as u32) << 16)
                    | ((raw[i + 1] as u32) << 8)
                    | (raw[i + 2] as u32);
            }
        }
    }
}

/// Darken everything in `buf` except the optional `inside` rect (x, y, w, h).
///
/// Hot path: called every redraw. Operate on row slices instead of 2D index
/// pairs so the inner loop is a tight `&mut [u32]` walk LLVM can vectorize.
/// Behavior preserved exactly: each non-selection pixel's R/G/B channels are
/// halved (matches the previous `/ 2` arithmetic — bit-for-bit identical).
pub fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    #[inline(always)]
    fn dim_slice(row: &mut [u32]) {
        for px in row.iter_mut() {
            let r = ((*px >> 16) & 0xFF) / 2;
            let g = ((*px >> 8) & 0xFF) / 2;
            let b = (*px & 0xFF) / 2;
            *px = (r << 16) | (g << 8) | b;
        }
    }

    let w_us = w as usize;

    match inside {
        None => {
            // No selection: dim the whole buffer in one straight walk.
            dim_slice(buf);
        }
        Some((ix, iy, iw, ih)) => {
            let iy2 = iy.saturating_add(ih);
            let ix2 = ix.saturating_add(iw).min(w);
            let ix_clamped = ix.min(w);

            for y in 0..h {
                let row_start = (y as usize) * w_us;
                let row_end = row_start + w_us;
                let row = &mut buf[row_start..row_end];

                if y >= iy && y < iy2 && iw > 0 && ih > 0 {
                    // Row crosses the inside rect: dim the strips on either side,
                    // leave the inside untouched.
                    let left = ix_clamped as usize;
                    let right = ix2 as usize;
                    dim_slice(&mut row[..left]);
                    dim_slice(&mut row[right..]);
                } else {
                    // Entire row is outside the selection.
                    dim_slice(row);
                }
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
/// `scale` is the window's DPI scale factor (1.0 on non-Retina, 2.0 on Retina).
#[allow(clippy::too_many_arguments)]
pub fn draw_size_label(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: Rect,
    frame_size: (u32, u32),
    window_size: (u32, u32),
    font: &mut Font,
    scale: f32,
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

    let font_px = LABEL_FONT_PX * scale;
    let pad_x = (LABEL_PAD_X as f32 * scale) as i32;
    let pad_y = (LABEL_PAD_Y as f32 * scale) as i32;
    let gap = (LABEL_GAP as f32 * scale) as i32;
    let corner_radius = (LABEL_CORNER_RADIUS as f32 * scale) as i32;

    let text_w = estimate_text_width(&text, font_px);
    let text_h = font_px as i32; // approximate cap height
    let box_w = text_w + pad_x * 2;
    let box_h = text_h + pad_y * 2;

    let (bx, by, _placement) = size_label_position(rect, (box_w, box_h), gap);
    draw_rounded_rect_alpha(buf, w, h, bx, by, box_w, box_h, corner_radius, 0x000000, 0.7);
    font.render_text(
        buf,
        w,
        h,
        bx + pad_x,
        by + pad_y,
        &text,
        font_px,
        0x00FFFFFF,
    );
}

/// Rough width estimate for monospace: each glyph is ~`0.6 * px_size` wide.
/// Used only for laying out the background pill; the text renderer handles
/// real advance widths.
/// TODO Iter 2b: the 0.6 multiplier has no safety margin for wide strings;
/// raise to ~0.65 or switch to a real advance-width query via fontdue metrics
/// if labels visibly overhang.
fn estimate_text_width(text: &str, px_size: f32) -> i32 {
    (text.chars().count() as f32 * px_size * 0.6).ceil() as i32
}

/// Filled rect with alpha-blended solid color and squared corners masked into
/// a 4-px rounded pill.
// All args are primitives for a pixel-blending inner loop; no shared struct
// type exists that would aid clarity over just listing the parameters.
#[allow(clippy::too_many_arguments)]
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
const MAG_LABEL_H: i32 = 24;
const MAG_FONT_PX: f32 = 14.0;

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

/// `scale` is the window's DPI scale factor (1.0 on non-Retina, 2.0 on Retina).
/// All logical-pixel constants (MAG_SIZE, MAG_OFFSET, MAG_LABEL_H, MAG_FONT_PX)
/// are multiplied by scale to get physical pixels. MAG_ZOOM is kept as-is
/// (it's a zoom ratio, not a size).
#[allow(clippy::too_many_arguments)]
pub fn draw_magnifier(
    buf: &mut [u32],
    w: u32,
    h: u32,
    frame: &image::RgbaImage,
    cursor: (i32, i32),
    window_size: (u32, u32),
    font: &mut Font,
    scale: f32,
) {
    let mag_size = (MAG_SIZE as f32 * scale) as i32;
    let offset = (MAG_OFFSET as f32 * scale) as i32;
    let mag_label_h = (MAG_LABEL_H as f32 * scale) as i32;
    let font_px = MAG_FONT_PX * scale;

    let (mx, my) = magnifier_position(cursor, window_size, mag_size, offset);
    let (fw, fh) = frame.dimensions();
    let (ww, wh) = (window_size.0.max(1) as u64, window_size.1.max(1) as u64);

    // Map cursor (window-space) to physical pixel coords in the frame.
    let cfx = (cursor.0.max(0) as u64 * fw as u64 / ww) as i32;
    let cfy = (cursor.1.max(0) as u64 * fh as u64 / wh) as i32;
    let src_span = mag_size / MAG_ZOOM;

    // 1 px white border + black backfill, then upscaled pixels, then crosshair, then label.
    fill_square(buf, w, h, mx - 1, my - 1, mag_size + 2, 0x00FFFFFF);
    fill_square(buf, w, h, mx,     my,     mag_size,     0x00000000);

    for dy in 0..(mag_size - mag_label_h) {
        for dx in 0..mag_size {
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
    let cx = mx + mag_size / 2;
    let cy = my + (src_span / 2) * MAG_ZOOM;
    for dx in 0..mag_size {
        let px = mx + dx;
        if px < 0 || cy < 0 || px >= w as i32 || cy >= h as i32 { continue; }
        buf[(cy as u32 * w + px as u32) as usize] = 0x0000FFFF; // cyan
    }
    for dy in 0..(mag_size - mag_label_h) {
        let py = my + dy;
        if cx < 0 || py < 0 || cx >= w as i32 || py >= h as i32 { continue; }
        buf[(py as u32 * w + cx as u32) as usize] = 0x0000FFFF;
    }

    // Label strip at the bottom (black, 70% opaque per spec).
    let label_y = my + mag_size - mag_label_h;
    draw_rounded_rect_alpha(buf, w, h, mx, label_y, mag_size, mag_label_h, 0, 0x000000, 0.7);

    let cfx_clamped = cfx.clamp(0, fw as i32 - 1) as u32;
    let cfy_clamped = cfy.clamp(0, fh as i32 - 1) as u32;
    let center = frame.get_pixel(cfx_clamped, cfy_clamped);
    let [r, g, b, _a] = center.0;
    let text = format!(
        "#{:02X}{:02X}{:02X} {}px, {}px",
        r, g, b, cfx_clamped, cfy_clamped
    );
    let label_pad = (4.0 * scale) as i32;
    let label_text_offset = (2.0 * scale) as i32;
    font.render_text(buf, w, h, mx + label_pad, label_y + label_text_offset, &text, font_px, 0x00FFFFFF);
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
mod background_tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn gradient_frame(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(
                    x,
                    y,
                    Rgba([
                        (x * 7 + 3) as u8,
                        (y * 5 + 11) as u8,
                        (x + y) as u8,
                        255,
                    ]),
                );
            }
        }
        img
    }

    /// The precompute + blit fast path must be bit-for-bit identical to the
    /// original `draw_background` + `apply_dim` two-pass path.
    fn assert_equiv(frame: &RgbaImage, inside: Option<(u32, u32, u32, u32)>) {
        let (w, h) = frame.dimensions();
        let n = (w * h) as usize;

        let mut old = vec![0u32; n];
        draw_background(&mut old, w, h, frame);
        apply_dim(&mut old, w, h, inside);

        let dimmed = precompute_dimmed(frame);
        let mut new = vec![0u32; n];
        blit_background_dimmed(&mut new, w, h, frame, &dimmed, inside);

        assert_eq!(old, new, "inside={inside:?}");
    }

    #[test]
    fn blit_matches_two_pass_no_selection() {
        assert_equiv(&gradient_frame(37, 29), None);
    }

    #[test]
    fn blit_matches_two_pass_with_selection() {
        let frame = gradient_frame(64, 48);
        assert_equiv(&frame, Some((10, 8, 20, 15)));
    }

    #[test]
    fn blit_matches_two_pass_selection_clamped_past_edge() {
        // Selection running off the right/bottom edge exercises the min-clamps.
        let frame = gradient_frame(40, 40);
        assert_equiv(&frame, Some((30, 30, 50, 50)));
    }

    #[test]
    fn blit_matches_two_pass_zero_size_selection() {
        assert_equiv(&gradient_frame(24, 24), Some((5, 5, 0, 0)));
    }

    #[test]
    fn blit_falls_back_on_size_mismatch() {
        // dimmed cache built for the frame; buffer sized to a *different* WxH
        // forces the scaling fallback, which must equal draw_background+apply_dim
        // at that target size.
        let frame = gradient_frame(32, 32);
        let dimmed = precompute_dimmed(&frame);
        let (w, h) = (20u32, 16u32);
        let inside = Some((2, 2, 6, 6));

        let mut expected = vec![0u32; (w * h) as usize];
        draw_background(&mut expected, w, h, &frame);
        apply_dim(&mut expected, w, h, inside);

        let mut got = vec![0u32; (w * h) as usize];
        blit_background_dimmed(&mut got, w, h, &frame, &dimmed, inside);

        assert_eq!(expected, got);
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
