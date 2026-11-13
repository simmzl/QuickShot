pub(crate) mod annotate;
pub(crate) mod annotate_render;
pub(crate) mod hit;
pub(crate) mod present;
pub(crate) mod render;
pub(crate) mod snap;
pub(crate) mod state;
pub(crate) mod toolbar;

use anyhow::{Context, Result};
use image::RgbaImage;
use std::rc::Rc;
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::keyboard::{Key, NamedKey, ModifiersState};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use crate::capture::{Captured, MonitorGeom};
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

/// In-flight text annotation, edited through a native OS text control
/// (see `crate::native_text`). Typing, IME, selection, caret movement and
/// copy/paste are all handled by the OS; we only own placement and commit.
pub(crate) struct TextEdit {
    pub origin_frame: (i32, i32),
    pub editor: crate::native_text::NativeTextEditor,
}

/// State for an in-progress annotation drag (Move tool). When a press hits an
/// existing annotation, we lift it out of history into this slot; redraw paints
/// the dragged copy at the current cursor delta; release pushes the translated
/// version back into history.
pub(crate) struct AnnotationDrag {
    /// Original index in `history.undo_stack` — reserved for a future
    /// `History::insert_at(idx, ann)` that preserves z-order on restore.
    /// Currently unused (Esc-restore appends to the end of the stack).
    #[allow(dead_code)]
    pub index: usize,
    pub original: crate::overlay::annotate::Annotation,
    pub press_frame: (i32, i32),
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

/// The overlay's OS window + softbuffer surface. Created once (hidden) at
/// startup and reused for every capture: showing an existing window is far
/// cheaper than creating an NSWindow / HWND + CALayer surface per hotkey
/// press, and level / collection behavior / animation flags only need to be
/// configured once.
pub struct OverlayHost {
    pub window: Rc<Window>,
    presenter: present::Presenter,
    /// Geometry the window is currently placed at (avoid redundant moves).
    placed: Option<MonitorGeom>,
    /// App that was frontmost when the overlay opened (macOS). Re-activated
    /// on hide so focus goes back to it instead of to whatever app macOS
    /// picks next (often not the one the user was in).
    #[cfg(target_os = "macos")]
    previous_app: std::cell::RefCell<Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>>,
}

impl OverlayHost {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self> {
        let attrs = WindowAttributes::default()
            .with_title("QuickShot overlay")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(winit::dpi::LogicalSize::new(100.0, 100.0));
        let win = event_loop.create_window(attrs).context("create window")?;
        #[cfg(target_os = "macos")]
        {
            // Animation behavior must be applied FIRST so subsequent
            // setLevel: / setCollectionBehavior: / makeKeyAndOrderFront: don't
            // trigger macOS's default appear/zoom animation.
            set_macos_window_animation_none(&win);
            // macOS 26 (Tahoe): without an explicit collectionBehavior a
            // borderless accessory-app overlay can land at the desktop layer.
            set_macos_overlay_collection_behavior(&win);
            // kCGAssistiveTechHighWindowLevel = 1500.
            set_macos_window_level(&win, 1500);
            // canBecomeKeyWindow must return YES for ESC/Enter to be delivered
            // before any mouse click.
            ensure_nswindow_can_become_key();
            log_macos_window_level(&win);
        }
        let window = Rc::new(win);
        // Opt into IME events (harmless; text entry itself goes through the
        // native editor control).
        window.set_ime_allowed(true);
        let presenter = present::Presenter::new(window.clone())?;
        Ok(Self {
            window,
            presenter,
            placed: None,
            #[cfg(target_os = "macos")]
            previous_app: std::cell::RefCell::new(None),
        })
    }

    /// Move/resize the hidden window onto `geom`'s monitor.
    fn place(&mut self, geom: &MonitorGeom) {
        if self.placed.as_ref() == Some(geom) {
            return;
        }
        #[cfg(target_os = "macos")]
        set_macos_window_frame(&self.window, geom);
        #[cfg(not(target_os = "macos"))]
        {
            self.window
                .set_outer_position(winit::dpi::PhysicalPosition::new(geom.x, geom.y));
            let _ = self
                .window
                .request_inner_size(winit::dpi::PhysicalSize::new(geom.width, geom.height));
        }
        self.placed = Some(*geom);
    }

    fn show(&self) {
        #[cfg(target_os = "macos")]
        {
            // Remember who had focus before we take it.
            let front = objc2_app_kit::NSWorkspace::sharedWorkspace().frontmostApplication();
            let me = std::process::id() as i32;
            *self.previous_app.borrow_mut() = front.filter(|a| a.processIdentifier() != me);
            make_macos_key_window(&self.window);
        }
        #[cfg(not(target_os = "macos"))]
        {
            #[cfg(windows)]
            self.window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);
            self.window.set_visible(true);
            self.window.focus_window();
        }
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
        // Promote the next capture anew, above pins created between sessions.
        #[cfg(windows)]
        self.window.set_window_level(winit::window::WindowLevel::Normal);
        #[cfg(target_os = "macos")]
        restore_previous_app(self.previous_app.borrow_mut().take());
    }
}

