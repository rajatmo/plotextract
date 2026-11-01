//! Preparing an image for tracing: which part of it is the plot (the crop), and which patches inside that are to be
//! ignored (the mask). Both are kept as shapes in image pixel coordinates, so they do not depend on any zoom, and are
//! turned into pixels only when something needs them.
//!
//! Masked pixels are *unknown*, not background: a later step must not read a masked legend as empty paper (a curve
//! passing behind it would be cut in two) nor let its colours count as ink.

use std::ops::Range;

use kurbo::{Point, Rect};
use serde::{Deserialize, Serialize};

/// One shape of the mask, in image pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Rect(Rect),
    /// A closed polygon, filled by the even-odd rule.
    Polygon(Vec<Point>),
    /// A brush stroke: round-capped, `width` pixels thick, along the points.
    Stroke {
        points: Vec<Point>,
        width: f64,
    },
}

/// One edit of the mask: paint a shape, or erase what earlier shapes painted. Applied in order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MaskOp {
    pub shape: Shape,
    pub erase: bool,
}

/// The crop and the mask. The crop is the whole image until one is set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prepare {
    pub crop: Option<Rect>,
    pub mask: Vec<MaskOp>,
}

/// A rectangle of whole pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The smallest crop, in pixels per side.
pub const MIN_CROP: f64 = 8.0;

/// A crop rectangle made valid: sides ordered, inside the image, at least [`MIN_CROP`] wide and high. `None` when it
/// cannot be (it misses the image).
pub fn clamp_crop(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    let image = Rect::new(0.0, 0.0, f64::from(width), f64::from(height));
    let r = rect.abs().intersect(image);
    (r.width() >= MIN_CROP.min(image.width()) && r.height() >= MIN_CROP.min(image.height())).then_some(r)
}

/// A grid of output pixels laid over image space: pixel `(i, j)` has its centre at `origin + (i + 0.5, j + 0.5) / scale`.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub origin: Point,
    /// Output pixels per image pixel.
    pub scale: f64,
    pub width: usize,
    pub height: usize,
}

impl Grid {
    /// The whole image, one output pixel per image pixel.
    pub fn image(width: u32, height: u32) -> Self {
        Self { origin: Point::ZERO, scale: 1.0, width: width as usize, height: height as usize }
    }

    /// The columns whose centres lie in `[lo, hi)` of image x.
    fn columns(&self, lo: f64, hi: f64) -> Range<usize> {
        span(lo, hi, self.origin.x, self.scale, self.width)
    }

    fn rows(&self, lo: f64, hi: f64) -> Range<usize> {
        span(lo, hi, self.origin.y, self.scale, self.height)
    }

    fn y_of_row(&self, j: usize) -> f64 {
        self.origin.y + (j as f64 + 0.5) / self.scale
    }

    fn x_of_column(&self, i: usize) -> f64 {
        self.origin.x + (i as f64 + 0.5) / self.scale
    }
}

/// The indices below `n` whose centres, at `origin + (k + 0.5) / scale`, lie in `[lo, hi)`.
fn span(lo: f64, hi: f64, origin: f64, scale: f64, n: usize) -> Range<usize> {
    let first = ((lo - origin) * scale - 0.5).ceil().max(0.0);
    let end = ((hi - origin) * scale - 0.5).ceil().max(0.0);
    let (first, end) = (first.min(n as f64) as usize, end.min(n as f64) as usize);
    first..end.max(first)
}

impl Prepare {
    /// The crop in whole pixels, widened to take in any pixel it touches. The whole image without a crop.
    pub fn crop_pixels(&self, width: u32, height: u32) -> PixelRect {
        let Some(c) = self.crop else { return PixelRect { x: 0, y: 0, width, height } };
        let x0 = c.x0.floor().clamp(0.0, f64::from(width.saturating_sub(1))) as u32;
        let y0 = c.y0.floor().clamp(0.0, f64::from(height.saturating_sub(1))) as u32;
        let x1 = (c.x1.ceil().clamp(0.0, f64::from(width)) as u32).max(x0 + 1);
        let y1 = (c.y1.ceil().clamp(0.0, f64::from(height)) as u32).max(y0 + 1);
        PixelRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
    }

    /// Paint the mask onto `grid`: 255 where it is masked, 0 elsewhere. The crop is not applied.
    pub fn rasterize_mask(&self, grid: &Grid) -> Vec<u8> {
        let mut out = vec![0u8; grid.width * grid.height];
        paint_ops(&self.mask, grid, &mut out);
        out
    }

