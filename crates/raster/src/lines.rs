//! Straight lines in a mask: the frame, the axes and the ticks of a plot.
//!
//! The detecting is done by existing packages, one per [`Algorithm`]: the line segment detector (`lsdetect`) and the Hough
//! transform (`imageproc`). What is written here is what they do not give: images of plots are upright, so only
//! horizontal and vertical lines are kept; a line drawn two pixels thick comes out of an algorithm as two edges or several
//! near-identical lines, so those are merged; and the position across a line is measured on the mask, as the centre of its
//! coverage, so it is known to a fraction of a pixel whatever the algorithm said.

use image::{GrayImage, Luma};
use imageproc::distance_transform::Norm;
use imageproc::hough::{self, LineDetectionOptions};
use imageproc::morphology;

use crate::mask::Mask;
use crate::prepare::PixelRect;

/// How a mask is searched for straight lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Algorithm {
    /// The Hough transform: every covered pixel votes for the lines through it. Upright lines only, short ticks as well as
    /// long lines.
    #[default]
    Hough,
    /// The line segment detector: follows the edges of strokes, so it finds long lines (the frame and the axes) even if
    /// they are slightly tilted, but not lines shorter than about 15 pixels.
    Lsd,
}

impl Algorithm {
    pub const ALL: [Algorithm; 2] = [Algorithm::Hough, Algorithm::Lsd];

    pub fn label(self) -> &'static str {
        match self {
            Algorithm::Lsd => "Line segments (LSD)",
            Algorithm::Hough => "Hough transform",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Algorithm::Hough => {
                "Lets every pixel vote for the lines through it. Finds short ticks as well as long lines"
            }
            Algorithm::Lsd => "Follows the edges of strokes. Finds the long lines (frame and axes), not short ticks",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

/// A horizontal or vertical line, in image pixels. Pixel `n` has its centre at `n + 0.5`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub orientation: Orientation,
    /// Where it starts and ends along its length: x for a horizontal line, y for a vertical one.
    pub from: f32,
    pub to: f32,
    /// Where it is across its length: y for a horizontal line, x for a vertical one.
    pub across: f32,
    /// How thick it is.
    pub width: f32,
}

impl Line {
    pub fn length(&self) -> f32 {
        self.to - self.from
    }

    /// The pixels the line is in: its length, and its thickness with a pixel to spare each side.
    pub fn band(&self) -> PixelRect {
        let half = self.width / 2.0 + 1.0;
        let (lo, hi) = ((self.across - half).floor().max(0.0) as u32, (self.across + half).ceil().max(0.0) as u32);
        let (a, b) = (self.from.floor().max(0.0) as u32, self.to.ceil().max(0.0) as u32);
        match self.orientation {
            Orientation::Horizontal => PixelRect { x: a, y: lo, width: b.saturating_sub(a), height: hi - lo },
            Orientation::Vertical => PixelRect { x: lo, y: a, width: hi - lo, height: b.saturating_sub(a) },
        }
    }

