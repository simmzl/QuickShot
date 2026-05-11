//! A pinned floating window — a cropped screenshot image that the user
//! "tacked" onto the desktop. Multiple pins can coexist; each owns its own
//! winit Window + softbuffer Surface.

use anyhow::Result;
use image::RgbaImage;
use softbuffer::Surface;
use std::rc::Rc;
use std::time::Instant;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

#[derive(Debug, Clone, Copy)]
pub enum PinOutcome {
    Continue,
    Closed,
}

pub struct PinWindow {
    pub window: Rc<Window>,
    surface: Surface<Rc<Window>, Rc<Window>>,
    image: RgbaImage,
    #[allow(dead_code)]
    press_pos: Option<(i32, i32)>,
    #[allow(dead_code)]
    win_pos_at_press: Option<(i32, i32)>,
    #[allow(dead_code)]
    last_click: Option<Instant>,
}

/// Compute where to place a pin window so it visually "comes from" the
/// captured selection. Inputs are all in physical pixels and screen-logical
/// coordinates; output is screen-logical for `winit::dpi::LogicalPosition`.
///
/// `overlay_outer_logical`: top-left of the overlay window in CG screen-logical
/// coordinates (same units winit's `with_position(LogicalPosition::new(...))`
/// takes).
///
/// `selection_physical`: the rect inside the overlay in physical pixels
/// (overlay-window-local).
///
/// `scale_factor`: the overlay window's scale factor (Retina = 2.0).
///
/// Returns the screen-logical position where the pin window's top-left
/// should sit, plus an 8-px down-right offset so the pin doesn't perfectly
/// cover the captured area.
pub fn compute_pin_screen_position(
    overlay_outer_logical: (i32, i32),
    selection_physical: (i32, i32),
    scale_factor: f32,
) -> (i32, i32) {
    let (ox, oy) = overlay_outer_logical;
    let (sx, sy) = selection_physical;
    let sx_logical = (sx as f32 / scale_factor).round() as i32;
    let sy_logical = (sy as f32 / scale_factor).round() as i32;
    (ox + sx_logical + 8, oy + sy_logical + 8)
}

impl PinWindow {
    #[allow(dead_code)]
    pub fn create(
        event_loop: &ActiveEventLoop,
        image: RgbaImage,
        screen_pos_logical: (i32, i32),
        logical_size: (u32, u32),
    ) -> Result<Self> {
        use anyhow::Context;
        use softbuffer::Context as SoftContext;
        use winit::dpi::{LogicalPosition, LogicalSize};
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("quickshot pin")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(
                logical_size.0 as f64,
                logical_size.1 as f64,
            ))
            .with_position(LogicalPosition::new(
                screen_pos_logical.0 as f64,
                screen_pos_logical.1 as f64,
            ));
        let win = event_loop
            .create_window(attrs)
            .context("pin: create window")?;

        let window = Rc::new(win);

        // NSFloatingWindowLevel = 3. Above normal apps, below the overlay's
        // 1500 so a fresh capture appears above pins.
        #[cfg(target_os = "macos")]
        crate::overlay::set_macos_window_level(&window, 3);

        let context =
            SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            image,
            press_pos: None,
            win_pos_at_press: None,
            last_click: None,
        })
    }

    /// Stub — actual implementation lands in Tasks 7–9.
    #[allow(dead_code)]
    pub fn handle_event(&mut self, _event: WindowEvent) -> PinOutcome {
        PinOutcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_position_no_scale() {
        let pos = compute_pin_screen_position((100, 200), (50, 30), 1.0);
        assert_eq!(pos, (100 + 50 + 8, 200 + 30 + 8));
    }

    #[test]
    fn pin_position_retina_scale_2() {
        let pos = compute_pin_screen_position((0, 0), (400, 200), 2.0);
        assert_eq!(pos, (200 + 8, 100 + 8));
    }

    #[test]
    fn pin_position_overlay_offset() {
        let pos = compute_pin_screen_position((200, 100), (60, 40), 2.0);
        assert_eq!(pos, (200 + 30 + 8, 100 + 20 + 8));
    }

    #[test]
    fn pin_position_zero_selection() {
        let pos = compute_pin_screen_position((50, 50), (0, 0), 2.0);
        assert_eq!(pos, (58, 58));
    }
}
