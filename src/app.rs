use anyhow::Result;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::crop;
use crate::overlay::{state::Rect, Outcome, Overlay};

#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    /// `Cmd/Ctrl+Shift+A` pressed, or "Capture Region" menu item clicked.
    CaptureRegion,
    /// `Cmd/Ctrl+Shift+S` pressed, or "Capture Screen" menu item clicked.
    CaptureScreen,
    /// "Quit" menu item clicked.
    Quit,
}

pub struct App {
    overlay: Option<Overlay>,
    config: crate::config::Config,
}

impl App {
    pub fn new(config: crate::config::Config) -> Self {
        Self {
            overlay: None,
            config,
        }
    }

    fn open_overlay(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let (frame, geom) = capture::capture_at_cursor()?;
        let overlay = Overlay::create(event_loop, frame, &geom)?;
        overlay.window.request_redraw();
        self.overlay = Some(overlay);
        Ok(())
    }

    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        let frame_rect = overlay.window_rect_to_frame_rect(rect);
        let cropped = crop::crop_rgba(&overlay.frame, frame_rect);
        match clipboard::put_image(&cropped) {
            Ok(()) => {
                println!("copied {}x{} to clipboard", cropped.width(), cropped.height());
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &cropped,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => {
                eprintln!("clipboard error: {e:?}");
            }
        }
        drop(overlay);
    }

    fn cancel(&mut self) {
        self.overlay = None;
    }

    fn capture_full_screen(&mut self) {
        // If the region overlay is open, ignore the full-screen request so we
        // don't steal the monitor while the user is mid-selection.
        if self.overlay.is_some() {
            return;
        }
        match capture::capture_at_cursor() {
            Ok((frame, _geom)) => {
                let (w, h) = frame.dimensions();
                if let Err(e) = clipboard::put_image(&frame) {
                    eprintln!("clipboard error: {e:?}");
                    return;
                }
                println!("copied {}x{} (full screen) to clipboard", w, h);
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &frame,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Fullscreen,
                    ) {
                        Ok(path) => println!("saved → {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
                if self.config.general.notification_on_fullscreen {
                    if let Err(e) = crate::notification::screenshot_copied(w, h) {
                        eprintln!("notification error: {e:?}");
                    }
                }
            }
            Err(e) => {
                eprintln!("capture error: {e:?}");
            }
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

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
        match overlay.handle_event(event) {
            Outcome::Continue => {}
            Outcome::Confirmed(rect) => self.confirm(rect),
            Outcome::Cancelled => self.cancel(),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::CaptureRegion => {
                if let Err(e) = self.open_overlay(event_loop) {
                    eprintln!("open overlay error: {e:?}");
                }
            }
            UserEvent::CaptureScreen => {
                self.capture_full_screen();
            }
            UserEvent::Quit => {
                event_loop.exit();
            }
        }
    }
}
