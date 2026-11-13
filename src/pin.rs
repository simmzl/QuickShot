//! A pinned floating window — a cropped screenshot image that the user
//! "tacked" onto the desktop. Multiple pins can coexist; each owns its own
//! winit Window + softbuffer Surface.

use crate::overlay::toolbar::Canvas;
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
    image: PinImage,
    press_pos: Option<(i32, i32)>,
    win_pos_at_press: Option<(i32, i32)>,
    last_cursor: Option<(i32, i32)>,
    last_click: Option<Instant>,
    pointer_inside: bool,
    close_pressed: bool,
}

struct PinImage {
    original: RgbaImage,
    scaled: Option<RgbaImage>,
}

impl PinImage {
    fn for_size(&mut self, width: u32, height: u32) -> &RgbaImage {
        let size = (width, height);
        if self.original.dimensions() == size {
            self.scaled = None;
            return &self.original;
        }
        if self.scaled.as_ref().is_some_and(|img| img.dimensions() != size) {
            self.scaled = None;
        }
        self.scaled.get_or_insert_with(|| image::imageops::resize(
            &self.original, width, height, image::imageops::FilterType::Triangle,
        ))
    }
}

/// Display and hit-test the same circular button in physical window pixels.
struct CloseButton {
    x: u32,
    y: u32,
    size: u32,
}

impl CloseButton {
    fn layout(w: u32, h: u32, scale: f32) -> Self {
        let size = ((28.0 * scale).round() as u32).min(w).min(h);
        let inset = ((6.0 * scale).round() as u32).min((w - size) / 2).min((h - size) / 2);
        Self { x: w - size - inset, y: inset, size }
    }

    fn center(&self) -> (f32, f32, f32) {
        let r = self.size as f32 / 2.0;
        (self.x as f32 + r, self.y as f32 + r, r)
    }

    fn contains(&self, p: (i32, i32)) -> bool {
        let (cx, cy, r) = self.center();
        (p.0 as f32 + 0.5 - cx).hypot(p.1 as f32 + 0.5 - cy) < r
    }

    fn draw(&self, c: &mut Canvas<'_>, highlighted: bool) {
        if self.size == 0 { return; }
        let (cx, cy, r) = self.center();
        let sigma = (r * 0.32).max(0.5);
        // Sample a halo around the button too, so the blur follows the image
        // at the rim. Only this small patch of the displayed pin needs filtering.
        let halo = (3.0 * sigma).ceil() as u32;
        let left = self.x.saturating_sub(halo);
        let top = self.y.saturating_sub(halo);
        let right = (self.x + self.size + halo).min(c.w);
        let bottom = (self.y + self.size + halo).min(c.h);
        let patch = image::RgbImage::from_fn(right - left, bottom - top, |x, y| {
            let pixel = c.buf[((y + top) * c.w + x + left) as usize];
            image::Rgb([(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8])
        });
        let blurred = image::imageops::blur(&patch, sigma);
        for y in self.y..self.y + self.size {
            for x in self.x..self.x + self.size {
                let coverage = (r + 0.5 - (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy)).clamp(0.0, 1.0);
                let [red, green, blue] = blurred.get_pixel(x - left, y - top).0;
                let color = u32::from_be_bytes([0, red, green, blue]);
                c.blend(x as i32, y as i32, color, coverage);
            }
        }
        c.disk(cx, cy, r, 0x10141A, if highlighted { 0.68 } else { 0.5 });
        let arm = r * 0.32;
        let stroke = (r * 0.14).max(1.0);
        c.line(cx - arm, cy - arm, cx + arm, cy + arm, stroke, 0xffffff);
        c.line(cx - arm, cy + arm, cx + arm, cy - arm, stroke, 0xffffff);
    }
}

/// Locate the selection in physical desktop pixels, including on monitors
/// to the left of the primary display. Leave an eight-point visual offset.
pub fn compute_pin_screen_position(
    overlay_origin: winit::dpi::PhysicalPosition<i32>,
    selection: (i32, i32),
    scale: f64,
) -> winit::dpi::PhysicalPosition<i32> {
    let offset = (8.0 * scale).round() as i32;
    winit::dpi::PhysicalPosition::new(
        overlay_origin.x + selection.0 + offset,
        overlay_origin.y + selection.1 + offset,
    )
}

impl PinWindow {
    pub fn create(
        event_loop: &ActiveEventLoop,
        image: RgbaImage,
        screen_position: winit::dpi::PhysicalPosition<i32>,
        capture_scale: f64,
    ) -> Result<Self> {
        use anyhow::Context;
        use softbuffer::Context as SoftContext;
        use winit::dpi::PhysicalSize;
        use winit::window::WindowAttributes;

        let attrs = WindowAttributes::default()
            .with_title("QuickShot pin")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(image.width(), image.height()).to_logical::<f64>(capture_scale));
        // Windows converts logical positions using the new window's initial
        // monitor, which may have a different DPI from the captured monitor.
        #[cfg(windows)]
        let attrs = attrs.with_position(screen_position);
        #[cfg(not(windows))]
        let attrs = attrs.with_position(screen_position.to_logical::<f64>(capture_scale));
        #[cfg(target_os = "macos")]
        let attrs = winit::platform::macos::WindowAttributesExtMacOS::with_accepts_first_mouse(attrs, true);
        let win = event_loop
            .create_window(attrs)
            .context("pin: create window")?;

