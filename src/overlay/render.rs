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
#[cfg_attr(target_os = "macos", allow(dead_code))] // macOS fuses this into capture
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

    // Fast path: 1:1 mapping (the common case). Per row: memcpy the dimmed
    // background, then re-brighten just the selection interior straight from
    // the frame. Rows are split across a few threads — a full Retina frame
    // is ~80 MB of traffic, which one core can't move within a 120 Hz budget.
    let raw = frame.as_raw();
    let w_us = w as usize;
    let sel = inside.filter(|&(_, _, iw, ih)| iw > 0 && ih > 0).map(|(ix, iy, iw, ih)| {
        (
            ix.min(w) as usize,
            ix.saturating_add(iw).min(w) as usize,
            iy as usize,
            iy.saturating_add(ih).min(h) as usize,
        )
    });
    let paint_rows = |rows: &mut [u32], y0: usize| {
        for (r, dst) in rows.chunks_exact_mut(w_us).enumerate() {
            let y = y0 + r;
            dst.copy_from_slice(&dimmed[y * w_us..(y + 1) * w_us]);
            if let Some((x0, x1, sy0, sy1)) = sel {
                if y >= sy0 && y < sy1 {
                    let src = &raw[(y * w_us + x0) * 4..(y * w_us + x1) * 4];
                    for (px, s) in dst[x0..x1].iter_mut().zip(src.chunks_exact(4)) {
                        *px = ((s[0] as u32) << 16) | ((s[1] as u32) << 8) | (s[2] as u32);
                    }
                }
            }
        }
    };
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4);
    let rows_per = (h as usize).div_ceil(threads).max(1);
    if threads == 1 || (w as usize * h as usize) < 1 << 20 {
        paint_rows(buf, 0);
        return;
    }
    std::thread::scope(|s| {
        for (i, chunk) in buf.chunks_mut(rows_per * w_us).enumerate() {
            let paint_rows = &paint_rows;
            s.spawn(move || paint_rows(chunk, i * rows_per));
        }
    });
}

/// `dst.copy_from_slice(src)` split across a few threads (a Retina frame is
/// ~80 MB; one core's memcpy bandwidth alone eats most of a frame budget).
pub fn par_copy(dst: &mut [u32], src: &[u32]) {
    assert_eq!(dst.len(), src.len());
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, 4);
    if threads == 1 || dst.len() < 1 << 20 {
        dst.copy_from_slice(src);
        return;
    }
    let chunk = dst.len().div_ceil(threads);
    std::thread::scope(|s| {
        for (d, sr) in dst.chunks_mut(chunk).zip(src.chunks(chunk)) {
            s.spawn(move || d.copy_from_slice(sr));
        }
    });
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

