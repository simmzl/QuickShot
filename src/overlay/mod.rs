pub(crate) mod annotate;
pub(crate) mod annotate_render;
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod state;
pub(crate) mod toolbar;

use anyhow::{Context, Result};
use image::RgbaImage;
use softbuffer::{Context as SoftContext, Surface};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey, ModifiersState};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::MonitorGeom;
use state::{OverlayState, Rect, Transition};
use annotate::{AnnotationStyle, PendingDraw};

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
    pub(crate) state: OverlayState,
    pub(crate) cursor: (i32, i32),
    last_click: Option<std::time::Instant>,
    last_redraw: Option<std::time::Instant>,
    font: crate::text::Font,
    scale_factor: f32,
    // Iter 5a: annotation state
    pub(crate) tool: annotate::Tool,
    pub(crate) history: annotate::History,
    pub(crate) pending_draw: Option<PendingDraw>,
    modifiers: ModifiersState,
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
            // macOS 26 (Tahoe) tightened how it places accessory-app borderless
            // windows: without an explicit collectionBehavior, an overlay with
            // setLevel:1000 can land at the desktop layer instead of covering
            // app windows. canJoinAllSpaces+stationary+fullScreenAuxiliary is
            // the standard idiom for screen overlays / screenshot tools.
            set_macos_overlay_collection_behavior(&win);
            // kCGAssistiveTechHighWindowLevel = 1500 — Apple-reserved high level for
            // assistive overlays, sits above kCGScreenSaverWindowLevel (1000) and is
            // robust against macOS 26's new accessory-app demotion behavior.
            set_macos_window_level(&win, 1500);
            // Suppress the default NSWindow appear/order-front animation so the
            // overlay doesn't fade-in or zoom on every capture.
            set_macos_window_animation_none(&win);
            // Without this, ESC/Enter before the first mouse click are dropped
            // because the borderless high-level window isn't automatically key.
            make_macos_key_window(&win);
            // Read back the level so a future regression is visible in stderr
            // without having to attach a debugger.
            log_macos_window_level(&win);
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
            let win = event_loop.create_window(attrs).context("create window")?;
            win.focus_window();
            win
        };

        let scale_factor = window.scale_factor() as f32;
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
            last_click: None,
            last_redraw: None,
            font: crate::text::Font::embedded(),
            scale_factor,
            tool: annotate::Tool::Move,
            history: annotate::History::new(),
            pending_draw: None,
            modifiers: ModifiersState::default(),
        })
    }

    /// Translate one winit WindowEvent into a state transition and return
    /// what app.rs should do next.
    pub fn handle_event(&mut self, event: WindowEvent) -> Outcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);

                // If a PendingDraw is in flight, update its endpoint.
                if self.pending_draw.is_some() {
                    let fp = self.window_point_to_frame_point(self.cursor);
                    if let Some(p) = self.pending_draw.as_mut() {
                        p.extend_to(fp);
                    }
                    self.request_redraw_throttled();
                    return Outcome::Continue;
                }

                match self.state {
                    OverlayState::Dragging { start, .. } => {
                        self.state = state::on_mouse_move_dragging(start, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { edit: Some(_), .. } => {
                        self.state = state::update_edit(self.state, self.cursor);
                        self.request_redraw_throttled();
                    }
                    OverlayState::Adjusting { rect, edit: None } => {
                        // No redraw needed — just update the cursor icon.
                        let icon = hit::cursor_icon_for(hit::classify(self.cursor, rect));
                        self.window.set_cursor(icon);
                    }
                    OverlayState::Idle => {
                        // Magnifier follows cursor while idle; throttle to ~60 Hz.
                        self.request_redraw_throttled();
                    }
                }
                Outcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => self.handle_left_press(),
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.handle_left_release(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        repeat: false,
                        ..
                    },
                ..
            } => self.handle_key(logical_key),
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
                Outcome::Continue
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
                Outcome::Continue
            }
            WindowEvent::CloseRequested => Outcome::Cancelled,
            _ => Outcome::Continue,
        }
    }

    fn handle_left_press(&mut self) -> Outcome {
        // Detect double-click: two presses within 400 ms (position-agnostic).
        let now = std::time::Instant::now();
        let is_double_click = matches!(
            self.last_click,
            Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
        );
        self.last_click = Some(now); // running window: each click resets the 400 ms gate

        match self.state {
            OverlayState::Idle => {
                self.state = state::on_mouse_down_idle(self.cursor);
                self.window.request_redraw();
                Outcome::Continue
            }
            OverlayState::Dragging { .. } => Outcome::Continue,
            OverlayState::Adjusting { rect, .. } => {
                if is_double_click {
                    match state::on_double_click_adjusting(rect, self.cursor) {
                        Transition::Confirm(r) => return Outcome::Confirmed(r),
                        Transition::Cancel => return Outcome::Cancelled,
                        Transition::Stay => {}
                    }
                }

                // Toolbar click takes priority over everything else.
                let win_size = self.window.inner_size();
                let tb = toolbar::Toolbar::layout(rect, (win_size.width, win_size.height));
                match tb.hit_with_tool(self.cursor, self.tool) {
                    toolbar::ToolbarHit::Tool(t) => {
                        self.pending_draw = None;
                        if let OverlayState::Adjusting { rect: r, .. } = self.state {
                            self.state = OverlayState::Adjusting { rect: r, edit: None };
                        }
                        self.tool = t;
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Undo => {
                        if self.history.undo() {
                            self.window.request_redraw();
                        }
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Redo => {
                        if self.history.redo() {
                            self.window.request_redraw();
                        }
                        return Outcome::Continue;
                    }
                    // Color/Stroke clicks are wired in Task 8.
                    toolbar::ToolbarHit::Color(_) | toolbar::ToolbarHit::Stroke(_) => {
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::None => {}
                }

                // Drawing tool + click inside selection → start PendingDraw.
                if self.tool.is_drawing() && rect.contains(self.cursor) {
                    let fp = self.window_point_to_frame_point(self.cursor);
                    self.pending_draw = Some(PendingDraw::shape(
                        self.tool,
                        AnnotationStyle::default(),
                        fp,
                        fp,
                    ));
                    self.window.request_redraw();
                    return Outcome::Continue;
                }

                // Move tool: existing Iter 2a behavior (anchor resize / translate / clear).
                match hit::classify(self.cursor, rect) {
                    hit::HitZone::Anchor(a) => {
                        self.state = state::start_resize(rect, a, self.cursor);
                        self.window.request_redraw();
                    }
                    hit::HitZone::Inside => {
                        self.state = state::start_translate(rect, self.cursor);
                        self.window.request_redraw();
                    }
                    hit::HitZone::Outside => {
                        self.state = OverlayState::Idle;
                        self.window.request_redraw();
                    }
                }
                Outcome::Continue
            }
        }
    }

    fn handle_left_release(&mut self) -> Outcome {
        // Commit an in-flight annotation draw first.
        if let Some(pending) = self.pending_draw.take() {
            if let Some(ann) = pending.finalize() {
                self.history.push(ann);
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }

        match self.state {
            OverlayState::Dragging { start, end } => {
                let rect = Rect::normalize(start, end);
                if rect.w > 0 && rect.h > 0 {
                    self.state = OverlayState::Adjusting { rect, edit: None };
                } else {
                    self.state = OverlayState::Idle;
                }
                self.window.request_redraw();
            }
            OverlayState::Adjusting { edit: Some(_), .. } => {
                self.state = state::commit_edit(self.state);
                self.window.request_redraw();
            }
            _ => {}
        }
        Outcome::Continue
    }

    fn handle_key(&mut self, key: Key) -> Outcome {
        // Cmd+Shift+Z → redo; Cmd+Z → undo (only when Cmd held)
        if self.modifiers.super_key() {
            if let Key::Character(s) = &key {
                let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
                if ch == 'z' {
                    let changed = if self.modifiers.shift_key() {
                        self.history.redo()
                    } else {
                        self.history.undo()
                    };
                    if changed {
                        self.window.request_redraw();
                    }
                    return Outcome::Continue;
                }
            }
        }

        // Tool shortcuts (only in Adjusting, no modifier)
        if matches!(self.state, OverlayState::Adjusting { .. }) && !self.modifiers.super_key() {
            if let Key::Character(s) = &key {
                let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
                let new_tool = match ch {
                    'm' => Some(annotate::Tool::Move),
                    'a' => Some(annotate::Tool::Arrow),
                    'r' => Some(annotate::Tool::Rect),
                    'e' => Some(annotate::Tool::Ellipse),
                    'b' => Some(annotate::Tool::Mosaic),
                    _ => None,
                };
                if let Some(t) = new_tool {
                    self.pending_draw = None;
                    if let OverlayState::Adjusting { rect, edit: _ } = self.state {
                        self.state = OverlayState::Adjusting { rect, edit: None };
                    }
                    self.tool = t;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
            }
        }

        match key {
            Key::Named(NamedKey::Escape) => match state::on_escape(self.state) {
                Transition::Cancel => Outcome::Cancelled,
                _ => Outcome::Continue,
            },
            Key::Named(NamedKey::Enter) => match state::on_enter(self.state) {
                Transition::Confirm(r) => Outcome::Confirmed(r),
                _ => Outcome::Continue,
            },
            _ => Outcome::Continue,
        }
    }

    fn current_selection_rect(&self) -> Option<Rect> {
        let r = match self.state {
            OverlayState::Idle => return None,
            OverlayState::Dragging { start, end } => Rect::normalize(start, end),
            OverlayState::Adjusting { rect, .. } => rect,
        };
        let size = self.window.inner_size();
        let r = r.clamp_to((size.width, size.height));
        if r.w == 0 || r.h == 0 { None } else { Some(r) }
    }

    fn current_selection_rect_window(&self) -> Option<(u32, u32, u32, u32)> {
        self.current_selection_rect().map(|r| r.as_tuple_u32())
    }

    fn window_point_to_frame_point(&self, p: (i32, i32)) -> (i32, i32) {
        let size = self.window.inner_size();
        let (ww, wh) = (size.width.max(1) as i64, size.height.max(1) as i64);
        let (fw, fh) = self.frame.dimensions();
        let x = (p.0 as i64 * fw as i64 / ww) as i32;
        let y = (p.1 as i64 * fh as i64 / wh) as i32;
        (x, y)
    }

    /// Produce the final cropped + annotated RGBA image for export to clipboard / file.
    pub fn flatten_for_export(&mut self, rect: Rect) -> image::RgbaImage {
        let frame_rect = self.window_rect_to_frame_rect(rect);
        let mut cropped = crate::crop::crop_rgba(&self.frame, frame_rect);
        let offset = (frame_rect.0 as i32, frame_rect.1 as i32);
        // Snapshot the history so we can mutably borrow `self.font` while
        // iterating. Cloning here is fine — flatten_for_export runs once at
        // export time, not on the redraw hot path.
        let history_clone: Vec<crate::overlay::annotate::Annotation> =
            self.history.current().to_vec();
        for ann in &history_clone {
            annotate_render::paint_on_cropped(&mut cropped, ann, offset, &mut self.font);
        }
        cropped
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

        let sel_tuple = self.current_selection_rect_window();
        let sel_rect = self.current_selection_rect();
        let show_label = matches!(
            self.state,
            OverlayState::Dragging { .. } | OverlayState::Adjusting { .. }
        );
        let show_magnifier = matches!(
            self.state,
            OverlayState::Idle | OverlayState::Dragging { .. }
        );
        let frame_size = self.frame.dimensions();
        let window_size = (w, h);
        let cursor = self.cursor;
        let frame_ref = &self.frame;
        let font = &mut self.font;
        let scale = self.scale_factor;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        render::draw_background(&mut buf, w, h, frame_ref);
        render::apply_dim(&mut buf, w, h, sel_tuple);
        if let Some(r) = sel_tuple {
            render::draw_selection_outline(&mut buf, w, h, r, 0x00FFFFFF);
        }
        if let OverlayState::Adjusting { .. } = self.state {
            // Annotations first (committed history, then in-flight pending).
            for ann in self.history.current() {
                annotate_render::draw_annotation_on_buf(
                    &mut buf,
                    w,
                    h,
                    frame_ref,
                    ann,
                    font,
                );
            }
            if let Some(pending) = self.pending_draw.as_ref() {
                annotate_render::draw_pending_on_buf(
                    &mut buf,
                    w,
                    h,
                    frame_ref,
                    pending,
                    font,
                );
            }

            // Anchors only when Move tool is active
            if self.tool == annotate::Tool::Move {
                if let Some(r) = sel_rect {
                    render::draw_anchors(&mut buf, w, h, r);
                }
            }

            // Toolbar
            if let Some(sel_win) = sel_rect {
                let tb = toolbar::Toolbar::layout(sel_win, (w, h));
                toolbar::draw_toolbar(
                    &mut buf,
                    w,
                    h,
                    &tb,
                    self.tool,
                    self.history.can_undo(),
                    self.history.can_redo(),
                );
            }
        }
        if show_magnifier {
            render::draw_magnifier(
                &mut buf,
                w,
                h,
                frame_ref,
                cursor,
                window_size,
                font,
                scale,
            );
        }
        if show_label {
            if let Some(r) = sel_rect {
                render::draw_size_label(
                    &mut buf,
                    w,
                    h,
                    r,
                    frame_size,
                    window_size,
                    font,
                    scale,
                );
            }
        }

        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }

    /// Request a redraw at most once per ~16 ms (~60 Hz). Missed requests are
    /// dropped rather than queued — the state update that preceded the call
    /// has already landed, so the next un-throttled redraw will paint the
    /// latest state. Winit additionally coalesces concurrent request_redraw
    /// calls within a single frame, so no visible frame is lost.
    fn request_redraw_throttled(&mut self) {
        let now = std::time::Instant::now();
        let should = self.last_redraw.is_none_or(|t| {
            now.duration_since(t) >= std::time::Duration::from_millis(16)
        });
        if should {
            self.last_redraw = Some(now);
            self.window.request_redraw();
        }
    }

}

/// Resolve the NSWindow* backing a winit Window via [nsView window].
#[cfg(target_os = "macos")]
fn ns_window_of(window: &Window) -> Option<*mut std::ffi::c_void> {
    use winit::raw_window_handle::HasWindowHandle;
    let handle = window.window_handle().ok()?;
    let winit::raw_window_handle::RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let ns_view = appkit.ns_view.as_ptr();
    let ns_window = unsafe {
        crate::macos_objc::msg_send_id(ns_view, crate::macos_objc::sel(c"window"))
    };
    if ns_window.is_null() { None } else { Some(ns_window) }
}

/// Promote the NSWindow to key-and-main so it receives keyboard events.
/// Without this, a borderless high-level window doesn't auto-focus on creation
/// and ESC/Enter before the first click are dropped.
#[cfg(target_os = "macos")]
fn make_macos_key_window(window: &Window) {
    use crate::macos_objc::{class, msg_send_id, msg_send_set_bool, msg_send_set_id, sel};
    unsafe {
        // Activate the app so it becomes frontmost. CLI daemons start with
        // NSApp inactive, which means key window doesn't receive global
        // keyboard events. activateIgnoringOtherApps:YES bypasses the usual
        // "don't steal focus" behavior — appropriate for a user-triggered
        // screenshot overlay.
        let ns_app_class = class(c"NSApplication");
        let ns_app = msg_send_id(ns_app_class, sel(c"sharedApplication"));
        if !ns_app.is_null() {
            // BOOL is signed char; YES = 1.
            msg_send_set_bool(ns_app, sel(c"activateIgnoringOtherApps:"), 1);
        }

        let Some(ns_window) = ns_window_of(window) else { return };
        // makeKeyAndOrderFront: takes an id sender; nil is fine.
        msg_send_set_id(ns_window, sel(c"makeKeyAndOrderFront:"), std::ptr::null_mut());
    }
}

/// Set the NSWindow level via NSWindow.setLevel:.
/// 1000 = kCGScreenSaverWindowLevel, 1500 = kCGAssistiveTechHighWindowLevel.
#[cfg(target_os = "macos")]
fn set_macos_window_level(window: &Window, level: i64) {
    use crate::macos_objc::{msg_send_set_int, sel};
    let Some(ns_window) = ns_window_of(window) else { return };
    unsafe { msg_send_set_int(ns_window, sel(c"setLevel:"), level) }
}

/// Set NSWindow.animationBehavior = NSWindowAnimationBehaviorNone (1) so the
/// window does not fade in / zoom on appear. Without this, every capture
/// flashes a brief NSWindow-default appearance animation now that the overlay
/// actually sits at the top of the z-stack.
#[cfg(target_os = "macos")]
fn set_macos_window_animation_none(window: &Window) {
    use crate::macos_objc::{msg_send_set_int, sel};
    let Some(ns_window) = ns_window_of(window) else { return };
    // NSWindowAnimationBehaviorNone = 1 (NSWindowAnimationBehavior is NSInteger).
    unsafe { msg_send_set_int(ns_window, sel(c"setAnimationBehavior:"), 1) }
}

/// Configure NSWindow.collectionBehavior for overlay use:
///   canJoinAllSpaces (1) | stationary (16) | fullScreenAuxiliary (256) = 273
/// Without this, macOS can demote a borderless accessory-app window to the
/// desktop layer regardless of setLevel:.
#[cfg(target_os = "macos")]
fn set_macos_overlay_collection_behavior(window: &Window) {
    use crate::macos_objc::{msg_send_set_uint, sel};
    const BEHAVIOR: u64 = 1 | 16 | 256;
    let Some(ns_window) = ns_window_of(window) else { return };
    unsafe { msg_send_set_uint(ns_window, sel(c"setCollectionBehavior:"), BEHAVIOR) }
}

/// Read back NSWindow.level/collectionBehavior so regressions are visible in
/// stderr without attaching a debugger.
#[cfg(target_os = "macos")]
fn log_macos_window_level(window: &Window) {
    use crate::macos_objc::{msg_send_int, msg_send_uint, sel};
    let Some(ns_window) = ns_window_of(window) else { return };
    unsafe {
        let level = msg_send_int(ns_window, sel(c"level"));
        let behavior = msg_send_uint(ns_window, sel(c"collectionBehavior"));
        eprintln!(
            "quickshot: overlay NSWindow level={level} collectionBehavior={behavior:#x}"
        );
    }
}