    /// The distance from image position `(x, y)` to the line.
    pub fn distance(&self, x: f32, y: f32) -> f32 {
        let (along, across) = match self.orientation {
            Orientation::Horizontal => (x, y),
            Orientation::Vertical => (y, x),
        };
        let beyond = (self.from - along).max(along - self.to).max(0.0);
        let aside = ((across - self.across).abs() - self.width / 2.0).max(0.0);
        beyond.hypot(aside)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// The shortest line, in pixels.
    pub min_length: f32,
    /// How far from horizontal or vertical a line may be, in degrees (for the line segment detector; the Hough transform
    /// looks at the upright angles only).
    pub tilt: f32,
    /// The thickest line, in pixels: anything thicker is a block, not a line.
    pub max_width: f32,
    /// How many times the mask is enlarged before the line segment detector looks at it. A segment is accepted only if it
    /// has more aligned pixels than chance explains, so a short line passes once it is enlarged enough. The Hough
    /// transform does not use it.
    pub enlarge: u32,
}

impl Default for Params {
    fn default() -> Self {
        Self { min_length: 5.0, tilt: 3.0, max_width: 8.0, enlarge: 1 }
    }
}

/// The most pixels the enlarged mask may have.
const MAX_ENLARGED_PIXELS: usize = 40_000_000;

/// The lines of `mask`, found with `algorithm`.
pub fn detect(mask: &Mask, algorithm: Algorithm, params: &Params) -> Vec<Line> {
    let crop = mask.crop();
    let (w, h) = (crop.width as usize, crop.height as usize);
    let origin = (crop.x as f32, crop.y as f32);
    let thin = without_blocks(mask, params);
    let mut lines = match algorithm {
        Algorithm::Lsd => lsd(&thin, w, h, params),
        Algorithm::Hough => hough(&thin, w, h, params),
    };
    for line in &mut lines {
        let (dx, dy) = match line.orientation {
            Orientation::Horizontal => (origin.0, origin.1),
            Orientation::Vertical => (origin.1, origin.0),
        };
        // from the crop to the image
        line.from += dx;
        line.to += dx;
        line.across += dy;
    }
    let mut lines = merge(mask, lines, params);
    lines.retain(|l| l.length() >= params.min_length);
    for line in &mut lines {
        centre(mask, line);
    }
    // the width is known only now, from the mask itself
    lines.retain(|l| l.width <= params.max_width);
    lines
}

/// The coverage of `mask` with its blocks taken out: what an opening (erode, then dilate) by the thickest line keeps is
/// wider than any line, so it is a block, not a stroke.
fn without_blocks(mask: &Mask, params: &Params) -> Vec<u8> {
    let crop = mask.crop();
    let binary = GrayImage::from_fn(crop.width, crop.height, |x, y| {
        Luma([if mask.data()[y as usize * crop.width as usize + x as usize] >= 128 { 255 } else { 0 }])
    });
    let radius = (params.max_width / 2.0).ceil() as u8;
    let blocks = morphology::open(&binary, Norm::LInf, radius);
    mask.data().iter().zip(blocks.as_raw()).map(|(&c, &b)| if b > 0 { 0 } else { c }).collect()
}

/// `data` (`w` x `h`) with every pixel made `k` x `k`.
fn enlarged(data: &[u8], w: usize, h: usize, k: usize) -> Vec<u8> {
    let mut big = Vec::with_capacity(w * h * k * k);
    for row in data.chunks_exact(w) {
        let wide: Vec<u8> = row.iter().flat_map(|&c| std::iter::repeat_n(c, k)).collect();
        for _ in 0..k {
            big.extend_from_slice(&wide);
        }
    }
    big
}

fn lsd(data: &[u8], w: usize, h: usize, params: &Params) -> Vec<Line> {
    let mut k = params.enlarge.max(1) as usize;
    while k > 1 && w * h * k * k > MAX_ENLARGED_PIXELS {
        k -= 1;
    }
    let big;
    let (image, bw, bh) = if k == 1 {
        (data, w, h)
    } else {
        big = enlarged(data, w, h, k);
        (big.as_slice(), w * k, h * k)
    };
    // scale 1: the detector smooths the image first at its default scale, which would blur a one pixel line away
    let Ok(segments) = lsdetect::detect(image, bw, bh, 1.0) else { return Vec::new() };
    // the detector's pixel centres are at whole numbers; an original pixel `n` has its centre at `n + 0.5`
    let back = |e: f64| ((e + 0.5) / k as f64) as f32;
    segments
        .into_iter()
        .filter_map(|s| {
            let angle = s.angle().abs() as f32;
            let (orientation, tilt) =
                if angle <= 45.0 { (Orientation::Horizontal, angle) } else { (Orientation::Vertical, 90.0 - angle) };
            if tilt > params.tilt {
                return None;
            }
            let width = s.width as f32 / k as f32;
            Some(match orientation {
                Orientation::Horizontal => Line {
                    orientation,
                    from: back(s.x1.min(s.x2)),
                    to: back(s.x1.max(s.x2)),
                    across: back((s.y1 + s.y2) / 2.0),
                    width,
                },
                Orientation::Vertical => Line {
                    orientation,
                    from: back(s.y1.min(s.y2)),
                    to: back(s.y1.max(s.y2)),
                    across: back((s.x1 + s.x2) / 2.0),
                    width,
                },
            })
        })
        .collect()
}

/// The upright lines of `data` (`w` x `h`) by the Hough transform. It is run on the mask for the vertical lines and on the
/// transposed mask for the horizontal ones: the transform is exact at 0 degrees (a line's distance is its x), while at 90
/// degrees its rounding splits a horizontal line's votes between two neighbouring buckets, and the rows could be wrong.
fn hough(data: &[u8], w: usize, h: usize, params: &Params) -> Vec<Line> {
    let mut found = upright(data, w, h, Orientation::Vertical, params);
    let mut transposed = vec![0u8; w * h];
    for (y, row) in data.chunks_exact(w).enumerate() {
        for (x, &c) in row.iter().enumerate() {
            transposed[x * h + y] = c;
        }
    }
    found.extend(upright(&transposed, h, w, Orientation::Horizontal, params));
    found
}

/// The vertical lines of `data` (`w` x `h`), reported as lines of `orientation` (the transposed mask's vertical lines are
/// the horizontal lines of the mask).
fn upright(data: &[u8], w: usize, h: usize, orientation: Orientation, params: &Params) -> Vec<Line> {
    let binary = GrayImage::from_fn(w as u32, h as u32, |x, y| {
        Luma([if data[y as usize * w + x as usize] >= 128 { 255 } else { 0 }])
    });
    // the votes of a column are its covered pixels
    let options = LineDetectionOptions { vote_threshold: params.min_length.ceil() as u32, suppression_radius: 0 };
    let mut found = Vec::new();
    for polar in hough::detect_lines(&binary, options) {
        let column = polar.r.round();
        if polar.angle_in_degrees != 0 || column < 0.0 || column >= w as f32 {
            continue;
        }
        let x = column as usize;
        // the stretches of the column that are covered, bridging a gap of one pixel
        let mut start = None;
        let mut gap = 0;
        for y in 0..=h {
            if y < h && data[y * w + x] >= 128 {
                start.get_or_insert(y);
                gap = 0;
            } else if let Some(s) = start {
                gap += 1;
                if gap > 1 || y == h {
                    let e = y + 1 - gap;
                    // a run shorter than the shortest line is not one (and would join its neighbours into a block)
                    if (e - s) as f32 >= params.min_length {
                        found.push(Line {
                            orientation,
                            from: s as f32,
                            to: e as f32,
                            across: x as f32 + 0.5,
                            width: 1.0,
                        });
                    }
                    start = None;
                    gap = 0;
                }
            }
        }
    }
    found
}

/// One line for the lines that are the same line: the same direction, along the same stretch, and either on top of each
/// other (neighbouring Hough buckets) or the two edges of one stroke (the detector finds each edge of a thick line
/// separately): edges with ink between them.
fn merge(mask: &Mask, mut lines: Vec<Line>, params: &Params) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    lines.sort_by(|a, b| a.across.total_cmp(&b.across));
    'next: for line in lines {
        for o in out.iter_mut().rev().take(64) {
            let (from, to) = (o.from.max(line.from), o.to.min(line.to));
            let shorter = o.length().min(line.length());
            if o.orientation != line.orientation || to - from < 0.5 * shorter {
                continue;
            }
            let (lo, hi) = (o.across - o.width / 2.0, o.across + o.width / 2.0);
            let (lo2, hi2) = (line.across - line.width / 2.0, line.across + line.width / 2.0);
            let gap = lo.max(lo2) - hi.min(hi2);
            let between = (o.across + line.across) / 2.0;
            let stroke = gap <= params.max_width
                && (line.across - o.across) <= params.max_width
                && ink_at(mask, o.orientation, between, from, to);
            // lines wider than the widest line are dropped afterwards; the cap here only stops a chain of lines from running on
            let thick = hi.max(hi2) - lo.min(lo2);
            if (gap <= 1.5 || stroke) && thick <= 2.0 * params.max_width {
                let (lo, hi) = (lo.min(lo2), hi.max(hi2));
                *o = Line {
                    orientation: o.orientation,
                    from: o.from.min(line.from),
                    to: o.to.max(line.to),
                    across: (lo + hi) / 2.0,
                    width: hi - lo,
                };
                continue 'next;
            }
        }
        out.push(line);
    }
    out
}

