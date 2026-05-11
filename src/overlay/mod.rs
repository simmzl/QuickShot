pub(crate) mod annotate;
pub(crate) mod annotate_render;
pub(crate) mod hit;
pub(crate) mod render;
pub(crate) mod snap;
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
    Pinned(Rect),
    SaveAs(Rect),
    Cancelled,
}

pub(crate) struct TextEdit {
    pub origin_frame: (i32, i32),
    pub buffer: String,
    /// In-flight IME preedit (pinyin / candidate composition string). Owned by
    /// the IME — we never mutate this from key handling, only from
    /// `WindowEvent::Ime`. Rendered alongside `buffer` so the user sees what
    /// they're typing before they confirm a candidate.
    pub preedit: String,
    pub last_blink: std::time::Instant,
    pub cursor_visible: bool,
}

impl TextEdit {
    pub fn new(origin_frame: (i32, i32)) -> Self {
        Self {
            origin_frame,
            buffer: String::new(),
            preedit: String::new(),
            last_blink: std::time::Instant::now(),
            cursor_visible: true,
        }
    }

    /// Append a printable character to the buffer. Returns true if the buffer
    /// changed (so the caller can request a redraw).
    /// Backspace / Delete / control characters are ignored here — the caller
    /// dispatches Named keys via `backspace()` / explicit Enter handling.
    pub fn handle_char(&mut self, ch: char) -> bool {
        if ch.is_control() && ch != '\n' {
            return false;
        }
        self.buffer.push(ch);
        true
    }

    /// Pop the last character (if any). Returns true if something was popped.
    pub fn backspace(&mut self) -> bool {
        self.buffer.pop().is_some()
    }
}

