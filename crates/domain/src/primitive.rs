//! Candidate pieces of data found in a figure.

use kurbo::{Point, Rect};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveKind {
    /// A stroked path.
    Stroke,
    /// A filled path.
    Fill,
}

impl PrimitiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PrimitiveKind::Stroke => "stroke",
            PrimitiveKind::Fill => "fill",
        }
    }
}

/// How a primitive was drawn; lets later steps group primitives that look alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    /// RGBA of the paint, when it is a plain colour.
    pub color: Option<[u8; 4]>,
}

/// Where a primitive came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Provenance {
    /// Read exactly from a vector file.
    Exact,
    /// Found in a raster by a detector.
    Detected { detector: String },
}

/// One path of a figure as a polyline in figure space (origin top left, y down).
#[derive(Clone, Debug)]
pub struct Primitive {
    pub kind: PrimitiveKind,
    /// Position among the primitives of its kind.
    pub index: usize,
    pub points: Vec<Point>,
    /// Vertex indices where a new, disconnected piece starts (a path of broken lines, such as all of a plot's ticks
    /// drawn in one stroke). Vertices on either side of a break are not joined by a line.
    pub breaks: Vec<usize>,
    pub style: Style,
    pub provenance: Provenance,
}

impl Primitive {
    pub fn new(kind: PrimitiveKind, index: usize, points: Vec<Point>, breaks: Vec<usize>) -> Self {
        Self { kind, index, points, breaks, style: Style::default(), provenance: Provenance::Exact }
    }

    pub fn name(&self) -> String {
        format!("{}[{}]", self.kind.as_str(), self.index)
    }

    /// Bounding box of the points, or `None` for an empty primitive.
    pub fn extent(&self) -> Option<Rect> {
        let first = *self.points.first()?;
        Some(self.points[1..].iter().fold(Rect::from_points(first, first), |r, p| r.union_pt(*p)))
    }

    /// The connected pieces as slices of vertices.
    pub fn pieces(&self) -> impl Iterator<Item = &[Point]> {
        let mut starts = vec![0];
        starts.extend(self.breaks.iter().copied().filter(|&b| b > 0 && b < self.points.len()));
        starts.push(self.points.len());
        (0..starts.len() - 1).map(move |i| &self.points[starts[i]..starts[i + 1]]).collect::<Vec<_>>().into_iter()
    }
}
