use super::{clipped, element_at, MonitorGeom, Rect, WindowEntry};
use crate::app::UserEvent;
use std::{collections::HashMap, mem::size_of, sync::mpsc, thread};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, POINT, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS},
    Graphics::Gdi::ClientToScreen,
    UI::WindowsAndMessaging::*,
};
use winit::event_loop::EventLoopProxy;

struct WindowList {
    monitor: Rect,
    my_pid: u32,
    entries: Vec<WindowEntry>,
}

pub fn enumerate_windows(geom: &MonitorGeom, my_pid: u32, _: f32) -> Vec<WindowEntry> {
    let mut list = WindowList {
        monitor: Rect {
            x: geom.x,
            y: geom.y,
            w: geom.width as i32,
            h: geom.height as i32,
        },
        my_pid,
        entries: Vec::new(),
    };
    // EnumWindows visits top-level windows before the capture overlay is shown.
    unsafe { EnumWindows(Some(visit_window), &mut list as *mut _ as LPARAM) };
    list.entries
}

unsafe extern "system" fn visit_window(hwnd: HWND, data: LPARAM) -> i32 {
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return 1;
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 || pid == (*(data as *const WindowList)).my_pid {
        return 1;
    }
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        (&mut cloaked as *mut u32).cast(),
        size_of::<u32>() as u32,
    );
    if cloaked != 0 {
        return 1;
    }
    let mut name = [0u16; 128];
    let count = GetClassNameW(hwnd, name.as_mut_ptr(), name.len() as i32);
    match String::from_utf16_lossy(&name[..count as usize]).as_str() {
        // A desktop widget can be parented to the wallpaper host instead of
        // appearing as a top-level window. Keep its place in desktop z-order.
        "Progman" | "WorkerW" => {
            let mut child = GetWindow(hwnd, GW_CHILD);
            while !child.is_null() {
                visit_window(child, data);
                child = GetWindow(child, GW_HWNDNEXT);
            }
            return 1;
        }
        "SHELLDLL_DefView" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "tooltips_class32" => {
            return 1
        }
        _ => {}
    }
    let list = &mut *(data as *mut WindowList);
    let mut r = std::mem::zeroed::<RECT>();
    if DwmGetWindowAttribute(
        hwnd,
        DWMWA_EXTENDED_FRAME_BOUNDS as u32,
        (&mut r as *mut RECT).cast(),
        size_of::<RECT>() as u32,
    ) < 0
        && GetWindowRect(hwnd, &mut r) == 0
    {
        return 1;
    }
    if let Some(bounds) = clipped(r.left, r.top, r.right, r.bottom, list.monitor) {
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if bounds != list.monitor || ex & WS_EX_TRANSPARENT == 0 {
            list.entries.push(WindowEntry {
                bounds: bounds.translate(-list.monitor.x, -list.monitor.y),
                hwnd: hwnd as isize,
                pid,
            });
        }
    }
    1
}

// One lookup at a time, restricted to the window actually under the pointer.
// The receiver and its cache belong to this capture. Late replies cannot
// update a later overlay, and a slow external provider cannot block the UI.
pub struct ElementCache {
    pub element_mode: bool,
    origin: (i32, i32),
    elements: HashMap<isize, Vec<Rect>>,
    pending: Option<mpsc::Receiver<(isize, Vec<Rect>)>>,
}

impl ElementCache {
    pub fn new(geom: &MonitorGeom) -> Self {
        Self {
            element_mode: false,
            origin: (geom.x, geom.y),
            elements: HashMap::new(),
            pending: None,
        }
    }

    pub fn target(
        &mut self,
        cursor: (i32, i32),
        windows: &[WindowEntry],
        proxy: &EventLoopProxy<UserEvent>,
    ) -> Option<Rect> {
        if !self.element_mode {
            return super::window_under_cursor(cursor, windows);
        }
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok((hwnd, elements)) => {
                    self.elements.insert(hwnd, elements);
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let window = *windows.iter().find(|w| w.bounds.contains(cursor))?;
        if let Some(elements) = self.elements.get(&window.hwnd) {
            return Some(element_at(cursor, window.bounds, elements));
        }
        if self.pending.is_none() {
            let (tx, rx) = mpsc::channel();
            let origin = self.origin;
            let proxy = proxy.clone();
            self.pending = Some(rx);
            thread::spawn(move || {
                let elements = window_elements(window, origin);
                if tx.send((window.hwnd, elements)).is_ok() {
                    let _ = proxy.send_event(UserEvent::Redraw);
                }
            });
        }
        Some(window.bounds)
    }
}

fn window_elements(window: WindowEntry, origin: (i32, i32)) -> Vec<Rect> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(window.hwnd as HWND, &mut pid) };
    if pid != window.pid {
        return Vec::new();
    }
    let mut elements = Vec::new();
    // Keep the non-client frame (including the title bar) as a whole-window
    // target. Work in physical screen pixels before translating to overlay pixels.
    let hwnd = window.hwnd as HWND;
    let frame = window.bounds.translate(origin.0, origin.1);
    let outer = unsafe {
        let mut client = std::mem::zeroed::<RECT>();
        let mut offset = POINT { x: 0, y: 0 };
        if GetClientRect(hwnd, &mut client) == 0 || ClientToScreen(hwnd, &mut offset) == 0 {
            return Vec::new();
        }
        let Some(client) = clipped(
            offset.x,
            offset.y,
            offset.x.saturating_add(client.right),
            offset.y.saturating_add(client.bottom),
            frame,
        ) else {
            return Vec::new();
        };
        client
    };
    let mut add = |r: RECT| {
        if let Some(rect) = clipped(r.left, r.top, r.right, r.bottom, outer) {
            if rect != outer && rect.w >= 4 && rect.h >= 4 {
                elements.push(rect.translate(-origin.0, -origin.1));
            }
        }
    };
    // Legacy controls may expose only child HWNDs. Keep these even when the
    // application has no UI Automation provider.
    unsafe extern "system" fn child(hwnd: HWND, data: LPARAM) -> i32 {
        let add = &mut *(data as *mut &mut dyn FnMut(RECT));
        let mut r = std::mem::zeroed::<RECT>();
        if IsWindowVisible(hwnd) != 0 && GetWindowRect(hwnd, &mut r) != 0 {
            add(r);
        }
        1
    }
    unsafe {
        let mut callback: &mut dyn FnMut(RECT) = &mut add;
        EnumChildWindows(
            window.hwnd as HWND,
            Some(child),
            &mut callback as *mut _ as LPARAM,
        );
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() {
            let _ = automation_elements(window.hwnd, &mut add);
            CoUninitialize();
        }
    }
    elements
}