    /// The pixels of the crop that a later step must treat as unknown (255) or may read (0), one per pixel of the crop.
    pub fn unknown_in_crop(&self, width: u32, height: u32) -> (PixelRect, Vec<u8>) {
        let crop = self.crop_pixels(width, height);
        let grid = Grid {
            origin: Point::new(f64::from(crop.x), f64::from(crop.y)),
            scale: 1.0,
            width: crop.width as usize,
            height: crop.height as usize,
        };
        (crop, self.rasterize_mask(&grid))
    }
}

/// Paint `ops`, in order, onto a raster of `grid` that may already hold the mask of the operations before them: what
/// they paint is added, what they erase is taken away. Painting the new operations onto the raster of the old ones gives
/// the same pixels as painting all of them at once.
pub fn paint_ops(ops: &[MaskOp], grid: &Grid, out: &mut [u8]) {
    for op in ops {
        let value = if op.erase { 0 } else { 255 };
        match &op.shape {
            Shape::Rect(r) => {
                let r = r.abs();
                let columns = grid.columns(r.x0, r.x1);
                for j in grid.rows(r.y0, r.y1) {
                    out[j * grid.width + columns.start..j * grid.width + columns.end].fill(value);
                }
            }
            Shape::Polygon(points) => fill_polygon(out, grid, points, value),
            Shape::Stroke { points, width } => stroke(out, grid, points, width / 2.0, value),
        }
    }
}

