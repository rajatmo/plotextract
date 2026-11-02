//! Finding a plot's four ticks with two small networks (trained in the plotML project), but for the networks
//! themselves: what goes into them and what is made of what comes out.
//!
//! 1. The plot, as grey levels, is fitted into the tick finder's square input ([`fit`]).
//! 2. The finder's maps give every tick where it meets its axis, and every label's box ([`candidates`]); each tick takes
//!    the label nearest it on its side of the axis.
//! 3. The reader reads each label's crop ([`label_crop`], [`decode`]) and the text is a value ([`value_of`]).
//! 4. On each axis the values must lie on a line against position, or their logarithms must ([`calibrate`]): the line
//!    through the most ticks decides the scale and drops misreadings, a least-squares fit through the ticks on it places
//!    the two outermost, and those are the axis' min and max.
//!
//! The steps are those of plotML's `pipeline.py`, and give what it gives: a test holds this port to its results.

use image::{GrayImage, RgbaImage, imageops::FilterType};
use plotextract_domain::{Axis, PixelRect};

/// The side of the tick finder's square input, in pixels.
pub const SIDE: usize = 512;
/// Input pixels to a cell of the tick finder's maps.
pub const STRIDE: usize = 4;
/// The side of the maps, in cells.
pub const CELLS: usize = SIDE / STRIDE;
/// The maps: x ticks, y ticks, x labels, y labels (heat), the offset of each within its cell (two), a label's size (two).
pub const CHANNELS: usize = 8;
const X_TICK: usize = 0;
const Y_TICK: usize = 1;
const X_LABEL: usize = 2;
const Y_LABEL: usize = 3;
const OFFSET: usize = 4;
const SIZE: usize = 6;

/// A label crop's height and width, in pixels.
pub const LABEL_HEIGHT: usize = 32;
pub const LABEL_WIDTH: usize = 192;
/// The characters the reader knows; its class 0 is CTC's blank, class `k` is `CHARS[k - 1]`.
pub const CHARS: &[u8] = b"0123456789-.^";
pub const CLASSES: usize = CHARS.len() + 1;
/// The reader's steps along a crop.
pub const STEPS: usize = LABEL_WIDTH / 4;

/// The least heat a tick or a label is taken at.
const PEAK: f32 = 0.3;
/// A label's box is made this share wider each side than the finder says: a label cut short (the 10 of 10^-24) is read
/// wrong, a little more paper is not.
const WIDEN: f64 = 0.1;
/// Pixels of paper kept around a label's box when it is cropped.
const PAD: f64 = 3.0;

/// The part `crop` of an image as grey levels (ink dark, paper light), weighted as ITU-R 601 does.
pub fn grey(image: &RgbaImage, crop: PixelRect) -> GrayImage {
    GrayImage::from_fn(crop.width, crop.height, |x, y| {
        let [r, g, b, _] = image.get_pixel(crop.x + x, crop.y + y).0;
        let l = (u32::from(r) * 19595 + u32::from(g) * 38470 + u32::from(b) * 7471 + 0x8000) >> 16;
        image::Luma([l as u8])
    })
}

/// The plot scaled so its longer side is [`SIDE`] and padded with paper to the square the finder takes (row by row, 0
/// ink to 1 paper), and the scale across and down. The two scales differ a little, since the shorter side is rounded to
/// whole pixels.
pub fn fit(grey: &GrayImage) -> (Vec<f32>, (f64, f64)) {
    let (w, h) = (f64::from(grey.width().max(1)), f64::from(grey.height().max(1)));
    let scale = SIDE as f64 / w.max(h);
    let nw = ((w * scale).round_ties_even() as u32).clamp(1, SIDE as u32);
    let nh = ((h * scale).round_ties_even() as u32).clamp(1, SIDE as u32);
    let small = image::imageops::resize(grey, nw, nh, FilterType::Triangle);
    let mut square = vec![1.0f32; SIDE * SIDE];
    for (x, y, p) in small.enumerate_pixels() {
        square[y as usize * SIDE + x as usize] = f32::from(p.0[0]) / 255.0;
    }
    (square, (f64::from(nw) / w, f64::from(nh) / h))
}