unsafe fn automation_elements(hwnd: isize, mut add: impl FnMut(RECT)) -> windows::core::Result<()> {
    use windows::Win32::{
        Foundation::HWND as WindowHandle,
        System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER},
        UI::Accessibility::*,
    };
    let automation: IUIAutomation2 = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
    automation.SetConnectionTimeout(400)?;
    automation.SetTransactionTimeout(700)?;
    // Anchor to the captured application, as ElementFromPoint would hit our overlay.
    let root = automation.ElementFromHandle(WindowHandle(hwnd as _))?;
    let cache = automation.CreateCacheRequest()?;
    cache.SetAutomationElementMode(AutomationElementMode_None)?;
    cache.AddProperty(UIA_BoundingRectanglePropertyId)?;
    cache.AddProperty(UIA_IsOffscreenPropertyId)?;
    let found = root.FindAllBuildCache(
        TreeScope_Descendants,
        &automation.ControlViewCondition()?,
        &cache,
    )?;
    for i in 0..found.Length()? {
        let Ok(el) = found.GetElement(i) else {
            continue;
        };
        if el.CachedIsOffscreen().is_ok_and(|v| v.as_bool()) {
            continue;
        }
        if let Ok(r) = el.CachedBoundingRectangle() {
            add(RECT {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerates_a_real_window_and_snaps_to_its_child_button() {
        // Pump messages on the window's owning thread while Automation queries
        // its controls, just as it would for another application's window.
        let (ready, window) = mpsc::channel();
        let owner = thread::spawn(move || unsafe {
            let parent = CreateWindowExW(
                WS_EX_NOACTIVATE,
                windows_sys::core::w!("STATIC"),
                std::ptr::null(),
                WS_POPUP,
                60,
                80,
                320,
                200,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!parent.is_null());
            let button = CreateWindowExW(
                0,
                windows_sys::core::w!("BUTTON"),
                windows_sys::core::w!("Snap me"),
                WS_CHILD | WS_VISIBLE,
                20,
                30,
                100,
                40,
                parent,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!button.is_null());
            ShowWindow(parent, SW_SHOWNOACTIVATE);
            let mut r = std::mem::zeroed::<RECT>();
            assert_ne!(GetWindowRect(button, &mut r), 0);
            ready
                .send((parent as isize, r.left, r.top, r.right, r.bottom))
                .unwrap();
            let mut msg = std::mem::zeroed::<MSG>();
            while IsWindow(parent) != 0 && GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let (hwnd, l, t, r, b) = window.recv().unwrap();
        struct Close(isize);
        impl Drop for Close {
            fn drop(&mut self) {
                unsafe {
                    PostMessageW(self.0 as HWND, WM_CLOSE, 0, 0);
                }
            }
        }
        let close = Close(hwnd);
        let geom = MonitorGeom {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
        };
        let windows = enumerate_windows(&geom, 0, 1.0);
        let root = *windows
            .iter()
            .find(|entry| entry.hwnd == hwnd)
            .expect("visible test window");
        let elements = window_elements(root, (0, 0));
        let expected = Rect {
            x: l,
            y: t,
            w: r - l,
            h: b - t,
        };
        assert_eq!(element_at((l + 2, t + 2), root.bounds, &elements), expected);
        assert_eq!(
            element_at(
                (root.bounds.x + 3, root.bounds.y + 3),
                root.bounds,
                &elements
            ),
            root.bounds
        );
        assert!(enumerate_windows(&geom, std::process::id(), 1.0)
            .iter()
            .all(|w| w.hwnd != hwnd));
        drop(close);
        owner.join().unwrap();
    }
}
