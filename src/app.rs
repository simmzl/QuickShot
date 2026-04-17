use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::crop;
use crate::overlay::Overlay;

#[derive(Debug, Clone)]
pub enum UserEvent {
    HotkeyFired,
}

pub struct App {
    overlay: Option<Overlay>,
    cursor: (i32, i32),
}

impl App {
    pub fn new() -> Self {
        Self {
            overlay: None,
            cursor: (0, 0),
        }
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

    fn finish_selection(&mut self) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let Some(win_rect) = overlay.current_window_rect() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(win_rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        if let Err(e) = clipboard::put_image(&cropped) {
            eprintln!("clipboard error: {e:?}");
        } else {
            println!(
                "copied {}x{} to clipboard",
                cropped.width(),
                cropped.height()
            );
        }
        drop(overlay);
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
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
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);
                if overlay.drag_start.is_some() {
                    overlay.drag_end = Some(self.cursor);
                    overlay.window.request_redraw();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                overlay.drag_start = Some(self.cursor);
                overlay.drag_end = Some(self.cursor);
                overlay.window.request_redraw();
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                overlay.drag_end = Some(self.cursor);
                self.finish_selection();
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = overlay.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
            }
            _ => {}
        }
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