pub struct Overlay {
    pub window: Rc<Window>,
    /// The persistent host (window + surface); handed back to `App` when the
    /// session ends via [`Overlay::into_host`].
    host: OverlayHost,
    pub frame: RgbaImage,
    /// Precomputed fully-dimmed background (swizzled 0x00RRGGBB, channels
    /// halved). The frame is fixed for the overlay's lifetime, so we build this
    /// once and memcpy it every redraw instead of re-dimming ~8M px per frame.
    dimmed_bg: Vec<u32>,
    /// Dimmed background with the selection interior re-brightened, for
    /// the selection rect in `.0`. While the user draws annotations the
    /// selection is fixed, so each frame starts from one parallel memcpy of
    /// this instead of re-swizzling the selection from the RGBA frame.
    composed_bg: Option<(Option<(u32, u32, u32, u32)>, Vec<u32>)>,
    /// Memoized per-block averages for committed mosaic annotations, so we
    /// don't re-sum the source pixels of every mosaic on each redraw.
    mosaic_cache: annotate_render::MosaicCache,
    pub(crate) state: OverlayState,
    pub(crate) cursor: (i32, i32),
    last_click: Option<std::time::Instant>,
    last_redraw: Option<std::time::Instant>,
    font: crate::text::Font,
    icon_font: crate::icon_font::IconFont,
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
    #[cfg(windows)]
    elements: snap::ElementCache,
    /// Active annotation drag (Move tool): when present, history excludes
    /// the dragged annotation and redraw paints it at the cursor's current
    /// delta from `press_frame`.
    pub(crate) annotation_drag: Option<AnnotationDrag>,
    /// Annotation under the cursor while Move tool is active and no drag is
    /// in progress. Drives the faint hover-outline in `redraw`.
    pub(crate) hovered_annotation_idx: Option<usize>,
    /// Toolbar button under the cursor (drives hover highlight).
    toolbar_hover: toolbar::ToolbarHit,
    toolbar_origin: Option<(i32, i32)>,
    grip_press_offset: Option<(i32, i32)>,
    /// When the C shortcut last copied a color (shows a "Copied" badge).
    color_copied_at: Option<std::time::Instant>,
    /// Used by the native text editor to wake the event loop when it
    /// consumes Return / Escape itself.
    proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>,
}

impl Overlay {
    /// Start a capture session on the reusable `host`. `window_list` is the
    /// snap-target enumeration (computed in parallel with the capture).
    pub fn begin(
        mut host: OverlayHost,
        captured: Captured,
        monitor_geom: &MonitorGeom,
        window_list: Vec<snap::WindowEntry>,
        font: crate::text::Font,
        icon_font: crate::icon_font::IconFont,
        proxy: winit::event_loop::EventLoopProxy<crate::app::UserEvent>,
    ) -> Result<Self> {
        host.place(monitor_geom);
        let window = host.window.clone();
        let scale_factor = window.scale_factor() as f32;
        let Captured { frame, dimmed } = captured;

        let mut overlay = Self {
            window,
            host,
            frame,
            dimmed_bg: dimmed,
            composed_bg: None,
            mosaic_cache: annotate_render::MosaicCache::new(),
            state: OverlayState::Idle,
            cursor: (0, 0),
            last_click: None,
            last_redraw: None,
            font,
            icon_font,
            scale_factor,
            tool: annotate::Tool::Move,
            history: annotate::History::new(),
            pending_draw: None,
            current_style: AnnotationStyle::default(),
            text_edit: None,
            modifiers: ModifiersState::default(),
            show_hints: false,
            snap_target: None,
            press_pos: None,
            snap_at_press: None,
            window_list,
            #[cfg(windows)]
            elements: snap::ElementCache::new(monitor_geom),
            annotation_drag: None,
            hovered_annotation_idx: None,
            toolbar_hover: toolbar::ToolbarHit::None,
            toolbar_origin: None,
            grip_press_offset: None,
            color_copied_at: None,
            proxy,
        };

        // Seed the cursor so the magnifier / snap highlight are right on the
        // very first frame (before any CursorMoved arrives).
        overlay.cursor = overlay.initial_cursor(monitor_geom);
        overlay.snap_target = overlay.hover_target();

        // Paint the first frame while still hidden, then reveal — no flash of
        // stale content from the previous session.
        let t = std::time::Instant::now();
        if let Err(e) = overlay.redraw() {
            eprintln!("initial redraw error: {e:?}");
        }
        let t_draw = t.elapsed();
        overlay.host.show();
        if std::env::var_os("QUICKSHOT_DEBUG").is_some() {
            eprintln!(
                "QuickShot timing: first frame {:?}, show {:?}, window {:?}",
                t_draw,
                t.elapsed() - t_draw,
                overlay.window.inner_size()
            );
        }
        Ok(overlay)
    }

