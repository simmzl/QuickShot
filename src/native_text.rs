//! Native in-place text editor for the Text annotation tool.
//!
//! Instead of hand-rolling key handling / IME / caret, we drop a real OS text
//! control on top of the overlay:
//!   * macOS: an `NSTextView` subview of the overlay's content view. IME,
//!     selection, arrow keys, ⌘C/⌘V/⌘X/⌘A/⌘Z all come for free (⌘-shortcuts
//!     are routed directly in `performKeyEquivalent:` because an accessory
//!     app has no Edit menu to dispatch them).
//!   * Windows: a multiline `EDIT` child control (Ctrl+C/V/X/A/Z built in).
//!
//! The overlay keeps owning the annotation model; the editor only reports
//! back through [`EditorEvent`]s that the overlay polls after each event.
//! Text is committed as a regular `Annotation::Text` and rendered by the same
//! platform text stack (`crate::text`) so the exported PNG matches.

use std::cell::RefCell;
use std::rc::Rc;
use winit::window::Window;

/// What the editor wants the overlay to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorEvent {
    /// Return / click-outside: commit the current content.
    Commit,
    /// Escape: discard the edit.
    Cancel,
}

/// Visual style for the editor, in **window physical pixels**.
#[derive(Debug, Clone, Copy)]
pub struct EditorStyle {
    pub font_px: f32,
    pub color_rgb: u32,
}

/// Event queue shared with the native control's callbacks. `wake` nudges the
/// winit event loop: keys consumed by the native control never surface as
/// winit `WindowEvent`s, so without it Return/Escape would sit unprocessed.
pub struct EventSink {
    events: RefCell<Vec<EditorEvent>>,
    wake: Box<dyn Fn()>,
}

impl EventSink {
    pub fn push(&self, ev: EditorEvent) {
        self.events.borrow_mut().push(ev);
        (self.wake)();
    }
}

type Queue = Rc<EventSink>;

pub struct NativeTextEditor {
    inner: imp::Editor,
    queue: Queue,
}

impl NativeTextEditor {
    /// Create the editor at window-physical position `pos` (top-left of the
    /// first line box), pre-filled with `initial`, and focus it.
    pub fn new(
        window: &Window,
        pos: (i32, i32),
        initial: &str,
        style: EditorStyle,
        scale_factor: f64,
        wake: Box<dyn Fn()>,
    ) -> Option<Self> {
        let queue: Queue = Rc::new(EventSink { events: RefCell::new(Vec::new()), wake });
        let inner = imp::Editor::new(window, pos, initial, style, scale_factor, queue.clone())?;
        Some(Self { inner, queue })
    }

    pub fn text(&self) -> String {
        self.inner.text()
    }

    pub fn set_style(&self, style: EditorStyle) {
        self.inner.set_style(style);
    }

    /// Drain pending commit/cancel requests.
    pub fn take_events(&self) -> Vec<EditorEvent> {
        std::mem::take(&mut *self.queue.events.borrow_mut())
    }

    /// Give keyboard focus back to the editor (e.g. after a toolbar click).
    pub fn focus(&self) {
        self.inner.focus();
    }
}

// ================================================================ macOS ====