/// A tick the finder found, in the grey image's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// Where it meets its axis.
    pub x: f64,
    pub y: f64,
    /// How sure the finder is, from 0 to 1.
    pub score: f32,
    /// The box of its label (x0, y0, x1, y1), if it has one.
    pub label: Option<[f64; 4]>,
}

/// The maps' peaks in one channel: cells whose heat is at least [`PEAK`] and the most in their 3 x 3 neighbourhood, in
/// row order, as (x, y) in input pixels, the heat, and the cell.
fn peaks(maps: &[f32], channel: usize) -> Vec<(f64, f64, f32, usize)> {
    let at = |c: usize, i: usize| maps[c * CELLS * CELLS + i];
    let heat = |i: usize| 1.0 / (1.0 + (-at(channel, i)).exp());
    let mut out = Vec::new();
    for y in 0..CELLS {
        for x in 0..CELLS {
            let i = y * CELLS + x;
            let h = heat(i);
            if h <= PEAK {
                continue;
            }
            let most = (y.saturating_sub(1)..=(y + 1).min(CELLS - 1))
                .flat_map(|v| (x.saturating_sub(1)..=(x + 1).min(CELLS - 1)).map(move |u| v * CELLS + u))
                .all(|j| heat(j) <= h);
            if most {
                let px = f64::from(x as f32 + at(OFFSET, i)) * STRIDE as f64;
                let py = f64::from(y as f32 + at(OFFSET + 1, i)) * STRIDE as f64;
                out.push((px, py, h, i));
            }
        }
    }
    out
}

/// The ticks of one axis the finder's `maps` show (channel by channel, [`CELLS`] x [`CELLS`] each), in the grey image's
/// pixels; `scale` is the one [`fit`] gave. Each takes the label nearest it: below an x tick, left of a y tick.
pub fn candidates(maps: &[f32], scale: (f64, f64), axis: Axis) -> Vec<Candidate> {
    assert_eq!(maps.len(), CHANNELS * CELLS * CELLS, "the tick finder's maps");
    let (sx, sy) = scale;
    let (tick, label) = match axis {
        Axis::X => (X_TICK, X_LABEL),
        Axis::Y => (Y_TICK, Y_LABEL),
    };
    let size = |c: usize, i: usize| f64::from(maps[c * CELLS * CELLS + i]);
    let labels: Vec<(f64, f64, [f64; 4])> = peaks(maps, label)
        .into_iter()
        .map(|(lx, ly, _, i)| {
            let w = size(SIZE, i) * SIDE as f64 / sx * (1.0 + 2.0 * WIDEN);
            let h = size(SIZE + 1, i) * SIDE as f64 / sy;
            let (cx, cy) = (lx / sx, ly / sy);
            (cx, cy, [cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0])
        })
        .collect();
    peaks(maps, tick)
        .into_iter()
        .map(|(tx, ty, score, _)| {
            let (x, y) = (tx / sx, ty / sy);
            // the first of the nearest, as the pipeline takes it
            let mut best: Option<(f64, [f64; 4])> = None;
            for &(cx, cy, b) in &labels {
                let d = match axis {
                    Axis::X if cy > y => (cx - x).abs() + 0.2 * (cy - y).abs(),
                    Axis::Y if cx < x => (cy - y).abs() + 0.2 * (cx - x).abs(),
                    _ => continue,
                };
                if best.is_none_or(|(e, _)| d < e) {
                    best = Some((d, b));
                }
            }
            Candidate { x, y, score, label: best.map(|(_, b)| b) }
        })
        .collect()
}

