//! Pure state machine + geometry helpers for the overlay.
//! No winit, no softbuffer — everything here is unit-testable.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor { TL, T, TR, R, BR, B, BL, L }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    Resize { anchor: Anchor, origin: Rect, from: (i32, i32) },
    Translate { origin: Rect, from: (i32, i32) },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayState {
    Idle,
    Dragging { start: (i32, i32), end: (i32, i32) },
    Adjusting { rect: Rect, edit: Option<Edit> },
}

impl Rect {
    pub fn normalize(a: (i32, i32), b: (i32, i32)) -> Rect {
        let (ax, ay) = a;
        let (bx, by) = b;
        let x = ax.min(bx);
        let y = ay.min(by);
        let w = (ax - bx).abs();
        let h = (ay - by).abs();
        Rect { x, y, w, h }
    }

    pub fn contains(&self, p: (i32, i32)) -> bool {
        p.0 >= self.x && p.0 < self.x + self.w && p.1 >= self.y && p.1 < self.y + self.h
    }

    pub fn clamp_to(&self, bounds: (u32, u32)) -> Rect {
        let (bw, bh) = (bounds.0 as i32, bounds.1 as i32);
        let x = self.x.clamp(0, bw);
        let y = self.y.clamp(0, bh);
        let x2 = (self.x + self.w).clamp(0, bw);
        let y2 = (self.y + self.h).clamp(0, bh);
        Rect { x, y, w: (x2 - x).max(0), h: (y2 - y).max(0) }
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect { x: self.x + dx, y: self.y + dy, w: self.w, h: self.h }
    }

    pub fn resize_from_anchor(&self, anchor: Anchor, dx: i32, dy: i32) -> Rect {
        let (mut x, mut y, mut w, mut h) = (self.x, self.y, self.w, self.h);
        match anchor {
            Anchor::TL => { x += dx; y += dy; w -= dx; h -= dy; }
            Anchor::T  => { y += dy; h -= dy; }
            Anchor::TR => { y += dy; w += dx; h -= dy; }
            Anchor::R  => { w += dx; }
            Anchor::BR => { w += dx; h += dy; }
            Anchor::B  => { h += dy; }
            Anchor::BL => { x += dx; w -= dx; h += dy; }
            Anchor::L  => { x += dx; w -= dx; }
        }
        if w < 1 { w = 1; }
        if h < 1 { h = 1; }
        Rect { x, y, w, h }
    }

    pub fn as_tuple_u32(&self) -> (u32, u32, u32, u32) {
        (
            self.x.max(0) as u32,
            self.y.max(0) as u32,
            self.w.max(0) as u32,
            self.h.max(0) as u32,
        )
    }
}

/// Returned by confirm/cancel helpers so the caller (app.rs) knows what to do.
#[derive(Debug, Clone, Copy)]
pub enum Transition {
    Stay,
    Confirm(Rect),
    Cancel,
}

pub fn on_mouse_down_idle(cursor: (i32, i32)) -> OverlayState {
    OverlayState::Dragging { start: cursor, end: cursor }
}

pub fn on_mouse_move_dragging(start: (i32, i32), cursor: (i32, i32)) -> OverlayState {
    OverlayState::Dragging { start, end: cursor }
}

pub fn on_mouse_up_dragging(start: (i32, i32), end: (i32, i32)) -> OverlayState {
    OverlayState::Adjusting {
        rect: Rect::normalize(start, end),
        edit: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_reversed_drag() {
        let r = Rect::normalize((80, 60), (20, 10));
        assert_eq!(r, Rect { x: 20, y: 10, w: 60, h: 50 });
    }

    #[test]
    fn normalize_zero_drag() {
        let r = Rect::normalize((50, 50), (50, 50));
        assert_eq!(r, Rect { x: 50, y: 50, w: 0, h: 0 });
    }

    #[test]
    fn contains_inside_and_edge() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 };
        assert!(r.contains((10, 10)));
        assert!(r.contains((29, 29)));
        assert!(!r.contains((30, 30)));
        assert!(!r.contains((9, 10)));
    }

    #[test]
    fn clamp_shrinks_rect() {
        let r = Rect { x: -5, y: -5, w: 100, h: 100 }.clamp_to((50, 50));
        assert_eq!(r, Rect { x: 0, y: 0, w: 50, h: 50 });
    }

    #[test]
    fn translate_offsets() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 }.translate(3, -4);
        assert_eq!(r, Rect { x: 13, y: 6, w: 5, h: 5 });
    }

    #[test]
    fn resize_from_br_grows_both() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 }
            .resize_from_anchor(Anchor::BR, 5, 7);
        assert_eq!(r, Rect { x: 10, y: 10, w: 25, h: 27 });
    }

    #[test]
    fn resize_from_tl_grows_up_left() {
        let r = Rect { x: 10, y: 10, w: 20, h: 20 }
            .resize_from_anchor(Anchor::TL, -3, -4);
        assert_eq!(r, Rect { x: 7, y: 6, w: 23, h: 24 });
    }

    #[test]
    fn resize_collapses_to_minimum_1px() {
        let r = Rect { x: 10, y: 10, w: 5, h: 5 }
            .resize_from_anchor(Anchor::BR, -100, -100);
        assert_eq!(r.w, 1);
        assert_eq!(r.h, 1);
    }

    #[test]
    fn resize_each_edge_only_moves_that_edge() {
        let base = Rect { x: 10, y: 10, w: 20, h: 20 };
        assert_eq!(
            base.resize_from_anchor(Anchor::T, 0, -5),
            Rect { x: 10, y: 5, w: 20, h: 25 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::B, 0, 5),
            Rect { x: 10, y: 10, w: 20, h: 25 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::L, -5, 0),
            Rect { x: 5, y: 10, w: 25, h: 20 }
        );
        assert_eq!(
            base.resize_from_anchor(Anchor::R, 5, 0),
            Rect { x: 10, y: 10, w: 25, h: 20 }
        );
    }

    #[test]
    fn mouse_down_idle_starts_dragging() {
        let s = on_mouse_down_idle((42, 17));
        assert!(matches!(
            s,
            OverlayState::Dragging { start: (42, 17), end: (42, 17) }
        ));
    }

    #[test]
    fn mouse_move_dragging_updates_end() {
        let s = on_mouse_move_dragging((10, 10), (25, 30));
        assert!(matches!(
            s,
            OverlayState::Dragging { start: (10, 10), end: (25, 30) }
        ));
    }

    #[test]
    fn mouse_up_dragging_enters_adjusting() {
        let s = on_mouse_up_dragging((80, 60), (20, 10));
        match s {
            OverlayState::Adjusting { rect, edit } => {
                assert_eq!(rect, Rect { x: 20, y: 10, w: 60, h: 50 });
                assert!(edit.is_none());
            }
            _ => panic!("expected Adjusting, got {s:?}"),
        }
    }
}
