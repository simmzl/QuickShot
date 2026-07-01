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
pub fn enumerate_windows(
    monitor_geom: &MonitorGeom,
    my_pid: u32,
    scale_factor: f32,
) -> Vec<WindowEntry> {
    use core_foundation::array::{CFArray, CFArrayRef};
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;

    const KCG_WINDOW_LIST_OPTION_ON_SCREEN_ONLY: u32 = 1 << 0; // 0x01
    const KCG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4; // 0x10
    const KCG_NULL_WINDOW_ID: u32 = 0;
    /// Drop windows smaller than this (notification dots, badges, menu items).
    /// Logical points — applies pre-scale, so a "small badge" stays small
    /// regardless of display DPI.
    const MIN_WINDOW_DIMENSION_PX: f64 = 50.0;

    extern "C" {
        fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
    }

    // Per-window snap tracing is verbose (one line for every on-screen window
    // on each capture) and adds synchronous stderr latency to overlay open.
    // Gate it behind QUICKSHOT_DEBUG so it's opt-in for diagnostics only.
    let debug = std::env::var_os("QUICKSHOT_DEBUG").is_some();

    if debug {
        eprintln!(
            "QuickShot: snap enumeration: monitor_geom=({}, {}, {}x{}) scale={}",
            monitor_geom.x, monitor_geom.y, monitor_geom.width, monitor_geom.height, scale_factor,
        );
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

        // Missing pid / layer / bounds field → drop this window. CG should
        // always populate them for an on-screen window, so absence likely means
        // a transient or partially-initialized entry we don't want to snap to.
        let Some(pid) = read_i64(dict, "kCGWindowOwnerPID") else { continue };
        // Defensive: real CG windows always have positive pids. A zero or
        // negative pid means a transient / corrupt entry we don't want to snap.
        if pid <= 0 {
            continue;
        }
        // Skip our own overlay window so we don't snap to ourselves.
        if pid as u64 == my_pid as u64 {
            continue;
        }

        let Some(layer) = read_i64(dict, "kCGWindowLayer") else { continue };

        // Read owner + window names for diagnostics (best-effort — may be
        // absent for some apps). Only used in the QUICKSHOT_DEBUG eprintlns, so
        // skip the CFString→String allocation entirely when not tracing.
        let (owner_name, window_name) = if debug {
            (
                read_string(dict, "kCGWindowOwnerName").unwrap_or_default(),
                read_string(dict, "kCGWindowName").unwrap_or_default(),
            )
        } else {
            (String::new(), String::new())
        };

        // Layer filter: accept any user-facing window (0..20). This catches
        // normal app windows (0), floating panels like iTerm's hotkey window
        // (3), torn-off menus (4), modal panels (8), utility windows (19),
        // and any custom levels apps use for "always on top" panels. Excludes
        // dock (20), menu bar (24+), popup menus (100+), our overlay (1500),
        // and wallpaper (negative).
        let layer_ok = layer >= 0 && layer < 20;
        if !layer_ok {
            if debug {
                eprintln!(
                    "snap-debug: SKIP layer={} pid={} owner={:?} name={:?} (layer out of range)",
                    layer, pid, owner_name, window_name,
                );
            }
            continue;
        }
        let layer = layer as i32;

        let Some(bounds_dict) = read_dict(dict, "kCGWindowBounds") else { continue };
        let Some(cg_x) = read_f64(&bounds_dict, "X") else { continue };
        let Some(cg_y) = read_f64(&bounds_dict, "Y") else { continue };
        let Some(cg_w) = read_f64(&bounds_dict, "Width") else { continue };
        let Some(cg_h) = read_f64(&bounds_dict, "Height") else { continue };

        if cg_w < MIN_WINDOW_DIMENSION_PX || cg_h < MIN_WINDOW_DIMENSION_PX {
            if debug {
                eprintln!(
                    "snap-debug: SKIP small layer={} pid={} owner={:?} name={:?} size={}x{}",
                    layer, pid, owner_name, window_name, cg_w as i32, cg_h as i32,
                );
            }
            continue;
        }

        // CG returns bounds in points (logical units). winit's cursor and the
        // softbuffer are in physical pixels, so multiply by scale_factor to
        // bring window bounds into the same unit.
        let local_logical_x = cg_x - monitor_geom.x as f64;
        let local_logical_y = cg_y - monitor_geom.y as f64;
        let local_x = (local_logical_x * scale_factor as f64).round() as i32;
        let local_y = (local_logical_y * scale_factor as f64).round() as i32;
        let phys_w = (cg_w * scale_factor as f64).round() as i32;
        let phys_h = (cg_h * scale_factor as f64).round() as i32;

        if debug {
            eprintln!(
                "snap-debug: KEEP layer={} pid={} owner={:?} name={:?} cg=({},{},{}x{}) win=({},{},{}x{})",
                layer, pid, owner_name, window_name,
                cg_x as i32, cg_y as i32, cg_w as i32, cg_h as i32,
                local_x, local_y, phys_w, phys_h,
            );
        }

        out.push(WindowEntry {
            bounds: Rect {
                x: local_x,
                y: local_y,
                w: phys_w,
                h: phys_h,
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

#[cfg(target_os = "macos")]
fn read_string(
    dict: &core_foundation::dictionary::CFDictionary,
    key: &str,
) -> Option<String> {
    let s: core_foundation::string::CFString = read_value(dict, key)?;
    Some(s.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn enumerate_windows(
    _monitor_geom: &MonitorGeom,
    _my_pid: u32,
    _scale_factor: f32,
) -> Vec<WindowEntry> {
    Vec::new()
}

/// Returns true if the cursor has moved at least 4 pixels (Manhattan
/// distance) from the press position. Used to disambiguate "click on a
/// window to snap" from "drag a rectangle".
pub fn drag_threshold_exceeded(start: (i32, i32), now: (i32, i32)) -> bool {
    (now.0 - start.0).abs() + (now.1 - start.1).abs() >= 4
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
    #[ignore = "requires a desktop session with at least one user window; \
                unavailable on headless CI runners"]
    fn enumerate_windows_returns_some_windows_on_macos() {
        // On any Mac running this test there should be at least one user
        // window (Finder, Terminal, IDE…). Smoke test only — exact contents
        // depend on the test machine. Ignored in CI: headless runners expose
        // only system UI (Dock, Menubar…), which the layer filter drops.
        let geom = MonitorGeom { x: 0, y: 0, width: 1920, height: 1080 };
        let entries = enumerate_windows(&geom, std::process::id(), 1.0);
        assert!(
            !entries.is_empty(),
            "expected at least one window in the list on macOS",
        );
    }

    #[test]
    fn drag_threshold_at_zero_is_false() {
        assert!(!drag_threshold_exceeded((0, 0), (0, 0)));
    }

    #[test]
    fn drag_threshold_under_4_is_false() {
        assert!(!drag_threshold_exceeded((0, 0), (1, 1)));
        assert!(!drag_threshold_exceeded((0, 0), (3, 0)));
        assert!(!drag_threshold_exceeded((0, 0), (0, 3)));
        assert!(!drag_threshold_exceeded((0, 0), (1, 2)));
    }

    #[test]
    fn drag_threshold_at_or_over_4_is_true() {
        assert!(drag_threshold_exceeded((0, 0), (4, 0)));
        assert!(drag_threshold_exceeded((0, 0), (0, 4)));
        assert!(drag_threshold_exceeded((0, 0), (2, 2)));
        assert!(drag_threshold_exceeded((10, 10), (12, 12)));
    }

    #[test]
    fn drag_threshold_works_with_negative_motion() {
        // |Δx| + |Δy| uses .abs(), so cursor moving up-left also counts.
        assert!(drag_threshold_exceeded((10, 10), (6, 10)));
        assert!(drag_threshold_exceeded((10, 10), (10, 6)));
        assert!(drag_threshold_exceeded((10, 10), (8, 8)));
        assert!(!drag_threshold_exceeded((10, 10), (9, 9)));
    }
}