/// A label's crop for the reader ([`LABEL_HEIGHT`] x [`LABEL_WIDTH`], row by row, 0 ink to 1 paper): its box with a
/// little paper round it, scaled to the height and padded with paper to the width.
pub fn label_crop(grey: &GrayImage, label: [f64; 4]) -> Vec<f32> {
    let mut out = vec![1.0f32; LABEL_HEIGHT * LABEL_WIDTH];
    let (w, h) = (f64::from(grey.width()), f64::from(grey.height()));
    let x0 = (label[0] - PAD).floor().max(0.0);
    let y0 = (label[1] - PAD).floor().max(0.0);
    let x1 = (label[2] + PAD).ceil().min(w);
    let y1 = (label[3] + PAD).ceil().min(h);
    if x1 - x0 < 2.0 || y1 - y0 < 2.0 {
        return out;
    }
    let patch = image::imageops::crop_imm(grey, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image();
    let scale = LABEL_HEIGHT as f64 / (y1 - y0);
    let width = (((x1 - x0) * scale).round_ties_even() as usize).clamp(1, LABEL_WIDTH);
    let scaled = image::imageops::resize(&patch, width as u32, LABEL_HEIGHT as u32, FilterType::Triangle);
    for (x, y, p) in scaled.enumerate_pixels() {
        out[y as usize * LABEL_WIDTH + x as usize] = f32::from(p.0[0]) / 255.0;
    }
    out
}

/// The text of one crop from the reader's scores ([`STEPS`] x [`CLASSES`]): the likeliest class at each step, repeats
/// merged, blanks dropped.
pub fn decode(scores: &[f32]) -> String {
    let mut out = String::new();
    let mut last = 0;
    for step in scores.as_chunks::<CLASSES>().0 {
        // the first of the likeliest, as argmax takes it
        let k = step.iter().enumerate().fold(0, |best, (k, &s)| if s > step[best] { k } else { best });
        if k != last && k != 0 {
            out.push(char::from(CHARS[k - 1]));
        }
        last = k;
    }
    out
}

/// The number a label reads as (`-0.25`, `10^-24`), or `None`.
pub fn value_of(text: &str) -> Option<f64> {
    let number = |s: &str| s.parse::<f64>().ok();
    let value = match text.split_once('^') {
        Some((base, exponent)) => number(base)?.powf(number(exponent)?),
        None => number(text)?,
    };
    value.is_finite().then_some(value)
}

/// The mark of a tick at (`x`, `y`) on `axis`: the pixels across the axis line there that stand out from it, as a box
/// (x0, y0, x1, y1) of whole pixels. For an x tick it is the column of most ink within 2 pixels and the rows of ink in
/// it within 12; a y tick's the other way round.
pub fn mark(grey: &GrayImage, x: f64, y: f64, axis: Axis) -> [u32; 4] {
    const SNAP: i64 = 2;
    const REACH: i64 = 12;
    let (w, h) = (i64::from(grey.width()), i64::from(grey.height()));
    let ink = |u: i64, v: i64| 1.0 - f64::from(grey.get_pixel(u as u32, v as u32).0[0]) / 255.0;
    let (ix, iy) = (x as i64, y as i64);
    let clamp = |v: i64, hi: i64| v.clamp(0, hi) as u32;
    // along: where the mark is looked for across its length; across: the reach either side of the axis line
    let (along, across) = match axis {
        Axis::X => ((ix - SNAP).max(0)..(ix + SNAP + 1).min(w), (iy - REACH).max(0)..(iy + REACH + 1).min(h)),
        Axis::Y => ((iy - SNAP).max(0)..(iy + SNAP + 1).min(h), (ix - REACH).max(0)..(ix + REACH + 1).min(w)),
    };
    let at = |a: i64, c: i64| match axis {
        Axis::X => ink(a, c),
        Axis::Y => ink(c, a),
    };
    let stub = match axis {
        Axis::X => [clamp(ix, w), clamp(iy - 2, h), clamp(ix + 1, w), clamp(iy + 3, h)],
        Axis::Y => [clamp(ix - 2, w), clamp(iy, h), clamp(ix + 3, w), clamp(iy + 1, h)],
    };
    if along.is_empty() || across.is_empty() {
        return stub;
    }
    // the axis line is ink in every column (or row): what stands out is the mark's
    let sums: Vec<f64> = along.clone().map(|a| across.clone().map(|c| at(a, c)).sum()).collect();
    let median = median(&sums);
    let (k, most) =
        sums.iter().enumerate().fold((0, f64::MIN), |b, (k, &s)| if s - median > b.1 { (k, s - median) } else { b });
    if most <= 0.5 {
        return stub;
    }
    let a = along.start + k as i64;
    let inked: Vec<i64> = across.clone().filter(|&c| at(a, c) > 0.4).collect();
    let (lo, hi) = match (inked.first(), inked.last()) {
        (Some(&lo), Some(&hi)) => (lo, hi + 1),
        _ => (across.start, across.start + 1),
    };
    match axis {
        Axis::X => [a as u32, lo as u32, a as u32 + 1, hi as u32],
        Axis::Y => [lo as u32, a as u32, hi as u32, a as u32 + 1],
    }
}

fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// One end of a calibrated axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct End {
    /// Which of the points it is.
    pub index: usize,
    /// Where the fitted line puts its value, in the points' positions.
    pub position: f64,
    /// Its value, as read.
    pub value: f64,
}

