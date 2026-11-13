//! A short-lived, click-through highlight on the captured monitor.
use crate::capture::MonitorGeom;
use std::time::{Duration, Instant};

pub struct Flash {
    pub until: Instant,
    #[cfg(windows)]
    window: windows_sys::Win32::Foundation::HWND,
}

impl Flash {
    pub fn show(geom: MonitorGeom) -> anyhow::Result<Self> {
        #[cfg(windows)]
        let window = windows::show(geom)?;
        #[cfg(target_os = "macos")]
        unsafe {
            qs_show_flash(geom.display_id);
        }
        Ok(Self {
            until: Instant::now() + Duration::from_millis(160),
            #[cfg(windows)]
            window,
        })
    }
}

impl Drop for Flash {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(self.window);
            // Commit the hide before another capture on older Windows versions.
            windows_sys::Win32::Graphics::Dwm::DwmFlush();
        }
        #[cfg(target_os = "macos")]
        unsafe {
            qs_hide_flash();
        }
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn qs_show_flash(display_id: u32);
    fn qs_hide_flash();
}

#[cfg(windows)]
mod windows {
    use super::MonitorGeom;
    use windows_sys::Win32::{Foundation::HWND, UI::WindowsAndMessaging::*};

    pub fn show(geom: MonitorGeom) -> anyhow::Result<HWND> {
        unsafe {
            // The built-in STATIC class paints SS_WHITERECT without a render surface.
            const SS_WHITERECT: u32 = 6;
            let window = CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
                windows_sys::core::w!("STATIC"),
                std::ptr::null(),
                WS_POPUP | SS_WHITERECT,
                geom.x,
                geom.y,
                geom.width as i32,
                geom.height as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if window.is_null() {
                return Err(std::io::Error::last_os_error().into());
            }
            if SetLayeredWindowAttributes(window, 0, 76, LWA_ALPHA) == 0 {
                let error = std::io::Error::last_os_error();
                DestroyWindow(window);
                return Err(error.into());
            }
            SetWindowDisplayAffinity(window, WDA_EXCLUDEFROMCAPTURE);
            ShowWindow(window, SW_SHOWNOACTIVATE);
            windows_sys::Win32::Graphics::Gdi::UpdateWindow(window);
            Ok(window)
        }
    }
}
