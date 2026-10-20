//! Plain geometry in global screen coordinates: origin at the top-left of the
//! primary display, y grows downward (the convention the Accessibility API uses).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.max_x() && py >= self.y && py < self.max_y()
    }

    /// Shrink the rect by the given amount on each edge. Never returns a
    /// negative size.
    pub fn inset(&self, top: f64, right: f64, bottom: f64, left: f64) -> Rect {
        Rect {
            x: self.x + left,
            y: self.y + top,
            w: (self.w - left - right).max(1.0),
            h: (self.h - top - bottom).max(1.0),
        }
    }

    /// True if both rects are equal within `tol` pixels on every component.
    pub fn approx_eq(&self, other: &Rect, tol: f64) -> bool {
        (self.x - other.x).abs() <= tol
            && (self.y - other.y).abs() <= tol
            && (self.w - other.w).abs() <= tol
            && (self.h - other.h).abs() <= tol
    }

    /// Vertical ranges overlap by more than a pixel.
    pub fn overlaps_vertically(&self, other: &Rect) -> bool {
        self.y < other.max_y() - 1.0 && other.y < self.max_y() - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_and_contains() {
        let r = Rect::new(0.0, 25.0, 1440.0, 875.0).inset(8.0, 8.0, 8.0, 8.0);
        assert_eq!(r, Rect::new(8.0, 33.0, 1424.0, 859.0));
        assert!(r.contains(8.0, 33.0));
        assert!(!r.contains(1432.0, 40.0));
    }

    #[test]
    fn inset_never_negative() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0).inset(20.0, 20.0, 20.0, 20.0);
        assert_eq!((r.w, r.h), (1.0, 1.0));
    }
}