    /// End the session: hide the window and hand back the reusable host plus
    /// the label/icon fonts (so their glyph caches survive across sessions).
    pub fn into_parts(self) -> (OverlayHost, crate::text::Font, crate::icon_font::IconFont) {
        let Overlay { host, font, icon_font, text_edit, window, .. } = self;
        drop(text_edit); // detach native editor before hiding
        window.set_cursor(winit::window::CursorIcon::Default);
        host.hide();
        (host, font, icon_font)
    }

    fn hover_target(&mut self) -> Option<Rect> {
        #[cfg(windows)]
        { self.elements.target(self.cursor, &self.window_list, &self.proxy) }
        #[cfg(not(windows))]
        { snap::window_under_cursor(self.cursor, &self.window_list) }
    }

    fn initial_cursor(&self, geom: &MonitorGeom) -> (i32, i32) {
        match crate::capture::cursor_global() {
            Some((gx, gy)) => {
                #[cfg(target_os = "macos")]
                let (lx, ly) = (
                    (gx - geom.x as f64) * geom.scale,
                    (gy - geom.y as f64) * geom.scale,
                );
                #[cfg(not(target_os = "macos"))]
                let (lx, ly) = (gx - geom.x as f64, gy - geom.y as f64);
                (lx.round() as i32, ly.round() as i32)
            }
            None => (0, 0),
        }
    }

