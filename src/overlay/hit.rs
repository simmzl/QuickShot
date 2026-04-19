//! Classify a cursor position relative to an Adjusting-state selection rect.
//! Pure — no winit Window refs, no state. Unit-tested.

use winit::window::CursorIcon;

use super::state::{Anchor, Rect};

pub const ANCHOR_SIZE: i32 = 6;
/// Extra padding around each visible anchor that still counts as a hit.
/// Visible anchor is 6×6, hit box is 12×12 centered on the anchor point.
pub const ANCHOR_HIT_PAD: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitZone {
    Anchor(Anchor),
    Inside,
    Outside,
}

pub(crate) fn anchor_center(rect: Rect, anchor: Anchor) -> (i32, i32) {
    let (l, t) = (rect.x, rect.y);
    let (r, b) = (rect.x + rect.w - 1, rect.y + rect.h - 1);
    let (cx, cy) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
    match anchor {
        Anchor::TL => (l, t),
        Anchor::T  => (cx, t),
        Anchor::TR => (r, t),
        Anchor::R  => (r, cy),
        Anchor::BR => (r, b),
        Anchor::B  => (cx, b),
        Anchor::BL => (l, b),
        Anchor::L  => (l, cy),
    }
}

const ALL_ANCHORS: [Anchor; 8] = [
    Anchor::TL, Anchor::T, Anchor::TR, Anchor::R,
    Anchor::BR, Anchor::B, Anchor::BL, Anchor::L,
];

pub fn classify(cursor: (i32, i32), rect: Rect) -> HitZone {
    let half = ANCHOR_SIZE / 2 + ANCHOR_HIT_PAD;
    for a in ALL_ANCHORS {
        let (ax, ay) = anchor_center(rect, a);
        if (cursor.0 - ax).abs() <= half && (cursor.1 - ay).abs() <= half {
            return HitZone::Anchor(a);
        }
    }
    if rect.contains(cursor) {
        HitZone::Inside
    } else {
        HitZone::Outside
    }
}

pub fn cursor_icon_for(zone: HitZone) -> CursorIcon {
    match zone {
        HitZone::Anchor(Anchor::TL) | HitZone::Anchor(Anchor::BR) => CursorIcon::NwseResize,
        HitZone::Anchor(Anchor::TR) | HitZone::Anchor(Anchor::BL) => CursorIcon::NeswResize,
        HitZone::Anchor(Anchor::T)  | HitZone::Anchor(Anchor::B)  => CursorIcon::NsResize,
        HitZone::Anchor(Anchor::L)  | HitZone::Anchor(Anchor::R)  => CursorIcon::EwResize,
        HitZone::Inside => CursorIcon::Move,
        HitZone::Outside => CursorIcon::Default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect() -> Rect { Rect { x: 100, y: 100, w: 100, h: 100 } }

    #[test]
    fn anchor_centers_are_rect_corners_and_edge_midpoints() {
        let r = rect();
        assert_eq!(anchor_center(r, Anchor::TL), (100, 100));
        assert_eq!(anchor_center(r, Anchor::TR), (199, 100));
        assert_eq!(anchor_center(r, Anchor::BR), (199, 199));
        assert_eq!(anchor_center(r, Anchor::BL), (100, 199));
        assert_eq!(anchor_center(r, Anchor::T),  (150, 100));
        assert_eq!(anchor_center(r, Anchor::B),  (150, 199));
        assert_eq!(anchor_center(r, Anchor::L),  (100, 150));
        assert_eq!(anchor_center(r, Anchor::R),  (199, 150));
    }

    #[test]
    fn classify_each_anchor_center_hits() {
        for a in ALL_ANCHORS {
            let c = anchor_center(rect(), a);
            assert_eq!(classify(c, rect()), HitZone::Anchor(a), "{a:?} miss");
        }
    }

    #[test]
    fn classify_respects_hit_pad() {
        let half = ANCHOR_SIZE / 2 + ANCHOR_HIT_PAD;
        assert_eq!(classify((100 + half, 100 + half), rect()), HitZone::Anchor(Anchor::TL));
        assert_eq!(classify((100 + half + 1, 100 + half + 1), rect()), HitZone::Inside);
    }

    #[test]
    fn classify_inside_not_on_anchor() {
        assert_eq!(classify((150, 150), rect()), HitZone::Inside);
    }

    #[test]
    fn classify_outside() {
        assert_eq!(classify((10, 10), rect()), HitZone::Outside);
        assert_eq!(classify((300, 300), rect()), HitZone::Outside);
    }

    #[test]
    fn cursor_icons() {
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::TL)), CursorIcon::NwseResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::TR)), CursorIcon::NeswResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::T)),  CursorIcon::NsResize);
        assert_eq!(cursor_icon_for(HitZone::Anchor(Anchor::L)),  CursorIcon::EwResize);
        assert_eq!(cursor_icon_for(HitZone::Inside),  CursorIcon::Move);
        assert_eq!(cursor_icon_for(HitZone::Outside), CursorIcon::Default);
    }
}