#[cfg(windows)]
pub fn draw_snap_mode(
    buf: &mut [u32], w: u32, h: u32, font: &mut Font, scale: f32,
    element_mode: bool, cursor: (i32, i32),
) {
    let label = if element_mode { "Element  |  Alt: Window" } else { "Window  |  Alt: Element" };
    let font_px = LABEL_FONT_PX * scale;
    let pad = (6.0 * scale).round() as i32;
    let width = font.measure_label(label, font_px).ceil() as i32 + 2 * pad;
    let height = font_px.ceil() as i32 + 2 * pad;
    let bottom = h as i32 - height - pad;
    let y = if cursor.0 < width + 2 * pad && cursor.1 >= bottom - pad { pad } else { bottom };
    draw_rounded_rect_alpha(buf, w, h, pad, y, width, height, pad, 0, 0.78);
    font.render_text(buf, w, h, 2 * pad, y + pad, label, font_px, 0x00ffffff);
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

// ---- magnifier (all in points; multiplied by the display scale) ----------
/// Zoomed-pixel grid: MAG_CELLS × MAG_CELLS source pixels, each MAG_CELL pt.
const MAG_CELLS: i32 = 17; // odd → a true center pixel
const MAG_CELL: i32 = 10;
const MAG_PAD: i32 = 6;
const MAG_INFO_H: i32 = 50;
const MAG_OFFSET: i32 = 22;
const MAG_RADIUS: f32 = 12.0;
const MAG_FONT_PX: f32 = 12.0;
const MAG_FONT_SMALL_PX: f32 = 10.5;
/// How long the "Copied" confirmation replaces the shortcut hint.
pub const COPIED_BADGE_DURATION: std::time::Duration = std::time::Duration::from_millis(1200);


/// Decide where to put the magnifier given cursor + window size.
/// Default: bottom-right of cursor with a gap. Flip to the opposite side
/// on each axis when the default would clip the window edge.
#[cfg(test)]
pub fn magnifier_position(
    cursor: (i32, i32),
    window_size: (u32, u32),
    mag_size: i32,
    offset: i32,
) -> (i32, i32) {
    magnifier_position_wh(cursor, window_size, (mag_size, mag_size), offset)
}

fn magnifier_position_wh(
    cursor: (i32, i32),
    window_size: (u32, u32),
    size: (i32, i32),
    offset: i32,
) -> (i32, i32) {
    let (ww, wh) = (window_size.0 as i32, window_size.1 as i32);
    let mut x = cursor.0 + offset;
    let mut y = cursor.1 + offset;
    if x + size.0 >= ww {
        x = cursor.0 - offset - size.0;
    }
    if y + size.1 >= wh {
        y = cursor.1 - offset - size.1;
    }
    (x.max(0), y.max(0))
}

/// Perceived luminance (0..255) of an 0x00RRGGBB color.
fn luma(rgb: u32) -> u32 {
    let (r, g, b) = ((rgb >> 16) & 0xFF, (rgb >> 8) & 0xFF, rgb & 0xFF);
    (r * 299 + g * 587 + b * 114) / 1000
}

/// Loupe shown while choosing a region: a rounded card with a crisp pixel
/// grid zoom, a highlighted center pixel, a color chip + HEX value and the
/// cursor coordinates. `scale` is the display backing scale.
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
    copied: bool,
) {
    let s = scale.max(0.5);
    let px = |v: f32| (v * s).round() as i32;
    let cell = px(MAG_CELL as f32).max(1);
    let zoom = cell * MAG_CELLS; // physical zoom-area size (exact multiple of cell)
    let pad = px(MAG_PAD as f32);
    let info_h = px(MAG_INFO_H as f32);
    let (cw, ch) = (zoom + 2 * pad, zoom + 2 * pad + info_h);
    let offset = px(MAG_OFFSET as f32);
    let (mx, my) = magnifier_position_wh(cursor, window_size, (cw, ch), offset);

    let (fw, fh) = frame.dimensions();
    let (ww, wh) = (window_size.0.max(1) as u64, window_size.1.max(1) as u64);
    let cfx = ((cursor.0.max(0) as u64 * fw as u64 / ww) as i32).clamp(0, fw as i32 - 1);
    let cfy = ((cursor.1.max(0) as u64 * fh as u64 / wh) as i32).clamp(0, fh as i32 - 1);

    let mut c = crate::overlay::toolbar::Canvas { buf, w, h };
    // Card: shadow + translucent dark surface + hairline border.
    c.surface((mx, my), (cw, ch), MAG_RADIUS * s, s);

    // ---- zoomed pixels (nearest neighbour, whole-cell blocks → crisp)
    let (zx, zy) = (mx + pad, my + pad);
    let half = MAG_CELLS / 2;
    let raw = frame.as_raw();
    let zr = (MAG_RADIUS - 4.0).max(2.0) * s; // inner corner radius
    for gy in 0..MAG_CELLS {
        let sy = (cfy - half + gy).clamp(0, fh as i32 - 1) as usize;
        for gx in 0..MAG_CELLS {
            let sx = (cfx - half + gx).clamp(0, fw as i32 - 1) as usize;
            let i = (sy * fw as usize + sx) * 4;
            let rgb = ((raw[i] as u32) << 16) | ((raw[i + 1] as u32) << 8) | raw[i + 2] as u32;
            let (bx, by) = (zx + gx * cell, zy + gy * cell);
            for yy in by..by + cell {
                if yy < 0 || yy >= h as i32 {
                    continue;
                }
                let row = (yy as u32 * w) as usize;
                for xx in bx..bx + cell {
                    if xx < 0 || xx >= w as i32 {
                        continue;
                    }
                    // Round the zoom area's corners so it nests in the card.
                    let corner = rrect_cov(xx as f32 + 0.5, yy as f32 + 0.5, zx as f32, zy as f32, zoom as f32, zoom as f32, zr);
                    if corner >= 1.0 {
                        c.buf[row + xx as usize] = rgb;
                    } else if corner > 0.0 {
                        c.blend(xx, yy, rgb, corner);
                    }
                }
            }
        }
    }
    // Subtle pixel grid (only when cells are big enough to read).
    if cell >= 6 {
        for k in 1..MAG_CELLS {
            let gx = zx + k * cell;
            let gy = zy + k * cell;
            for t in 0..zoom {
                c.blend(gx, zy + t, 0x000000, 0.10);
                c.blend(zx + t, gy, 0x000000, 0.10);
            }
        }
    }

    // ---- center pixel: contrasting double frame (dark + light) so it reads
    // on any content.
    let center = {
        let i = (cfy as usize * fw as usize + cfx as usize) * 4;
        ((raw[i] as u32) << 16) | ((raw[i + 1] as u32) << 8) | raw[i + 2] as u32
    };
    let (ccx, ccy) = (zx + half * cell, zy + half * cell);
    let ring = s.max(1.0).round() as i32;
    frame_rect(&mut c, ccx - ring, ccy - ring, cell + 2 * ring, cell + 2 * ring, ring, 0x000000, 0.85);
    frame_rect(&mut c, ccx - 2 * ring, ccy - 2 * ring, cell + 4 * ring, cell + 4 * ring, ring, 0xFFFFFF, 0.95);

    // ---- info rows
    //   [chip] #RRGGBB           R G B      ← row 1 centerline
    //   x, y                [C] copy        ← row 2 centerline
    // Every element on a row is vertically centered on that row's
    // centerline: boxes by their geometry, text by its cap height (digits
    // and capitals), so chips / pills / glyphs line up exactly.
    let fpx = MAG_FONT_PX * s;
    let small = MAG_FONT_SMALL_PX * s;
    let left = mx + pad + px(4.0);
    let right = mx + cw - pad - px(4.0);
    let info_top = (my + pad + zoom) as f32;
    let row1_c = info_top + 15.0 * s;
    let row2_c = info_top + 35.0 * s;
    let gap = px(8.0);

    // Row 1: chip + HEX on the left, RGB on the right.
    let chip = px(14.0) as f32;
    let chip_y = (row1_c - chip / 2.0).round();
    c.fill_rrect(left as f32, chip_y, chip, chip, 3.5 * s, center, 1.0);
    // Hairline around the chip so very dark / light colors stay visible.
    let edge = if luma(center) < 60 { 0xFFFFFF } else { 0x000000 };
    frame_rrect(&mut c, left as f32, chip_y, chip, chip, 3.5 * s, s, edge, 0.35);

    let (r, g, b) = ((center >> 16) & 0xFF, (center >> 8) & 0xFF, center & 0xFF);
    let hex = format!("#{r:02X}{g:02X}{b:02X}");
    let hex_x = left + chip as i32 + px(7.0);
    let hex_w = font.measure_label(&hex, fpx).ceil() as i32;
    let y1 = font.label_y_centered(row1_c, fpx);
    font.render_text(c.buf, w, h, hex_x, y1, &hex, fpx, 0x00F2F2F7);
    // RGB as three right-aligned 3-digit columns ("255 200 180"), sized to
    // whatever room is left after the HEX, so the value never overlaps it
    // and digits don't jitter as the color changes. If the card is ever too
    // narrow the font shrinks to fit instead of colliding.
    let rgb_txt = format!("{r:>3} {g:>3} {b:>3}");
    let (rgb_x, rgb_px) = fit_right(font, &rgb_txt, small, hex_x + hex_w + gap, right);
    let y1s = font.label_y_centered(row1_c, rgb_px);
    font.render_text(c.buf, w, h, rgb_x, y1s, &rgb_txt, rgb_px, 0x008E8E93);

    // Row 2: coordinates on the left, shortcut hint / confirmation right.
    let y2s = font.label_y_centered(row2_c, small);
    let coords = format!("{cfx}, {cfy}");
    font.render_text(c.buf, w, h, left, y2s, &coords, small, 0x008E8E93);
    let pill_h = (16.0 * s).round();
    let pill_y = (row2_c - pill_h / 2.0).round();
    if copied {
        // Green confirmation pill replaces the hint briefly.
        let label = "Copied";
        let label_w = font.measure_label(label, small);
        let pw = (label_w + 14.0 * s).round();
        let pxx = right as f32 - pw;
        c.fill_rrect(pxx, pill_y, pw, pill_h, pill_h / 2.0, 0x0034C759, 0.22);
        let tx = (pxx + (pw - label_w) / 2.0).round() as i32;
        font.render_text(c.buf, w, h, tx, y2s, label, small, 0x0030D158);
    } else {
        // Shortcut hint as a small key-cap: [C] copy
        let label = "copy";
        let label_w = font.measure_label(label, small).ceil() as i32;
        let cap = pill_h;
        let cap_x = (right - label_w - px(5.0)) as f32 - cap;
        c.fill_rrect(cap_x, pill_y, cap, cap, 3.5 * s, 0xFFFFFF, 0.14);
        let cw_c = font.measure_label("C", small);
        let tx = (cap_x + (cap - cw_c) / 2.0).round() as i32;
        font.render_text(c.buf, w, h, tx, y2s, "C", small, 0x00D1D1D6);
        font.render_text(c.buf, w, h, right - label_w, y2s, label, small, 0x00636366);
    }
}

