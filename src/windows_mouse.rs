use windows_sys::Win32::{
    Foundation::{HWND, POINT},
    Graphics::Gdi::ScreenToClient,
    UI::{
        Input::KeyboardAndMouse::{GetCapture, GetKeyState, VK_LBUTTON},
        WindowsAndMessaging::GetCursorPos,
    },
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

fn hwnd(window: &Window) -> Option<HWND> {
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as HWND),
        _ => None,
    }
}

pub fn client_position(window: &Window) -> Option<(i32, i32)> {
    let hwnd = hwnd(window)?;
    let mut point = POINT { x: 0, y: 0 };
    unsafe {
        if GetCursorPos(&mut point) == 0 || ScreenToClient(hwnd, &mut point) == 0 {
            return None;
        }
    }
    Some((point.x, point.y))
}

// winit takes capture on mouse down. Windows can transfer it to another
// window without delivering a mouse-up or a focus change to this one.
pub fn has_left_capture(window: &Window) -> bool {
    hwnd(window).is_some_and(|hwnd| unsafe {
        GetCapture() == hwnd && GetKeyState(VK_LBUTTON as i32) < 0
    })
}