impl MaskOp {
    /// A box, in image pixels, that holds everything the operation paints or erases.
    pub fn bounds(&self) -> Rect {
        let around = |points: &[Point], pad: f64| {
            let mut r = Rect::new(f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
            for p in points {
                r = Rect::new(r.x0.min(p.x), r.y0.min(p.y), r.x1.max(p.x), r.y1.max(p.y));
            }
            if r.x0 > r.x1 { Rect::ZERO } else { r.inflate(pad, pad) }
        };
        match &self.shape {
            Shape::Rect(r) => r.abs(),
            Shape::Polygon(points) => around(points, 0.0),
            Shape::Stroke { points, width } => around(points, width / 2.0),
        }
    }
}

fn fill_polygon(out: &mut [u8], grid: &Grid, points: &[Point], value: u8) {
    if points.len() < 3 {
        return;
    }
    let (top, bottom) = points.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(t, b), p| (t.min(p.y), b.max(p.y)));
    let mut crossings = Vec::new();
    for j in grid.rows(top, bottom + f64::EPSILON) {
        let y = grid.y_of_row(j);
        crossings.clear();
        for (k, a) in points.iter().enumerate() {
            let b = points[(k + 1) % points.len()];
            if (a.y <= y) != (b.y <= y) {
                crossings.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
        }
        crossings.sort_by(f64::total_cmp);
        for pair in crossings.as_chunks::<2>().0 {
            let columns = grid.columns(pair[0], pair[1]);
            out[j * grid.width + columns.start..j * grid.width + columns.end].fill(value);
        }
    }
}

fn stroke(out: &mut [u8], grid: &Grid, points: &[Point], radius: f64, value: u8) {
    let mut dot = |a: Point, b: Point| {
        let (x0, x1) = (a.x.min(b.x) - radius, a.x.max(b.x) + radius);
        let (y0, y1) = (a.y.min(b.y) - radius, a.y.max(b.y) + radius);
        let ab = b - a;
        let len2 = ab.hypot2();
        for j in grid.rows(y0, y1) {
            let y = grid.y_of_row(j);
            for i in grid.columns(x0, x1) {
                let p = Point::new(grid.x_of_column(i), y);
                let t = if len2 > 0.0 { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
                if (a + ab * t - p).hypot2() <= radius * radius {
                    out[j * grid.width + i] = value;
                }
            }
        }
    };
    match points {
        [] => {}
        [only] => dot(*only, *only),
        _ => points.windows(2).for_each(|w| dot(w[0], w[1])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(prepare: &Prepare, w: u32, h: u32) -> usize {
        prepare.rasterize_mask(&Grid::image(w, h)).iter().filter(|&&v| v == 255).count()
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Shape {
        Shape::Rect(Rect::new(x0, y0, x1, y1))
    }

    fn paint(shape: Shape) -> MaskOp {
        MaskOp { shape, erase: false }
    }

    #[test]
    fn a_rectangle_masks_the_pixels_whose_centres_it_covers() {
        let p = Prepare { crop: None, mask: vec![paint(rect(2.0, 3.0, 6.0, 5.0))] };
        assert_eq!(masked(&p, 10, 10), 4 * 2);
        let m = p.rasterize_mask(&Grid::image(10, 10));
        assert_eq!((m[3 * 10 + 2], m[3 * 10 + 5], m[3 * 10 + 6], m[5 * 10 + 2]), (255, 255, 0, 0));
        // a corner dragged the wrong way round is the same rectangle
        let flipped = Prepare { crop: None, mask: vec![paint(rect(6.0, 5.0, 2.0, 3.0))] };
        assert_eq!(masked(&flipped, 10, 10), 8);
        // half a pixel is not enough to take a pixel in
        let thin = Prepare { crop: None, mask: vec![paint(rect(2.0, 3.0, 2.4, 5.0))] };
        assert_eq!(masked(&thin, 10, 10), 0);
    }

    #[test]
    fn a_polygon_is_filled_by_the_even_odd_rule() {
        let triangle = Shape::Polygon(vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(0.0, 10.0)]);
        let n = masked(&Prepare { crop: None, mask: vec![paint(triangle)] }, 10, 10);
        assert!((45..=55).contains(&n), "a right triangle of leg 10 covers about 50 pixels, not {n}");

        // a square with a square hole: crossing the polygon's own outline twice leaves the hole empty
        let ring = Shape::Polygon(
            [
                (0.0, 0.0),
                (20.0, 0.0),
                (20.0, 20.0),
                (0.0, 20.0),
                (0.0, 0.0),
                (5.0, 5.0),
                (5.0, 15.0),
                (15.0, 15.0),
                (15.0, 5.0),
                (5.0, 5.0),
            ]
            .map(|(x, y)| Point::new(x, y))
            .to_vec(),
        );
        let m = Prepare { crop: None, mask: vec![paint(ring)] }.rasterize_mask(&Grid::image(20, 20));
        assert_eq!(m[2 * 20 + 2], 255);
        assert_eq!(m[10 * 20 + 10], 0, "the hole");
        // fewer than three points is nothing
        assert_eq!(
            masked(
                &Prepare { crop: None, mask: vec![paint(Shape::Polygon(vec![Point::ZERO, Point::new(5.0, 5.0)]))] },
                10,
                10
            ),
            0
        );
    }

    #[test]
    fn a_brush_stroke_is_a_round_capped_band_and_a_click_is_a_dot() {
        let stroke = Shape::Stroke { points: vec![Point::new(5.0, 10.0), Point::new(25.0, 10.0)], width: 6.0 };
        let m = Prepare { crop: None, mask: vec![paint(stroke)] }.rasterize_mask(&Grid::image(30, 20));
        assert_eq!(m[10 * 30 + 15], 255);
        assert_eq!(m[7 * 30 + 15], 255, "within the half width");
        assert_eq!(m[5 * 30 + 15], 0, "beyond it");
        assert_eq!(m[10 * 30 + 3], 255, "the round cap reaches past the first point");
        assert_eq!(m[10 * 30 + 1], 0);

        let dot = Shape::Stroke { points: vec![Point::new(10.0, 10.0)], width: 5.0 };
        let n = masked(&Prepare { crop: None, mask: vec![paint(dot)] }, 20, 20);
        assert!((16..=25).contains(&n), "a disc of diameter 5, not {n} pixels");
    }

    #[test]
    fn erasing_takes_back_what_earlier_shapes_painted_in_order() {
        let p = Prepare {
            crop: None,
            mask: vec![
                paint(rect(0.0, 0.0, 10.0, 10.0)),
                MaskOp { shape: rect(2.0, 2.0, 5.0, 5.0), erase: true },
                paint(rect(3.0, 3.0, 4.0, 4.0)),
            ],
        };
        let m = p.rasterize_mask(&Grid::image(10, 10));
        assert_eq!(m.iter().filter(|&&v| v == 255).count(), 100 - 9 + 1);
        assert_eq!(m[3 * 10 + 3], 255, "painted again after the erase");
        assert_eq!(m[2 * 10 + 2], 0);
    }

    #[test]
    fn painting_new_operations_onto_the_old_raster_gives_the_same_pixels_as_painting_all_at_once() {
        let stroke =
            |x: f64| Shape::Stroke { points: vec![Point::new(x, 4.0), Point::new(x + 20.0, 24.0)], width: 7.0 };
        let ops = vec![
            paint(stroke(2.0)),
            paint(rect(10.0, 10.0, 30.0, 20.0)),
            MaskOp { shape: rect(12.0, 12.0, 18.0, 18.0), erase: true },
            paint(Shape::Polygon(vec![Point::new(25.0, 2.0), Point::new(38.0, 2.0), Point::new(30.0, 14.0)])),
            MaskOp { shape: stroke(8.0), erase: true },
        ];
        let grid = Grid::image(40, 30);
        let all = Prepare { crop: None, mask: ops.clone() }.rasterize_mask(&grid);
        for split in 0..=ops.len() {
            let mut raster = Prepare { crop: None, mask: ops[..split].to_vec() }.rasterize_mask(&grid);
            paint_ops(&ops[split..], &grid, &mut raster);
            assert!(raster == all, "split after {split} operations");
        }
    }

    #[test]
    fn an_operation_knows_the_box_it_changes() {
        let op = |shape| MaskOp { shape, erase: false };
        assert_eq!(op(rect(30.0, 20.0, 10.0, 5.0)).bounds(), Rect::new(10.0, 5.0, 30.0, 20.0));
        let poly = Shape::Polygon(vec![Point::new(3.0, 4.0), Point::new(9.0, 2.0), Point::new(5.0, 11.0)]);
        assert_eq!(op(poly).bounds(), Rect::new(3.0, 2.0, 9.0, 11.0));
        let stroke = Shape::Stroke { points: vec![Point::new(10.0, 10.0), Point::new(20.0, 12.0)], width: 6.0 };
        assert_eq!(op(stroke).bounds(), Rect::new(7.0, 7.0, 23.0, 15.0));
        assert_eq!(op(Shape::Polygon(vec![])).bounds(), Rect::ZERO);
        // everything painted lies inside the box
        let grid = Grid::image(40, 30);
        let stroke = Shape::Stroke { points: vec![Point::new(10.0, 10.0), Point::new(20.0, 12.0)], width: 6.0 };
        let b = op(stroke.clone()).bounds();
        let m = Prepare { crop: None, mask: vec![op(stroke)] }.rasterize_mask(&grid);
        for (i, v) in m.iter().enumerate() {
            let (x, y) = ((i % 40) as f64 + 0.5, (i / 40) as f64 + 0.5);
            assert!(*v == 0 || (b.x0 <= x && x <= b.x1 && b.y0 <= y && y <= b.y1), "pixel ({x}, {y})");
        }
    }

    #[test]
    fn the_mask_can_be_drawn_at_any_scale_and_offset() {
        let p = Prepare { crop: None, mask: vec![paint(rect(10.0, 10.0, 20.0, 20.0))] };
        // the same patch at a quarter of the resolution
        let small = Grid { origin: Point::ZERO, scale: 0.25, width: 10, height: 10 };
        let m = p.rasterize_mask(&small);
        assert_eq!(
            m.iter().filter(|&&v| v == 255).count(),
            9,
            "the centres 10, 14 and 18 fall in the patch, in each direction"
        );
        // a window of the image at four times the resolution
        let zoomed = Grid { origin: Point::new(8.0, 8.0), scale: 4.0, width: 16, height: 16 };
        let m = p.rasterize_mask(&zoomed);
        assert_eq!(m[0], 0);
        assert_eq!(m[8 * 16 + 8], 255, "image pixel (10, 10) starts at output pixel (8, 8)");
        assert_eq!(m.iter().filter(|&&v| v == 255).count(), 8 * 8, "the window ends at image pixel 12");
    }

    #[test]
    fn the_crop_is_whole_pixels_that_take_in_every_pixel_it_touches() {
        let none = Prepare::default();
        assert_eq!(none.crop_pixels(40, 30), PixelRect { x: 0, y: 0, width: 40, height: 30 });
        let p = Prepare { crop: Some(Rect::new(10.3, 5.9, 20.1, 25.0)), mask: vec![] };
        assert_eq!(p.crop_pixels(40, 30), PixelRect { x: 10, y: 5, width: 11, height: 20 });
        let off = Prepare { crop: Some(Rect::new(-5.0, -5.0, 100.0, 100.0)), mask: vec![] };
        assert_eq!(off.crop_pixels(40, 30), PixelRect { x: 0, y: 0, width: 40, height: 30 });
    }

    #[test]
    fn the_unknown_pixels_are_the_mask_cut_to_the_crop() {
        let p =
            Prepare { crop: Some(Rect::new(10.0, 10.0, 30.0, 20.0)), mask: vec![paint(rect(0.0, 0.0, 15.0, 100.0))] };
        let (crop, unknown) = p.unknown_in_crop(50, 50);
        assert_eq!((crop.width, crop.height), (20, 10));
        assert_eq!(unknown.len(), 200);
        assert_eq!((unknown[0], unknown[4], unknown[5]), (255, 255, 0), "x from 10 to 14 is masked, 15 is not");
    }

    #[test]
    fn a_crop_is_made_valid() {
        let r = clamp_crop(Rect::new(30.0, 25.0, -10.0, 5.0), 40, 30).unwrap();
        assert_eq!(r, Rect::new(0.0, 5.0, 30.0, 25.0), "ordered and inside the image");
        assert_eq!(clamp_crop(Rect::new(10.0, 10.0, 12.0, 30.0), 40, 30), None, "too thin");
        assert_eq!(clamp_crop(Rect::new(100.0, 100.0, 200.0, 200.0), 40, 30), None, "outside the image");
    }
}