/// Whether the mask is mostly covered along the line at `across`, between `from` and `to`.
fn ink_at(mask: &Mask, orientation: Orientation, across: f32, from: f32, to: f32) -> bool {
    const SAMPLES: usize = 16;
    let covered = (0..SAMPLES)
        .filter(|&i| {
            let along = from + (to - from) * (i as f32 + 0.5) / SAMPLES as f32;
            let (x, y) = if orientation == Orientation::Horizontal { (along, across) } else { (across, along) };
            mask.at(x.floor().max(0.0) as u32, y.floor().max(0.0) as u32) >= 128
        })
        .count();
    covered * 2 >= SAMPLES
}

/// Put a line's position across it at the coverage-weighted centre of the mask in its band, and its width at the stretch
/// of the band that is covered.
fn centre(mask: &Mask, line: &mut Line) {
    let band = line.band();
    let (across, along) = match line.orientation {
        Orientation::Horizontal => ((band.y, band.height), (band.x, band.width)),
        Orientation::Vertical => ((band.x, band.width), (band.y, band.height)),
    };
    let (mut sum, mut weight, mut covered) = (0.0f64, 0.0f64, 0.0f32);
    for b in across.0..across.0 + across.1 {
        let column: f64 = (along.0..along.0 + along.1)
            .map(|a| {
                let (x, y) = if line.orientation == Orientation::Horizontal { (a, b) } else { (b, a) };
                f64::from(mask.at(x, y)) / 255.0
            })
            .sum();
        sum += column * (f64::from(b) + 0.5);
        weight += column;
        if column >= 0.5 * f64::from(along.1) {
            covered += 1.0;
        }
    }
    if weight > 0.0 {
        line.across = (sum / weight) as f32;
        line.width = covered.max(1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: usize = 200;
    const H: usize = 150;

    /// A mask over the crop (5, 7) W x H, drawn by `put(x, y, coverage)` in crop coordinates.
    fn mask(draw: impl Fn(&mut dyn FnMut(usize, usize, u8))) -> Mask {
        let crop = PixelRect { x: 5, y: 7, width: W as u32, height: H as u32 };
        let mut data = vec![0u8; W * H];
        draw(&mut |x, y, c| data[y * W + x] = data[y * W + x].max(c));
        Mask::from_data(crop, data)
    }

    /// A frame with its bottom-left corner at (20, 80), 100 wide and 60 high, 2 pixels thick, with ticks `length` long
    /// pointing out at the given x positions (bottom) and y positions (left).
    fn plot(x_ticks: &[usize], y_ticks: &[usize], length: usize) -> Mask {
        mask(|put| {
            for x in 20..120 {
                for dy in 0..2 {
                    put(x, 80 + dy, 255);
                    put(x, 20 + dy, 255);
                }
            }
            for y in 20..82 {
                for dx in 0..2 {
                    put(20 + dx, y, 255);
                    put(118 + dx, y, 255);
                }
            }
            for &x in x_ticks {
                for y in 82..82 + length {
                    put(x, y, 255);
                    put(x + 1, y, 255);
                }
            }
            for &y in y_ticks {
                for x in 20 - length..20 {
                    put(x, y, 255);
                    put(x, y + 1, 255);
                }
            }
        })
    }

    fn has(lines: &[Line], orientation: Orientation, across: f32, length: f32) -> bool {
        lines.iter().any(|l| {
            l.orientation == orientation && (l.across - across).abs() < 0.6 && (l.length() - length).abs() <= 3.0
        })
    }

    #[test]
    fn both_algorithms_find_the_frame_to_a_fraction_of_a_pixel() {
        let m = plot(&[], &[], 0);
        for algorithm in Algorithm::ALL {
            let lines = detect(&m, algorithm, &Params::default());
            // the crop is at (5, 7); the bottom line covers rows 80 and 81, so its centre is at 7 + 81
            assert!(has(&lines, Orientation::Horizontal, 7.0 + 81.0, 100.0), "{algorithm:?} bottom: {lines:?}");
            assert!(has(&lines, Orientation::Horizontal, 7.0 + 21.0, 100.0), "{algorithm:?} top: {lines:?}");
            assert!(has(&lines, Orientation::Vertical, 5.0 + 21.0, 62.0), "{algorithm:?} left: {lines:?}");
            assert!(has(&lines, Orientation::Vertical, 5.0 + 119.0, 62.0), "{algorithm:?} right: {lines:?}");
            assert_eq!(lines.len(), 4, "{algorithm:?}: one line for each side, not one for each edge: {lines:?}");
        }
    }

    #[test]
    fn the_hough_transform_finds_short_ticks() {
        let m = plot(&[30, 60, 90], &[30, 50], 6);
        let lines = detect(&m, Algorithm::Hough, &Params::default());
        for x in [30.0, 60.0, 90.0] {
            assert!(has(&lines, Orientation::Vertical, 5.0 + x + 1.0, 6.0), "x tick {x}: {lines:?}");
        }
        for y in [30.0, 50.0] {
            assert!(has(&lines, Orientation::Horizontal, 7.0 + y + 1.0, 6.0), "y tick {y}: {lines:?}");
        }
    }

    #[test]
    fn the_line_segment_detector_needs_longer_ticks_than_that() {
        // a segment must have more aligned pixels than chance explains, which is more than a 6 pixel tick has
        let short = detect(&plot(&[60], &[], 6), Algorithm::Lsd, &Params::default());
        assert!(!has(&short, Orientation::Vertical, 5.0 + 61.0, 6.0), "{short:?}");
        let long = detect(&plot(&[60], &[], 24), Algorithm::Lsd, &Params::default());
        assert!(has(&long, Orientation::Vertical, 5.0 + 61.0, 24.0), "{long:?}");
    }

    #[test]
    fn nothing_is_found_in_an_empty_mask_or_a_blob() {
        for algorithm in Algorithm::ALL {
            assert!(detect(&mask(|_| {}), algorithm, &Params::default()).is_empty());
            let blob = mask(|put| {
                for y in 20..60 {
                    for x in 30..70 {
                        put(x, y, 255);
                    }
                }
            });
            let lines = detect(&blob, algorithm, &Params::default());
            assert!(lines.is_empty(), "{algorithm:?}: a block is not a line: {lines:?}");
        }
    }

    #[test]
    fn a_tilted_stroke_is_not_a_line_of_the_plot() {
        let m = mask(|put| {
            for i in 0..60 {
                put(20 + i, 20 + i / 2, 255); // 27 degrees
                put(20 + i, 21 + i / 2, 255);
            }
        });
        for algorithm in Algorithm::ALL {
            let lines = detect(&m, algorithm, &Params::default());
            assert!(lines.is_empty(), "{algorithm:?}: {lines:?}");
        }
    }

    #[test]
    fn the_position_across_a_line_comes_from_the_coverage() {
        // a vertical line whose columns cover 25%, 100%, 100%, 75%: its centre is weighted by that
        let m = mask(|put| {
            for y in 10..60 {
                for (dx, c) in [64u8, 255, 255, 191].into_iter().enumerate() {
                    put(30 + dx, y, c);
                }
            }
        });
        let weights = [0.25f32, 1.0, 1.0, 0.75];
        let centre =
            weights.iter().enumerate().map(|(i, w)| w * (i as f32 + 0.5)).sum::<f32>() / weights.iter().sum::<f32>();
        for algorithm in Algorithm::ALL {
            let lines = detect(&m, algorithm, &Params::default());
            let line = lines.iter().find(|l| l.orientation == Orientation::Vertical && l.length() > 40.0);
            let line = line.unwrap_or_else(|| panic!("{algorithm:?}: {lines:?}"));
            assert!(
                (line.across - (5.0 + 30.0 + centre)).abs() < 0.15,
                "{algorithm:?}: {} vs {}",
                line.across,
                35.0 + centre
            );
        }
    }

    #[test]
    fn a_line_knows_its_band_and_its_distance_from_a_point() {
        let l = Line { orientation: Orientation::Vertical, from: 20.0, to: 30.0, across: 41.0, width: 2.0 };
        assert_eq!(l.band(), PixelRect { x: 39, y: 20, width: 4, height: 10 });
        assert_eq!(l.distance(41.0, 25.0), 0.0);
        assert_eq!(l.distance(44.0, 25.0), 2.0);
        assert_eq!(l.distance(41.0, 34.0), 4.0);
        assert!((l.distance(44.0, 34.0) - 2.0f32.hypot(4.0)).abs() < 1e-6);
    }

    #[test]
    fn enlarging_the_mask_lets_the_line_segment_detector_see_short_ticks() {
        let m = plot(&[60], &[], 6);
        let tick = |enlarge| {
            let lines = detect(&m, Algorithm::Lsd, &Params { enlarge, ..Params::default() });
            has(&lines, Orientation::Vertical, 5.0 + 61.0, 6.0)
        };
        assert!(!tick(1), "not enlarged: too few aligned pixels");
        assert!(tick(4), "enlarged four times");
        // the frame is found where it is whatever the enlargement
        for enlarge in 1..=4 {
            let lines = detect(&plot(&[], &[], 0), Algorithm::Lsd, &Params { enlarge, ..Params::default() });
            assert!(has(&lines, Orientation::Horizontal, 7.0 + 81.0, 100.0), "{enlarge}x: {lines:?}");
            assert!(has(&lines, Orientation::Vertical, 5.0 + 21.0, 62.0), "{enlarge}x: {lines:?}");
        }
    }

    #[test]
    fn the_shortest_and_the_widest_line_are_limits() {
        let m = plot(&[30, 60, 90], &[], 6);
        let ticks = |params: Params| {
            detect(&m, Algorithm::Hough, &params)
                .iter()
                .filter(|l| l.orientation == Orientation::Vertical && l.length() < 10.0)
                .count()
        };
        assert_eq!(ticks(Params::default()), 3);
        assert_eq!(
            ticks(Params { min_length: 10.0, ..Params::default() }),
            0,
            "ticks, with the frame they stand on, are 8 long"
        );
        // a frame 2 pixels thick is wider than a 1 pixel limit
        let frame = detect(&plot(&[], &[], 0), Algorithm::Hough, &Params { max_width: 1.0, ..Params::default() });
        assert!(frame.is_empty(), "{frame:?}");
    }

    #[test]
    fn a_horizontal_line_is_found_wherever_it_is_and_whatever_the_shortest_line() {
        // `imageproc` computes a distance in single precision, which at 90 degrees puts the votes of a horizontal line far
        // from the left edge into the bucket of the row above; the line is found because it is looked for as a vertical
        // line of the transposed mask
        let m = mask(|put| {
            for x in 120..180 {
                put(x, 60, 255);
            }
        });
        for shortest in [5.0, 30.0, 55.0] {
            let lines = detect(&m, Algorithm::Hough, &Params { min_length: shortest, ..Params::default() });
            assert!(has(&lines, Orientation::Horizontal, 7.0 + 60.5, 60.0), "{shortest}: {lines:?}");
        }
    }
}
