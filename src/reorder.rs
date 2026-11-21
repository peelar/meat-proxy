//! Dragging a list's fixed-height rows into a new order.

/// Points the pointer travels before a press on a handle becomes a drag.
const SLOP: f32 = 4.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Reorder {
    from: Option<usize>,
    to: usize,
    dy: f32,
    pitch: f32,
}

impl Reorder {
    pub fn new() -> Reorder {
        Reorder::default()
    }

    pub fn dragging(&self) -> Option<usize> {
        self.from
    }

    /// The pointer moved `dy` since the press on row `index`, which is `height` tall.
    pub fn follow(&mut self, index: usize, count: usize, dy: f32, height: f32) {
        if self.from.is_none() {
            if dy.abs() < SLOP {
                return;
            }
            self.from = Some(index);
            self.pitch = height.max(1.0);
        }
        self.dy = dy;
        let to = index as f32 + (dy / self.pitch).round();
        self.to = to.clamp(0.0, count.saturating_sub(1) as f32) as usize;
    }

    /// The drag ended: `(from, to)` for a drop that moves a row, `None` for a cancel or no move.
    pub fn end(&mut self, cancelled: bool) -> Option<(usize, usize)> {
        let from = self.from.take()?;
        self.dy = 0.0;
        (!cancelled && from != self.to).then_some((from, self.to))
    }

    /// How far row `index` sits from its place: the dragged row follows the pointer, and rows
    /// between its place and its target step toward the gap it left.
    pub fn shift(&self, index: usize) -> f32 {
        let Some(from) = self.from else { return 0.0 };
        if index == from {
            self.dy
        } else if from < index && index <= self.to {
            -self.pitch
        } else if self.to <= index && index < from {
            self.pitch
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_between_the_place_and_the_target_step_aside() {
        let mut r = Reorder::new();
        r.follow(1, 5, 3.0, 32.0);
        assert_eq!(r.dragging(), None, "inside the slop it's still a press");
        r.follow(1, 5, 70.0, 32.0);
        assert_eq!(r.dragging(), Some(1));
        assert_eq!(r.shift(1), 70.0);
        assert_eq!(r.shift(2), -32.0);
        assert_eq!(r.shift(3), -32.0);
        assert_eq!(r.shift(4), 0.0);
        assert_eq!(r.end(false), Some((1, 3)));
        r.follow(2, 5, 60.0, 32.0);
        assert_eq!(r.end(true), None);
    }
}
