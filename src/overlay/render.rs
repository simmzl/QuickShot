use image::RgbaImage;

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
