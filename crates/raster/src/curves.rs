//! The curves of a plot: the pieces the masks of the curve colours break into, and the points traced along them.
//!
//! A piece is a connected set of pixels of one colour: a line, a dash of a dashed line, a marker, a letter. Finding them is
//! done by an existing package (the connected components of `imageproc`); what is written here is what it does not give:
//! the straight lines of the axes are cut out first, for a colour that is the axes as well as a curve, specks are dropped,
//! and a piece is remembered by one of its pixels so that it can be found again when the pieces are.
//!
//! A series is made of pieces, and traced into points ([`Pieces::trace`]): a line gives a point for every column of each of
//! its pieces, markers a point for each piece, each at the centre of the coverage it stands for, so to a fraction of a pixel.

use image::{GrayImage, ImageBuffer, Luma};
use imageproc::region_labelling::{Connectivity, connected_components};
use kurbo::Point;
use rayon::prelude::*;

use crate::mask::Mask;
use crate::prepare::PixelRect;

/// How a series is traced into points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Trace {
    /// A line: a point at the centre of every column of each piece.
    #[default]
    Line,
    /// Markers: a point at the centre of each piece.
    Markers,
    /// Dots and dashes along a path: a point at the centre of a dot, points along a dash; in the order of the path.
    Pattern,
}

impl Trace {
    pub const ALL: [Trace; 3] = [Trace::Line, Trace::Markers, Trace::Pattern];
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// The fewest pixels a piece has: anything smaller is a speck.
    pub min_pixels: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self { min_pixels: 4 }
    }
}

/// A connected set of pixels of one colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    /// The ink it is drawn in.
    pub ink: usize,
    /// The pixels it is in.
    pub bounds: PixelRect,
    /// How many pixels it has (mostly covered ones).
    pub pixels: u64,
    /// Its first pixel, row after row. A piece is remembered by it: when the pieces are found again, the piece that has
    /// this pixel is the same one.
    pub seed: (u32, u32),
}

impl Piece {
    /// The longer side of its bounds.
    pub fn extent(&self) -> u32 {
        self.bounds.width.max(self.bounds.height)
    }
}

/// A colour to break into pieces: its ink, its mask, and the parts of the image to leave out of it (the straight lines of
/// the axes, for a colour that is the axes too).
pub struct Source {
    pub ink: usize,
    pub mask: Mask,
    pub cut: Vec<PixelRect>,
}

/// The pieces of the curve colours.
#[derive(Clone, Debug, PartialEq)]
pub struct Pieces {
    pub(crate) crop: PixelRect,
    /// For every pixel of the crop, the piece it is in, counting from 1; 0 for none.
    pub(crate) labels: Vec<u32>,
    /// For every pixel of the crop, how much of it is a curve colour (once the cuts are made): the faint pixels at the
    /// edge of a stroke count too, when its centre is measured.
    pub(crate) coverage: Vec<u8>,
    pub(crate) pieces: Vec<Piece>,
}

