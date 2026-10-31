//! Spatial queries over all primitives, for hover snapping and click picking.

use kurbo::Point;

use crate::Primitive;

/// A vertex found by a query.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub primitive: usize,
    pub vertex: usize,
    pub pos: Point,
}

struct Segment {
    a: Point,
    b: Point,
    primitive: usize,
}

/// Vertices sorted by x, so a vertex query touches only a thin vertical strip; segments are filtered by bounding box.
pub struct SnapIndex {
    by_x: Vec<(f64, usize, usize)>, // x, primitive, vertex
    segments: Vec<Segment>,
}

impl SnapIndex {
    pub fn new(primitives: &[Primitive]) -> Self {
        let mut by_x = Vec::new();
        let mut segments = Vec::new();
        for (k, prim) in primitives.iter().enumerate() {
            let mut breaks = prim.breaks.iter().copied().peekable();
            for (i, p) in prim.points.iter().enumerate() {
                by_x.push((p.x, k, i));
                while breaks.next_if(|&b| b < i).is_some() {}
                let broken = breaks.peek() == Some(&i);
                if i > 0 && !broken {
                    segments.push(Segment { a: prim.points[i - 1], b: *p, primitive: k });
                }
            }
        }
        by_x.sort_by(|a, b| a.0.total_cmp(&b.0));
        Self { by_x, segments }
    }

    /// The nearest vertex of a visible primitive within `radius` of `at`.
    pub fn nearest_vertex(&self, primitives: &[Primitive], visible: &[bool], at: Point, radius: f64) -> Option<Hit> {
        let lo = self.by_x.partition_point(|v| v.0 < at.x - radius);
        let mut best: Option<(f64, Hit)> = None;
        for &(x, k, i) in &self.by_x[lo..] {
            if x > at.x + radius {
                break;
            }
            if !visible[k] {
                continue;
            }
            let pos = primitives[k].points[i];
            let d2 = (pos - at).hypot2();
            if d2 <= radius * radius && best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, Hit { primitive: k, vertex: i, pos }));
            }
        }
        best.map(|(_, hit)| hit)
    }

    /// The visible primitive with a segment within `radius` of `at` (the nearest one).
    pub fn nearest_primitive(&self, visible: &[bool], at: Point, radius: f64) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for s in &self.segments {
            if !visible[s.primitive]
                || s.a.x.max(s.b.x) < at.x - radius
                || s.a.x.min(s.b.x) > at.x + radius
                || s.a.y.max(s.b.y) < at.y - radius
                || s.a.y.min(s.b.y) > at.y + radius
            {
                continue;
            }
            let ab = s.b - s.a;
            let len2 = ab.hypot2();
            let t = if len2 > 0.0 { ((at - s.a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
            let d2 = (s.a + ab * t - at).hypot2();
            if d2 <= radius * radius && best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, s.primitive));
            }
        }
        best.map(|(_, k)| k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrimitiveKind;

    fn prim(points: &[(f64, f64)], breaks: Vec<usize>) -> Primitive {
        Primitive::new(PrimitiveKind::Stroke, 0, points.iter().map(|&(x, y)| Point::new(x, y)).collect(), breaks)
    }

    #[test]
    fn nearest_vertex_respects_radius_and_visibility() {
        let prims = vec![prim(&[(0.0, 0.0), (10.0, 0.0)], vec![]), prim(&[(1.0, 0.0), (11.0, 5.0)], vec![])];
        let idx = SnapIndex::new(&prims);
        let hit = idx.nearest_vertex(&prims, &[true, true], Point::new(0.9, 0.2), 2.0).unwrap();
        assert_eq!((hit.primitive, hit.vertex), (1, 0));
        let hit = idx.nearest_vertex(&prims, &[true, false], Point::new(0.9, 0.2), 2.0).unwrap();
        assert_eq!((hit.primitive, hit.vertex), (0, 0));
        assert!(idx.nearest_vertex(&prims, &[true, true], Point::new(5.0, 5.0), 2.0).is_none());
    }

    #[test]
    fn segments_do_not_cross_breaks() {
        // two pieces: (0,0)-(10,0) and (0,10)-(10,10); the gap between (10,0) and (0,10) is not a line
        let prims = vec![prim(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)], vec![2])];
        let idx = SnapIndex::new(&prims);
        assert_eq!(idx.nearest_primitive(&[true], Point::new(5.0, 0.5), 1.0), Some(0));
        assert_eq!(idx.nearest_primitive(&[true], Point::new(5.0, 5.0), 1.0), None); // on the would-be diagonal
        assert_eq!(idx.nearest_primitive(&[false], Point::new(5.0, 0.5), 1.0), None);
    }
}
