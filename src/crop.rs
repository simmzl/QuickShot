use image::RgbaImage;

// Used in tests; may be called by future callers (e.g. a Linux/Windows capture path).
#[allow(dead_code)]
pub fn normalize_rect(
    start: (i32, i32),
    end: (i32, i32),
    bounds: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    let (bw, bh) = (bounds.0 as i32, bounds.1 as i32);
    let x0 = start.0.min(end.0).max(0).min(bw);
    let y0 = start.1.min(end.1).max(0).min(bh);
    let x1 = start.0.max(end.0).max(0).min(bw);
    let y1 = start.1.max(end.1).max(0).min(bh);
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;
    if w == 0 || h == 0 {
        return None;
    }
    Some((x0 as u32, y0 as u32, w, h))
}

pub fn crop_rgba(img: &RgbaImage, rect: (u32, u32, u32, u32)) -> RgbaImage {
    let (x, y, w, h) = rect;
    image::imageops::crop_imm(img, x, y, w, h).to_image()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn normalize_handles_reversed_drag() {
        // dragging right-to-left / bottom-to-top must still yield positive w/h
        let r = normalize_rect((80, 60), (20, 10), (100, 100)).unwrap();
        assert_eq!(r, (20, 10, 60, 50));
    }

    #[test]
    fn normalize_clamps_to_bounds() {
        // end point past the right/bottom edges is clipped
        let r = normalize_rect((10, 10), (1000, 1000), (100, 100)).unwrap();
        assert_eq!(r, (10, 10, 90, 90));
    }

    #[test]
    fn normalize_clamps_negative_start() {
        // negative start (e.g. mouse went above the window top) clips to 0
        let r = normalize_rect((-50, -50), (40, 40), (100, 100)).unwrap();
        assert_eq!(r, (0, 0, 40, 40));
    }

    #[test]
    fn normalize_rejects_degenerate() {
        // zero-area selection -> None
        assert!(normalize_rect((50, 50), (50, 50), (100, 100)).is_none());
    }

    #[test]
    fn crop_extracts_exact_region() {
        // build a 4x4 image; top-left 2x2 is red, rest blue
        let mut img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 255, 255]));
        for y in 0..2 {
            for x in 0..2 {
                img.put_pixel(x, y, Rgba([255, 0, 0, 255]));
            }
        }
        let cropped = crop_rgba(&img, (0, 0, 2, 2));
        assert_eq!(cropped.dimensions(), (2, 2));
        for p in cropped.pixels() {
            assert_eq!(*p, Rgba([255, 0, 0, 255]));
        }
    }
}
