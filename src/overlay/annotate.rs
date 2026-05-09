//! Pure types + undo/redo for annotation state. No IO, no drawing.

use super::state::Rect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Yellow,
    Green,
    Blue,
}

impl Color {
    /// softbuffer ARGB layout: 0x00RRGGBB.
    pub fn argb(self) -> u32 {
        match self {
            Color::Red    => 0x00_FF_3B_30,
            Color::Yellow => 0x00_FF_CC_00,
            Color::Green  => 0x00_34_C7_59,
            Color::Blue   => 0x00_00_7A_FF,
        }
    }
    /// `image::Rgba<u8>` payload.
    pub fn rgba(self) -> [u8; 4] {
        let argb = self.argb();
        [
            ((argb >> 16) & 0xFF) as u8,
            ((argb >> 8)  & 0xFF) as u8,
            ( argb        & 0xFF) as u8,
            0xFF,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Thin,
    Medium,
    Thick,
}

impl Stroke {
    pub fn px(self) -> i32 {
        match self { Self::Thin => 2, Self::Medium => 4, Self::Thick => 6 }
    }
    pub fn font_px(self) -> f32 {
        match self { Self::Thin => 14.0, Self::Medium => 20.0, Self::Thick => 28.0 }
    }
    pub fn step_up(self) -> Self {
        match self { Self::Thin => Self::Medium, Self::Medium => Self::Thick, Self::Thick => Self::Thick }
    }
    pub fn step_down(self) -> Self {
        match self { Self::Thick => Self::Medium, Self::Medium => Self::Thin, Self::Thin => Self::Thin }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnnotationStyle {
    pub color: Color,
    pub stroke: Stroke,
}

impl Default for AnnotationStyle {
    fn default() -> Self {
        Self { color: Color::Red, stroke: Stroke::Medium }
    }
}

/// A single placed annotation, in FRAME-space coordinates (physical pixels
/// of the captured image, matching what the PNG contains).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Annotation {
    Arrow   { from: (i32, i32), to: (i32, i32), style: AnnotationStyle },
    Rect    { rect: Rect, style: AnnotationStyle },
    Ellipse { rect: Rect, style: AnnotationStyle },
    Mosaic  { rect: Rect, block_size: u32 },
    Pen     { points: Vec<(i32, i32)>, style: AnnotationStyle },
    Text    { origin: (i32, i32), content: String, style: AnnotationStyle },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Arrow,
    Rect,
    Ellipse,
    Mosaic,
    Pen,
    Text,
}

impl Tool {
    pub fn is_drawing(self) -> bool {
        !matches!(self, Tool::Move)
    }
}

/// In-flight drawing: user has mouse-pressed while a drawing tool is active.
/// Either a Shape (rubber-band: from/to anchors) or a Pen (free-form polyline).
#[derive(Debug, Clone)]
pub enum PendingDraw {
    Shape {
        tool: Tool,
        from_frame: (i32, i32),
        to_frame: (i32, i32),
        style: AnnotationStyle,
    },
    Pen {
        points: Vec<(i32, i32)>,
        style: AnnotationStyle,
    },
}

impl PendingDraw {
    pub fn shape(tool: Tool, style: AnnotationStyle, from: (i32, i32), to: (i32, i32)) -> Self {
        Self::Shape { tool, from_frame: from, to_frame: to, style }
    }

    pub fn pen(style: AnnotationStyle, first: (i32, i32)) -> Self {
        Self::Pen { points: vec![first], style }
    }

    /// For Shape: replaces `to_frame`. For Pen: pushes the point unless it's
    /// identical to the last sample. The cursor stream is integer pixels, so
    /// "≥ 1 px Manhattan from last" is equivalent to "not exactly equal to
    /// last" — the dedup drops frames where the cursor sat still between
    /// samples (e.g. user paused mid-stroke), not sub-pixel jitter.
    pub fn extend_to(&mut self, p: (i32, i32)) {
        match self {
            PendingDraw::Shape { to_frame, .. } => *to_frame = p,
            PendingDraw::Pen   { points, .. } => {
                // Manhattan ≥ 1 on integer coords == "not equal to last".
                let keep = points.last().is_none_or(|&last| {
                    (last.0 - p.0).abs() + (last.1 - p.1).abs() >= 1
                });
                if keep { points.push(p); }
            }
        }
    }

    pub fn push_point(&mut self, p: (i32, i32)) { self.extend_to(p) }

    /// Produce the Annotation that this pending draw represents.
    /// Returns None for Move/Pen/Text shape misuse, or for Pen with <2 points.
    pub fn finalize(self) -> Option<Annotation> {
        match self {
            PendingDraw::Shape { tool, from_frame, to_frame, style } => match tool {
                Tool::Move | Tool::Pen | Tool::Text => None,
                Tool::Arrow => Some(Annotation::Arrow {
                    from: from_frame,
                    to: to_frame,
                    style,
                }),
                Tool::Rect => Some(Annotation::Rect {
                    rect: Rect::normalize(from_frame, to_frame),
                    style,
                }),
                Tool::Ellipse => Some(Annotation::Ellipse {
                    rect: Rect::normalize(from_frame, to_frame),
                    style,
                }),
                Tool::Mosaic => Some(Annotation::Mosaic {
                    rect: Rect::normalize(from_frame, to_frame),
                    block_size: 8,
                }),
            },
            PendingDraw::Pen { points, style } => {
                if points.len() < 2 { None } else { Some(Annotation::Pen { points, style }) }
            }
        }
    }
}

/// Undo/redo stack. Each completed annotation is pushed onto `undo_stack`;
/// Undo moves the top annotation onto `redo_stack`. Any new push clears
/// the redo stack.
pub struct History {
    undo_stack: Vec<Annotation>,
    redo_stack: Vec<Annotation>,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn push(&mut self, a: Annotation) {
        self.undo_stack.push(a);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self) -> bool {
        if let Some(a) = self.undo_stack.pop() {
            self.redo_stack.push(a);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self) -> bool {
        if let Some(a) = self.redo_stack.pop() {
            self.undo_stack.push(a);
            true
        } else {
            false
        }
    }

    pub fn current(&self) -> &[Annotation] {
        &self.undo_stack
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_drawing_variants() {
        assert!(!Tool::Move.is_drawing());
        assert!(Tool::Arrow.is_drawing());
        assert!(Tool::Rect.is_drawing());
        assert!(Tool::Ellipse.is_drawing());
        assert!(Tool::Mosaic.is_drawing());
        assert!(Tool::Pen.is_drawing());
        assert!(Tool::Text.is_drawing());
    }

    #[test]
    fn finalize_move_returns_none() {
        let p = PendingDraw::shape(Tool::Move, AnnotationStyle::default(), (0, 0), (10, 10));
        assert!(p.finalize().is_none());
    }

    #[test]
    fn finalize_arrow() {
        let p = PendingDraw::shape(Tool::Arrow, AnnotationStyle::default(), (10, 20), (50, 80));
        let a = p.finalize().unwrap();
        assert_eq!(
            a,
            Annotation::Arrow {
                from: (10, 20),
                to: (50, 80),
                style: AnnotationStyle::default(),
            }
        );
    }

    #[test]
    fn finalize_rect_normalizes() {
        let p = PendingDraw::shape(Tool::Rect, AnnotationStyle::default(), (80, 60), (20, 10));
        match p.finalize().unwrap() {
            Annotation::Rect { rect, .. } => {
                assert_eq!(rect, Rect { x: 20, y: 10, w: 60, h: 50 });
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn finalize_ellipse_normalizes() {
        let p = PendingDraw::shape(Tool::Ellipse, AnnotationStyle::default(), (0, 0), (30, 40));
        match p.finalize().unwrap() {
            Annotation::Ellipse { rect, .. } => {
                assert_eq!(rect, Rect { x: 0, y: 0, w: 30, h: 40 });
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn finalize_mosaic_has_block_size_8() {
        let p = PendingDraw::shape(Tool::Mosaic, AnnotationStyle::default(), (5, 5), (50, 50));
        match p.finalize().unwrap() {
            Annotation::Mosaic { rect, block_size } => {
                assert_eq!(rect, Rect { x: 5, y: 5, w: 45, h: 45 });
                assert_eq!(block_size, 8);
            }
            _ => panic!("expected Mosaic"),
        }
    }

    #[test]
    fn pen_finalize_returns_pen_with_points() {
        let mut p = PendingDraw::pen(AnnotationStyle::default(), (10, 20));
        p.push_point((11, 21));
        p.push_point((12, 22));
        let a = p.finalize().expect("pen finalize");
        match a {
            Annotation::Pen { ref points, style } => {
                assert_eq!(points, &vec![(10, 20), (11, 21), (12, 22)]);
                assert_eq!(style, AnnotationStyle::default());
            }
            _ => panic!("expected Pen"),
        }
    }

    #[test]
    fn pen_dedup_drops_consecutive_duplicates() {
        let mut p = PendingDraw::pen(AnnotationStyle::default(), (10, 10));
        p.push_point((10, 10));   // exact dup of last, dropped
        p.push_point((10, 11));   // distinct point, kept
        p.push_point((10, 11));   // exact dup of last, dropped
        let a = p.finalize().unwrap();
        match a {
            Annotation::Pen { points, .. } => assert_eq!(points, vec![(10, 10), (10, 11)]),
            _ => panic!(),
        }
    }

    #[test]
    fn pen_finalize_single_point_returns_none() {
        // No drag (mouse press-release without movement) — discard.
        let p = PendingDraw::pen(AnnotationStyle::default(), (10, 10));
        assert!(p.finalize().is_none());
    }

    #[test]
    fn shape_finalize_unchanged_after_enum_conversion() {
        let p = PendingDraw::shape(Tool::Rect, AnnotationStyle::default(), (5, 5), (15, 25));
        match p.finalize().unwrap() {
            Annotation::Rect { rect, .. } => {
                assert_eq!(rect, Rect { x: 5, y: 5, w: 10, h: 20 });
            }
            _ => panic!(),
        }
    }

    #[test]
    fn history_empty() {
        let h = History::new();
        assert!(!h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 0);
    }

    #[test]
    fn history_push() {
        let mut h = History::new();
        h.push(Annotation::Arrow {
            from: (0, 0),
            to: (10, 10),
            style: AnnotationStyle::default(),
        });
        assert!(h.can_undo());
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 1);
    }

    #[test]
    fn history_undo_moves_to_redo() {
        let mut h = History::new();
        h.push(Annotation::Rect {
            rect: Rect { x: 0, y: 0, w: 10, h: 10 },
            style: AnnotationStyle::default(),
        });
        assert!(h.undo());
        assert!(!h.can_undo());
        assert!(h.can_redo());
        assert_eq!(h.current().len(), 0);
    }

    #[test]
    fn history_redo_restores() {
        let mut h = History::new();
        let a = Annotation::Ellipse {
            rect: Rect { x: 5, y: 5, w: 20, h: 20 },
            style: AnnotationStyle::default(),
        };
        h.push(a.clone());
        h.undo();
        assert!(h.redo());
        assert_eq!(h.current(), &[a]);
        assert!(!h.can_redo());
    }

    #[test]
    fn push_after_undo_clears_redo() {
        let mut h = History::new();
        h.push(Annotation::Arrow { from: (0, 0), to: (5, 5), style: AnnotationStyle::default() });
        h.undo();
        assert!(h.can_redo());
        h.push(Annotation::Arrow { from: (10, 10), to: (20, 20), style: AnnotationStyle::default() });
        assert!(!h.can_redo());
        assert_eq!(h.current().len(), 1);
    }

    #[test]
    fn undo_empty_returns_false() {
        let mut h = History::new();
        assert!(!h.undo());
        assert!(!h.redo());
    }

    #[test]
    fn multiple_undo_redo() {
        let mut h = History::new();
        let a = Annotation::Arrow { from: (0, 0), to: (1, 1), style: AnnotationStyle::default() };
        let b = Annotation::Arrow { from: (2, 2), to: (3, 3), style: AnnotationStyle::default() };
        let c = Annotation::Arrow { from: (4, 4), to: (5, 5), style: AnnotationStyle::default() };
        h.push(a.clone());
        h.push(b.clone());
        h.push(c);
        assert_eq!(h.current().len(), 3);
        h.undo();
        h.undo();
        assert_eq!(h.current(), &[a.clone()]);
        h.redo();
        assert_eq!(h.current(), &[a, b]);
    }

    #[test]
    fn color_argb_values() {
        assert_eq!(Color::Red.argb(),    0x00_FF_3B_30);
        assert_eq!(Color::Yellow.argb(), 0x00_FF_CC_00);
        assert_eq!(Color::Green.argb(),  0x00_34_C7_59);
        assert_eq!(Color::Blue.argb(),   0x00_00_7A_FF);
    }

    #[test]
    fn color_rgba_values() {
        assert_eq!(Color::Red.rgba(),    [0xFF, 0x3B, 0x30, 0xFF]);
        assert_eq!(Color::Blue.rgba(),   [0x00, 0x7A, 0xFF, 0xFF]);
    }

    #[test]
    fn stroke_px_values() {
        assert_eq!(Stroke::Thin.px(),    2);
        assert_eq!(Stroke::Medium.px(),  4);
        assert_eq!(Stroke::Thick.px(),   6);
    }

    #[test]
    fn stroke_font_px_values() {
        assert_eq!(Stroke::Thin.font_px(),    14.0);
        assert_eq!(Stroke::Medium.font_px(),  20.0);
        assert_eq!(Stroke::Thick.font_px(),   28.0);
    }

    #[test]
    fn stroke_step() {
        assert_eq!(Stroke::Thin.step_up(),     Stroke::Medium);
        assert_eq!(Stroke::Medium.step_up(),   Stroke::Thick);
        assert_eq!(Stroke::Thick.step_up(),    Stroke::Thick);  // clamped
        assert_eq!(Stroke::Thick.step_down(),  Stroke::Medium);
        assert_eq!(Stroke::Medium.step_down(), Stroke::Thin);
        assert_eq!(Stroke::Thin.step_down(),   Stroke::Thin);   // clamped
    }

    #[test]
    fn annotation_style_default_is_red_medium() {
        let s = AnnotationStyle::default();
        assert_eq!(s.color,  Color::Red);
        assert_eq!(s.stroke, Stroke::Medium);
    }

    #[test]
    fn text_annotation_round_trip() {
        let mut h = History::new();
        let a = Annotation::Text {
            origin: (10, 20),
            content: "hi".to_string(),
            style: AnnotationStyle::default(),
        };
        h.push(a.clone());
        assert_eq!(h.current(), &[a]);
    }
}