#[cfg(target_os = "macos")]
mod imp {
    use super::{EditorEvent, EditorStyle, Queue};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, ProtocolObject, Sel};
    use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSColor, NSEvent, NSEventModifierFlags, NSResponder, NSText, NSTextDelegate, NSTextView,
        NSTextViewDelegate, NSView,
    };
    use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect, NSSize, NSString};
    use std::cell::RefCell;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    pub struct DelegateIvars {
        queue: RefCell<Option<Queue>>,
    }

    define_class!(
        // SAFETY: NSObject has no subclassing requirements; no Drop impl.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "QuickShotTextDelegate"]
        #[ivars = DelegateIvars]
        struct TextDelegate;

        unsafe impl NSObjectProtocol for TextDelegate {}

        unsafe impl NSTextDelegate for TextDelegate {
            #[unsafe(method(textDidChange:))]
            fn text_did_change(&self, notification: &objc2_foundation::NSNotification) {
                if let Some(obj) = notification.object() {
                    if let Ok(tv) = obj.downcast::<NSTextView>() {
                        tv.sizeToFit();
                    }
                }
            }
        }

        unsafe impl NSTextViewDelegate for TextDelegate {
            #[unsafe(method(textView:doCommandBySelector:))]
            fn do_command(&self, _tv: &NSTextView, cmd: Sel) -> bool {
                match command_event(cmd) {
                    Some(ev) => {
                        if let Some(q) = self.ivars().queue.borrow().as_ref() {
                            q.push(ev);
                        }
                        true
                    }
                    None => false,
                }
            }
        }
    );

    define_class!(
        // SAFETY: NSTextView supports subclassing; no Drop impl.
        #[unsafe(super(NSTextView, NSText, NSView, NSResponder, NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "QuickShotTextView"]
        struct QsTextView;

        impl QsTextView {
            /// Accessory apps have no Edit menu, so ⌘-shortcuts would never
            /// reach the text view. Route the standard editing shortcuts to
            /// the matching NSText/NSResponder actions ourselves.
            #[unsafe(method(performKeyEquivalent:))]
            fn perform_key_equivalent(&self, event: &NSEvent) -> bool {
                let handled = match edit_action_for(event) {
                    Some(action) => unsafe {
                        // tryToPerform:with: walks the responder chain, so
                        // undo:/redo: reach the window's undo manager too.
                        msg_send![self, tryToPerform: action, with: Some(self as &AnyObject)]
                    },
                    None => false,
                };
                if handled {
                    true
                } else {
                    unsafe { msg_send![super(self), performKeyEquivalent: event] }
                }
            }
        }
    );

    /// Map a text-view command to a commit/cancel request. Shift+Return
    /// inserts a line break (returns None → default handling).
    fn command_event(cmd: Sel) -> Option<EditorEvent> {
        if cmd == sel!(insertNewline:) {
            let shift = NSEvent::modifierFlags_class().contains(NSEventModifierFlags::Shift);
            (!shift).then_some(EditorEvent::Commit)
        } else if cmd == sel!(cancelOperation:) {
            Some(EditorEvent::Cancel)
        } else {
            None
        }
    }

    /// Accessory apps have no Edit menu, so ⌘-shortcuts never reach the text
    /// view through menu key equivalents. Map them to the standard actions.
    fn edit_action_for(event: &NSEvent) -> Option<Sel> {
        {
            {
                let flags = event.modifierFlags();
                let relevant = flags
                    & (NSEventModifierFlags::Command
                        | NSEventModifierFlags::Shift
                        | NSEventModifierFlags::Option
                        | NSEventModifierFlags::Control);
                let cmd = NSEventModifierFlags::Command;
                let cmd_shift = NSEventModifierFlags::Command | NSEventModifierFlags::Shift;
                let key = event
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string().to_lowercase())
                    .unwrap_or_default();
                let action = if relevant == cmd {
                    match key.as_str() {
                        "c" => Some(sel!(copy:)),
                        "v" => Some(sel!(paste:)),
                        "x" => Some(sel!(cut:)),
                        "a" => Some(sel!(selectAll:)),
                        "z" => Some(sel!(undo:)),
                        _ => None,
                    }
                } else if relevant == cmd_shift && key == "z" {
                    Some(sel!(redo:))
                } else {
                    None
                };
                action
            }
        }
    }

    pub struct Editor {
        view: Retained<QsTextView>,
        _delegate: Retained<TextDelegate>,
        scale: f64,
    }

    fn ns_color(rgb: u32) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(
            ((rgb >> 16) & 0xFF) as f64 / 255.0,
            ((rgb >> 8) & 0xFF) as f64 / 255.0,
            (rgb & 0xFF) as f64 / 255.0,
            1.0,
        )
    }

    impl Editor {
        pub fn new(
            window: &Window,
            pos: (i32, i32),
            initial: &str,
            style: EditorStyle,
            scale: f64,
            queue: Queue,
        ) -> Option<Self> {
            let mtm = MainThreadMarker::new()?;
            let handle = window.window_handle().ok()?;
            let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return None };
            // SAFETY: winit guarantees ns_view is a live NSView for the
            // window's lifetime; we only borrow it here.
            let host: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };

            // winit's view is flipped (top-left origin) so window-physical px
            // / scale = view points directly.
            let origin = NSPoint::new(pos.0 as f64 / scale, pos.1 as f64 / scale);
            let frame = NSRect::new(origin, NSSize::new(40.0, style.font_px as f64 / scale * 1.3));
            let view: Retained<QsTextView> =
                unsafe { msg_send![QsTextView::alloc(mtm), initWithFrame: frame] };

            let delegate: Retained<TextDelegate> = {
                let this = TextDelegate::alloc(mtm).set_ivars(DelegateIvars {
                    queue: RefCell::new(Some(queue)),
                });
                unsafe { msg_send![super(this), init] }
            };

            let tv: &NSTextView = &view;
            tv.setRichText(false);
            tv.setDrawsBackground(false);
            tv.setAllowsUndo(true);
            tv.setHorizontallyResizable(true);
            tv.setVerticallyResizable(true);
            tv.setMinSize(NSSize::new(8.0, 8.0));
            tv.setMaxSize(NSSize::new(1.0e6, 1.0e6));
            tv.setTextContainerInset(NSSize::new(0.0, 0.0));
            tv.setAutomaticQuoteSubstitutionEnabled(false);
            tv.setAutomaticDashSubstitutionEnabled(false);
            tv.setAutomaticTextReplacementEnabled(false);
            tv.setContinuousSpellCheckingEnabled(false);
            if let Some(tc) = unsafe { tv.textContainer() } {
                tc.setLineFragmentPadding(0.0);
                tc.setWidthTracksTextView(false);
                tc.setContainerSize(NSSize::new(1.0e6, 1.0e6));
            }
            tv.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));

            tv.setString(&NSString::from_str(initial));
            host.addSubview(tv);

            let ed = Self { view, _delegate: delegate, scale };
            ed.set_style(style);
            ed.focus();
            Some(ed)
        }

        pub fn set_style(&self, style: EditorStyle) {
            let tv: &NSTextView = &self.view;
            // Same font as the export rasterizer (crate::text), in points.
            let font = crate::text::annotation_font(style.font_px as f64 / self.scale);
            let color = ns_color(style.color_rgb);
            tv.setFont(Some(&font));
            tv.setTextColor(Some(&color));
            tv.setInsertionPointColor(Some(&color));
            tv.sizeToFit();
        }

        pub fn text(&self) -> String {
            self.view.string().to_string()
        }

        pub fn focus(&self) {
            let tv: &NSTextView = &self.view;
            if let Some(win) = tv.window() {
                win.makeFirstResponder(Some(tv));
            }
        }
    }

    impl Drop for Editor {
        fn drop(&mut self) {
            let view: &NSView = &self.view;
            // Hand keyboard focus back to winit's view before detaching so
            // overlay shortcuts (Esc / Enter / tool keys) keep working.
            if let (Some(win), Some(sup)) = (view.window(), unsafe { view.superview() }) {
                win.makeFirstResponder(Some(&sup));
            }
            let tv: &NSTextView = &self.view;
            tv.setDelegate(None);
            view.removeFromSuperview();
        }
    }
}

