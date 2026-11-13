use anyhow::Result;
use image::RgbaImage;
use std::collections::HashMap;
use winit::application::ApplicationHandler;
use winit::event::{StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoopProxy};
use winit::window::WindowId;

use crate::capture;
use crate::clipboard;
use crate::hotkey::HotkeyGuard;
use crate::overlay::{state::Rect, Outcome, Overlay};
use crate::tray::TrayGuard;

#[derive(Debug, Clone)]
pub enum UserEvent {
    /// `Cmd/Ctrl+Shift+A` pressed, or "Capture Region" menu item clicked.
    CaptureRegion,
    /// `Cmd/Ctrl+Shift+S` pressed, or "Capture Screen" menu item clicked.
    CaptureScreen,
    FullscreenSaved {
        notice: Option<crate::notification::CaptureNotice>,
    },
    /// "Edit Config\u{2026}" menu item clicked — opens the config.toml in default editor.
    EditConfig,
    /// "Start at Login" check item clicked — toggles autostart install/uninstall.
    ToggleAutostart,
    /// `config.toml` changed on disk (detected by the watcher thread).
    /// Triggers re-read of the config and re-registration of hotkeys.
    ReloadConfig,
    /// "Quit" menu item clicked.
    Quit,
    /// The native text editor consumed Return / Escape and wants the overlay
    /// to process its commit / cancel request.
    TextEditorWake,
    /// Ask the overlay to repaint (e.g. a transient badge expired).
    Redraw,
}

pub struct App {
    flash: Option<crate::flash::Flash>,
    pending_notifications: Vec<crate::notification::CaptureNotice>,
    fullscreen_writer: Option<crate::fullscreen_save::SaveQueue>,
    pending_saves: usize,
    quitting: bool,
    overlay: Option<Overlay>,
    /// Reusable hidden overlay window (+ surface) — see `OverlayHost`.
    /// `None` while a session holds it (or before first creation).
    host: Option<crate::overlay::OverlayHost>,
    /// Label / icon fonts, kept across sessions so their glyph caches stay
    /// warm. Moved into the overlay while a session is active.
    fonts: Option<(crate::text::Font, crate::icon_font::IconFont)>,
    pins: Vec<crate::pin::PinWindow>,
    pin_window_ids: HashMap<WindowId, usize>,
    config: crate::config::Config,
    proxy: EventLoopProxy<UserEvent>,
    region_label: String,
    fullscreen_label: String,
    tray: Option<TrayGuard>,
    hotkey_guard: Option<HotkeyGuard>,
}

impl App {
    pub fn new(
        config: crate::config::Config,
        proxy: EventLoopProxy<UserEvent>,
        hotkey_guard: HotkeyGuard,
    ) -> Self {
        let region_label = config.hotkey.region.menu_label();
        let fullscreen_label = config.hotkey.fullscreen.menu_label();
        Self {
            flash: None,
            pending_notifications: Vec::new(),
            fullscreen_writer: None,
            pending_saves: 0,
            quitting: false,
            overlay: None,
            host: None,
            fonts: None,
            pins: Vec::new(),
            pin_window_ids: HashMap::new(),
            config,
            proxy,
            region_label,
            fullscreen_label,
            tray: None,
            hotkey_guard: Some(hotkey_guard),
        }
    }

