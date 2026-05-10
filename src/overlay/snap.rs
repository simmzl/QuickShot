//! Window enumeration + cursor-to-window hit testing for smart snap mode.
//!
//! macOS uses CGWindowListCopyWindowInfo (already permitted by the existing
//! Screen Recording grant — no new permission prompt). Windows / Linux paths
//! return empty lists for now.

use super::state::Rect;
use crate::capture::MonitorGeom;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowEntry {
    /// In overlay-window coordinates (monitor-local — already translated from
    /// CG screen-space by subtracting the monitor's origin).
    pub bounds: Rect,
    pub layer: i32,
}

/// Return the bounds of the topmost window whose rect contains `cursor`,
/// or None if none does. `entries` must already be in z-order (front to back).
pub fn window_under_cursor(cursor: (i32, i32), entries: &[WindowEntry]) -> Option<Rect> {
    entries.iter().find(|e| e.bounds.contains(cursor)).map(|e| e.bounds)
}

#[cfg(target_os = "macos")]
pub fn enumerate_windows(monitor_geom: &MonitorGeom, my_pid: u32) -> Vec<WindowEntry> {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;

    const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0; // 0x01
    const KCG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4; // 0x10
    const KCG_NULL_WINDOW_ID: u32 = 0;

    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
    }

    let option =
        KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY | KCG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS;
    let array_ref = unsafe { CGWindowListCopyWindowInfo(option, KCG_NULL_WINDOW_ID) };
    if array_ref.is_null() {
        return Vec::new();
    }

    // SAFETY: CGWindowListCopyWindowInfo returns a +1 retained CFArray; wrap
    // under create rule so it's released on Drop. We use the untyped
    // CFDictionary because only the untyped variant impls ConcreteCFType
    // (needed below for downcast).
    let array: CFArray<CFDictionary> =
        unsafe { CFArray::wrap_under_create_rule(array_ref) };

    let mut out = Vec::with_capacity(array.len() as usize);
    for dict_item in array.iter() {
        let dict = &*dict_item;

        let pid = read_i64(dict, "kCGWindowOwnerPID").unwrap_or(0);
        if pid as u32 == my_pid {
            continue;
        }

        let layer = read_i64(dict, "kCGWindowLayer").unwrap_or(i64::MIN) as i32;
        if layer != 0 {
            continue;
        }

        let Some(bounds_dict) = read_dict(dict, "kCGWindowBounds") else {
            continue;
        };
        let cg_x = read_f64(&bounds_dict, "X").unwrap_or(0.0);
        let cg_y = read_f64(&bounds_dict, "Y").unwrap_or(0.0);
        let cg_w = read_f64(&bounds_dict, "Width").unwrap_or(0.0);
        let cg_h = read_f64(&bounds_dict, "Height").unwrap_or(0.0);

        if cg_w < 50.0 || cg_h < 50.0 {
            continue;
        }

        let local_x = (cg_x as i32) - monitor_geom.x;
        let local_y = (cg_y as i32) - monitor_geom.y;

        out.push(WindowEntry {
            bounds: Rect {
                x: local_x,
                y: local_y,
                w: cg_w as i32,
                h: cg_h as i32,
            },
            layer,
        });
    }

    out
}

/// Look up `key` in the untyped dict and downcast the value to `T`.
///
/// Untyped CFDictionary values come back as raw `*const c_void`; we wrap
/// them as a CFType (get-rule, since the dict still owns them) and let
/// `downcast` verify the runtime CFTypeID before producing `T`.
#[cfg(target_os = "macos")]
fn read_value<T: core_foundation::ConcreteCFType>(
    dict: &core_foundation::dictionary::CFDictionary,
    key: &str,
) -> Option<T> {
    use core_foundation::base::{CFType, CFTypeRef, TCFType, ToVoid};
    use core_foundation::string::CFString;
    let key_cf = CFString::new(key);
    let raw = *dict.find(key_cf.to_void())?;
    if raw.is_null() {
        return None;
    }
    let val: CFType = unsafe { TCFType::wrap_under_get_rule(raw as CFTypeRef) };
    val.downcast::<T>()
}

#[cfg(target_os = "macos")]
fn read_i64(dict: &core_foundation::dictionary::CFDictionary, key: &str) -> Option<i64> {
    let num: core_foundation::number::CFNumber = read_value(dict, key)?;
    num.to_i64().or_else(|| num.to_i32().map(|v| v as i64))
}

#[cfg(target_os = "macos")]
fn read_f64(dict: &core_foundation::dictionary::CFDictionary, key: &str) -> Option<f64> {
    let num: core_foundation::number::CFNumber = read_value(dict, key)?;
    num.to_f64()
}

#[cfg(target_os = "macos")]
fn read_dict(
    dict: &core_foundation::dictionary::CFDictionary,
    key: &str,
) -> Option<core_foundation::dictionary::CFDictionary> {
    read_value(dict, key)
}

#[cfg(not(target_os = "macos"))]
pub fn enumerate_windows(_monitor_geom: &MonitorGeom, _my_pid: u32) -> Vec<WindowEntry> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(x: i32, y: i32, w: i32, h: i32, layer: i32) -> WindowEntry {
        WindowEntry { bounds: Rect { x, y, w, h }, layer }
    }

    #[test]
    fn window_under_cursor_returns_topmost() {
        // Both entries contain (75, 75); the first (topmost in z-order) wins.
        let entries = vec![
            entry(0, 0, 200, 200, 0),
            entry(50, 50, 200, 200, 0),
        ];
        assert_eq!(
            window_under_cursor((75, 75), &entries),
            Some(Rect { x: 0, y: 0, w: 200, h: 200 })
        );
    }

    #[test]
    fn window_under_cursor_returns_none_when_no_match() {
        let entries = vec![entry(0, 0, 100, 100, 0)];
        assert_eq!(window_under_cursor((500, 500), &entries), None);
    }

    #[test]
    fn window_under_cursor_skips_empty_list() {
        assert_eq!(window_under_cursor((0, 0), &[]), None);
    }

    #[test]
    fn window_under_cursor_finds_only_match() {
        let entries = vec![
            entry(0, 0, 100, 100, 0),
            entry(200, 200, 100, 100, 0),
        ];
        assert_eq!(
            window_under_cursor((250, 250), &entries),
            Some(Rect { x: 200, y: 200, w: 100, h: 100 })
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn enumerate_windows_returns_some_windows_on_macos() {
        // On any Mac running this test there should be at least one user
        // window (Finder, Terminal, IDE…). Smoke test only — exact contents
        // depend on the test machine.
        let geom = MonitorGeom { x: 0, y: 0, width: 1920, height: 1080 };
        let entries = enumerate_windows(&geom, std::process::id());
        assert!(
            !entries.is_empty(),
            "expected at least one window in the list on macOS",
        );
    }
}
