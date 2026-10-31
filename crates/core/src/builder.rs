//! Collecting the painted paths of a figure into numbered primitives.

use crate::{Pieces, Primitive, PrimitiveKind, Style};

/// What an importer feeds as it walks a document: every stroked and filled path, in drawing order.
#[derive(Default)]
pub struct PrimitiveBuilder {
    stroked: Vec<(Pieces, Style)>,
    filled: Vec<(Pieces, Style)>,
}

impl PrimitiveBuilder {
    pub fn stroke(&mut self, pieces: Pieces, style: Style) {
        if !pieces.0.is_empty() {
            self.stroked.push((pieces, style));
        }
    }

    pub fn fill(&mut self, pieces: Pieces, style: Style) {
        if !pieces.0.is_empty() {
            self.filled.push((pieces, style));
        }
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
}