        let window = Rc::new(win);

        // NSFloatingWindowLevel = 3. Above normal apps, below the overlay's
        // 1500 so a fresh capture appears above pins.
        #[cfg(target_os = "macos")]
        crate::overlay::set_macos_window_level(&window, 3);

        // Windows (and any non-macOS): use winit's cross-platform
        // `set_window_level(AlwaysOnTop)`, which maps to
        // `SetWindowPos(HWND_TOPMOST, ...)` on Win32. Without this the pin
        // gets z-ordered like a normal window and any subsequent click on
        // another app raises that app above the pin.
        #[cfg(not(target_os = "macos"))]
        window.set_window_level(winit::window::WindowLevel::AlwaysOnTop);

        let context =
            SoftContext::new(window.clone()).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        Ok(Self {
            window,
            surface,
            image: PinImage { original: image, scaled: None },
            press_pos: None,
            win_pos_at_press: None,
            last_cursor: None,
            last_click: None,
            pointer_inside: false,
            close_pressed: false,
        })
    }

    pub fn redraw(&mut self) -> Result<()> {
        use std::num::NonZeroU32;

        let size = self.window.inner_size();
        let (w, h) = (size.width.max(1), size.height.max(1));
        self.surface
            .resize(NonZeroU32::new(w).unwrap(), NonZeroU32::new(h).unwrap())
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        let mut buf = self
            .surface
            .buffer_mut()
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;

        for (dst, src) in buf.iter_mut().zip(self.image.for_size(w, h).pixels()) {
            *dst = u32::from_be_bytes([0, src[0], src[1], src[2]]);
        }

        if self.pointer_inside {
            let button = CloseButton::layout(w, h, self.window.scale_factor() as f32);
            let highlighted = self.last_cursor.is_some_and(|p| button.contains(p));
            button.draw(&mut Canvas { buf: &mut buf, w, h }, highlighted);
        }
        buf.present().map_err(|e| anyhow::anyhow!("{e:?}"))?;
        Ok(())
    }

    fn over_close(&self) -> bool {
        let size = self.window.inner_size();
        self.pointer_inside && self.last_cursor.is_some_and(|p| {
            CloseButton::layout(size.width, size.height, self.window.scale_factor() as f32).contains(p)
        })
    }

    fn update_cursor(&mut self, position: Option<(i32, i32)>, inside: bool) {
        let was_over = self.over_close();
        let was_inside = self.pointer_inside;
        self.pointer_inside = inside;
        self.last_cursor = position;
        let over = self.over_close();
        if was_inside != inside || was_over != over {
            self.window.set_cursor(if over {
                winit::window::CursorIcon::Pointer
            } else {
                winit::window::CursorIcon::Default
            });
            self.window.request_redraw();
        }
    }

