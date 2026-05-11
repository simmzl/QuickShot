use anyhow::Result;
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::overlay::{state::Rect, Outcome, Overlay};
use crate::tray::TrayGuard;

#[derive(Debug, Clone, Copy)]
pub enum UserEvent {
    /// `Cmd/Ctrl+Shift+A` pressed, or "Capture Region" menu item clicked.
    CaptureRegion,
    /// `Cmd/Ctrl+Shift+S` pressed, or "Capture Screen" menu item clicked.
    CaptureScreen,
    /// "Edit Config\u{2026}" menu item clicked — opens the config.toml in default editor.
    EditConfig,
    /// "Start at Login" check item clicked — toggles autostart install/uninstall.
    ToggleAutostart,
    /// "Quit" menu item clicked.
    Quit,
}

pub struct App {
    overlay: Option<Overlay>,
    pins: Vec<crate::pin::PinWindow>,
    pin_window_ids: HashMap<WindowId, usize>,
    config: crate::config::Config,
    proxy: EventLoopProxy<UserEvent>,
    region_label: String,
    fullscreen_label: String,
    tray: Option<TrayGuard>,
}

impl App {
    pub fn new(
        config: crate::config::Config,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Self {
        let region_label = config.hotkey.region.raw.clone();
        let fullscreen_label = config.hotkey.fullscreen.raw.clone();
        Self {
            overlay: None,
            pins: Vec::new(),
            pin_window_ids: HashMap::new(),
            config,
            proxy,
            region_label,
            fullscreen_label,
            tray: None,
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
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        match clipboard::put_image(&final_image) {
            Ok(()) => {
                println!(
                    "copied {}x{} to clipboard",
                    final_image.width(),
                    final_image.height()
                );
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &final_image,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved \u{2192} {}", path.display()),
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

    fn pin(&mut self, rect: Rect, event_loop: &ActiveEventLoop) {
        let Some(mut overlay) = self.overlay.take() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        let (img_w, img_h) = final_image.dimensions();

        // Same clipboard + save logic as App::confirm.
        match clipboard::put_image(&final_image) {
            Ok(()) => {
                println!("copied {}x{} to clipboard (pinned)", img_w, img_h);
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        &final_image,
                        &self.config.save.directory,
                        &self.config.save.filename_template,
                        crate::file_save::CaptureMode::Region,
                    ) {
                        Ok(path) => println!("saved \u{2192} {}", path.display()),
                        Err(e) => eprintln!("save error: {e:?}"),
                    }
                }
            }
            Err(e) => eprintln!("clipboard error: {e:?}"),
        }

        // Compute pin position + logical size.
        let scale_factor = overlay.scale_factor();
        let overlay_outer = overlay
            .window
            .outer_position()
            .unwrap_or_default();
        let screen_pos = crate::pin::compute_pin_screen_position(
            (overlay_outer.x, overlay_outer.y),
            (rect.x, rect.y),
            scale_factor,
        );
        let logical_size = (
            (img_w as f32 / scale_factor).round() as u32,
            (img_h as f32 / scale_factor).round() as u32,
        );

        match crate::pin::PinWindow::create(event_loop, final_image, screen_pos, logical_size) {
            Ok(pin_win) => {
                let id = pin_win.window.id();
                let idx = self.pins.len();
                self.pins.push(pin_win);
                self.pin_window_ids.insert(id, idx);
            }
            Err(e) => eprintln!("pin create error: {e:?}"),
        }

        drop(overlay);
    }

    fn close_pin(&mut self, idx: usize) {
        if idx >= self.pins.len() {
            return;
        }
        let pin = self.pins.swap_remove(idx);
        self.pin_window_ids.remove(&pin.window.id());
        // swap_remove moved the last element into `idx`; remap its WindowId.
        if idx < self.pins.len() {
            let moved_id = self.pins[idx].window.id();
            self.pin_window_ids.insert(moved_id, idx);
        }
        // `pin` drops here → winit closes the window.
    }

    fn cancel(&mut self) {
        self.overlay = None;
    }

    fn capture_full_screen(&mut self) {
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
                        Ok(path) => println!("saved \u{2192} {}", path.display()),
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

    fn edit_config(&self) {
        let Some(path) = crate::config::config_path() else {
            eprintln!("edit config: could not resolve config path");
            return;
        };
        // Ensure the file exists so `open` has something to target — Config::load
        // writes the default on first run, but if the user wiped ~/.config manually
        // between launches we may hit a missing file.
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, crate::config::DEFAULT_TOML);
        }
        if let Err(e) = std::process::Command::new("open").arg(&path).spawn() {
            eprintln!("edit config: could not open {:?}: {e}", path);
        }
    }

    fn toggle_autostart(&mut self) {
        // The CheckMenuItem auto-toggles its check state on click; read the
        // new state and call install/uninstall accordingly. On failure, revert
        // the check state so the UI reflects truth.
        let Some(tray) = self.tray.as_ref() else {
            return;
        };
        let now_checked = tray.autostart_item.is_checked();
        let result = if now_checked {
            crate::autostart::install()
        } else {
            crate::autostart::uninstall()
        };
        if let Err(e) = result {
            eprintln!("toggle autostart: {e:?}");
            // Revert the check to reflect actual state
            tray.autostart_item.set_checked(!now_checked);
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn new_events(&mut self, _event_loop: &ActiveEventLoop, cause: StartCause) {
        if matches!(cause, StartCause::Init) && self.tray.is_none() {
            eprintln!("quickshot: new_events(Init) — setting NSApp policy and installing tray");
            #[cfg(target_os = "macos")]
            set_macos_activation_policy_accessory();
            let initial_autostart = crate::autostart::is_installed();
            match crate::tray::install(
                self.proxy.clone(),
                &self.region_label,
                &self.fullscreen_label,
                initial_autostart,
            ) {
                Ok(guard) => {
                    eprintln!("quickshot: tray installed OK");
                    self.tray = Some(guard);
                }
                Err(e) => eprintln!("quickshot: tray install error: {e:?}"),
            }
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        // Try overlay first.
        if let Some(overlay) = self.overlay.as_mut() {
            if overlay.window.id() == id {
                match overlay.handle_event(event) {
                    Outcome::Continue => {}
                    Outcome::Confirmed(rect) => self.confirm(rect),
                    Outcome::Pinned(rect) => self.pin(rect, event_loop),
                    Outcome::Cancelled => self.cancel(),
                }
                return;
            }
        }
        // Try pins.
        if let Some(&idx) = self.pin_window_ids.get(&id) {
            match self.pins[idx].handle_event(event) {
                crate::pin::PinOutcome::Continue => {}
                crate::pin::PinOutcome::Closed => self.close_pin(idx),
            }
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
            UserEvent::EditConfig => {
                self.edit_config();
            }
            UserEvent::ToggleAutostart => {
                self.toggle_autostart();
            }
            UserEvent::Quit => {
                event_loop.exit();
            }
        }
    }
}

/// Force NSApplication.activationPolicy = Accessory so the process participates
/// in AppKit well enough to own an NSStatusBar item. For bundled LSUIElement
/// apps this is redundant but harmless; for direct-exec CLI it's required.
#[cfg(target_os = "macos")]
fn set_macos_activation_policy_accessory() {
    use crate::macos_objc::{class, msg_send_id, msg_send_set_int, sel};
    unsafe {
        let ns_app_class = class(c"NSApplication");
        if ns_app_class.is_null() {
            eprintln!("quickshot: objc_getClass(NSApplication) = null");
            return;
        }
        let ns_app = msg_send_id(ns_app_class, sel(c"sharedApplication"));
        if ns_app.is_null() {
            eprintln!("quickshot: NSApplication sharedApplication = null");
            return;
        }
        // NSApplicationActivationPolicyAccessory = 1 (NSInteger, 64-bit on macOS).
        msg_send_set_int(ns_app, sel(c"setActivationPolicy:"), 1);
    }
}
