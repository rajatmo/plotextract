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

/// How far outside a query the approximate (single precision) keys are searched, relative to the coordinates, so that
/// rounding never hides a vertex that is really inside.
const KEY_SLACK: f64 = 1e-6;

/// Vertices sorted by x, so a vertex query touches only a thin vertical strip, and a bounding box per primitive, so a
/// line query skips the primitives that are nowhere near. Both are approximate (single precision) and only choose what
/// to look at: distances are always computed from the primitives' own coordinates. That keeps the index at about twelve
/// bytes per vertex however large the figure is.
pub struct SnapIndex {
    by_x: Vec<(f32, u32, u32)>, // x, primitive, vertex
    bounds: Vec<[f32; 4]>,      // x0, y0, x1, y1 of each primitive, rounded outwards
}

impl SnapIndex {
    pub fn new(primitives: &[Primitive]) -> Self {
        let vertices = primitives.iter().map(|p| p.points.len()).sum();
        let mut by_x = Vec::with_capacity(vertices);
        let mut bounds = Vec::with_capacity(primitives.len());
        for (k, prim) in primitives.iter().enumerate() {
            let mut b = [f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY];
            for (i, p) in prim.points.iter().enumerate() {
                by_x.push((p.x as f32, k as u32, i as u32));
                b = [b[0].min(p.x), b[1].min(p.y), b[2].max(p.x), b[3].max(p.y)];
            }
            bounds.push([
                (b[0] as f32).next_down(),
                (b[1] as f32).next_down(),
                (b[2] as f32).next_up(),
                (b[3] as f32).next_up(),
            ]);
        }
        by_x.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        Self { by_x, bounds }
    }

    /// A box that holds all of a primitive's vertices (a little larger than needed).
    pub fn bounds(&self, primitive: usize) -> kurbo::Rect {
        let [x0, y0, x1, y1] = self.bounds[primitive];
        kurbo::Rect::new(f64::from(x0), f64::from(y0), f64::from(x1), f64::from(y1))
    }

    /// The nearest vertex of a visible primitive within `radius` of `at`.
    pub fn nearest_vertex(
        &self,
        primitives: &[Primitive],
        visible: impl Fn(usize) -> bool,
        at: Point,
        radius: f64,
    ) -> Option<Hit> {
        let slack = (at.x.abs() + radius) * KEY_SLACK;
        let (lo_x, hi_x) = (at.x - radius - slack, at.x + radius + slack);
        let lo = self.by_x.partition_point(|v| f64::from(v.0) < lo_x);
        let mut best: Option<(f64, Hit)> = None;
        for &(x, k, i) in &self.by_x[lo..] {
            if f64::from(x) > hi_x {
                break;
            }
            let (k, i) = (k as usize, i as usize);
            if !visible(k) {
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
    pub fn nearest_primitive(
        &self,
        primitives: &[Primitive],
        visible: impl Fn(usize) -> bool,
        at: Point,
        radius: f64,
    ) -> Option<usize> {
        let mut best: Option<(f64, usize)> = None;
        for (k, prim) in primitives.iter().enumerate() {
            let [x0, y0, x1, y1] = self.bounds[k];
            if f64::from(x1) < at.x - radius
                || f64::from(x0) > at.x + radius
                || f64::from(y1) < at.y - radius
                || f64::from(y0) > at.y + radius
                || !visible(k)
            {
                continue;
            }
            let mut breaks = prim.breaks.iter().copied().peekable();
            for (i, &b) in prim.points.iter().enumerate() {
                while breaks.next_if(|&brk| brk < i).is_some() {}
                if i == 0 || breaks.peek() == Some(&i) {
                    continue; // a piece starts here: no segment joins it to the previous vertex
                }
                let a = prim.points[i - 1];
                if a.x.max(b.x) < at.x - radius
                    || a.x.min(b.x) > at.x + radius
                    || a.y.max(b.y) < at.y - radius
                    || a.y.min(b.y) > at.y + radius
                {
                    continue;
                }
                let ab = b - a;
                let len2 = ab.hypot2();
                let t = if len2 > 0.0 { ((at - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
                let d2 = (a + ab * t - at).hypot2();
                if d2 <= radius * radius && best.is_none_or(|(d, _)| d2 < d) {
                    best = Some((d2, k));
                }
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
        let hit = idx.nearest_vertex(&prims, |_| true, Point::new(0.9, 0.2), 2.0).unwrap();
        assert_eq!((hit.primitive, hit.vertex), (1, 0));
        let hit = idx.nearest_vertex(&prims, |k| k == 0, Point::new(0.9, 0.2), 2.0).unwrap();
        assert_eq!((hit.primitive, hit.vertex), (0, 0));
        assert!(idx.nearest_vertex(&prims, |_| true, Point::new(5.0, 5.0), 2.0).is_none());
    }

    #[test]
    fn segments_do_not_cross_breaks() {
        // two pieces: (0,0)-(10,0) and (0,10)-(10,10); the gap between (10,0) and (0,10) is not a line
        let prims = vec![prim(&[(0.0, 0.0), (10.0, 0.0), (0.0, 10.0), (10.0, 10.0)], vec![2])];
        let idx = SnapIndex::new(&prims);
        assert_eq!(idx.nearest_primitive(&prims, |_| true, Point::new(5.0, 0.5), 1.0), Some(0));
        assert_eq!(idx.nearest_primitive(&prims, |_| true, Point::new(5.0, 5.0), 1.0), None); // on the would-be diagonal
        assert_eq!(idx.nearest_primitive(&prims, |_| false, Point::new(5.0, 0.5), 1.0), None);
    }

    #[test]
    fn single_precision_keys_never_hide_a_vertex_far_from_the_origin() {
        // at x = 1e6 a single precision number can only tell 0.0625 apart: the search must still find the vertex
        let prims = vec![prim(&[(1_000_000.3, 5.0), (1_000_000.9, 5.0)], vec![])];
        let idx = SnapIndex::new(&prims);
        let hit = idx.nearest_vertex(&prims, |_| true, Point::new(1_000_000.3, 5.2), 0.5).unwrap();
        assert_eq!((hit.primitive, hit.vertex), (0, 0));
        assert_eq!(hit.pos, Point::new(1_000_000.3, 5.0), "distances use the real coordinates");
        let hit = idx.nearest_vertex(&prims, |_| true, Point::new(1_000_000.9, 5.0), 0.01).unwrap();
        assert_eq!(hit.vertex, 1);
    }

    #[test]
    fn a_line_far_from_a_query_is_skipped_by_its_bounds() {
        let prims = vec![prim(&[(0.0, 0.0), (10.0, 0.0)], vec![]), prim(&[(0.0, 100.0), (10.0, 100.0)], vec![])];
        let idx = SnapIndex::new(&prims);
        assert_eq!(idx.nearest_primitive(&prims, |_| true, Point::new(5.0, 99.5), 1.0), Some(1));
        assert!(idx.bounds(0).contains(Point::new(5.0, 0.0)) && !idx.bounds(0).contains(Point::new(5.0, 50.0)));
        assert_eq!(idx.nearest_primitive(&prims, |_| true, Point::new(5.0, 50.0), 1.0), None);
    }
}
