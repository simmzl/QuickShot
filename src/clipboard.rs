use anyhow::{Context, Result};
use arboard::{Clipboard, ImageData};
use image::RgbaImage;
use std::borrow::Cow;

pub fn put_image(img: &RgbaImage) -> Result<()> {
    let (w, h) = img.dimensions();
    let data = ImageData {
        width: w as usize,
        height: h as usize,
        bytes: Cow::Borrowed(img.as_raw()),
    };
    let mut cb = Clipboard::new().context("open clipboard")?;
    cb.set_image(data).context("write image to clipboard")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    #[ignore]
    fn roundtrip_clipboard() {
        let mut img = RgbaImage::new(16, 16);
        for (i, p) in img.pixels_mut().enumerate() {
            *p = Rgba([(i % 256) as u8, 0, 0, 255]);
        }
        put_image(&img).expect("put");

        let mut cb = arboard::Clipboard::new().unwrap();
        let got = cb.get_image().unwrap();
        assert_eq!(got.width, 16);
        assert_eq!(got.height, 16);
        assert_eq!(got.bytes.len(), 16 * 16 * 4);
    }
}
