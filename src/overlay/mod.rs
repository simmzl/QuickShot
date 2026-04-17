pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;

use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::MonitorGeom;
use state::{OverlayState, Rect, Transition};

/// What `Overlay::handle_event` reports back to the caller after processing
/// one winit event. Keeps `app.rs` from needing to inspect `OverlayState`.
#[derive(Debug, Clone, Copy)]
pub enum Outcome {
    Continue,
    Confirmed(Rect),
    Cancelled,
}

pub struct Overlay {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    pub frame: RgbaImage,
    pub state: OverlayState,
    pub cursor: (i32, i32),
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
        let surface =
            Surface::new(&context, window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            frame,
            state: OverlayState::Idle,
            cursor: (0, 0),
        })
    }

    /// Translate one winit WindowEvent into a state transition and return
    /// what app.rs should do next.
    pub fn handle_event(&mut self, event: WindowEvent) -> Outcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                if let OverlayState::Dragging { start, .. } = self.state {
                    self.state = state::on_mouse_move_dragging(start, self.cursor);
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if matches!(self.state, OverlayState::Idle) {
                    self.state = state::on_mouse_down_idle(self.cursor);
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                // Task 3 keeps Iter-1 behavior: MouseUp confirms immediately
                // if there's an actual drag. Task 4 will flip this to enter
                // Adjusting instead.
                if let OverlayState::Dragging { start, end } = self.state {
                    let rect = Rect::normalize(start, end);
                    self.state = OverlayState::Idle;
                    if rect.w > 0 && rect.h > 0 {
                        return Outcome::Confirmed(rect);
                    }
                    return Outcome::Cancelled;
                }
                Outcome::Continue
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
                Outcome::Continue
            }
            WindowEvent::CloseRequested => Outcome::Cancelled,
            _ => Outcome::Continue,
        }
    }

    fn current_selection_rect_window(&self) -> Option<(u32, u32, u32, u32)> {
        let r = match self.state {
            OverlayState::Idle => return None,
            OverlayState::Dragging { start, end } => Rect::normalize(start, end),
            OverlayState::Adjusting { rect, .. } => rect,
        };
        let size = self.window.inner_size();
        let r = r.clamp_to((size.width, size.height));
        if r.w == 0 || r.h == 0 {
            None
        } else {
            Some(r.as_tuple_u32())
        }
    }

    /// Translate a window-space rect into a frame-space rect.
    pub fn window_rect_to_frame_rect(&self, rect: Rect) -> (u32, u32, u32, u32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1), size.height.max(1));
        let (fw, fh) = self.frame.dimensions();
        let clamped = rect.clamp_to((ww, wh));
        let (x, y, w, h) = clamped.as_tuple_u32();
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
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let sel = self.current_selection_rect_window();

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, &self.frame);
        render::apply_dim(&mut buf, w, h, sel);
        if let Some(r) = sel {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }

    #[allow(dead_code)]
    fn mark_transition(&mut self, t: Transition) -> Outcome {
        // Helper so Task 4+ can route Enter/ESC/double-click results.
        match t {
            Transition::Stay => Outcome::Continue,
            Transition::Confirm(r) => Outcome::Confirmed(r),
            Transition::Cancel => Outcome::Cancelled,
        }
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
    extern "C" {
        fn objc_msgSend(
            obj: *mut std::ffi::c_void,
            sel: *mut std::ffi::c_void,
            ...
        ) -> *mut std::ffi::c_void;
        fn sel_registerName(name: *const u8) -> *mut std::ffi::c_void;
    }
    unsafe {
        // appkit.ns_view is a NonNull<c_void> pointing to the NSView.
        // We send [[[nsView window] setLevel:level] to set the NSWindow level.
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