    /// Translate one winit WindowEvent into a state transition and return
    /// what app.rs should do next.
    pub fn handle_event(&mut self, event: WindowEvent) -> Outcome {
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = (position.x as i32, position.y as i32);

                #[cfg(windows)]
                if self.grip_press_offset.is_some() && !crate::windows_mouse::has_left_capture(&self.window) {
                    self.grip_press_offset = None;
                    // Fall through to normal hover handling at the new position.
                }
                if let Some((dx, dy)) = self.grip_press_offset {
                    self.toolbar_origin = Some((self.cursor.0 - dx, self.cursor.1 - dy));
                    self.window.request_redraw();
                    return Outcome::Continue;
                }

                // If a PendingDraw is in flight, update its endpoint.
                if self.pending_draw.is_some() {
                    let fp = self.window_point_to_frame_point(self.cursor);
                    if let Some(p) = self.pending_draw.as_mut() {
                        p.extend_to(fp);
                    }
                    self.request_redraw_throttled();
                    return Outcome::Continue;
                }

                // If an annotation drag is in flight, redraw — the dragged
                // annotation is painted at the cursor's current delta from
                // `press_frame` in `redraw`.
                if self.annotation_drag.is_some() {
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
                        // Toolbar hover: highlight + arrow cursor; skip the
                        // selection-edge cursor logic while over the bar.
                        let size = self.window.inner_size();
                        let tb = toolbar::Toolbar::layout(
                            rect,
                            (size.width, size.height),
                            self.scale_factor,
                            self.toolbar_origin,
                        );
                        let hover = tb.hit_with_tool(self.cursor, self.tool);
                        if hover != self.toolbar_hover {
                            self.toolbar_hover = hover;
                            self.window.request_redraw();
                        }
                        if tb.contains(self.cursor, self.tool) {
                            self.window.set_cursor(match hover {
                                toolbar::ToolbarHit::None => winit::window::CursorIcon::Default,
                                toolbar::ToolbarHit::Grip => winit::window::CursorIcon::Grab,
                                _ => winit::window::CursorIcon::Pointer,
                            });
                            if self.hovered_annotation_idx.take().is_some() {
                                self.window.request_redraw();
                            }
                            return Outcome::Continue;
                        }
                        let icon = hit::cursor_icon_for(hit::classify(self.cursor, rect));
                        self.window.set_cursor(icon);

                        // Move-tool hover: highlight the annotation under cursor.
                        // Skip while a drag is in progress (the drag state owns
                        // the highlight then).
                        if self.tool == annotate::Tool::Move
                            && self.annotation_drag.is_none()
                            && rect.contains(self.cursor)
                        {
                            let cursor_frame = self.window_point_to_frame_point(self.cursor);
                            // Disjoint field borrows: `history.current()` borrows
                            // `self.history`, `&mut self.font` borrows `self.font`
                            // — no clone of the annotation list needed per move.
                            let new_hover = annotate_render::annotation_at_cursor(
                                cursor_frame,
                                self.history.current(),
                                &mut self.font,
                            );
                            if new_hover != self.hovered_annotation_idx {
                                self.hovered_annotation_idx = new_hover;
                                self.request_redraw_throttled();
                            }
                        } else if self.hovered_annotation_idx.is_some() {
                            // Tool switched away from Move, or cursor outside
                            // rect — clear hover.
                            self.hovered_annotation_idx = None;
                            self.request_redraw_throttled();
                        }
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
                            let new_target = self.hover_target();
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
            #[cfg(windows)]
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key: Key::Named(NamedKey::Alt),
                    state: ElementState::Pressed,
                    repeat: false,
                    ..
                },
                is_synthetic: false,
                ..
            } if self.state == OverlayState::Idle && self.press_pos.is_none() => {
                self.elements.element_mode = !self.elements.element_mode;
                if self.snap_target.is_some() {
                    self.snap_target = self.hover_target();
                }
                self.window.request_redraw();
                Outcome::Continue
            }
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
                // A background element lookup may have completed under a stationary pointer.
                // Freeze the highlighted rectangle once the button has been pressed.
                #[cfg(windows)]
                if self.elements.element_mode && self.state == OverlayState::Idle
                    && self.press_pos.is_none() && self.snap_target.is_some() {
                    self.snap_target = self.hover_target();
                }
                if let Err(e) = self.redraw() {
                    eprintln!("redraw error: {e:?}");
                }
                Outcome::Continue
            }
            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
                Outcome::Continue
            }
            // Text entry (incl. IME composition) is owned by the native
            // editor control; winit's own IME events are irrelevant here.
            WindowEvent::Ime(_) => Outcome::Continue,
            #[cfg(windows)]
            WindowEvent::CursorEntered { .. } if self.state == OverlayState::Idle && self.press_pos.is_none() => {
                if let Some(cursor) = crate::windows_mouse::client_position(&self.window) {
                    self.cursor = cursor;
                    self.snap_target = self.hover_target();
                    self.window.request_redraw();
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
            WindowEvent::Focused(false) if self.grip_press_offset.take().is_some() => {
                self.toolbar_hover = toolbar::ToolbarHit::None;
                self.window.set_cursor(winit::window::CursorIcon::Default);
                self.window.request_redraw();
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
            let content = t.editor.text();
            let origin = t.origin_frame;
            // Dropping `t` detaches the native control and returns focus to
            // the overlay's own view.
            drop(t);
            if !content.trim().is_empty() {
                self.history.push(annotate::Annotation::Text {
                    origin,
                    content,
                    style: self.current_style,
                });
            }
            self.window.request_redraw();
        }
    }

    /// Open the native text editor at frame-space `origin_frame`.
    fn start_text_edit(&mut self, origin_frame: (i32, i32), initial: &str) {
        let pos = annotate_render::frame_to_window(
            origin_frame,
            self.frame.dimensions(),
            {
                let s = self.window.inner_size();
                (s.width, s.height)
            },
        );
        let proxy = self.proxy.clone();
        let wake: Box<dyn Fn()> = Box::new(move || {
            let _ = proxy.send_event(crate::app::UserEvent::TextEditorWake);
        });
        match crate::native_text::NativeTextEditor::new(
            &self.window,
            pos,
            initial,
            self.editor_style(),
            self.scale_factor as f64,
            wake,
        ) {
            Some(editor) => {
                self.text_edit = Some(TextEdit { origin_frame, editor });
            }
            None => eprintln!("text: could not create native editor"),
        }
        self.window.request_redraw();
    }

    /// Editor font/color in window-physical px (frame px scaled to window).
    fn editor_style(&self) -> crate::native_text::EditorStyle {
        let fh = self.frame.height().max(1) as f32;
        let wh = self.window.inner_size().height.max(1) as f32;
        crate::native_text::EditorStyle {
            font_px: self.current_style.stroke.font_px() * wh / fh,
            color_rgb: self.current_style.color.argb(),
        }
    }

    /// Push the current color/size to the open editor (toolbar / shortcut
    /// changes while editing).
    fn sync_editor_style(&self) {
        if let Some(t) = self.text_edit.as_ref() {
            t.editor.set_style(self.editor_style());
            t.editor.focus();
        }
    }

    /// Process commit/cancel requests raised by the native editor. Called by
    /// `App` whenever the editor wakes the loop.
    pub fn poll_text_editor(&mut self) {
        let events = match self.text_edit.as_ref() {
            Some(t) => t.editor.take_events(),
            None => return,
        };
        for ev in events {
            match ev {
                crate::native_text::EditorEvent::Commit => self.commit_text_edit(),
                crate::native_text::EditorEvent::Cancel => {
                    self.text_edit = None;
                    self.window.request_redraw();
                }
            }
            if self.text_edit.is_none() {
                break;
            }
        }
    }

    fn handle_left_press(&mut self) -> Outcome {
        // A fresh press also terminates a previous drag whose release was lost.
        self.grip_press_offset = None;
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
                // Toolbar click takes priority over everything else.
                let win_size = self.window.inner_size();
                let tb = toolbar::Toolbar::layout(
                    rect,
                    (win_size.width, win_size.height),
                    self.scale_factor,
                    self.toolbar_origin,
                );
                if tb.contains(self.cursor, self.tool) {
                    self.last_click = None;
                }
                match tb.hit_with_tool(self.cursor, self.tool) {
                    toolbar::ToolbarHit::Grip => {
                        self.commit_text_edit();
                        self.grip_press_offset = Some((self.cursor.0 - tb.origin.0, self.cursor.1 - tb.origin.1));
                        self.toolbar_hover = toolbar::ToolbarHit::Grip;
                        self.window.set_cursor(winit::window::CursorIcon::Grabbing);
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Tool(t) => {
                        if t != annotate::Tool::Text {
                            self.commit_text_edit();
                        }
                        self.pending_draw = None;
                        self.hovered_annotation_idx = None;
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
                        self.sync_editor_style();
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }
                    toolbar::ToolbarHit::Stroke(s) => {
                        self.current_style.stroke = s;
                        self.sync_editor_style();
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
                    toolbar::ToolbarHit::Cancel => return Outcome::Cancelled,
                    toolbar::ToolbarHit::Confirm => {
                        self.commit_text_edit();
                        return Outcome::Confirmed(rect);
                    }
                    toolbar::ToolbarHit::None => {
                        // Clicks on the toolbar's padding / separators must
                        // not fall through to drawing or resizing.
                        if tb.contains(self.cursor, self.tool) {
                            return Outcome::Continue;
                        }
                    }
                }

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

                // Tool::Text: clicks inside the selection rect either lift an
                // existing Text annotation back into edit mode (when the click
                // lands on its rendered bbox) or commit-then-start a fresh
                // edit at the click point. Goes BEFORE the is_drawing() check
                // so Text-tool clicks don't fall into the Pen/Shape branch.
                // Toolbar hit check stays earlier so toolbar clicks still
                // work when Text tool is active.
                if self.tool == annotate::Tool::Text && rect.contains(self.cursor) {
                    let cursor_frame = self.window_point_to_frame_point(self.cursor);

                    // Hit-test existing Text annotations in reverse z-order
                    // (latest-drawn = on top). If we land on one, lift it back
                    // into edit mode. Iterate the history slice directly —
                    // `history.current()` and `&mut self.font` are disjoint
                    // field borrows, so no per-click clone of the list is needed.
                    let hit_idx = self
                        .history
                        .current()
                        .iter()
                        .enumerate()
                        .rev()
                        .find(|(_, ann)| {
                            annotate_render::text_bbox(ann, &mut self.font)
                                .is_some_and(|bb| bb.contains(cursor_frame))
                        })
                        .map(|(i, _)| i);

                    if let Some(idx) = hit_idx {
                        // Commit any in-flight text BEFORE replacing it with
                        // the one we just lifted from history.
                        self.commit_text_edit();
                        if let Some(annotate::Annotation::Text { origin, content, style }) =
                            self.history.remove_at(idx)
                        {
                            self.current_style = style;
                            self.start_text_edit(origin, &content);
                        }
                        self.window.request_redraw();
                        return Outcome::Continue;
                    }

                    // No hit — existing behavior: commit current text, start fresh.
                    self.commit_text_edit();
                    self.start_text_edit(cursor_frame, "");
                    return Outcome::Continue;
                }
                if self.tool == annotate::Tool::Text {
                    // Outside the selection rect: commit any in-flight text
                    // and fall through to the Move-tool arm below for the
                    // existing anchor / outside-click-clear behavior.
                    self.commit_text_edit();
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
                        // Move tool: hit-test annotations first. If the click
                        // lands on an existing annotation's footprint, start an
                        // annotation drag (priority above selection translate).
                        let cursor_frame = self.window_point_to_frame_point(self.cursor);
                        // Disjoint field borrows (history vs. font) — no clone.
                        if let Some(idx) = annotate_render::annotation_at_cursor(
                            cursor_frame,
                            self.history.current(),
                            &mut self.font,
                        ) {
                            // Lift the annotation out of history at press time
                            // so live drag paints only the dragged copy. On
                            // release we push the translated version back.
                            if let Some(original) = self.history.remove_at(idx) {
                                self.annotation_drag = Some(AnnotationDrag {
                                    index: idx,
                                    original,
                                    press_frame: cursor_frame,
                                });
                            }
                            self.window.request_redraw();
                            return Outcome::Continue;
                        }
                        // No annotation under cursor — translate selection rect.
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
        if self.grip_press_offset.take().is_some() {
            if let OverlayState::Adjusting { rect, .. } = self.state {
                let size = self.window.inner_size();
                let bar = toolbar::Toolbar::layout(rect, (size.width, size.height), self.scale_factor, self.toolbar_origin);
                self.toolbar_hover = bar.hit_with_tool(self.cursor, self.tool);
                self.window.set_cursor(if self.toolbar_hover == toolbar::ToolbarHit::Grip {
                    winit::window::CursorIcon::Grab
                } else {
                    winit::window::CursorIcon::Default
                });
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }
        // Commit an in-flight annotation draw first.
        if let Some(pending) = self.pending_draw.take() {
            if let Some(ann) = pending.finalize() {
                self.history.push(ann);
            }
            self.window.request_redraw();
            return Outcome::Continue;
        }

        // Commit an in-flight annotation drag: translate the original by the
        // total cursor delta and push back to history. `history.push` clears
        // the redo stack — the drag-edit invalidates redo, which is correct.
        if let Some(drag) = self.annotation_drag.take() {
            let cursor_frame = self.window_point_to_frame_point(self.cursor);
            let dx = cursor_frame.0 - drag.press_frame.0;
            let dy = cursor_frame.1 - drag.press_frame.1;
            let translated = drag.original.translated(dx, dy);
            self.history.push(translated);
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
        // While the native text editor has focus it consumes keystrokes
        // itself; anything that still reaches winit (e.g. focus momentarily
        // on the overlay after a toolbar click) must not trigger tool
        // shortcuts. Escape still closes the editor.
        if self.text_edit.is_some() {
            if let Key::Named(NamedKey::Escape) = key {
                self.text_edit = None;
                self.window.request_redraw();
            } else if let Some(t) = self.text_edit.as_ref() {
                t.editor.focus();
            }
            return Outcome::Continue;
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

        // C (Idle / Dragging — while the magnifier is visible): copy the
        // HEX color under the cursor and keep the overlay open.
        if matches!(self.state, OverlayState::Idle | OverlayState::Dragging { .. })
            && !self.modifiers.super_key()
        {
            if let Key::Character(s) = &key {
                if s.eq_ignore_ascii_case("c") {
                    let size = self.window.inner_size();
                    let hex = render::color_hex_at(&self.frame, self.cursor, (size.width, size.height));
                    match crate::clipboard::put_text(&hex) {
                        Ok(()) => {
                            println!("copied color {hex}");
                            self.color_copied_at = Some(std::time::Instant::now());
                            self.window.request_redraw();
                            let proxy = self.proxy.clone();
                            // Repaint once more when the "Copied" badge expires.
                            std::thread::spawn(move || {
                                std::thread::sleep(render::COPIED_BADGE_DURATION);
                                let _ = proxy.send_event(crate::app::UserEvent::Redraw);
                            });
                        }
                        Err(e) => eprintln!("copy color: {e:?}"),
                    }
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
                    self.hovered_annotation_idx = None;
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
                // If a drag is in progress, abort the drag rather than the overlay.
                // Restore the original annotation to history. Note: `push`
                // appends to the end of the undo stack, so the annotation
                // re-appears at the top of z-order rather than its original
                // `drag.index`. Minor visual quirk: a re-appearing annotation
                // jumps to the topmost layer.
                if let Some(drag) = self.annotation_drag.take() {
                    self.history.push(drag.original);
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
        let t_frame = std::time::Instant::now();
        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));

        let sel_tuple = self.current_selection_rect_window();
        let sel_rect = self.current_selection_rect();
        // Snap preview: when Idle and hovering a window, render that window's
        // bounds the same way as a confirmed selection but with a blue outline.
        let (sel_rect, sel_tuple, outline_color) = match (sel_rect, &self.state, self.snap_target) {
            (None, OverlayState::Idle, Some(snap)) => {
                let clamped = snap.clamp_to((w, h));
                if clamped.w == 0 || clamped.h == 0 {
                    (None, None, toolbar::THEME_COLOR)
                } else {
                    let tup = clamped.as_tuple_u32();
                    (Some(clamped), Some(tup), toolbar::SNAP_PREVIEW_COLOR)
                }
            }
            (existing, _, _) => (existing, sel_tuple, toolbar::THEME_COLOR),
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
        let hovered_idx = self.hovered_annotation_idx;
        let active_tool = self.tool;
        let frame_ref = &self.frame;
        let dimmed_bg = &self.dimmed_bg;
        let font = &mut self.font;
        let icon_font = &mut self.icon_font;
        let mosaic_cache = &mut self.mosaic_cache;
        let scale = self.scale_factor;

        let mut buf = self.host.presenter.frame(w, h)?;

        let t_acq = t_frame.elapsed();
        let composed_ok = frame_ref.dimensions() == (w, h);
        match (&mut self.composed_bg, composed_ok) {
            (Some((key, cached)), true) if *key == sel_tuple && cached.len() == buf.len() => {
                render::par_copy(&mut buf, cached);
            }
            (slot, true) => {
                let mut cached = vec![0u32; buf.len()];
                render::blit_background_dimmed(&mut cached, w, h, frame_ref, dimmed_bg, sel_tuple);
                render::par_copy(&mut buf, &cached);
                *slot = Some((sel_tuple, cached));
            }
            (_, false) => {
                render::blit_background_dimmed(&mut buf, w, h, frame_ref, dimmed_bg, sel_tuple);
            }
        }
        let t_bg = t_frame.elapsed();
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
                    Some(&mut *mosaic_cache),
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

            // Hover highlight: faint SNAP_PREVIEW_COLOR outline around the
            // annotation under the cursor (Move tool, no drag in progress).
            // Paint BEFORE the drag block so the drag's THEME_COLOR outline
            // wins if both somehow apply.
            if active_tool == annotate::Tool::Move && self.annotation_drag.is_none() {
                if let Some(idx) = hovered_idx {
                    if let Some(ann) = self.history.current().get(idx) {
                        let bbox_frame = annotate_render::annotation_bbox(ann, font);
                        if let Some(bbox_win) = annotate_render::frame_rect_to_window_rect(
                            bbox_frame, frame_size, window_size,
                        ) {
                            render::draw_selection_outline(
                                &mut buf, w, h, bbox_win, toolbar::SNAP_PREVIEW_COLOR,
                            );
                        }
                    }
                }
            }

            // Dragged annotation preview (Move tool drag in progress). The
            // annotation was removed from history at press time; we paint a
            // translated copy at the current cursor delta.
            if let Some(drag) = self.annotation_drag.as_ref() {
                let cursor_frame = {
                    let size = self.window.inner_size();
                    let (ww, wh) = (size.width.max(1) as i64, size.height.max(1) as i64);
                    let (fw, fh) = frame_ref.dimensions();
                    let x = (cursor.0 as i64 * fw as i64 / ww) as i32;
                    let y = (cursor.1 as i64 * fh as i64 / wh) as i32;
                    (x, y)
                };
                let dx = cursor_frame.0 - drag.press_frame.0;
                let dy = cursor_frame.1 - drag.press_frame.1;
                let translated = drag.original.translated(dx, dy);
                annotate_render::draw_annotation_on_buf(
                    &mut buf, w, h, frame_ref, &translated, font, None,
                );
                let bbox_frame = annotate_render::annotation_bbox(&translated, font);
                if let Some(bbox_win) = annotate_render::frame_rect_to_window_rect(
                    bbox_frame, frame_size, window_size,
                ) {
                    render::draw_selection_outline(
                        &mut buf, w, h, bbox_win, toolbar::THEME_COLOR,
                    );
                }
            }

            // Anchors only when Move tool is active
            if self.tool == annotate::Tool::Move {
                if let Some(r) = sel_rect {
                    render::draw_anchors(&mut buf, w, h, r);
                }
            }

            // Toolbar
            if let Some(sel_win) = sel_rect {
                let tb = toolbar::Toolbar::layout(sel_win, (w, h), scale, self.toolbar_origin);
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
                    self.toolbar_hover,
                    icon_font,
                    font,
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
                self.color_copied_at
                    .is_some_and(|t| t.elapsed() < render::COPIED_BADGE_DURATION),
            );
        }
        #[cfg(windows)]
        if self.state == OverlayState::Idle {
            render::draw_snap_mode(&mut buf, w, h, font, scale, self.elements.element_mode, cursor);
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

        let t_paint = t_frame.elapsed();
        buf.present()?;
        if frame_debug() {
            eprintln!(
                "QuickShot frame: acquire {:?} bg {:?} paint {:?}, present {:?}",
                t_acq, t_bg - t_acq,
                t_paint,
                t_frame.elapsed() - t_paint
            );
        }
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

fn frame_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("QUICKSHOT_FRAME_DEBUG").is_some())
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
                    "QuickShot: NSWindow class not found; can't swizzle canBecomeKeyWindow"
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
            eprintln!("QuickShot: swizzled NSWindow.canBecomeKeyWindow -> YES");
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

/// Place the NSWindow exactly over the monitor `geom` (CG points, top-left
/// origin) with `setFrame:display:`. winit's set_outer_position +
/// request_inner_size on a hidden window that's moving between displays can
/// end up constrained / offset; setting the Cocoa frame directly is exact.
#[cfg(target_os = "macos")]
fn set_macos_window_frame(window: &Window, geom: &MonitorGeom) {
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    let Some(ns_window) = ns_window_of(window) else { return };
    // Cocoa global coords: origin at bottom-left of the primary display.
    let primary_h = core_graphics::display::CGDisplay::main().bounds().size.height;
    let rect = NSRect::new(
        NSPoint::new(geom.x as f64, primary_h - (geom.y as f64 + geom.height as f64)),
        NSSize::new(geom.width as f64, geom.height as f64),
    );
    unsafe {
        let obj = &*(ns_window as *const AnyObject);
        let _: () = objc2::msg_send![obj, setFrame: rect, display: false];
    }
}

/// Hand focus back to the app that was frontmost before the overlay opened.
///
/// When our (accessory) app gives up activation, macOS activates the *next*
/// app in its own ordering, which is frequently not the one the user was
/// using: e.g. capturing over WeChat sent focus to a different app, and
/// WeChat's window dropped behind it. Explicitly re-activating the
/// remembered app restores exactly the pre-capture state, like the
/// built-in / WeChat screenshot tools.
#[cfg(target_os = "macos")]
fn restore_previous_app(app: Option<objc2::rc::Retained<objc2_app_kit::NSRunningApplication>>) {
    use objc2_app_kit::NSApplicationActivationOptions;
    let Some(app) = app else { return };
    if app.isTerminated() {
        return;
    }
    #[allow(deprecated)]
    let _ = app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
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
            "QuickShot: overlay NSWindow level={level} collectionBehavior={behavior:#x}"
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
}


/// Renders the README screenshots from the real drawing code (no screen
/// recording). Run with:
///
/// ```text
/// QUICKSHOT_README_BG=path/to/wallpaper.png QUICKSHOT_README_OUT=docs/images \
///   cargo test --release render_readme_images -- --ignored
/// ```
#[cfg(test)]
mod readme_images {
    use super::*;
    use annotate::{Annotation, Color, Stroke};

    fn to_rgba(buf: &[u32], w: u32, h: u32) -> RgbaImage {
        let mut out = RgbaImage::new(w, h);
        for (p, &v) in out.pixels_mut().zip(buf) {
            *p = image::Rgba([(v >> 16) as u8, (v >> 8) as u8, v as u8, 255]);
        }
        out
    }

    #[test]
    #[ignore = "writes README images; run manually"]
    fn render_readme_images() {
        let out_dir = std::env::var("QUICKSHOT_README_OUT").unwrap_or_else(|_| "docs/images".into());
        std::fs::create_dir_all(&out_dir).unwrap();
        let bg_path = std::env::var("QUICKSHOT_README_BG").expect("set QUICKSHOT_README_BG");
        // 2x "Retina" canvas.
        let (w, h) = (2400u32, 1500u32);
        let frame = image::imageops::resize(
            &image::open(&bg_path).unwrap().to_rgba8(),
            w,
            h,
            image::imageops::FilterType::CatmullRom,
        );
        let scale = 2.0f32;
        let dimmed = render::precompute_dimmed(&frame);
        let mut font = crate::text::Font::embedded();
        let mut icons = crate::icon_font::IconFont::embedded();

        // ---- 1. annotate: selection + annotations + toolbar
        let sel = Rect { x: 280, y: 130, w: 1840, h: 1240 };
        let sel_t = sel.as_tuple_u32();
        let mut buf = vec![0u32; (w * h) as usize];
        render::blit_background_dimmed(&mut buf, w, h, &frame, &dimmed, Some(sel_t));
        render::draw_selection_outline(&mut buf, w, h, sel_t, toolbar::THEME_COLOR);
        let red = annotate::AnnotationStyle { color: Color::Red, stroke: Stroke::Medium };
        let yellow = annotate::AnnotationStyle { color: Color::Yellow, stroke: Stroke::Medium };
        // Coordinates target the illustrative browser + chat mockup in the
        // README background (window 300,150 – 2100,1350).
        let anns = [
            // Box the code block.
            Annotation::Rect { rect: Rect { x: 896, y: 794, w: 1068, h: 178 }, style: red },
            // Circle the shortcut chip in step 1.
            Annotation::Ellipse { rect: Rect { x: 1352, y: 592, w: 204, h: 78 }, style: yellow },
            // Arrow from the note to the code block.
            Annotation::Arrow { from: (1500, 1090), to: (1360, 984), style: red },
            // Redact the chat history.
            Annotation::Mosaic { rect: Rect { x: 330, y: 528, w: 400, h: 230 }, block_size: 14 },
            Annotation::Text {
                origin: (1516, 1066),
                content: "Set your own hotkey 👆".into(),
                style: red,
            },
        ];
        for a in &anns {
            annotate_render::draw_annotation_on_buf(&mut buf, w, h, &frame, a, &mut font, None);
        }
        render::draw_anchors(&mut buf, w, h, sel);
        let tb = toolbar::Toolbar::layout(sel, (w, h), scale, None);
        toolbar::draw_toolbar(
            &mut buf, w, h, &tb, annotate::Tool::Arrow, red, true, false, false,
            toolbar::ToolbarHit::None, &mut icons, &mut font,
        );
        render::draw_size_label(&mut buf, w, h, sel, (w, h), (w, h), &mut font, scale);
        let shot = to_rgba(&buf, w, h);
        // Crop: selection (+ size label above) through the style bar.
        let x0 = (sel.x - 70).max(0) as u32;
        let y0 = (sel.y - 90).max(0) as u32;
        let x1 = ((sel.x + sel.w + 70) as u32).min(w);
        let y1 = ((tb.sub.0 .1 + tb.sub.1 .1 + 50) as u32).min(h);
        let crop = image::imageops::crop_imm(&shot, x0, y0, x1 - x0, y1 - y0).to_image();
        crop.save(format!("{out_dir}/annotate.png")).unwrap();

        // ---- 2. toolbar close-up
        let (tx, ty) = tb.main.0;
        let tw = tb.main.1 .0;
        let th = tb.sub.0 .1 + tb.sub.1 .1 - ty;
        let pad = 40;
        image::imageops::crop_imm(&shot, (tx - pad) as u32, (ty - pad) as u32, (tw + 2 * pad) as u32, (th + 2 * pad) as u32)
            .to_image()
            .save(format!("{out_dir}/toolbar.png"))
            .unwrap();

        // ---- 3. magnifier while choosing a region
        let mut buf = vec![0u32; (w * h) as usize];
        render::blit_background_dimmed(&mut buf, w, h, &frame, &dimmed, None);
        // Edge of the red "close" traffic light: colour + anti-aliased edge.
        let cursor = (334, 178);
        render::draw_magnifier(&mut buf, w, h, &frame, cursor, (w, h), &mut font, scale, false);
        let shot = to_rgba(&buf, w, h);
        image::imageops::crop_imm(&shot, 234, 98, 560, 620)
            .to_image()
            .save(format!("{out_dir}/magnifier.png"))
            .unwrap();
    }
}