/// Right-align `text` to `right`, never starting before `min_x`: shrinks the
/// font if the natural width doesn't fit. Returns (x, px_size).
fn fit_right(font: &mut Font, text: &str, px_size: f32, min_x: i32, right: i32) -> (i32, f32) {
    let avail = (right - min_x).max(1) as f32;
    let mut size = px_size;
    let mut width = font.measure_label(text, size);
    if width > avail {
        size *= avail / width;
        width = font.measure_label(text, size);
    }
    (right - width.ceil() as i32, size)
}

/// Coverage (0..1) of a pixel center inside a rounded rect.
fn rrect_cov(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let r = r.min(hw).min(hh);
    let qx = (px - (x + hw)).abs() - (hw - r);
    let qy = (py - (y + hh)).abs() - (hh - r);
    let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
    (0.5 - d).clamp(0.0, 1.0)
}

/// Axis-aligned rectangular frame of `t` px thickness.
#[allow(clippy::too_many_arguments)]
fn frame_rect(c: &mut crate::overlay::toolbar::Canvas, x: i32, y: i32, w: i32, h: i32, t: i32, color: u32, alpha: f32) {
    for yy in y..y + h {
        for xx in x..x + w {
            let inside = xx >= x + t && xx < x + w - t && yy >= y + t && yy < y + h - t;
            if !inside {
                c.blend(xx, yy, color, alpha);
            }
        }
    }
}

