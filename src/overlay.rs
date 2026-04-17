use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::MonitorGeom;
use crate::crop;

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub drag_start: Option<(i32, i32)>,
    pub drag_end: Option<(i32, i32)>,
}

impl Overlay {
    pub fn create(
        event_loop: &ActiveEventLoop,
        frame: RgbaImage,
        monitor_geom: &MonitorGeom,
    ) -> Result<Self> {
        #[cfg(target_os = "macos")]
        let window = {
            // On macOS: create a borderless window, then use raw NSWindow API
            // to position it on the target monitor with a level above the dock/menu bar.
            // This avoids both the Space-switching animation (Fullscreen::Borderless)
            // and the "always fullscreens on the app's own monitor" problem (set_simple_fullscreen).
            let size = winit::dpi::Size::Logical(winit::dpi::LogicalSize::new(
                monitor_geom.width as f64,
                monitor_geom.height as f64,
            ));
            let position = winit::dpi::Position::Logical(winit::dpi::LogicalPosition::new(
                monitor_geom.x as f64,
                monitor_geom.y as f64,
            ));
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_inner_size(size)
                .with_position(position);
            let win = event_loop.create_window(attrs).context("create window")?;
            // Set the NSWindow level high enough to cover dock and menu bar.
            // kCGScreenSaverWindowLevel = 1000, kCGMainMenuWindowLevel = 24.
            // We use 1000 to be above everything.
            set_macos_window_level(&win, 1000);
            win
        };

        #[cfg(not(target_os = "macos"))]
        let window = {
            let target_monitor = event_loop.available_monitors().find(|m| {
                let pos = m.position();
                pos.x == monitor_geom.x && pos.y == monitor_geom.y
            });
            let attrs = WindowAttributes::default()
                .with_title("quickshot overlay")
                .with_decorations(false)
                .with_resizable(false)
                .with_fullscreen(Some(winit::window::Fullscreen::Borderless(target_monitor)));
            event_loop.create_window(attrs).context("create window")?
        };

        let window = Rc::new(window);

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

/// Set the NSWindow level via raw Objective-C message send.
/// level 1000 = kCGScreenSaverWindowLevel, above dock and menu bar.
#[cfg(target_os = "macos")]
fn set_macos_window_level(window: &Window, level: i64) {
    use winit::raw_window_handle::HasWindowHandle;
    let Ok(handle) = window.window_handle() else {
        return;
    };
    let raw = handle.as_raw();
    let winit::raw_window_handle::RawWindowHandle::AppKit(appkit) = raw else {
        return;
    };
    // appkit.ns_view is a NonNull<c_void> pointing to the NSView.
    // We send [[[nsView window] setLevel:level] to set the NSWindow level.
    extern "C" {
        fn objc_msgSend(obj: *mut std::ffi::c_void, sel: *mut std::ffi::c_void, ...) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const u8) -> *mut std::ffi::c_void;
    }
    unsafe {
        let ns_view = appkit.ns_view.as_ptr();
        let sel_window = sel_registerName(b"window\0".as_ptr());
        let ns_window = objc_msgSend(ns_view, sel_window);
        if ns_window.is_null() {
            return;
        }
        let sel_set_level = sel_registerName(b"setLevel:\0".as_ptr());
        objc_msgSend(ns_window, sel_set_level, level);
    }
}