/// The pieces of `sources`, which are masks of the same crop. A piece has at least `params.min_pixels` pixels.
pub fn find(sources: &[Source], params: &Params) -> Pieces {
    let crop = sources.first().map_or(PixelRect { x: 0, y: 0, width: 0, height: 0 }, |s| s.mask.crop());
    let (w, h) = (crop.width as usize, crop.height as usize);
    let mut out = Pieces { crop, labels: vec![0; w * h], coverage: vec![0; w * h], pieces: Vec::new() };
    if w == 0 || h == 0 {
        return out;
    }
    // each colour is labelled on its own (on as many threads as there are colours), within the bounds of its ink only:
    // most colours cover a small part of the image
    // each colour's coverage (with its cuts made), and where its ink is with the components labelled there
    type Labelled = (Vec<u8>, Option<(PixelRect, ImageBuffer<Luma<u32>, Vec<u32>>)>);
    let labelled: Vec<Labelled> = sources
        .par_iter()
        .map(|source| {
            assert_eq!(source.mask.crop(), crop, "the colours are masks of the same crop");
            let mut data = source.mask.data().to_vec();
            for band in &source.cut {
                let (x0, x1) = (band.x.max(crop.x), (band.x + band.width).min(crop.x + crop.width));
                let (y0, y1) = (band.y.max(crop.y), (band.y + band.height).min(crop.y + crop.height));
                for y in y0..y1 {
                    let row = (y - crop.y) as usize * w;
                    data[row + (x0 - crop.x) as usize..row + x1.saturating_sub(crop.x) as usize].fill(0);
                }
            }
            let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
            for (k, _) in data.iter().enumerate().filter(|(_, d)| **d >= 128) {
                let (x, y) = (k % w, k / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
            }
            if x1 <= x0 {
                return (data, None);
            }
            let (bw, bh) = ((x1 - x0) as u32, (y1 - y0) as u32);
            let image = GrayImage::from_fn(bw, bh, |x, y| {
                Luma([u8::from(data[(y0 + y as usize) * w + x0 + x as usize] >= 128)])
            });
            let bounds = PixelRect { x: crop.x + x0 as u32, y: crop.y + y0 as u32, width: bw, height: bh };
            (data, Some((bounds, connected_components(&image, Connectivity::Eight, Luma([0])))))
        })
        .collect();
    for (source, (data, components)) in sources.iter().zip(labelled) {
        for (c, &d) in out.coverage.iter_mut().zip(&data) {
            *c = (*c).max(d);
        }
        let Some((bounds, components)) = components else { continue };
        // what each component is, by its label; its first pixel row by row is met first
        let mut found: Vec<Option<Piece>> = Vec::new();
        for (x, y, &Luma([label])) in components.enumerate_pixels() {
            if label == 0 {
                continue;
            }
            let k = label as usize;
            if found.len() <= k {
                found.resize(k + 1, None);
            }
            let (ix, iy) = (bounds.x + x, bounds.y + y);
            match &mut found[k] {
                None => {
                    found[k] = Some(Piece {
                        ink: source.ink,
                        bounds: PixelRect { x: ix, y: iy, width: 1, height: 1 },
                        pixels: 1,
                        seed: (ix, iy),
                    })
                }
                Some(piece) => {
                    let b = &mut piece.bounds;
                    let (x0, y0) = (b.x.min(ix), b.y.min(iy));
                    let (x1, y1) = ((b.x + b.width).max(ix + 1), (b.y + b.height).max(iy + 1));
                    *b = PixelRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
                    piece.pixels += 1;
                }
            }
        }
        // number the pieces that are not specks after the ones already found
        let mut number = vec![0u32; found.len()];
        for (k, piece) in found.into_iter().enumerate() {
            if let Some(piece) = piece.filter(|p| p.pixels >= u64::from(params.min_pixels)) {
                out.pieces.push(piece);
                number[k] = out.pieces.len() as u32;
            }
        }
        for (x, y, &Luma([label])) in components.enumerate_pixels() {
            if label != 0 && number[label as usize] != 0 {
                let i = (bounds.y - crop.y + y) as usize * w + (bounds.x - crop.x + x) as usize;
                out.labels[i] = number[label as usize];
            }
        }
    }
    out
}

impl Pieces {
    /// The part of the image the pieces are in.
    pub fn crop(&self) -> PixelRect {
        self.crop
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub(crate) fn index(&self, x: u32, y: u32) -> Option<usize> {
        let c = self.crop;
        (x >= c.x && y >= c.y && x < c.x + c.width && y < c.y + c.height)
            .then(|| (y - c.y) as usize * c.width as usize + (x - c.x) as usize)
    }

    /// The piece at image pixel `(x, y)`, counting from 0.
    pub fn at(&self, x: u32, y: u32) -> Option<usize> {
        let label = self.labels[self.index(x, y)?];
        (label != 0).then(|| label as usize - 1)
    }

    /// The piece nearest to image position `(x, y)` (fractions allowed) within `radius` pixels. A pointer beside a thin
    /// line is rarely exactly on it.
    pub fn nearest(&self, x: f64, y: f64, radius: f64) -> Option<usize> {
        let radius = radius.max(0.5);
        let r = radius.ceil() as i64;
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let mut best: Option<(f64, usize)> = None;
        for py in (cy - r).max(0)..=cy + r {
            for px in (cx - r).max(0)..=cx + r {
                let Some(piece) = self.at(px as u32, py as u32) else { continue };
                let d = (px as f64 + 0.5 - x).hypot(py as f64 + 0.5 - y);
                if d <= radius && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, piece));
                }
            }
        }
        best.map(|(_, piece)| piece)
    }

    /// The points of a series made of `pieces`, traced as `how` says, in image pixels: from left to right, or for a
    /// pattern in the order of `pieces` (the order along the path), each dash's points running on from the last point.
    pub fn trace(&self, pieces: &[usize], how: Trace) -> Vec<Point> {
        let mut points: Vec<Point> = match how {
            Trace::Line => pieces.iter().flat_map(|&p| self.columns(p)).collect(),
            Trace::Markers => pieces.iter().filter_map(|&p| self.centre(p)).collect(),
            Trace::Pattern => {
                let mut out: Vec<Point> = Vec::new();
                for &p in pieces {
                    let Some((_, long, short, direction)) = self.axes(p) else { continue };
                    if short >= 0.6 * long {
                        out.extend(self.centre(p));
                        continue;
                    }
                    // a dash, or dots run together: a point every stroke width along it
                    let count = (long / short.max(1.0)).round().max(1.0) as usize;
                    let mut along = self.along(p, direction, count);
                    if let (Some(last), Some(first), Some(end)) = (out.last(), along.first(), along.last())
                        && (*end - *last).hypot() < (*first - *last).hypot()
                    {
                        along.reverse();
                    }
                    out.extend(along);
                }
                return out;
            }
        };
        points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        points
    }

    /// The centre of piece `piece`, the lengths of its long and short axes (the sides of the rectangle with the same second
    /// moments: a square of side s gives s and s, however it is turned) and the direction of the long one, in radians.
    pub(crate) fn axes(&self, piece: usize) -> Option<(Point, f64, f64, f64)> {
        let b = self.pieces.get(piece)?.bounds;
        let label = piece as u32 + 1;
        let (mut n, mut sx, mut sy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for y in b.y..b.y + b.height {
            for x in b.x..b.x + b.width {
                if self.index(x, y).is_some_and(|i| self.labels[i] == label) {
                    let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    n += 1.0;
                    sx += fx;
                    sy += fy;
                    sxx += fx * fx;
                    syy += fy * fy;
                    sxy += fx * fy;
                }
            }
        }
        if n == 0.0 {
            return None;
        }
        let (cx, cy) = (sx / n, sy / n);
        let (vxx, vyy, vxy) = (sxx / n - cx * cx + 1.0 / 12.0, syy / n - cy * cy + 1.0 / 12.0, sxy / n - cx * cy);
        let (half, root) = ((vxx + vyy) / 2.0, (((vxx - vyy) / 2.0).powi(2) + vxy * vxy).sqrt());
        let direction = 0.5 * (2.0 * vxy).atan2(vxx - vyy);
        Some((Point::new(cx, cy), (12.0 * (half + root)).sqrt(), (12.0 * (half - root).max(0.0)).sqrt(), direction))
    }

    /// The centre of the curve colours' coverage within `radius` pixels of image position `(x, y)`: where a point put
    /// there by hand belongs, if there is ink under it.
    pub fn centre_near(&self, x: f64, y: f64, radius: f64) -> Option<Point> {
        let r = radius.max(1.0).ceil() as i64;
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let (mut sum_x, mut sum_y, mut weight) = (0.0, 0.0, 0.0);
        for py in (cy - r).max(0)..=cy + r {
            for px in (cx - r).max(0)..=cx + r {
                let Some(i) = self.index(px as u32, py as u32) else { continue };
                if self.labels[i] == 0 {
                    continue;
                }
                let c = f64::from(self.coverage[i]);
                sum_x += c * (px as f64 + 0.5);
                sum_y += c * (py as f64 + 0.5);
                weight += c;
            }
        }
        (weight > 0.0).then(|| Point::new(sum_x / weight, sum_y / weight))
    }

    /// Count the pieces again from the labels, after some were split: their bounds, pixels and first pixel.
    pub(crate) fn recount(&mut self) {
        let c = self.crop;
        for piece in &mut self.pieces {
            piece.pixels = 0;
        }
        let mut seen = vec![false; self.pieces.len()];
        for (i, &label) in self.labels.iter().enumerate() {
            if label == 0 {
                continue;
            }
            let k = label as usize - 1;
            let (x, y) = (c.x + (i % c.width as usize) as u32, c.y + (i / c.width as usize) as u32);
            let piece = &mut self.pieces[k];
            if !seen[k] {
                seen[k] = true;
                piece.bounds = PixelRect { x, y, width: 1, height: 1 };
                piece.seed = (x, y);
            } else {
                let b = &mut piece.bounds;
                let (x0, y0) = (b.x.min(x), b.y.min(y));
                let (x1, y1) = ((b.x + b.width).max(x + 1), (b.y + b.height).max(y + 1));
                *b = PixelRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
            }
            piece.pixels += 1;
        }
    }

    /// A point for each stretch of piece `piece` down each column it is in, at the centre of the coverage there. The faint
    /// pixel just above and just below a stretch count too, unless they are another piece's.
    fn columns(&self, piece: usize) -> Vec<Point> {
        let Some(b) = self.pieces.get(piece).map(|p| p.bounds) else { return Vec::new() };
        let label = piece as u32 + 1;
        let mut points = Vec::new();
        for x in b.x..b.x + b.width {
            let column = |y: u32| self.index(x, y);
            let mut y = b.y;
            while y < b.y + b.height {
                if column(y).is_none_or(|i| self.labels[i] != label) {
                    y += 1;
                    continue;
                }
                let start = y;
                while y < b.y + b.height && column(y).is_some_and(|i| self.labels[i] == label) {
                    y += 1;
                }
                let (mut sum, mut weight) = (0.0, 0.0);
                for yy in start.saturating_sub(1)..=y {
                    let Some(i) = column(yy) else { continue };
                    if self.labels[i] != label && self.labels[i] != 0 {
                        continue;
                    }
                    let c = f64::from(self.coverage[i]);
                    sum += c * (f64::from(yy) + 0.5);
                    weight += c;
                }
                if weight > 0.0 {
                    points.push(Point::new(f64::from(x) + 0.5, sum / weight));
                }
            }
        }
        points
    }

    /// `count` points along piece `piece`, whose long side runs in `direction` (radians): the centres of the coverage of
    /// as many equal stretches of it, from one end to the other.
    fn along(&self, piece: usize, direction: f64, count: usize) -> Vec<Point> {
        let Some(b) = self.pieces.get(piece).map(|p| p.bounds) else { return Vec::new() };
        let label = piece as u32 + 1;
        let (ux, uy) = (direction.cos(), direction.sin());
        let mut cells = Vec::new();
        for y in b.y..b.y + b.height {
            for x in b.x..b.x + b.width {
                if let Some(i) = self.index(x, y).filter(|&i| self.labels[i] == label) {
                    let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                    cells.push((fx * ux + fy * uy, fx, fy, f64::from(self.coverage[i])));
                }
            }
        }
        let (lo, hi) = cells.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| (lo.min(c.0), hi.max(c.0)));
        let span = (hi - lo).max(1e-9);
        let mut sums = vec![(0.0, 0.0, 0.0); count];
        for (t, x, y, c) in cells {
            let k = (((t - lo) / span * count as f64) as usize).min(count - 1);
            sums[k] = (sums[k].0 + c * x, sums[k].1 + c * y, sums[k].2 + c);
        }
        sums.into_iter().filter(|s| s.2 > 0.0).map(|(x, y, w)| Point::new(x / w, y / w)).collect()
    }

    /// The centre of the coverage of piece `piece`.
    fn centre(&self, piece: usize) -> Option<Point> {
        let b = self.pieces.get(piece)?.bounds;
        let label = piece as u32 + 1;
        let (mut sx, mut sy, mut weight) = (0.0, 0.0, 0.0);
        for y in b.y..b.y + b.height {
            for x in b.x..b.x + b.width {
                let Some(i) = self.index(x, y).filter(|&i| self.labels[i] == label) else { continue };
                let c = f64::from(self.coverage[i]);
                sx += c * (f64::from(x) + 0.5);
                sy += c * (f64::from(y) + 0.5);
                weight += c;
            }
        }
        (weight > 0.0).then(|| Point::new(sx / weight, sy / weight))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CROP: PixelRect = PixelRect { x: 10, y: 20, width: 100, height: 60 };

    /// A mask over [`CROP`] with what `draw` puts in it, at image positions.
    fn mask(draw: impl Fn(&mut dyn FnMut(u32, u32, u8))) -> Mask {
        let mut data = vec![0u8; CROP.width as usize * CROP.height as usize];
        draw(&mut |x, y, v| data[((y - CROP.y) * CROP.width + (x - CROP.x)) as usize] = v);
        Mask::from_data(CROP, data)
    }

    fn source(ink: usize, draw: impl Fn(&mut dyn FnMut(u32, u32, u8))) -> Source {
        Source { ink, mask: mask(draw), cut: Vec::new() }
    }

    #[test]
    fn a_colour_breaks_into_its_connected_pieces_and_specks_are_dropped() {
        let pieces = find(
            &[source(0, |put| {
                for x in 20..60 {
                    put(x, 30, 255); // a line
                }
                for x in 70..80 {
                    put(x, 50, 255); // a dash
                    put(x + 1, 51, 255); // touching it diagonally: the same piece
                }
                put(100, 70, 255); // a speck
            })],
            &Params::default(),
        );
        assert_eq!(pieces.pieces().len(), 2, "{:?}", pieces.pieces());
        let line = pieces.pieces()[0];
        assert_eq!((line.ink, line.pixels, line.seed), (0, 40, (20, 30)));
        assert_eq!(line.bounds, PixelRect { x: 20, y: 30, width: 40, height: 1 });
        assert_eq!(pieces.pieces()[1].pixels, 20);
        assert_eq!(
            (pieces.at(35, 30), pieces.at(75, 50), pieces.at(100, 70), pieces.at(0, 0)),
            (Some(0), Some(1), None, None)
        );
        assert_eq!(pieces.nearest(35.5, 32.4, 3.0), Some(0), "a pointer beside the line finds it");
        assert_eq!(pieces.nearest(35.5, 40.0, 3.0), None);
    }

    #[test]
    fn the_lines_of_the_axes_are_cut_out_of_a_colour_that_is_the_axes_too() {
        let draw = |put: &mut dyn FnMut(u32, u32, u8)| {
            for x in 20..100 {
                put(x, 70, 255); // the x axis
            }
            for y in 30..70 {
                put(50, y, 255); // a curve standing on it
            }
        };
        let whole = find(&[source(0, draw)], &Params::default());
        assert_eq!(whole.pieces().len(), 1, "the curve touches the axis");
        let axis = PixelRect { x: 20, y: 69, width: 80, height: 3 };
        let cut = find(&[Source { ink: 0, mask: mask(draw), cut: vec![axis] }], &Params::default());
        assert_eq!(cut.pieces().len(), 1, "the axis is gone and the curve is left");
        assert_eq!(cut.pieces()[0].bounds, PixelRect { x: 50, y: 30, width: 1, height: 39 });
    }

    #[test]
    fn a_line_is_traced_through_the_centre_of_each_column_to_a_fraction_of_a_pixel() {
        let pieces = find(
            &[source(0, |put| {
                for x in 20..30 {
                    put(x, 40, 255);
                    put(x, 41, 255);
                    put(x, 42, 64); // faint: not in the piece, but it pulls the centre down
                }
            })],
            &Params::default(),
        );
        let points = pieces.trace(&[0], Trace::Line);
        assert_eq!(points.len(), 10);
        let want = (40.5 * 255.0 + 41.5 * 255.0 + 42.5 * 64.0) / (255.0 + 255.0 + 64.0);
        for (k, p) in points.iter().enumerate() {
            assert!((p.x - (20.5 + k as f64)).abs() < 1e-9 && (p.y - want).abs() < 1e-9, "{p:?}");
        }
    }

    #[test]
    fn markers_are_traced_to_their_centres_from_left_to_right_and_inks_are_kept() {
        let square = |put: &mut dyn FnMut(u32, u32, u8), x0: u32, y0: u32| {
            for y in y0..y0 + 3 {
                for x in x0..x0 + 3 {
                    put(x, y, 255);
                }
            }
        };
        let pieces = find(
            &[
                source(2, |put| {
                    square(put, 60, 30);
                    square(put, 20, 50);
                }),
                source(5, |put| square(put, 90, 70)),
            ],
            &Params::default(),
        );
        assert_eq!(pieces.pieces().iter().map(|p| p.ink).collect::<Vec<_>>(), [2, 2, 5]);
        let points = pieces.trace(&[0, 1], Trace::Markers);
        assert_eq!(points, [Point::new(21.5, 51.5), Point::new(61.5, 31.5)]);
        // traced as a line instead, each square gives three columns
        assert_eq!(pieces.trace(&[0, 1], Trace::Line).len(), 6);
        assert!(pieces.trace(&[7], Trace::Markers).is_empty(), "a piece that is not there has no points");
    }
}