    fn update_client_position(&mut self, position: Option<(i32, i32)>) {
        let size = self.window.inner_size();
        let inside = position.is_some_and(|(x, y)| {
            x >= 0 && y >= 0 && (x as u32) < size.width && (y as u32) < size.height
        });
        self.update_cursor(position, inside);
    }

    #[cfg(target_os = "macos")]
    fn client_position(&self) -> Option<(i32, i32)> {
        let (x, y) = crate::capture::cursor_global()?;
        let origin = self.window.inner_position().ok()?;
        let scale = self.window.scale_factor();
        Some(((x * scale).floor() as i32 - origin.x, (y * scale).floor() as i32 - origin.y))
    }

    pub fn handle_event(&mut self, event: WindowEvent) -> PinOutcome {
        use winit::dpi::PhysicalPosition;
        use winit::event::{ElementState, MouseButton};

        // Windows can omit CursorMoved after re-entry at the same pixel.
        // Refresh both ends of a click, including releases outside the pin.
        #[cfg(windows)]
        if matches!(event, WindowEvent::CursorEntered { .. }
            | WindowEvent::MouseInput { button: MouseButton::Left, .. }) {
            self.update_client_position(crate::windows_mouse::client_position(&self.window));
        }
        match event {
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.window.request_redraw();
                PinOutcome::Continue
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("pin redraw error: {e:?}");
                }
                PinOutcome::Continue
            }
            WindowEvent::CloseRequested => PinOutcome::Closed,
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                self.close_pressed = false;
                self.press_pos = None;
                self.win_pos_at_press = None;
                let Some(cursor_local) = self.last_cursor.filter(|_| self.pointer_inside) else {
                    return PinOutcome::Continue;
                };
                if self.over_close() {
                    self.close_pressed = true;
                    self.last_click = None;
                    return PinOutcome::Continue;
                }
                // Double-click detection: two Pressed events within 400 ms
                // (position-agnostic, matches Iter 2a overlay convention).
                let now = std::time::Instant::now();
                let is_double_click = matches!(
                    self.last_click,
                    Some(t) if now.duration_since(t) < std::time::Duration::from_millis(400)
                );
                self.last_click = Some(now);
                if is_double_click {
                    return PinOutcome::Closed;
                }
                // Single press: record drag state. We store press_pos as
                // screen-absolute (window outer + cursor window-local) so the
                // reference frame doesn't shift when the window moves during
                // drag. Otherwise the cursor's window-local position changes
                // as we move the window, causing oscillation / jitter.
                let outer = self
                    .window
                    .outer_position()
                    .unwrap_or_default();
                self.win_pos_at_press = Some((outer.x, outer.y));
                self.press_pos = Some((outer.x + cursor_local.0, outer.y + cursor_local.1));
                PinOutcome::Continue
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => {
                if std::mem::take(&mut self.close_pressed) && self.over_close() {
                    return PinOutcome::Closed;
                }
                self.press_pos = None;
                self.win_pos_at_press = None;
                PinOutcome::Continue
            }
            WindowEvent::CursorEntered { .. } => {
                #[cfg(target_os = "macos")]
                self.update_client_position(self.client_position());
                #[cfg(not(any(windows, target_os = "macos")))]
                self.update_cursor(self.last_cursor, true);
                PinOutcome::Continue
            }
            WindowEvent::CursorLeft { .. } => {
                self.update_cursor(None, false);
                PinOutcome::Continue
            }
            WindowEvent::Focused(false) => {
                self.close_pressed = false;
                self.press_pos = None;
                self.win_pos_at_press = None;
                self.last_click = None;
                PinOutcome::Continue
            }
            WindowEvent::CursorMoved { position, .. } => {
                let cursor_local = (position.x.floor() as i32, position.y.floor() as i32);
                self.update_client_position(Some(cursor_local));
                #[cfg(windows)]
                if (self.close_pressed || self.press_pos.is_some())
                    && !crate::windows_mouse::has_left_capture(&self.window)
                {
                    self.close_pressed = false;
                    self.press_pos = None;
                    self.win_pos_at_press = None;
                    self.last_click = None;
                }
                if let (Some(press_screen), Some(win_at_press)) =
                    (self.press_pos, self.win_pos_at_press)
                {
                    // Compute current cursor position in screen-absolute coords
                    // using the window's current outer position. Then derive
                    // the drag delta from press (screen-absolute) so the math
                    // doesn't oscillate as the window moves underneath.
                    let outer_now = self
                        .window
                        .outer_position()
                        .unwrap_or_default();
                    let cursor_screen = (
                        outer_now.x + cursor_local.0,
                        outer_now.y + cursor_local.1,
                    );
                    let dx = cursor_screen.0 - press_screen.0;
                    let dy = cursor_screen.1 - press_screen.1;
                    if dx.abs() + dy.abs() >= 4 {
                        let new_x = win_at_press.0 + dx;
                        let new_y = win_at_press.1 + dy;
                        self.window
                            .set_outer_position(PhysicalPosition::new(new_x, new_y));
                    }
                }
                PinOutcome::Continue
            }
            _ => PinOutcome::Continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_between_dpi_scales_preserves_edges_and_original_pixels() {
        let original = RgbaImage::from_fn(80, 40, |x, y| {
            image::Rgba([if x < 40 { 0 } else { 255 }, if y < 20 { 0 } else { 255 }, 127, 255])
        });
        let mut pin = PinImage { original: original.clone(), scaled: None };
        for (w, h) in [(120, 60), (60, 30), (80, 40)] {
            let shown = pin.for_size(w, h);
            assert_eq!(shown.dimensions(), (w, h));
            for (x, y, ox, oy) in [(0, 0, 0, 0), (w-1, 0, 79, 0), (0, h-1, 0, 39), (w-1, h-1, 79, 39)] {
                assert_eq!(shown.get_pixel(x, y), original.get_pixel(ox, oy));
            }
        }
        assert_eq!(pin.for_size(80, 40), &original);
        assert!(pin.scaled.is_none());
    }

