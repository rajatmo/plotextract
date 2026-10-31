//! Turning a painted path into the vertices of a primitive, the same way for every file format.
//!
//! The rules reproduce what the Python prototype got from PyMuPDF, so the two can be compared curve by curve: an
//! element that starts where the previous one ended shares that join vertex, otherwise a new, disconnected piece
//! starts; a `re` rectangle starts at its top left and runs clockwise; any other closed four-corner shape is a quad.

use kurbo::{Affine, BezPath, PathEl, Point, Rect};

/// Vertices and the indices where a new, disconnected piece starts.
pub type Pieces = (Vec<Point>, Vec<usize>);

/// One painted path as vertices and break positions. An item (line, Bezier, rectangle) that starts where the previous
/// one ended shares that join vertex; otherwise a new, disconnected piece starts there.
pub fn flatten_path(path: &BezPath, transform: Affine, steps: usize, stroked: bool) -> Pieces {
    let mut points: Vec<Point> = Vec::new();
    let mut breaks: Vec<usize> = Vec::new();
    let mut has_item = false;
    for item in items(path, transform, steps, stroked) {
        match points.last() {
            Some(last) if has_item && *last == item[0] => points.extend_from_slice(&item[1..]),
            _ => {
                if has_item {
                    breaks.push(points.len());
                }
                points.extend_from_slice(&item);
            }
        }
        has_item = true;
    }
    (points, breaks)
}

/// The corners of a rectangle starting at its top left and running clockwise on the page, back to the start.
pub fn rect_corners(r: Rect) -> Vec<Point> {
    vec![
        Point::new(r.x0, r.y1),
        Point::new(r.x1, r.y1),
        Point::new(r.x1, r.y0),
        Point::new(r.x0, r.y0),
        Point::new(r.x0, r.y1),
    ]
}

/// The vertices of each drawing item of a path, in page coordinates.
fn items(path: &BezPath, transform: Affine, steps: usize, stroked: bool) -> Vec<Vec<Point>> {
    let mut out = Vec::new();
    let els = path.elements();
    let mut start = Point::ZERO; // start of the current subpath
    let mut cur = Point::ZERO;
    let mut i = 0;
    while i < els.len() {
        // A closed four-corner subpath is one rectangle (or quad) item, as a PDF library reports `re`.
        if let Some((corners, used)) = corner_subpath(&els[i..]) {
            let corners = corners.map(|p| transform * p);
            out.push(corner_item(corners, stroked));
            cur = corners[0];
            start = cur;
            i += used;
            continue;
        }
        match els[i] {
            PathEl::MoveTo(p) => {
                cur = transform * p;
                start = cur;
            }
            PathEl::LineTo(p) => {
                let to = transform * p;
                out.push(vec![cur, to]);
                cur = to;
            }
            PathEl::QuadTo(c, p) => {
                let (c, to) = (transform * c, transform * p);
                out.push(bezier(cur, cur + (c - cur) * (2.0 / 3.0), to + (c - to) * (2.0 / 3.0), to, steps));
                cur = to;
            }
            PathEl::CurveTo(c1, c2, p) => {
                let (c1, c2, to) = (transform * c1, transform * c2, transform * p);
                out.push(bezier(cur, c1, c2, to, steps));
                cur = to;
            }
            PathEl::ClosePath => {
                if cur != start {
                    out.push(vec![cur, start]);
                    cur = start;
                }
            }
        }
        i += 1;
    }
    out
}

/// If `els` starts with a subpath of exactly four corners that closes (`m l l l h` or `m l l l l` back to the start),
/// the corners and the number of elements the subpath takes.
fn corner_subpath(els: &[PathEl]) -> Option<([Point; 4], usize)> {
    let [PathEl::MoveTo(a), PathEl::LineTo(b), PathEl::LineTo(c), PathEl::LineTo(d), rest @ ..] = els else {
        return None;
    };
    let used = match rest.first() {
        Some(PathEl::ClosePath) => 5,
        Some(PathEl::LineTo(e))
            if e == a && !matches!(rest.get(1), Some(PathEl::LineTo(_) | PathEl::CurveTo(..) | PathEl::QuadTo(..))) =>
        {
            5
        }
        _ => return None,
    };
    Some(([*a, *b, *c, *d], used))
}

/// A rectangle as `re` draws it starts at its top left and runs clockwise on the page. Any other four-corner shape
/// keeps its first corner; a stroked one then runs the other way round, a filled one in path order.
fn corner_item([a, b, c, d]: [Point; 4], stroked: bool) -> Vec<Point> {
    // `re` runs along x first; the same rectangle drawn as lines starting vertically is a plain quad.
    let re = a.y == b.y && b.x == c.x && c.y == d.y && d.x == a.x;
    if re {
        rect_corners(Rect::from_points(a, c))
    } else if stroked {
        vec![a, d, c, b, a]
    } else {
        vec![a, b, c, d, a]
    }
}

/// `steps + 1` points of a cubic Bezier at evenly spaced parameters; the first and last are exactly the end points.
fn bezier(p0: Point, p1: Point, p2: Point, p3: Point, steps: usize) -> Vec<Point> {
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            Point::new(a * p0.x + b * p1.x + c * p2.x + d * p3.x, a * p0.y + b * p1.y + c * p2.y + d * p3.y)
        })
        .collect()
}
