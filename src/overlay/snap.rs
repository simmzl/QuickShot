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
pub fn enumerate_windows(_monitor_geom: &MonitorGeom, _my_pid: u32) -> Vec<WindowEntry> {
    // Filled in by Task 3.
    Vec::new()
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
}