    #[test]
    fn close_button_blurs_only_the_top_right_control() {
        let (w, h) = (160, 90);
        let mut pixels: Vec<u32> = (0..w*h).map(|i| if i % 2 == 0 { 0xff0000 } else { 0x0000ff }).collect();
        let original = pixels.clone();
        let button = CloseButton::layout(w, h, 1.5);
        button.draw(&mut Canvas { buf: &mut pixels, w, h }, false);
        let cx = button.x + button.size / 2;
        let cy = button.y + button.size / 2;
        assert!(button.contains((cx as i32, cy as i32)));
        assert!(!button.contains((button.x as i32, button.y as i32)));
        let filtered = pixels[((cy - button.size / 4) * w + cx) as usize];
        assert!(filtered >> 16 > 20 && filtered & 255 > 20);
        assert!(pixels[(cy * w + cx) as usize] > 0xdddddd);
        for y in 0..h {
            for x in 0..w {
                if x < button.x || x >= button.x + button.size || y < button.y || y >= button.y + button.size {
                    let i = (y * w + x) as usize;
                    assert_eq!(pixels[i], original[i]);
                }
            }
        }
    }

    #[test]
    fn close_button_fits_a_tiny_pin() {
        let button = CloseButton::layout(2, 1, 2.0);
        let mut pixels = [0xffffff; 2];
        button.draw(&mut Canvas { buf: &mut pixels, w: 2, h: 1 }, false);
        assert!(button.contains((1, 0)));
    }

    #[test]
    fn pin_position_on_monitors_with_different_scales() {
        use winit::dpi::PhysicalPosition as P;
        for (origin, selection, scale, expected) in [
            (P::new(100, 200), (50, 30), 1.0, P::new(158, 238)),
            (P::new(0, 0), (400, 200), 2.0, P::new(416, 216)),
            (P::new(-2560, -300), (60, 40), 1.5, P::new(-2488, -248)),
            (P::new(50, 50), (0, 0), 2.0, P::new(66, 66)),
        ] {
            assert_eq!(compute_pin_screen_position(origin, selection, scale), expected);
        }
    }
}