/// Rounded-rect outline of width `t`.
#[allow(clippy::too_many_arguments)]
fn frame_rrect(c: &mut crate::overlay::toolbar::Canvas, x: f32, y: f32, w: f32, h: f32, r: f32, t: f32, color: u32, alpha: f32) {
    let (x0, y0) = ((x - 1.0).floor() as i32, (y - 1.0).floor() as i32);
    let (x1, y1) = ((x + w + 1.0).ceil() as i32, (y + h + 1.0).ceil() as i32);
    for yy in y0..=y1 {
        for xx in x0..=x1 {
            let (pxf, pyf) = (xx as f32 + 0.5, yy as f32 + 0.5);
            let outer = rrect_cov(pxf, pyf, x, y, w, h, r);
            let inner = rrect_cov(pxf, pyf, x + t, y + t, w - 2.0 * t, h - 2.0 * t, (r - t).max(0.0));
            let a = (outer - inner).max(0.0);
            if a > 0.0 {
                c.blend(xx, yy, color, a * alpha);
            }
        }
    }
}

/// Hex color under the cursor, `#RRGGBB` (for the C-to-copy shortcut).
pub fn color_hex_at(frame: &image::RgbaImage, cursor: (i32, i32), window_size: (u32, u32)) -> String {
    let (fw, fh) = frame.dimensions();
    let (ww, wh) = (window_size.0.max(1) as u64, window_size.1.max(1) as u64);
    let x = ((cursor.0.max(0) as u64 * fw as u64 / ww) as u32).min(fw - 1);
    let y = ((cursor.1.max(0) as u64 * fh as u64 / wh) as u32).min(fh - 1);
    let p = frame.get_pixel(x, y).0;
    format!("#{:02X}{:02X}{:02X}", p[0], p[1], p[2])
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
    fn magnifier_card_flips_near_corner_and_stays_onscreen() {
        let size = (200, 260);
        let (x, y) = magnifier_position_wh((790, 590), (800, 600), size, 20);
        assert_eq!((x, y), (790 - 20 - 200, 590 - 20 - 260));
    }

    #[test]
    fn color_hex_at_maps_window_to_frame() {
        let mut frame = image::RgbaImage::new(4, 4);
        frame.put_pixel(2, 2, image::Rgba([0xAB, 0xCD, 0xEF, 255]));
        // Window is half the frame's resolution.
        assert_eq!(color_hex_at(&frame, (1, 1), (2, 2)), "#ABCDEF");
    }

    #[test]
    fn magnifier_draws_center_color_chip() {
        let (w, h) = (600u32, 600u32);
        let frame = image::RgbaImage::from_pixel(w, h, image::Rgba([10, 200, 30, 255]));
        let mut buf = vec![0u32; (w * h) as usize];
        let mut f = Font::embedded();
        draw_magnifier(&mut buf, w, h, &frame, (100, 100), (w, h), &mut f, 1.0, false);
        // Zoom area is filled with the (uniform) source color.
        // Sample the middle of cell (2, 2) — away from grid lines.
        let zx = (100 + MAG_OFFSET + MAG_PAD + 2 * MAG_CELL + MAG_CELL / 2) as usize;
        let zy = (100 + MAG_OFFSET + MAG_PAD + 2 * MAG_CELL + MAG_CELL / 2) as usize;
        assert_eq!(buf[zy * w as usize + zx], 0x000A_C81E);
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


#[cfg(test)]
mod mag_layout_tests {
    use super::*;

    #[test]
    fn rgb_column_never_overlaps_hex_at_any_scale() {
        let mut f = Font::embedded();
        for scale in [1.0f32, 1.5, 2.0, 3.0] {
            let s = scale;
            let left = 0;
            let right = ((MAG_CELLS * MAG_CELL) as f32 * s).round() as i32 - (8.0 * s).round() as i32;
            let fpx = MAG_FONT_PX * s;
            let hex_x = left + (14.0 * s).round() as i32 + (7.0 * s).round() as i32;
            let hex_w = f.measure_label("#FFC8B4", fpx).ceil() as i32;
            let gap = (8.0 * s).round() as i32;
            let (x, size) = fit_right(&mut f, "255 200 180", MAG_FONT_SMALL_PX * s, hex_x + hex_w + gap, right);
            // ≥ 1 px of clear space after the HEX glyphs, plus most of the gap
            // (right-alignment may consume the sub-pixel remainder).
            assert!(x >= hex_x + hex_w + gap - 1, "scale {s}: rgb x {x} too close to hex end {}", hex_x + hex_w);
            // At standard sizes the full-size font fits (no shrinking).
            assert!((size - MAG_FONT_SMALL_PX * s).abs() < 1e-3, "scale {s}: font shrank to {size}");
        }
        // A deliberately too-narrow box shrinks instead of overlapping.
        let (x, size) = fit_right(&mut f, "255 200 180", 21.0, 100, 160);
        assert!(x >= 100 && size < 21.0);
    }

    /// Vertical ink extent [top, bottom] of pixels matching `pred` within
    /// columns [x0, x1).
    fn ink_rows(buf: &[u32], w: u32, x0: i32, x1: i32, y0: i32, y1: i32, pred: impl Fn(u32) -> bool) -> Option<(i32, i32)> {
        let rows: Vec<i32> = (y0..y1)
            .filter(|&y| (x0..x1).any(|x| pred(buf[(y as u32 * w + x as u32) as usize])))
            .collect();
        Some((*rows.first()?, *rows.last()?))
    }

    #[test]
    fn info_rows_are_vertically_centered_and_rgb_never_overlaps_hex() {
        let (w, h) = (900u32, 900u32);
        // Worst case: all channels three digits.
        let frame = image::RgbaImage::from_pixel(w, h, image::Rgba([255, 200, 180, 255]));
        let mut f = Font::embedded();
        for copied in [false, true] {
            let mut buf = vec![0u32; (w * h) as usize];
            draw_magnifier(&mut buf, w, h, &frame, (100, 100), (w, h), &mut f, 2.0, copied);
            // Card geometry (mirrors draw_magnifier at scale 2).
            let s = 2;
            let cell = MAG_CELL * s;
            let zoom = cell * MAG_CELLS;
            let pad = MAG_PAD * s;
            let mx = 100 + MAG_OFFSET * s;
            let my = 100 + MAG_OFFSET * s;
            let info_top = my + pad + zoom;
            let row1_c = info_top as f32 + 15.0 * s as f32;
            let left = mx + pad + 4 * s;
            let right = mx + zoom + 2 * pad - pad - 4 * s;

            // Chip (salmon) and HEX text (near-white) share the row-1 centerline.
            let chip = ink_rows(&buf, w, left, left + 10, info_top, info_top + 30 * s, |p| p == 0x00FF_C8B4).unwrap();
            let hex = ink_rows(&buf, w, left + 21 * s, left + 21 * s + 100, info_top, info_top + 30 * s, |p| ((p >> 16) & 0xFF) > 200 && (p & 0xFF) > 200).unwrap();
            let c_chip = (chip.0 + chip.1) as f32 / 2.0;
            let c_hex = (hex.0 + hex.1) as f32 / 2.0;
            assert!((c_chip - row1_c).abs() <= 1.5, "chip center {c_chip} vs row {row1_c}");
            assert!((c_hex - row1_c).abs() <= 1.5, "hex center {c_hex} vs row {row1_c}");

            let _ = right;

        }
    }
}