pub(crate) fn key_to_color(c: char) -> Option<annotate::Color> {
    match c {
        '1' => Some(annotate::Color::Red),
        '2' => Some(annotate::Color::Yellow),
        '3' => Some(annotate::Color::Green),
        '4' => Some(annotate::Color::Blue),
        _ => None,
    }
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
    pub(crate) current_style: AnnotationStyle,
    pub(crate) text_edit: Option<TextEdit>,
    modifiers: ModifiersState,
    /// Whether per-button keyboard hint badges are currently shown on the
    /// toolbar. Toggled by `h` while in Adjusting state, dismissed by Esc or
    /// any mouse click.
    pub(crate) show_hints: bool,
    // Iter 6a: smart window snapping state.
    pub(crate) snap_target: Option<Rect>,
    pub(crate) press_pos: Option<(i32, i32)>,
    pub(crate) snap_at_press: Option<Rect>,
    window_list: Vec<snap::WindowEntry>,
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
            // canBecomeKeyWindow must return YES for ESC/Enter to be delivered
            // before any mouse click. Apply once per process; harmless to call
            // again on subsequent overlay opens.
            ensure_nswindow_can_become_key();
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
        // Opt into IME events so winit forwards `WindowEvent::Ime` for CJK /
        // dead-key composition. Without this, only `Key::Character` fires and
        // CJK input from the OS-level IME is silently dropped.
        window.set_ime_allowed(true);
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
            current_style: AnnotationStyle::default(),
            text_edit: None,
            modifiers: ModifiersState::default(),
            show_hints: false,
            // Iter 6a: enumerate windows once at overlay creation. Captures
            // are sub-second so window topology is effectively frozen for
            // the duration; no need to refresh.
            snap_target: None,
            press_pos: None,
            snap_at_press: None,
            window_list: snap::enumerate_windows(monitor_geom, std::process::id(), scale_factor),
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
                        if let Some(start) = self.press_pos {
                            // Mouse held — check drag threshold to decide
                            // click-to-snap vs drag-to-rect.
                            if snap::drag_threshold_exceeded(start, self.cursor) {
                                // Promote: use press position as the rect's start
                                // so the user's click point anchors the drag.
                                self.state = OverlayState::Dragging {
                                    start,
                                    end: self.cursor,
                                };
                                self.snap_target = None;
                                self.press_pos = None;
                                self.snap_at_press = None;
                                self.window.request_redraw();
                            }
                        } else {
                            // Hover-snap: recompute snap_target from cursor.
                            let new_target = snap::window_under_cursor(
                                self.cursor,
                                &self.window_list,
                            );
                            if new_target != self.snap_target {
                                self.snap_target = new_target;
                                self.window.request_redraw();
                            }
                            // Magnifier tick — preserves Iter 2a behavior.
                            self.request_redraw_throttled();
                        }
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
            WindowEvent::Ime(ime_event) => {
                use winit::event::Ime;
                if let Some(t) = self.text_edit.as_mut() {
                    match ime_event {
                        Ime::Commit(s) => {
                            // IME confirmed candidate(s) — append to buffer and
                            // clear preedit. winit guarantees Commit and
                            // Key::Character don't both fire for the same input.
                            t.buffer.push_str(&s);
                            t.preedit.clear();
                            self.window.request_redraw();
                        }
                        Ime::Preedit(s, _cursor) => {
                            // In-flight composition (pinyin etc). Render alongside
                            // buffer so the user sees what they're typing.
                            t.preedit = s;
                            self.window.request_redraw();
                        }
                        Ime::Enabled | Ime::Disabled => {
                            // Informational — nothing to do.
                        }
                    }
                }
                Outcome::Continue
            }
            WindowEvent::CursorLeft { .. } => {
                if self.snap_target.is_some() {
                    self.snap_target = None;
                    self.window.request_redraw();
                }
                Outcome::Continue
            }
            WindowEvent::CloseRequested => Outcome::Cancelled,
            _ => Outcome::Continue,
        }
    }

    /// Commit any in-flight TextEdit to history and clear the slot.
    ///
    /// Centralizes the take-and-push-if-nonempty pattern used by Enter,
    /// click-restart-in-rect, click-outside-rect, and double-click-confirm.
    /// Forgetting one of those sites silently drops the user's typed buffer,
    /// hence the helper.
    fn commit_text_edit(&mut self) {
        if let Some(t) = self.text_edit.take() {
            if !t.buffer.is_empty() {
                self.history.push(annotate::Annotation::Text {
                    origin: t.origin_frame,
                    content: t.buffer,
                    style: self.current_style,
                });
            }
        }
    }

    fn handle_left_press(&mut self) -> Outcome {
        // If hint badges are up, the click only dismisses them — don't
        // forward to any other handler so the user doesn't accidentally start
        // a drag/draw while reading shortcuts.
        if self.show_hints {
            self.show_hints = false;
            self.window.request_redraw();
            return Outcome::Continue;
        }

        // Detect double-click: two presses within 400 ms (position-agnostic).
        let now = std::time::Instant::now();
        let is_double_click = matches!(
            self.last_click,
            Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
        );
        self.last_click = Some(now); // running window: each click resets the 400 ms gate

        match self.state {
            OverlayState::Idle => {
                // Defer state transition until release. If the cursor moves
                // ≥ 4 px before release, CursorMoved promotes to Dragging
                // (Task 6). If release happens with no movement, we treat it
                // as a click — commit snap_target to Adjusting (handle_left_release).
                self.press_pos = Some(self.cursor);
                self.snap_at_press = self.snap_target;
                Outcome::Continue
            }
            OverlayState::Dragging { .. } => Outcome::Continue,
            OverlayState::Adjusting { rect, .. } => {
                if is_double_click {
                    match state::on_double_click_adjusting(rect, self.cursor) {
                        Transition::Confirm(r) => {
                            // Commit any in-flight TextEdit before confirming the
                            // overlay so we don't silently lose the user's text.
                            self.commit_text_edit();
                            return Outcome::Confirmed(r);
                        }
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
                    toolbar::ToolbarHit::Color(c) => {
                        self.current_style.color = c;
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Stroke(s) => {
                        self.current_style.stroke = s;
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Pin => {
                        // Commit any in-flight TextEdit so its content gets
                        // flattened into the pinned image (same pattern as
                        // double-click → Confirmed).
                        self.commit_text_edit();
                        return Outcome::Pinned(rect);
                    }
                    toolbar::ToolbarHit::Save => {
                        // Commit in-flight TextEdit so its content gets
                        // flattened into the saved PNG.
                        self.commit_text_edit();
                        return Outcome::SaveAs(rect);
                    }
                    toolbar::ToolbarHit::None => {}
                }

                // Tool::Text: clicks always commit any in-flight TextEdit
                // (regardless of whether the click is inside or outside the
                // rect — outside-click clears, inside-click restarts at the
                // new point). Goes BEFORE the is_drawing() check so Text-tool
                // clicks don't fall into the Pen/Shape branch. Toolbar hit
                // check stays earlier so toolbar clicks still work when Text
                // tool is active.
                if self.tool == annotate::Tool::Text {
                    self.commit_text_edit();
                    if rect.contains(self.cursor) {
                        let fp = self.window_point_to_frame_point(self.cursor);
                        self.text_edit = Some(TextEdit::new(fp));
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    // Click outside rect: text_edit is now committed/cleared.
                    // Fall through to the Move-tool arm below for the existing
                    // anchor / outside-click-clear behavior.
                }

                // Hit-test the cursor against the selection rect first. The
                // anchor zone (border) is a universal resize affordance — works
                // regardless of the active tool. Inside-the-rect dispatches by
                // tool (draw vs. translate). Outside is inert: only Esc and
                // double-click exit the overlay.
                match hit::classify(self.cursor, rect) {
                    hit::HitZone::Anchor(a) => {
                        self.state = state::start_resize(rect, a, self.cursor);
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    hit::HitZone::Inside => {
                        if self.tool.is_drawing() {
                            // Drawing tool inside rect → start PendingDraw.
                            let fp = self.window_point_to_frame_point(self.cursor);
                            self.pending_draw = Some(match self.tool {
                                annotate::Tool::Pen => PendingDraw::pen(self.current_style, fp),
                                _                   => PendingDraw::shape(
                                    self.tool, self.current_style, fp, fp,
                                ),
                            });
                            self.window.request_redraw();
                            return Outcome::Continue;
                        }
                        // Move tool inside rect → start translate.
                        self.state = state::start_translate(rect, self.cursor);
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    hit::HitZone::Outside => {
                        // No-op. Stay in Adjusting; only Esc / double-click exit.
                        return Outcome::Continue;
                    }
                }
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

        // Snap-click commit: if a press happened in Idle and CursorMoved never
        // promoted to Dragging (drag threshold not exceeded), treat this as a
        // click. Snap to the window the cursor was over at press time.
        if self.press_pos.take().is_some() {
            if let Some(rect) = self.snap_at_press.take() {
                self.state = OverlayState::Adjusting { rect, edit: None };
                self.snap_target = None;  // clear stale hover highlight
                self.window.request_redraw();
            }
            // Else: clicked on dim area (no window under cursor at press time)
            // — stay Idle, no transition.
            return Outcome::Continue;
        }

        match self.state {
            OverlayState::Dragging { start, end } => {
                let rect = Rect::normalize(start, end);
                if rect.w > 0 && rect.h > 0 {
                    self.state = OverlayState::Adjusting { rect, edit: None };
                } else {
                    self.state = OverlayState::Idle;
                    // Snap state could have lingered if Dragging was promoted
                    // mid-press; clear so hover-snap restarts cleanly.
                    self.snap_target = None;
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
        // While composing text, all keys route through TextEdit. Tool/color/stroke
        // shortcuts and undo/redo are suppressed — only Enter (commit), Esc
        // (discard text only — does NOT cancel the overlay; user must Esc twice),
        // Shift+Enter (newline), Backspace, and printable chars apply.
        if self.text_edit.is_some() {
            match &key {
                Key::Named(NamedKey::Enter) => {
                    if self.modifiers.shift_key() {
                        if let Some(t) = self.text_edit.as_mut() {
                            t.handle_char('\n');
                        }
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    // Plain Enter commits.
                    self.commit_text_edit();
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                Key::Named(NamedKey::Escape) => {
                    self.text_edit = None;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                Key::Named(NamedKey::Backspace) => {
                    if let Some(t) = self.text_edit.as_mut() {
                        t.backspace();
                    }
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                Key::Character(s) => {
                    // Stroke (font-size) shortcuts stay live during composition so
                    // the user can adjust text size mid-edit. Other shortcuts
                    // (tool, color, undo) remain suppressed — only [/] are special.
                    let ch = s.chars().next().unwrap_or('\0');
                    if ch == '[' || ch == ']' {
                        self.current_style.stroke = if ch == '[' {
                            self.current_style.stroke.step_down()
                        } else {
                            self.current_style.stroke.step_up()
                        };
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    if let Some(t) = self.text_edit.as_mut() {
                        for ch in s.chars() {
                            t.handle_char(ch);
                        }
                    }
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                _ => return Outcome::Continue,  // suppress all other keys while composing
            }
        }

        // Hint-badge toggle: only fires in Adjusting (the toolbar exists then).
        // The TextEdit-priority block above already returned, so this won't
        // intercept `h` while the user is composing text. `h` is only honored
        // when no super key is held so Cmd+H (hide app) remains forwardable to
        // the OS if ever wired.
        if matches!(self.state, OverlayState::Adjusting { .. })
            && self.text_edit.is_none()
            && !self.modifiers.super_key()
        {
            if let Key::Character(s) = &key {
                let ch = s.chars().next().unwrap_or('\0').to_ascii_lowercase();
                if ch == 'h' {
                    self.show_hints = !self.show_hints;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
            }
        }

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
                    'p' => Some(annotate::Tool::Pen),
                    't' => Some(annotate::Tool::Text),
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

                if let Some(c) = key_to_color(ch) {
                    self.current_style.color = c;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                if ch == '[' {
                    self.current_style.stroke = self.current_style.stroke.step_down();
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                if ch == ']' {
                    self.current_style.stroke = self.current_style.stroke.step_up();
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
            }
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                if self.show_hints {
                    self.show_hints = false;
                    self.window.request_redraw();
                    return Outcome::Continue;
                }
                match state::on_escape(self.state) {
                    Transition::Cancel => Outcome::Cancelled,
                    _ => Outcome::Continue,
                }
            }
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

    pub(crate) fn scale_factor(&self) -> f32 {
        self.scale_factor
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
        // Snap preview: when Idle and hovering a window, render that window's
        // bounds the same way as a confirmed selection but with a blue outline.
        let (sel_rect, sel_tuple, outline_color) = match (sel_rect, &self.state, self.snap_target) {
            (None, OverlayState::Idle, Some(snap)) => {
                let clamped = snap.clamp_to((w, h));
                if clamped.w == 0 || clamped.h == 0 {
                    (None, None, 0x00FFFFFF)
                } else {
                    let tup = clamped.as_tuple_u32();
                    (Some(clamped), Some(tup), 0x00007AFF) // Color::Blue
                }
            }
            (existing, _, _) => (existing, sel_tuple, 0x00FFFFFF),
        };
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
            render::draw_selection_outline(&mut buf, w, h, r, outline_color);
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

            // Live TextEdit: blink toggle pass (mut), then render pass (immut).
            // Two passes avoid a double-mutable-borrow on `self` between
            // `self.text_edit` and `font` (which is `&mut self.font`).
            if let Some(t) = self.text_edit.as_mut() {
                let now = std::time::Instant::now();
                if now.duration_since(t.last_blink) >= std::time::Duration::from_millis(530) {
                    t.cursor_visible = !t.cursor_visible;
                    t.last_blink = now;
                }
            }
            if let Some(t) = self.text_edit.as_ref() {
                // Display = committed buffer + in-flight IME preedit + caret.
                // Preedit is rendered inline so the user can see CJK composition
                // (pinyin, candidates) before they confirm.
                let mut display = String::with_capacity(
                    t.buffer.len() + t.preedit.len() + 1,
                );
                display.push_str(&t.buffer);
                display.push_str(&t.preedit);
                if t.cursor_visible {
                    display.push('|');
                }
                let origin = t.origin_frame;
                annotate_render::draw_text_on_buf(
                    &mut buf, w, h, frame_ref, origin, &display,
                    self.current_style, font,
                );
                // Position the IME candidate window roughly under the caret so
                // macOS doesn't park it at an arbitrary corner. We give the
                // bottom-left of the first text line: origin_frame mapped to
                // window-space, offset down by ~one line height. Approximate
                // sizing is fine — system IME treats this as a hint.
                let frame_size = frame_ref.dimensions();
                let window_size = (w, h);
                let scale_y = window_size.1 as f32 / frame_size.1.max(1) as f32;
                let line_h = (self.current_style.stroke.font_px() * scale_y)
                    .round()
                    .max(1.0) as i32;
                let (cx, cy) = annotate_render::frame_to_window(
                    origin, frame_size, window_size,
                );
                self.window.set_ime_cursor_area(
                    winit::dpi::Position::Physical(
                        winit::dpi::PhysicalPosition::new(cx, cy + line_h),
                    ),
                    winit::dpi::Size::Physical(
                        winit::dpi::PhysicalSize::new(1, 1),
                    ),
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
                    self.current_style,
                    self.history.can_undo(),
                    self.history.can_redo(),
                    self.show_hints,
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

/// Force NSWindow.canBecomeKeyWindow to return YES so the overlay's borderless
/// window can actually receive keyboard events before the first mouse click.
/// Without this, ESC / Enter typed in Idle state are silently dropped: by
/// AppKit default a window with NSWindowStyleMaskBorderless returns NO from
/// canBecomeKeyWindow, so makeKeyAndOrderFront: is silently a no-op until a
/// click hit-tests up the first responder.
///
/// We use Objective-C class_replaceMethod to swap the implementation
/// process-wide. Side-effect is bounded: the overlay is the only NSWindow in
/// this process (tray-icon uses NSStatusBar, not NSWindow).
///
/// Idempotent via OnceLock — only swizzles once per process even if the
/// overlay is created multiple times.
#[cfg(target_os = "macos")]
fn ensure_nswindow_can_become_key() {
    use std::sync::OnceLock;
    static APPLIED: OnceLock<()> = OnceLock::new();
    APPLIED.get_or_init(|| {
        unsafe extern "C" fn returns_yes(
            _self: *mut std::ffi::c_void,
            _sel: *mut std::ffi::c_void,
        ) -> i8 {
            1 // YES (Objective-C BOOL is signed char on Apple ABIs).
        }
        unsafe {
            let ns_window_class =
                crate::macos_objc::objc_getClass(c"NSWindow".as_ptr().cast());
            if ns_window_class.is_null() {
                eprintln!(
                    "quickshot: NSWindow class not found; can't swizzle canBecomeKeyWindow"
                );
                return;
            }
            let sel =
                crate::macos_objc::sel_registerName(c"canBecomeKeyWindow".as_ptr().cast());
            // Type encoding: "c@:" = returns signed-char (BOOL), self id, SEL.
            // Modern runtimes also accept "B@:" (explicit BOOL), but historic
            // BOOL = signed char and "c" is the canonical encoding on 64-bit.
            let imp_ptr = returns_yes as *const std::ffi::c_void;
            crate::macos_objc::class_replaceMethod(
                ns_window_class,
                sel,
                imp_ptr,
                c"c@:".as_ptr().cast(),
            );
            eprintln!("quickshot: swizzled NSWindow.canBecomeKeyWindow -> YES");
        }
    });
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
pub(crate) fn set_macos_window_level(window: &Window, level: i64) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use annotate::Color;

    #[test]
    fn key_to_color_mapping() {
        assert_eq!(key_to_color('1'), Some(Color::Red));
        assert_eq!(key_to_color('2'), Some(Color::Yellow));
        assert_eq!(key_to_color('3'), Some(Color::Green));
        assert_eq!(key_to_color('4'), Some(Color::Blue));
        assert_eq!(key_to_color('5'), None);
        assert_eq!(key_to_color('a'), None);
    }

    #[test]
    fn text_edit_appends_printable() {
        let mut t = TextEdit::new((10, 10));
        t.handle_char('h');
        t.handle_char('i');
        assert_eq!(t.buffer, "hi");
    }

    #[test]
    fn text_edit_backspace() {
        let mut t = TextEdit::new((0, 0));
        t.buffer.push_str("abc");
        assert!(t.backspace());
        assert_eq!(t.buffer, "ab");
        t.buffer.clear();
        assert!(!t.backspace());
    }

    #[test]
    fn text_edit_newline() {
        let mut t = TextEdit::new((0, 0));
        t.handle_char('a');
        t.handle_char('\n');
        t.handle_char('b');
        assert_eq!(t.buffer, "a\nb");
    }

    #[test]
    fn text_edit_can_be_committed_via_helper() {
        // Smoke: commit-take pattern preserves buffer to history.
        // Replicates the take + push_if_nonempty body of `commit_text_edit`
        // without requiring a full Overlay (which needs a Window).
        let mut h = annotate::History::new();
        let style = annotate::AnnotationStyle::default();
        let t = TextEdit {
            origin_frame: (5, 7),
            buffer: "wow".to_string(),
            preedit: String::new(),
            last_blink: std::time::Instant::now(),
            cursor_visible: true,
        };
        if !t.buffer.is_empty() {
            h.push(annotate::Annotation::Text {
                origin: t.origin_frame,
                content: t.buffer,
                style,
            });
        }
        assert_eq!(h.current().len(), 1);
    }
}