/// An axis calibrated from its ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisFit {
    pub log: bool,
    /// The ends: the smaller value first.
    pub min: End,
    pub max: End,
    /// How many of the ticks agree, and of how many.
    pub fit: usize,
    pub of: usize,
}

/// A point on the line being fitted: its position, its value (or its logarithm), and which point it is.
type Pt = (f64, f64, usize);

/// The line through the most points, within 2% of the values' range: the points on it, and their mean distance from it
/// as a share of that range (so a fit in values and one in logarithms compare).
fn best_line(pts: &[Pt]) -> Option<(Vec<Pt>, f64)> {
    let lo = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let hi = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let span = if hi - lo == 0.0 { 1.0 } else { hi - lo };
    let mut best: Option<(Vec<Pt>, f64)> = None;
    for (i, &(p1, v1, _)) in pts.iter().enumerate() {
        for &(p2, v2, _) in &pts[i + 1..] {
            if (p2 - p1).abs() < 1e-6 || v1 == v2 {
                continue;
            }
            let b = (v2 - v1) / (p2 - p1);
            let off = |&(p, v, _): &Pt| (v1 + b * (p - p1) - v).abs();
            let inliers: Vec<Pt> = pts.iter().copied().filter(|q| off(q) <= 0.02 * span).collect();
            let error = inliers.iter().map(off).sum::<f64>() / inliers.len() as f64 / span;
            let better = match &best {
                None => true,
                Some((b_in, b_err)) => inliers.len() > b_in.len() || (inliers.len() == b_in.len() && error < *b_err),
            };
            if better {
                best = Some((inliers, error));
            }
        }
    }
    best
}