    /// Re-read `config.toml` and apply changes. Re-registers hotkeys if the
    /// region/fullscreen combos changed, and updates the tray menu labels.
    /// On hotkey-registration failure, the previous bindings are preserved.
    fn reload_config(&mut self) {
        let new_cfg = match crate::config::Config::read_existing() {
            Ok(config) => config,
            Err(error) => {
                eprintln!("reload: {error:#}; keeping previous settings");
                return;
            }
        };
        let new_region_raw = new_cfg.hotkey.region.menu_label();
        let new_screen_raw = new_cfg.hotkey.fullscreen.menu_label();
        let hotkey_changed = new_region_raw != self.region_label
            || new_screen_raw != self.fullscreen_label;

        if hotkey_changed {
            if let Some(guard) = self.hotkey_guard.as_mut() {
                match guard.reregister(
                    new_cfg.hotkey.region.clone(),
                    new_cfg.hotkey.fullscreen.clone(),
                ) {
                    Ok(()) => {
                        self.region_label = new_region_raw;
                        self.fullscreen_label = new_screen_raw;
                        if let Some(tray) = self.tray.as_ref() {
                            tray.set_capture_labels(
                                &self.region_label,
                                &self.fullscreen_label,
                            );
                        }
                        println!(
                            "reloaded hotkeys: {} (region), {} (fullscreen)",
                            self.region_label, self.fullscreen_label,
                        );
                    }
                    Err(e) => {
                        eprintln!("reload: hotkey re-register failed: {e:?}");
                        #[cfg(windows)]
                        {
                            let mut dialog = rfd::MessageDialog::new()
                                .set_title("QuickShot")
                                .set_description(format!("Could not apply the new shortcut. It may already be in use.\n\n{e:#}"))
                                .set_level(rfd::MessageLevel::Warning);
                            if let Some(overlay) = &self.overlay {
                                dialog = dialog.set_parent(overlay.window.as_ref());
                            }
                            dialog.show();
                        }
                        // Keep the old config + labels — `guard` rolled back
                        // to the previous binding internally.
                        return;
                    }
                }
            }
        } else {
            println!("reloaded config (hotkeys unchanged)");
        }

        // Always update save/general settings — they take effect on next capture.
        self.config = new_cfg;
    }

    fn open_overlay(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        if self.overlay.is_some() {
            return Ok(());
        }
        let debug = std::env::var_os("QUICKSHOT_DEBUG").is_some();
        let t0 = std::time::Instant::now();

        let geom = capture::monitor_at_cursor()?;
        // Window enumeration (snap targets) runs in parallel with the capture
        // + pixel conversion. Snap rects are scaled with the monitor's backing
        // scale, which equals the overlay window's scale factor once placed.
        let (captured, window_list) = std::thread::scope(|s| {
            let enum_handle = s.spawn(|| {
                let t = std::time::Instant::now();
                let l = crate::overlay::snap::enumerate_windows(&geom, std::process::id(), geom.scale as f32);
                if debug {
                    eprintln!("QuickShot timing: enumerate_windows {:?}", t.elapsed());
                }
                l
            });
            let cap = capture::capture(&geom, true);
            (cap, enum_handle.join().unwrap_or_default())
        });
        let captured = captured?;
        let t_capture = t0.elapsed();

        let host = match self.host.take() {
            Some(h) => h,
            None => crate::overlay::OverlayHost::new(event_loop)?,
        };
        let (font, icon_font) = self.fonts.take().unwrap_or_else(|| {
            (crate::text::Font::embedded(), crate::icon_font::IconFont::embedded())
        });
        let t_prep = t0.elapsed();
        let overlay = Overlay::begin(
            host,
            captured,
            &geom,
            window_list,
            font,
            icon_font,
            self.proxy.clone(),
        )?;
        self.overlay = Some(overlay);
        if debug {
            eprintln!(
                "QuickShot timing: capture+enum {:?}, host/fonts ready {:?}, overlay visible {:?}",
                t_capture,
                t_prep,
                t0.elapsed()
            );
        }
        Ok(())
    }

