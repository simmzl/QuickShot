//! Pure types + undo/redo for annotation state. No IO, no drawing.

use super::state::Rect;

/// A single placed annotation, in FRAME-space coordinates (physical pixels
/// of the captured image, matching what the PNG contains).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Annotation {
    Arrow { from: (i32, i32), to: (i32, i32) },
    Rect { rect: Rect },
    Ellipse { rect: Rect },
    Mosaic { rect: Rect, block_size: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Move,
    Arrow,
    Rect,
    Ellipse,
    Mosaic,
}

impl Tool {
    pub fn is_drawing(self) -> bool {
        !matches!(self, Tool::Move)
    }
}

/// In-flight drawing: user has mouse-pressed while a drawing tool is active.
#[derive(Debug, Clone, Copy)]
pub struct PendingDraw {
    pub tool: Tool,
    pub from_frame: (i32, i32),
    pub to_frame: (i32, i32),
}

impl PendingDraw {
    /// Produce the Annotation that this pending draw represents.
    /// Returns None if the tool is not drawing-capable (shouldn't happen in
    /// normal flow but protects against misuse).
    pub fn finalize(self) -> Option<Annotation> {
        match self.tool {
            Tool::Move => None,
            Tool::Arrow => Some(Annotation::Arrow {
                from: self.from_frame,
                to: self.to_frame,
            }),
            Tool::Rect => Some(Annotation::Rect {
                rect: Rect::normalize(self.from_frame, self.to_frame),
            }),
            Tool::Ellipse => Some(Annotation::Ellipse {
                rect: Rect::normalize(self.from_frame, self.to_frame),
            }),
            Tool::Mosaic => Some(Annotation::Mosaic {
                rect: Rect::normalize(self.from_frame, self.to_frame),
                block_size: 8,
            }),
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
    }

    #[test]
    fn finalize_move_returns_none() {
        let p = PendingDraw {
            tool: Tool::Move,
            from_frame: (0, 0),
            to_frame: (10, 10),
        };
        assert!(p.finalize().is_none());
    }

    #[test]
    fn finalize_arrow() {
        let p = PendingDraw {
            tool: Tool::Arrow,
            from_frame: (10, 20),
            to_frame: (50, 80),
        };
        let a = p.finalize().unwrap();
        assert_eq!(
            a,
            Annotation::Arrow {
                from: (10, 20),
                to: (50, 80)
            }
        );
    }

    #[test]
    fn finalize_rect_normalizes() {
        let p = PendingDraw {
            tool: Tool::Rect,
            from_frame: (80, 60),
            to_frame: (20, 10),
        };
        match p.finalize().unwrap() {
            Annotation::Rect { rect } => {
                assert_eq!(rect, Rect { x: 20, y: 10, w: 60, h: 50 });
            }
            _ => panic!("expected Rect"),
        }
    }

    #[test]
    fn finalize_ellipse_normalizes() {
        let p = PendingDraw {
            tool: Tool::Ellipse,
            from_frame: (0, 0),
            to_frame: (30, 40),
        };
        match p.finalize().unwrap() {
            Annotation::Ellipse { rect } => {
                assert_eq!(rect, Rect { x: 0, y: 0, w: 30, h: 40 });
            }
            _ => panic!("expected Ellipse"),
        }
    }

    #[test]
    fn finalize_mosaic_has_block_size_8() {
        let p = PendingDraw {
            tool: Tool::Mosaic,
            from_frame: (5, 5),
            to_frame: (50, 50),
        };
        match p.finalize().unwrap() {
            Annotation::Mosaic { rect, block_size } => {
                assert_eq!(rect, Rect { x: 5, y: 5, w: 45, h: 45 });
                assert_eq!(block_size, 8);
            }
            _ => panic!("expected Mosaic"),
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
        };
        h.push(a);
        h.undo();
        assert!(h.redo());
        assert_eq!(h.current(), &[a]);
        assert!(!h.can_redo());
    }

    #[test]
    fn push_after_undo_clears_redo() {
        let mut h = History::new();
        h.push(Annotation::Arrow { from: (0, 0), to: (5, 5) });
        h.undo();
        assert!(h.can_redo());
        h.push(Annotation::Arrow { from: (10, 10), to: (20, 20) });
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
        let a = Annotation::Arrow { from: (0, 0), to: (1, 1) };
        let b = Annotation::Arrow { from: (2, 2), to: (3, 3) };
        let c = Annotation::Arrow { from: (4, 4), to: (5, 5) };
        h.push(a);
        h.push(b);
        h.push(c);
        assert_eq!(h.current().len(), 3);
        h.undo();
        h.undo();
        assert_eq!(h.current(), &[a]);
        h.redo();
        assert_eq!(h.current(), &[a, b]);
    }
}