/// An axis from its ticks' positions and values (`None` for a label not read as a number). Over a narrow range values
/// and their logarithms both lie nearly on a line, so the axis is linear unless the log fit takes in more ticks, or fits
/// clearly better, or its labels are mostly powers (`powers`).
pub fn calibrate(points: &[(f64, Option<f64>)], powers: bool) -> Option<AxisFit> {
    let fit_with = |log: bool| {
        let pts: Vec<Pt> = points
            .iter()
            .enumerate()
            .filter_map(|(i, &(p, v))| {
                let v = v?;
                if log { (v > 0.0).then(|| (p, v.log10(), i)) } else { Some((p, v, i)) }
            })
            .collect();
        if pts.len() >= 2 { best_line(&pts) } else { None }
    };
    let (lin, log) = (fit_with(false), fit_with(true));
    let is_log = match (&lin, &log) {
        (None, None) => return None,
        (_, None) => false,
        (None, Some(_)) => true,
        (Some((lin_in, lin_err)), Some((log_in, log_err))) => {
            powers || log_in.len() > lin_in.len() || (log_in.len() == lin_in.len() && *log_err < 0.3 * lin_err)
        }
    };
    let (inliers, _) = if is_log { log? } else { lin? };
    if inliers.len() < 2 {
        return None;
    }
    // the line through every tick that agrees, by least squares (position on value): one tick a pixel or two off moves
    // it little. The two outermost ticks are placed where the line puts their values.
    let n = inliers.len() as f64;
    let (mv, mp) = (inliers.iter().map(|q| q.1).sum::<f64>() / n, inliers.iter().map(|q| q.0).sum::<f64>() / n);
    let svv: f64 = inliers.iter().map(|q| (q.1 - mv).powi(2)).sum();
    let svp: f64 = inliers.iter().map(|q| (q.1 - mv) * (q.0 - mp)).sum();
    let slope = svp / svv;
    let intercept = mp - slope * mv;
    let order = |a: &Pt, b: &Pt| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1));
    let end = |q: Pt| End { index: q.2, position: slope * q.1 + intercept, value: points[q.2].1.unwrap_or(f64::NAN) };
    let a = end(inliers.iter().copied().min_by(order)?);
    let b = end(inliers.iter().copied().max_by(order)?);
    let (min, max) = if a.value <= b.value { (a, b) } else { (b, a) };
    Some(AxisFit { log: is_log, min, max, fit: inliers.len(), of: points.len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_as_numbers_and_powers() {
        assert_eq!(value_of("-0.25"), Some(-0.25));
        assert_eq!(value_of("10^-24"), Some(1e-24));
        assert_eq!(value_of("2^3"), Some(8.0));
        assert_eq!(value_of(".5"), Some(0.5));
        for text in ["", "-", "1.2.3", "--1", "10^", "^3", "0^-1", "10^999", "-8^0.5"] {
            assert_eq!(value_of(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_reader_merges_repeats_and_drops_blanks() {
        let mut scores = vec![0.0f32; STEPS * CLASSES];
        // "1", "1", blank, "1", "-", "-", "^"
        for (step, class) in [(0, 2), (1, 2), (2, 0), (3, 2), (4, 11), (5, 11), (6, 13)] {
            scores[step * CLASSES + class] = 5.0;
        }
        assert_eq!(decode(&scores), "11-^");
    }

    #[test]
    fn a_misreading_is_dropped_and_the_ends_are_fitted() {
        // x ticks every 100 px from 0 to 4, the third read as 7
        let points =
            [(10.0, Some(0.0)), (110.0, Some(1.0)), (210.0, Some(7.0)), (310.0, Some(3.0)), (410.0, Some(4.0))];
        let fit = calibrate(&points, false).unwrap();
        assert!(!fit.log);
        assert_eq!((fit.fit, fit.of), (4, 5));
        assert_eq!((fit.min.index, fit.max.index), (0, 4));
        assert!((fit.min.position - 10.0).abs() < 1e-9 && (fit.max.position - 410.0).abs() < 1e-9);
        assert_eq!((fit.min.value, fit.max.value), (0.0, 4.0));
    }

    #[test]
    fn decades_evenly_spaced_are_a_log_axis_and_y_runs_down() {
        let points = [(300.0, Some(1e-3)), (200.0, Some(1e-2)), (100.0, Some(1e-1)), (0.0, Some(1.0))];
        let fit = calibrate(&points, false).unwrap();
        assert!(fit.log);
        assert_eq!((fit.min.value, fit.max.value), (1e-3, 1.0));
        assert!((fit.min.position - 300.0).abs() < 1e-9 && fit.max.position.abs() < 1e-9);
    }

    #[test]
    fn an_axis_needs_two_numbers() {
        assert_eq!(calibrate(&[(0.0, Some(1.0)), (10.0, None)], false), None);
        assert_eq!(calibrate(&[], false), None);
    }

    #[test]
    fn a_wide_plot_fits_across_the_square() {
        let grey = GrayImage::from_pixel(1024, 300, image::Luma([0]));
        let (square, (sx, sy)) = fit(&grey);
        assert_eq!(square.len(), SIDE * SIDE);
        assert_eq!(sx, 0.5);
        assert!((sy - 150.0 / 300.0).abs() < 1e-12);
        assert_eq!(square[149 * SIDE + 511], 0.0, "ink inside");
        assert_eq!(square[150 * SIDE], 1.0, "paper below");
    }

    #[test]
    fn a_tick_mark_is_the_ink_that_stands_out_from_the_axis_line() {
        // an axis line along y = 50, and a tick 6 pixels down from it at x = 40
        let mut grey = GrayImage::from_pixel(100, 100, image::Luma([255]));
        for x in 0..100 {
            grey.put_pixel(x, 50, image::Luma([0]));
        }
        for y in 50..56 {
            grey.put_pixel(40, y, image::Luma([0]));
        }
        assert_eq!(mark(&grey, 40.6, 50.5, Axis::X), [40, 50, 41, 56]);
        // nothing stands out: a stub where the finder said
        assert_eq!(mark(&grey, 70.5, 50.5, Axis::X), [70, 48, 71, 53]);
    }
}