// ============================================================== Windows ====

#[cfg(target_os = "windows")]
mod imp {
    use super::{EditorEvent, EditorStyle, Queue};
    use std::cell::RefCell;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, ReleaseDC, SelectObject, DrawTextW, DT_CALCRECT, DT_NOPREFIX,
        DT_EXPANDTABS, HFONT, HGDIOBJ, HDC, SetTextColor, SetBkMode, SetBkColor, OPAQUE,
        CreateSolidBrush, HBRUSH,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VK_ESCAPE, VK_RETURN, VK_SHIFT};
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    const EDIT_SUBCLASS_ID: usize = 0x5153_4544; // 'QSED'
    const PARENT_SUBCLASS_ID: usize = 0x5153_5052; // 'QSPR'

    struct Shared {
        queue: Queue,
        color: u32,
        edit: HWND,
        /// Opaque editor background. A transparent EDIT inside a
        /// WS_CLIPCHILDREN parent leaves stale glyphs behind on edits, so we
        /// paint a solid box while editing; the committed annotation itself
        /// is rendered transparently over the screenshot.
        brush: HBRUSH,
    }

    const EDITOR_BG: u32 = 0x0020_2020; // COLORREF (BGR) dark gray

    pub struct Editor {
        edit: HWND,
        parent: HWND,
        font: RefCell<HFONT>,
        shared: Box<RefCell<Shared>>,
    }

    fn colorref(rgb: u32) -> u32 {
        // 0x00RRGGBB → COLORREF 0x00BBGGRR
        ((rgb & 0xFF) << 16) | (rgb & 0xFF00) | ((rgb >> 16) & 0xFF)
    }

    unsafe extern "system" fn edit_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        let shared = &*(data as *const RefCell<Shared>);
        match msg {
            WM_GETDLGCODE => return (DLGC_WANTALLKEYS | DLGC_WANTCHARS) as LRESULT,
            WM_KEYDOWN => {
                let vk = wparam as u16;
                if vk == VK_RETURN && GetKeyState(VK_SHIFT as i32) >= 0 {
                    shared.borrow().queue.push(EditorEvent::Commit);
                    return 0;
                }
                if vk == VK_ESCAPE {
                    shared.borrow().queue.push(EditorEvent::Cancel);
                    return 0;
                }
            }
            WM_CHAR => {
                // Swallow the CR/ESC chars matching the handled keydowns
                // (avoids the default beep); Shift+Enter still inserts CRLF.
                let c = wparam as u32;
                if (c == 0x0D && GetKeyState(VK_SHIFT as i32) >= 0) || c == 0x1B {
                    return 0;
                }
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// Parent subclass: color the EDIT's text and keep its background
    /// transparent so the screenshot shows through.
    unsafe extern "system" fn parent_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        data: usize,
    ) -> LRESULT {
        let shared = &*(data as *const RefCell<Shared>);
        if msg == WM_CTLCOLOREDIT && lparam as HWND == shared.borrow().edit {
            let hdc = wparam as HDC;
            SetTextColor(hdc, colorref(shared.borrow().color));
            SetBkColor(hdc, EDITOR_BG);
            SetBkMode(hdc, OPAQUE as _);
            return shared.borrow().brush as LRESULT;
        }
        if msg == WM_COMMAND && lparam as HWND == shared.borrow().edit {
            let code = ((wparam >> 16) & 0xFFFF) as u32;
            if code == EN_CHANGE {
                // Resize to content and repaint the parent beneath (the
                // transparent EDIT doesn't erase its own background).
                let ed = shared.borrow().edit;
                fit_to_content(ed);
            }
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    unsafe fn text_of(edit: HWND) -> String {
        let len = GetWindowTextLengthW(edit);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(edit, buf.as_mut_ptr(), buf.len() as i32);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    unsafe fn fit_to_content(edit: HWND) {
        let text = text_of(edit);
        let mut sample: Vec<u16> = if text.is_empty() { "M".into() } else { text + "M" }
            .encode_utf16()
            .collect();
        sample.push(0);
        let hdc = GetDC(edit);
        let font = SendMessageW(edit, WM_GETFONT, 0, 0) as HGDIOBJ;
        let old = SelectObject(hdc, font);
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        DrawTextW(hdc, sample.as_ptr(), -1, &mut rc, DT_CALCRECT | DT_NOPREFIX | DT_EXPANDTABS);
        SelectObject(hdc, old);
        ReleaseDC(edit, hdc);
        let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(edit, &mut wr);
        let parent = GetParent(edit);
        let mut pt = windows_sys::Win32::Foundation::POINT { x: wr.left, y: wr.top };
        windows_sys::Win32::Graphics::Gdi::ScreenToClient(parent, &mut pt);
        SetWindowPos(
            edit,
            std::ptr::null_mut(),
            0,
            0,
            rc.right + 8,
            rc.bottom + 4,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(parent, std::ptr::null(), 0);
    }

    impl Editor {
        pub fn new(
            window: &Window,
            pos: (i32, i32),
            initial: &str,
            style: EditorStyle,
            _scale: f64,
            queue: Queue,
        ) -> Option<Self> {
            let handle = window.window_handle().ok()?;
            let RawWindowHandle::Win32(h) = handle.as_raw() else { return None };
            let parent = h.hwnd.get() as HWND;
            unsafe {
                // Clip children so the overlay's softbuffer blit doesn't
                // paint over the edit control.
                let st = GetWindowLongPtrW(parent, GWL_STYLE);
                SetWindowLongPtrW(parent, GWL_STYLE, st | WS_CLIPCHILDREN as isize);

                let class = crate::text::wide("EDIT");
                let text = crate::text::wide(&initial.replace('\n', "\r\n"));
                let edit = CreateWindowExW(
                    0,
                    class.as_ptr(),
                    text.as_ptr(),
                    WS_CHILD
                        | WS_VISIBLE
                        | (ES_MULTILINE | ES_AUTOHSCROLL | ES_AUTOVSCROLL | ES_WANTRETURN) as u32,
                    pos.0,
                    pos.1,
                    64,
                    (style.font_px * 1.4) as i32,
                    parent,
                    std::ptr::null_mut(),
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                );
                if edit.is_null() {
                    return None;
                }
                let shared = Box::new(RefCell::new(Shared {
                    queue,
                    color: style.color_rgb,
                    edit,
                    brush: CreateSolidBrush(EDITOR_BG),
                }));
                let data = &*shared as *const RefCell<Shared> as usize;
                SetWindowSubclass(edit, Some(edit_proc), EDIT_SUBCLASS_ID, data);
                SetWindowSubclass(parent, Some(parent_proc), PARENT_SUBCLASS_ID, data);
                let ed = Self {
                    edit,
                    parent,
                    font: RefCell::new(std::ptr::null_mut()),
                    shared,
                };
                ed.set_style(style);
                let n = GetWindowTextLengthW(edit) as usize;
                SendMessageW(edit, windows_sys::Win32::UI::Controls::EM_SETSEL, n, n as isize);
                SetFocus(edit);
                Some(ed)
            }
        }

        pub fn set_style(&self, style: EditorStyle) {
            unsafe {
                let font = crate::text::create_annotation_font(style.font_px.round() as i32);
                SendMessageW(self.edit, WM_SETFONT, font as usize, 1);
                let old = self.font.replace(font);
                if !old.is_null() {
                    DeleteObject(old as HGDIOBJ);
                }
                self.shared.borrow_mut().color = style.color_rgb;
                fit_to_content(self.edit);
            }
        }

        pub fn text(&self) -> String {
            unsafe { text_of(self.edit).replace("\r\n", "\n") }
        }

        pub fn focus(&self) {
            unsafe { SetFocus(self.edit) };
        }
    }

    impl Drop for Editor {
        fn drop(&mut self) {
            unsafe {
                RemoveWindowSubclass(self.edit, Some(edit_proc), EDIT_SUBCLASS_ID);
                RemoveWindowSubclass(self.parent, Some(parent_proc), PARENT_SUBCLASS_ID);
                SetFocus(self.parent);
                DestroyWindow(self.edit);
                DeleteObject(self.shared.borrow().brush as HGDIOBJ);
                let f = *self.font.borrow();
                if !f.is_null() {
                    DeleteObject(f as HGDIOBJ);
                }
            }
        }
    }
}

// ================================================================ other ====

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    use super::{EditorStyle, Queue};
    use winit::window::Window;

    pub struct Editor;

    impl Editor {
        pub fn new(
            _w: &Window,
            _p: (i32, i32),
            _i: &str,
            _s: EditorStyle,
            _scale: f64,
            _q: Queue,
        ) -> Option<Self> {
            None
        }
        pub fn set_style(&self, _s: EditorStyle) {}
        pub fn text(&self) -> String {
            String::new()
        }
        pub fn focus(&self) {}
    }
}