    /// End the active overlay session, keeping the window + fonts for reuse.
    fn close_overlay(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            let (host, font, icon) = overlay.into_parts();
            self.host = Some(host);
            self.fonts = Some((font, icon));
        }
    }

    /// Pre-create the hidden overlay window and fonts, and warm up the
    /// capture path, so the first hotkey press is as fast as later ones.
    fn warm_up(&mut self, event_loop: &ActiveEventLoop) {
        if self.host.is_none() {
            match crate::overlay::OverlayHost::new(event_loop) {
                Ok(h) => self.host = Some(h),
                Err(e) => eprintln!("QuickShot: overlay pre-create failed: {e:?}"),
            }
        }
        if self.fonts.is_none() {
            let mut font = crate::text::Font::embedded();
            // Load the platform annotation font now (~45 ms cold on macOS)
            // instead of on the first Text-tool click.
            let _ = font.text_mask("你A", 24.0, 0x00FF_3B30);
            self.fonts = Some((font, crate::icon_font::IconFont::embedded()));
        }
        std::thread::spawn(|| {
            let t = std::time::Instant::now();
            capture::warm_up();
            if std::env::var_os("QUICKSHOT_DEBUG").is_some() {
                eprintln!("QuickShot: capture warm-up took {:?}", t.elapsed());
            }
        });
    }

    /// Copy `img` to the clipboard and save as PNG if config.save.enabled.
    /// `label` is appended to the "copied …" log line so the user knows
    /// whether this was a confirm or a pin.
    fn export_image(&self, img: &RgbaImage, label: &str) {
        let (w, h) = img.dimensions();
        match clipboard::put_image(img) {
            Ok(()) => {
                if label.is_empty() {
                    println!("copied {}x{} to clipboard", w, h);
                } else {
                    println!("copied {}x{} to clipboard ({})", w, h, label);
                }
                if self.config.save.enabled {
                    match crate::file_save::save_png(
                        img,
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
    }

    fn confirm(&mut self, rect: Rect) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        // Hide first so the overlay disappears immediately; clipboard + disk
        // writes happen after.
        self.close_overlay();
        self.export_image(&final_image, "");
    }

    /// Open a native save dialog and write the cropped+annotated image to the
    /// user-chosen path. No clipboard copy, no pin — pure save-to-disk. Keeps
    /// the overlay visible during the dialog (level temporarily lowered so the
    /// panel can render above it); closes overlay on save, restores it on cancel.
    fn save_as(&mut self, rect: Rect) {
        // Flatten image + clone the window handle while holding overlay borrow.
        // We need the handle later to flip the NSWindow level around the dialog.
        let (final_image, window_handle) = {
            let Some(overlay) = self.overlay.as_mut() else {
                return;
            };
            let img = overlay.flatten_for_export(rect);
            let handle = overlay.window.clone();   // Rc clone — cheap.
            (img, handle)
        };
        let (img_w, img_h) = final_image.dimensions();

        // Lower the overlay's NSWindow level so the save panel renders above it.
        // (The overlay would otherwise sit at level 1500 and cover the panel.)
        #[cfg(target_os = "macos")]
        crate::overlay::set_macos_window_level(&window_handle, 0);

        // Switch activation policy to Regular so the modal panel from our
        // Accessory app gets focus + renders properly.
        #[cfg(target_os = "macos")]
        set_macos_activation_policy(0);

        let default_name = crate::file_save::screenshot_filename(
            &self.config.save.filename_template, (img_w, img_h),
            crate::file_save::CaptureMode::Region, crate::file_save::capture_time(),
        );
        let state = crate::config::config_path().map(|p| p.with_file_name("last-save-directory"));
        let mut dialog = rfd::FileDialog::new()
            .set_title("Save Screenshot")
            .set_file_name(std::path::Path::new(&default_name).file_name()
                .and_then(|name| name.to_str()).unwrap_or("QuickShot.png"))
            .add_filter("PNG image", &["png"]);
        if let Some(dir) = crate::file_save::dialog_directory(state.as_deref(), &self.config.save.directory) {
            dialog = dialog.set_directory(dir);
        }
        // An owned dialog stays above the topmost capture overlay on Windows.
        #[cfg(windows)]
        let dialog = dialog.set_parent(window_handle.as_ref());
        let path = dialog.save_file();

        #[cfg(target_os = "macos")]
        set_macos_activation_policy(1);

        match path {
            Some(p) => {
                let mut p = p;
                if p.extension().is_none() {
                    p.set_extension("png");
                }
                match final_image.save(&p) {
                    Ok(()) => {
                        println!("saved {}x{} → {}", img_w, img_h, p.display());
                        if let Some(state) = state {
                            if let Err(error) = crate::file_save::remember_dialog_directory(&state, &p) {
                                eprintln!("remember save directory: {error:#}");
                            }
                        }
                    }
                    Err(e) => eprintln!("save error: {e:?}"),
                }
                // Saved — close the overlay.
                self.close_overlay();
            }
            None => {
                // Cancelled — restore overlay level so it pops back on top.
                // Overlay stays alive; user can keep editing or pick another action.
                #[cfg(target_os = "macos")]
                crate::overlay::set_macos_window_level(&window_handle, 1500);
                println!("save cancelled");
            }
        }
        // window_handle (the Rc clone) drops here; the window itself lives on
        // in the reusable OverlayHost.
    }

    fn pin(&mut self, rect: Rect, event_loop: &ActiveEventLoop) {
        let Some(overlay) = self.overlay.as_mut() else {
            return;
        };
        let final_image = overlay.flatten_for_export(rect);
        let scale = overlay.window.scale_factor();
        let screen_pos = crate::pin::compute_pin_screen_position(
            overlay.window.inner_position().unwrap_or_default(), (rect.x, rect.y), scale,
        );
        self.close_overlay();
        self.export_image(&final_image, "pinned");

        match crate::pin::PinWindow::create(event_loop, final_image, screen_pos, scale) {
            Ok(pin_win) => {
                let id = pin_win.window.id();
                let idx = self.pins.len();
                self.pins.push(pin_win);
                self.pin_window_ids.insert(id, idx);
            }
            Err(e) => eprintln!("pin create error: {e:?}"),
        }
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
        self.close_overlay();
    }

    fn capture_full_screen(&mut self) {
        if self.overlay.is_some() {
            return;
        }
        match capture::capture_at_cursor() {
            Ok((captured, geom)) => {
                let frame = captured.frame;
                let (w, h) = frame.dimensions();
                let feedback = self.config.general.fullscreen_feedback;
                if feedback.flashes() {
                    match crate::flash::Flash::show(geom) {
                        Ok(flash) => self.flash = Some(flash),
                        Err(error) => eprintln!("capture flash: {error:#}"),
                    }
                }
                let copied = match clipboard::put_image(&frame) {
                    Ok(()) => true,
                    Err(error) => {
                        eprintln!("fullscreen clipboard: {error:#}");
                        false
                    }
                };
                if self.config.save.enabled {
                    let queue = self.fullscreen_writer.get_or_insert_with(|| {
                        let proxy = self.proxy.clone();
                        crate::fullscreen_save::SaveQueue::new(move |notice| {
                            let _ = proxy.send_event(UserEvent::FullscreenSaved { notice });
                        })
                    });
                    let capture = crate::fullscreen_save::Capture {
                        frame,
                        save: self.config.save.clone(),
                        captured_at: crate::file_save::capture_time(),
                        notify: feedback.notifies(),
                        copied,
                    };
                    if let Err(reason) = queue.submit(capture) {
                        eprintln!("fullscreen save: {reason}");
                        if feedback.notifies() && copied {
                            self.pending_notifications.push(crate::notification::CaptureNotice::busy(w, h));
                        }
                    } else {
                        self.pending_saves += 1;
                    }
                } else if feedback.notifies() && copied {
                    self.pending_notifications.push(crate::notification::CaptureNotice::new(w, h, None));
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
        // Ensure the file exists so the OS opener has something to target —
        // Config::load writes the default on first run, but if the user wiped
        // the config dir manually between launches we may hit a missing file.
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, crate::config::DEFAULT_TOML);
        }
        if let Err(e) = open_config_in_editor(&path) {
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
            eprintln!("QuickShot: new_events(Init) — setting NSApp policy and installing tray");
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
                    eprintln!("QuickShot: tray installed OK");
                    self.tray = Some(guard);
                }
                Err(e) => eprintln!("QuickShot: tray install error: {e:?}"),
            }
            self.warm_up(_event_loop);
        }
    }

    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(flash) = &self.flash {
            if std::time::Instant::now() < flash.until {
                event_loop.set_control_flow(ControlFlow::WaitUntil(flash.until));
                return;
            }
            self.flash = None;
        }
        // Deliver after the full-monitor flash window is gone, including for clipboard-only captures.
        for notice in self.pending_notifications.drain(..) {
            if let Err(error) = notice.show() {
                eprintln!("capture notification: {error:#}");
            }
        }
        if self.quitting && self.pending_saves == 0 {
            event_loop.exit();
        }
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
                let outcome = overlay.handle_event(event);
                overlay.poll_text_editor();
                match outcome {
                    Outcome::Continue => {}
                    Outcome::Confirmed(rect) => self.confirm(rect),
                    Outcome::Pinned(rect) => self.pin(rect, event_loop),
                    Outcome::SaveAs(rect) => self.save_as(rect),
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
        // Drain accepted PNG writes and deliver their notifications before quitting.
        if self.quitting && !matches!(event, UserEvent::FullscreenSaved { .. }) {
            return;
        }
        match event {
            UserEvent::CaptureRegion => {
                self.flash = None;
                if let Err(e) = self.open_overlay(event_loop) {
                    eprintln!("open overlay error: {e:?}");
                }
            }
            UserEvent::CaptureScreen => {
                self.flash = None;
                self.capture_full_screen();
            }
            UserEvent::FullscreenSaved { notice } => {
                self.pending_saves -= 1;
                self.pending_notifications.extend(notice);
            }
            UserEvent::EditConfig => {
                self.edit_config();
            }
            UserEvent::ToggleAutostart => {
                self.toggle_autostart();
            }
            UserEvent::ReloadConfig => {
                self.reload_config();
            }
            UserEvent::Quit => {
                self.quitting = true;
                self.flash = None;
            }
            UserEvent::Redraw => {
                if let Some(overlay) = self.overlay.as_ref() {
                    overlay.window.request_redraw();
                }
            }
            UserEvent::TextEditorWake => {
                if let Some(overlay) = self.overlay.as_mut() {
                    overlay.poll_text_editor();
                }
            }
        }
    }
}

/// Set NSApplication.activationPolicy. NSApplicationActivationPolicy values:
///   Regular    = 0  (foreground app, Dock icon, menu bar — gains focus)
///   Accessory  = 1  (no Dock, no menu bar, but can show windows)
///   Prohibited = 2  (cannot show windows)
///
/// We run as Accessory at startup so the app participates in AppKit enough to
/// own an NSStatusBar item without grabbing a Dock icon. We temporarily flip
/// to Regular around modal dialogs (e.g. NSSavePanel) because panels from
/// Accessory apps may not get focus / may not display reliably.
#[cfg(target_os = "macos")]
fn set_macos_activation_policy(policy: i64) {
    use crate::macos_objc::{class, msg_send_id, msg_send_set_int, sel};
    unsafe {
        let ns_app_class = class(c"NSApplication");
        if ns_app_class.is_null() {
            eprintln!("QuickShot: objc_getClass(NSApplication) = null");
            return;
        }
        let ns_app = msg_send_id(ns_app_class, sel(c"sharedApplication"));
        if ns_app.is_null() {
            eprintln!("QuickShot: NSApplication sharedApplication = null");
            return;
        }
        msg_send_set_int(ns_app, sel(c"setActivationPolicy:"), policy);
    }
}

/// Force NSApplication.activationPolicy = Accessory so the process participates
/// in AppKit well enough to own an NSStatusBar item. For bundled LSUIElement
/// apps this is redundant but harmless; for direct-exec CLI it's required.
#[cfg(target_os = "macos")]
fn set_macos_activation_policy_accessory() {
    set_macos_activation_policy(1);
}

#[cfg(target_os = "macos")]
fn open_config_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    // `open` forwards our environment to the launched app. If QuickShot was
    // started from a shell that has ELECTRON_RUN_AS_NODE set (common inside
    // Electron-based terminals / agent tools), Electron editors such as
    // VS Code / Cursor / Antigravity would run as plain Node, try to execute
    // config.toml as JavaScript and exit instantly — "Edit Config" appears to
    // do nothing. Strip the Electron mode variables before handing off.
    std::process::Command::new("open")
        .env_remove("ELECTRON_RUN_AS_NODE")
        .env_remove("ELECTRON_NO_ATTACH_CONSOLE")
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "windows")]
fn open_config_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    // notepad.exe ships on every Windows install and is a GUI app, so it
    // won't flash a console window. We intentionally don't use
    // `cmd /C start` here because that briefly spawns a console.
    std::process::Command::new("notepad.exe")
        .creation_flags(DETACHED_PROCESS)
        .arg(path)
        .spawn()
        .map(|_| ())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn open_config_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    std::process::Command::new("xdg-open")
        .arg(path)
        .spawn()
        .map(|_| ())
}
