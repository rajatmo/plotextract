//! The curves of a plot: the pieces the masks of the curve colours break into, and where each one is.
//!
//! A piece is a connected set of pixels of one colour: a line, a dash of a dashed line, a marker, a letter. Finding them is
//! done by an existing package (the connected components of `imageproc`); what is written here is what it does not give:
//! the straight lines of the axes are cut out first, for a colour that is the axes as well as a curve, specks are dropped,
//! and a piece is remembered by one of its pixels so that it can be found again when the pieces are.
//!
//! A piece's point is the centre of the coverage it stands for ([`Pieces::centre`]), so to a fraction of a pixel.

use image::{GrayImage, ImageBuffer, Luma};
use imageproc::region_labelling::{Connectivity, connected_components};
use kurbo::Point;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::mask::Mask;
use crate::prepare::PixelRect;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

    /// Points along piece `piece` from one end to the other, `step` pixels apart along it: the centre of the coverage of
    /// each stretch of it. The stretches are cut by how far a pixel is from the far end, walking within the piece, so a
    /// stroke that runs up and back down (a spike, a fold, a curve with several values at one x) comes out in the order
    /// it runs, never sorted by x. A stroke that crosses itself is not told apart at the crossing.
    pub fn stroke(&self, piece: usize, step: f64) -> Vec<Point> {
        let Some(b) = self.pieces.get(piece).map(|p| p.bounds) else { return Vec::new() };
        let label = piece as u32 + 1;
        let (bw, bh) = (b.width as usize, b.height as usize);
        let inside =
            |x: usize, y: usize| self.index(b.x + x as u32, b.y + y as u32).filter(|&i| self.labels[i] == label);
        // the distances from `start`, walking in the piece but not through `blocked`, and the farthest pixel
        let walk = |start: (usize, usize), blocked: &dyn Fn(usize) -> bool| {
            let mut d = vec![u32::MAX; bw * bh];
            let mut queue = std::collections::VecDeque::from([start]);
            d[start.1 * bw + start.0] = 0;
            let mut far = start;
            while let Some((x, y)) = queue.pop_front() {
                let here = d[y * bw + x];
                if here > d[far.1 * bw + far.0] {
                    far = (x, y);
                }
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                        continue;
                    }
                    let (nx, ny) = (nx as usize, ny as usize);
                    if d[ny * bw + nx] == u32::MAX && !blocked(ny * bw + nx) && inside(nx, ny).is_some() {
                        d[ny * bw + nx] = here + 1;
                        queue.push_back((nx, ny));
                    }
                }
            }
            (d, far)
        };
        let Some(first) = (0..bw * bh).map(|k| (k % bw, k / bw)).find(|&(x, y)| inside(x, y).is_some()) else {
            return Vec::new();
        };
        // one end: the pixel farthest from any pixel. The stroke is cut just after it and walked from the cut, so that a
        // closed loop (which has no end) is walked once round rather than both ways at once; the cut comes first
        let (_, end) = walk(first, &|_| false);
        let (near, _) = walk(end, &|_| false);
        let cut = 2;
        let restart = (0..bw * bh).find(|&k| near[k] == cut + 1).map(|k| (k % bw, k / bw));
        let d = match restart {
            Some(start) => {
                let (mut d, _) = walk(start, &|k| near[k] <= cut);
                for (k, v) in d.iter_mut().enumerate() {
                    if near[k] <= cut {
                        *v = 0;
                    } else if *v != u32::MAX {
                        *v += cut + 1;
                    }
                }
                d
            }
            None => near,
        };
        let step = step.max(1.0);
        let bins = d.iter().filter(|&&v| v != u32::MAX).max().map_or(0, |&m| (f64::from(m) / step) as usize + 1);
        let mut sums = vec![(0.0f64, 0.0f64, 0.0f64); bins];
        for (k, &v) in d.iter().enumerate() {
            if v == u32::MAX {
                continue;
            }
            let (x, y) = (k % bw, k / bw);
            let Some(i) = inside(x, y) else { continue };
            let c = f64::from(self.coverage[i]);
            let s = &mut sums[(f64::from(v) / step) as usize];
            *s = (s.0 + c * (f64::from(b.x) + x as f64 + 0.5), s.1 + c * (f64::from(b.y) + y as f64 + 0.5), s.2 + c);
        }
        sums.into_iter().filter(|s| s.2 > 0.0).map(|(x, y, c)| Point::new(x / c, y / c)).collect()
    }

    /// The centre of the coverage of every piece, as [`Pieces::centre`] gives one, in one pass over the labels.
    pub fn centres(&self) -> Vec<Option<Point>> {
        let w = self.crop.width as usize;
        let mut sums = vec![(0.0f64, 0.0f64, 0.0f64); self.pieces.len()];
        for (i, &label) in self.labels.iter().enumerate() {
            if label == 0 {
                continue;
            }
            let c = f64::from(self.coverage[i]);
            let (x, y) = (f64::from(self.crop.x) + (i % w) as f64 + 0.5, f64::from(self.crop.y) + (i / w) as f64 + 0.5);
            let s = &mut sums[label as usize - 1];
            *s = (s.0 + c * x, s.1 + c * y, s.2 + c);
        }
        sums.into_iter().map(|(x, y, c)| (c > 0.0).then(|| Point::new(x / c, y / c))).collect()
    }

    /// The centre of the coverage of piece `piece`, to a fraction of a pixel: a pixel counts as much as it is covered.
    pub fn centre(&self, piece: usize) -> Option<Point> {
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
    fn a_piece_is_at_the_centre_of_its_coverage_and_inks_are_kept() {
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
        // the pieces are found row by row: the square higher up first
        assert_eq!((pieces.centre(0), pieces.centre(1)), (Some(Point::new(61.5, 31.5)), Some(Point::new(21.5, 51.5))));
        assert_eq!(pieces.centre(7), None, "a piece that is not there has no centre");
        assert_eq!(pieces.centres(), (0..3).map(|p| pieces.centre(p)).collect::<Vec<_>>(), "all at once, the same");
        // a pixel half covered pulls the centre half as much
        let edge = find(
            &[source(0, |put| {
                square(put, 40, 40);
                for y in 40..43 {
                    put(43, y, 128);
                }
            })],
            &Params::default(),
        );
        let c = edge.centre(0).unwrap();
        let want = (41.5 * 3.0 * 255.0 + 43.5 * 128.0) / (3.0 * 255.0 + 128.0);
        assert!((c.x - want).abs() < 1e-9 && (c.y - 41.5).abs() < 1e-9, "{c:?}");
    }

    #[test]
    fn a_stroke_that_doubles_back_is_followed_in_the_order_it_runs() {
        // a hairpin: right along y = 40, down the bend at x = 80, back left along y = 50; two pixels thick
        let pieces = find(
            &[source(0, |put| {
                for x in 20..82 {
                    put(x, 40, 255);
                    put(x, 41, 255);
                    put(x, 50, 255);
                    put(x, 51, 255);
                }
                for y in 40..52 {
                    put(80, y, 255);
                    put(81, y, 255);
                }
            })],
            &Params::default(),
        );
        let points = pieces.stroke(0, 2.0);
        assert!(points.len() >= 60, "about one every two pixels of its {} or so: {}", 62 + 62 + 10, points.len());
        let (first, last) = (points[0], *points.last().unwrap());
        assert!(first.x < 25.0 && last.x < 25.0, "it starts at one end and finishes at the other: {first:?} {last:?}");
        assert!((first.y - last.y).abs() > 8.0, "one end on each arm");
        // out along one arm and back along the other: x rises, then falls
        let turn = points.iter().enumerate().max_by(|a, b| a.1.x.total_cmp(&b.1.x)).unwrap().0;
        assert!(points[..turn].windows(2).all(|w| w[1].x >= w[0].x - 0.5), "out: {:?}", &points[..turn]);
        assert!(points[turn..].windows(2).all(|w| w[1].x <= w[0].x + 0.5), "back: {:?}", &points[turn..]);
        // the centre of the stroke: on an arm, half way across its two pixels
        assert!(points.iter().filter(|p| p.x < 70.0).all(|p| (p.y - 41.0).abs() < 0.6 || (p.y - 51.0).abs() < 0.6));
    }
}
