use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Fullscreen, Window, WindowAttributes};

use crate::crop;

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub drag_start: Option<(i32, i32)>,
    pub drag_end: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(event_loop: &ActiveEventLoop, frame: RgbaImage) -> Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title("quickshot overlay")
            .with_decorations(false)
            .with_resizable(false)
            .with_fullscreen(Some(Fullscreen::Borderless(None)));
        let window = Rc::new(event_loop.create_window(attrs).context("create window")?);

        let context = SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            drag_start: None,
            drag_end: None,
        })
    }

    /// Current window-space rect (x, y, w, h) while dragging, if any.
    pub fn current_window_rect(&self) -> Option<(u32, u32, u32, u32)> {
        let (s, e) = (self.drag_start?, self.drag_end?);
        let size = self.window.inner_size();
        crop::normalize_rect(s, e, (size.width, size.height))
    }

    /// Translate a window-space rect into a frame-space rect.
    pub fn window_rect_to_frame_rect(
        &self,
        rect: (u32, u32, u32, u32),
    ) -> (u32, u32, u32, u32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1), size.height.max(1));
        let (fw, fh) = self.frame.dimensions();
        let (x, y, w, h) = rect;
        let fx = (x as u64 * fw as u64 / ww as u64) as u32;
        let fy = (y as u64 * fh as u64 / wh as u64) as u32;
        let fw2 = (w as u64 * fw as u64 / ww as u64) as u32;
        let fh2 = (h as u64 * fh as u64 / wh as u64) as u32;
        (fx, fy, fw2.max(1), fh2.max(1))
    }

    pub fn redraw(&mut self) -> Result<()> {
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(
                NonZeroU32::new(w).unwrap(),
                NonZeroU32::new(h).unwrap(),
            )
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel = self.current_window_rect();

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        draw_background(&mut buf, w, h, &self.frame);
        apply_dim(&mut buf, w, h, sel);
        if let Some(r) = sel {
            draw_rect_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }
}

fn draw_background(buf: &mut [u32], w: u32, h: u32, frame: &RgbaImage) {
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

fn apply_dim(buf: &mut [u32], w: u32, h: u32, inside: Option<(u32, u32, u32, u32)>) {
    for y in 0..h {
        for x in 0..w {
            let in_selection = match inside {
                Some((ix, iy, iw, ih)) => {
                    x >= ix && y >= iy && x < ix + iw && y < iy + ih
                }
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

fn draw_rect_outline(
    buf: &mut [u32],
    w: u32,
    h: u32,
    rect: (u32, u32, u32, u32),
    color: u32,
) {
    let (rx, ry, rw, rh) = rect;
    let x1 = rx;
    let y1 = ry;
    let x2 = rx + rw.saturating_sub(1);
    let y2 = ry + rh.saturating_sub(1);
    for x in x1..=x2.min(w - 1) {
        buf[(y1 * w + x) as usize] = color;
        buf[(y2.min(h - 1) * w + x) as usize] = color;
    }
    for y in y1..=y2.min(h - 1) {
        buf[(y * w + x1) as usize] = color;
        buf[(y * w + x2.min(w - 1)) as usize] = color;
    }
}
