//! Collecting the painted paths of a figure into numbered primitives.

use plotextract_core::{Primitive, PrimitiveKind, Style};

use crate::{MAX_POINTS, MAX_PRIMITIVES, Pieces};

/// What an importer feeds as it walks a document: every stroked and filled path, in drawing order.
///
/// A hostile or merely enormous file can hold far more paths than a person can use or memory can hold, so the builder
/// stops at [`MAX_PRIMITIVES`] paths or [`MAX_POINTS`] vertices, drops the rest and remembers that it did.
pub struct PrimitiveBuilder {
    stroked: Vec<(Pieces, Style)>,
    filled: Vec<(Pieces, Style)>,
    points: usize,
    max_primitives: usize,
    max_points: usize,
    truncated: bool,
}

impl Default for PrimitiveBuilder {
    fn default() -> Self {
        Self::with_limits(MAX_PRIMITIVES, MAX_POINTS)
    }
}

impl PrimitiveBuilder {
    pub fn with_limits(max_primitives: usize, max_points: usize) -> Self {
        Self { stroked: Vec::new(), filled: Vec::new(), points: 0, max_primitives, max_points, truncated: false }
    }

    /// Whether another path would be kept. Importers check it to skip the work of flattening paths that would be
    /// dropped.
    pub fn has_room(&self) -> bool {
        !self.truncated
    }

    /// Whether paths were dropped because of the limits.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    pub fn stroke(&mut self, pieces: Pieces, style: Style) {
        if self.admit(&pieces) {
            self.stroked.push((pieces, style));
        }
    }

    pub fn fill(&mut self, pieces: Pieces, style: Style) {
        if self.admit(&pieces) {
            self.filled.push((pieces, style));
        }
    }

    fn admit(&mut self, pieces: &Pieces) -> bool {
        if pieces.0.is_empty() {
            return false;
        }
        if self.truncated
            || self.stroked.len() + self.filled.len() >= self.max_primitives
            || self.points + pieces.0.len() > self.max_points
        {
            self.truncated = true;
            return false;
        }
        self.points += pieces.0.len();
        true
    }

    /// The primitives: stroked paths first, then filled ones, each numbered within its kind.
    pub fn finish(self) -> Vec<Primitive> {
        let mut out = Vec::with_capacity(self.stroked.len() + self.filled.len());
        for (kind, group) in [(PrimitiveKind::Stroke, self.stroked), (PrimitiveKind::Fill, self.filled)] {
            for (index, ((points, breaks), style)) in group.into_iter().enumerate() {
                let mut prim = Primitive::new(kind, index, points, breaks);
                prim.style = style;
                out.push(prim);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use kurbo::Point;

    use super::*;

    #[test]
    fn strokes_come_first_and_each_kind_is_numbered_from_zero() {
        let line = |x: f64| (vec![Point::new(x, 0.0), Point::new(x, 1.0)], Vec::new());
        let mut b = PrimitiveBuilder::default();
        b.fill(line(0.0), Style::default());
        b.stroke(line(1.0), Style::default());
        b.stroke(line(2.0), Style::default());
        b.stroke((Vec::new(), Vec::new()), Style::default()); // an empty path is dropped
        let names: Vec<_> = b.finish().iter().map(Primitive::name).collect();
        assert_eq!(names, ["stroke[0]", "stroke[1]", "fill[0]"]);
    }

    #[test]
    fn a_file_with_too_many_paths_is_cut_short_and_says_so() {
        let line = |x: f64| (vec![Point::new(x, 0.0), Point::new(x, 1.0)], Vec::new());
        let mut b = PrimitiveBuilder::with_limits(3, 100);
        for i in 0..5 {
            b.stroke(line(f64::from(i)), Style::default());
        }
        assert!(b.truncated() && !b.has_room());
        assert_eq!(b.finish().len(), 3);

        let mut b = PrimitiveBuilder::with_limits(100, 5);
        b.stroke(line(0.0), Style::default());
        b.stroke(line(1.0), Style::default());
        b.stroke((vec![Point::ZERO; 3], Vec::new()), Style::default()); // 7 points would pass the limit of 5
        assert!(b.truncated());
        assert_eq!(b.finish().len(), 2);

        let mut b = PrimitiveBuilder::with_limits(10, 10);
        b.stroke(line(0.0), Style::default());
        assert!(!b.truncated() && b.has_room());
    }
}
