use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::capture;
use crate::overlay::Overlay;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    overlay: Option<Overlay>,
}

impl App {
    pub fn new() -> Self {
        Self { overlay: None }
    }

    fn open_overlay(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let frame = capture::capture_primary()?;
        let overlay = Overlay::create(event_loop, frame)?;
        overlay.window.request_redraw();
        self.overlay = Some(overlay);
        Ok(())
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        if overlay.window.id() != id {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.overlay = None;
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = overlay.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
            }
            _ => {}
        }
        let _ = event_loop;
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::HotkeyFired => {
                if let Err(e) = self.open_overlay(event_loop) {
                    eprintln!("open overlay error: {e:?}");
                }
            }
        }
    }
}
