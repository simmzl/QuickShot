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
    #[allow(dead_code)]
    surface: Surface<Rc<Window>, Rc<Window>>,
    #[allow(dead_code)]
    image: RgbaImage,
    #[allow(dead_code)]
    press_pos: Option<(i32, i32)>,
    #[allow(dead_code)]
    win_pos_at_press: Option<(i32, i32)>,
    #[allow(dead_code)]
    last_click: Option<Instant>,
}

impl PinWindow {
    /// Stub — actual implementation lands in Task 5.
    #[allow(dead_code)]
    pub fn create(
        _event_loop: &ActiveEventLoop,
        _image: RgbaImage,
        _screen_pos_logical: (i32, i32),
    ) -> Result<Self> {
        anyhow::bail!("PinWindow::create not yet implemented")
    }

    /// Stub — actual implementation lands in Tasks 7–9.
    #[allow(dead_code)]
    pub fn handle_event(&mut self, _event: WindowEvent) -> PinOutcome {
        PinOutcome::Continue
    }
}
