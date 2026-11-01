//! Step 5, curves: the series made of the shapes, proposed for each curve colour and changed by hand (pieces put in or
//! taken out, points taken out or put in), traced into points and written in plot values.

use std::collections::{BTreeMap, BTreeSet};

use kurbo::Point;
use plotextract_core::{Calibration, ExportError, Primitive, PrimitiveKind, Series};
use plotextract_raster::curves::{Pieces, Trace};
use plotextract_raster::shapes::{Kind, Shapes};
use plotextract_raster::{ColourMap, MaskMeta, Tag, Value};

/// A point taken out by hand takes out the traced point within this many pixels of it.
const NEAR_POINT: f64 = 1.5;

/// A series of data: pieces of the curve colours, and the points traced along them.
#[derive(Clone, Debug)]
pub struct CurveSeries {
    number: usize,
    /// A pixel of each of its pieces: the pieces it is made of are the ones that have these pixels, however often the
    /// pieces are found again.
    seeds: Vec<(u32, u32)>,
    trace: Trace,
    pub(super) shown: bool,
    /// The colour it was proposed for; `None` for one made by hand.
    ink: Option<usize>,
    /// Whether it was changed by hand: then it is never proposed afresh.
    by_hand: bool,
    /// Points put in by hand, and traced points taken out, in image pixels.
    added: Vec<Point>,
    removed: Vec<Point>,
    /// The pieces it is made of now, and the points traced along them (kept up to date by `CurveStep::retrace`).
    pieces: Vec<usize>,
    points: Vec<Point>,
}

impl CurveSeries {
    /// What it is known by while it exists, from 1: Series 1, 2, ...
    pub fn number(&self) -> usize {
        self.number
    }

    pub fn trace(&self) -> Trace {
        self.trace
    }

    /// The pieces it is made of.
    pub fn pieces(&self) -> &[usize] {
        &self.pieces
    }

    /// Its points, in image pixels: from left to right, or along the path for a pattern.
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// The points put in by hand.
    pub fn added(&self) -> &[Point] {
        &self.added
    }

    /// Where traced points were taken out by hand.
    pub fn removed(&self) -> &[Point] {
        &self.removed
    }

    /// Whether it is listed as a mask: it has pieces or points put in.
    pub fn exists(&self) -> bool {
        !self.pieces.is_empty() || !self.added.is_empty()
    }

    fn new(number: usize, ink: Option<usize>, trace: Trace, seeds: Vec<(u32, u32)>) -> Self {
        Self {
            number,
            seeds,
            trace,
            shown: true,
            ink,
            by_hand: ink.is_none(),
            added: Vec::new(),
            removed: Vec::new(),
            pieces: Vec::new(),
            points: Vec::new(),
        }
    }

    /// Bring its pieces and points up to date: the traced points, but those taken out by hand, and those put in.
    fn retrace(&mut self, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        self.pieces.clear();
        let mut points = Vec::new();
        if let Some(pieces) = pieces {
            self.pieces = self.seeds.iter().filter_map(|&(x, y)| pieces.at(x, y)).collect();
            self.pieces.sort_unstable();
            self.pieces.dedup();
            let order = match (self.trace, shapes) {
                (Trace::Pattern, Some(shapes)) => shapes.order(&self.pieces),
                _ => self.pieces.clone(),
            };
            points = pieces.trace(&order, self.trace);
        }
        let removed = &self.removed;
        points.retain(|p| !removed.iter().any(|r| (*r - *p).hypot() <= NEAR_POINT));
        for &p in &self.added {
            if self.trace == Trace::Pattern {
                insert_along(&mut points, p);
            } else {
                points.push(p);
            }
        }
        if self.trace != Trace::Pattern {
            points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
        }
        self.points = points;
    }
}

/// Put `p` among `points` (a path) where it lengthens the path least.
fn insert_along(points: &mut Vec<Point>, p: Point) {
    let cost = |k: usize| {
        let before = k.checked_sub(1).map(|k| points[k]);
        match (before, points.get(k)) {
            (Some(a), Some(&b)) => (p - a).hypot() + (b - p).hypot() - (b - a).hypot(),
            (Some(a), None) | (None, Some(&a)) => (p - a).hypot(),
            (None, None) => 0.0,
        }
    };
    let best = (0..=points.len()).min_by(|&a, &b| cost(a).total_cmp(&cost(b))).unwrap_or(0);
    points.insert(best, p);
}

pub struct CurveStep {
    /// Whether a click on the image puts points in and takes them out, rather than pieces.
    pub(super) edit_points: bool,
    series: Vec<CurveSeries>,
    /// The number the next series gets.
    next: usize,
    /// The colours series were proposed for, by their colour, with the shapes epoch they were proposed at: a colour's
    /// proposed series are proposed afresh when its shapes change, until one of them is changed by hand.
    proposed: BTreeMap<[u8; 3], u64>,
    /// Counts the changes to which series has which pieces, and which are shown, so that a picture of the pieces knows when
    /// it is out of date (points and tracing do not count: they are drawn on their own).
    epoch: u64,
}

impl Default for CurveStep {
    fn default() -> Self {
        Self { edit_points: false, series: Vec::new(), next: 1, proposed: BTreeMap::new(), epoch: 0 }
    }
}

impl CurveStep {
    /// The series, in the order they were made.
    pub fn series(&self) -> &[CurveSeries] {
        &self.series
    }

    pub fn find(&self, number: usize) -> Option<&CurveSeries> {
        self.series.iter().find(|s| s.number == number)
    }

    fn find_mut(&mut self, number: usize) -> Option<&mut CurveSeries> {
        self.series.iter_mut().find(|s| s.number == number)
    }

    /// The series piece `piece` is in, if any.
    pub fn series_of(&self, piece: usize) -> Option<usize> {
        self.series.iter().find(|s| s.pieces.contains(&piece)).map(|s| s.number)
    }

    /// Counts the changes to which series has which pieces, and which are shown.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    pub fn edit_points(&self) -> bool {
        self.edit_points
    }

    /// The series each curve colour's shapes make, proposed when its shapes are new or changed (`shapes_epoch`), unless
    /// one of the colour's series was changed by hand: a series for each curve of dots and dashes (the chains drawn with
    /// the same shapes), one for the solid lines, one for each kind of marker; text and other pieces are in none. Only the
    /// pieces `keep` accepts count (those inside the frame and not on its ticks).
    pub(super) fn propose(
        &mut self,
        pieces: &Pieces,
        shapes: &Shapes,
        map: &ColourMap,
        shapes_epoch: u64,
        keep: impl Fn(usize) -> bool,
    ) {
        let inks: BTreeSet<usize> = pieces.pieces().iter().map(|p| p.ink).collect();
        let kind = |p: usize| shapes.kind(p).unwrap_or(Kind::Other);
        let curves = shapes.curves();
        for ink in inks {
            let Some(rgb) = map.inks.get(ink).map(|i| i.rgb) else { continue };
            if self.proposed.get(&rgb) == Some(&shapes_epoch)
                || self.series.iter().any(|s| s.ink == Some(ink) && s.by_hand)
            {
                continue;
            }
            let mut made: Vec<(Trace, Vec<usize>)> = Vec::new();
            let mut on_curve = BTreeSet::new();
            for curve in &curves {
                let members: Vec<usize> = curve
                    .iter()
                    .copied()
                    .filter(|&p| pieces.pieces()[p].ink == ink && matches!(kind(p), Kind::Dot | Kind::Dash) && keep(p))
                    .collect();
                on_curve.extend(members.iter().copied());
                if !members.is_empty() {
                    made.push((Trace::Pattern, members));
                }
            }
            for t in shapes.types.iter().filter(|t| t.ink == ink) {
                let members: Vec<usize> =
                    t.members.iter().copied().filter(|&p| keep(p) && !on_curve.contains(&p)).collect();
                match t.kind {
                    _ if members.is_empty() => {}
                    // dots and dashes said by hand, on no chain
                    Kind::Dot | Kind::Dash => made.push((Trace::Pattern, members)),
                    Kind::Marker => made.push((Trace::Markers, members)),
                    Kind::Line => match made.iter_mut().find(|(trace, _)| *trace == Trace::Line) {
                        Some((_, all)) => all.extend(members),
                        None => made.push((Trace::Line, members)),
                    },
                    Kind::Text | Kind::Other => {}
                }
            }
            self.proposed.insert(rgb, shapes_epoch);
            self.series.retain(|s| s.ink != Some(ink));
            for (trace, members) in made {
                let seeds = members.iter().map(|&p| pieces.pieces()[p].seed).collect();
                self.series.push(CurveSeries::new(self.next, Some(ink), trace, seeds));
                self.next += 1;
            }
        }
    }

    /// Bring every series up to date with the pieces.
    pub(super) fn retrace(&mut self, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        for series in &mut self.series {
            series.retrace(pieces, shapes);
        }
        self.epoch += 1;
    }

    /// Change series `number` as `change` says, and bring it (only it) up to date. Whether there is such a series.
    fn change(
        &mut self,
        number: usize,
        pieces: Option<&Pieces>,
        shapes: Option<&Shapes>,
        change: impl FnOnce(&mut CurveSeries),
    ) -> bool {
        let Some(series) = self.find_mut(number) else { return false };
        change(series);
        series.retrace(pieces, shapes);
        true
    }

    /// Make a new series traced as `trace`; its number.
    pub(super) fn add(&mut self, trace: Trace) -> usize {
        let number = self.next;
        self.next += 1;
        self.series.push(CurveSeries::new(number, None, trace, Vec::new()));
        self.epoch += 1;
        number
    }

    /// Put piece `piece` in series `series` (taking it out of any other), or in none.
    pub(super) fn set_piece(&mut self, pieces: &Pieces, shapes: Option<&Shapes>, piece: usize, series: Option<usize>) {
        let Some(seed) = pieces.pieces().get(piece).map(|p| p.seed) else { return };
        for s in &mut self.series {
            let before = s.seeds.len();
            s.seeds.retain(|&(x, y)| pieces.at(x, y) != Some(piece));
            if s.seeds.len() != before {
                s.by_hand = true;
                s.retrace(Some(pieces), shapes);
            }
        }
        if let Some(n) = series {
            self.change(n, Some(pieces), shapes, |s| {
                s.seeds.push(seed);
                s.by_hand = true;
            });
        }
        self.epoch += 1;
    }

    pub(super) fn set_trace(&mut self, number: usize, trace: Trace, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        self.change(number, pieces, shapes, |s| {
            s.trace = trace;
            s.by_hand = true;
        });
    }

    pub(super) fn add_point(&mut self, number: usize, p: Point, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        self.change(number, pieces, shapes, |s| {
            s.added.push(p);
            s.by_hand = true;
        });
    }

    /// Take a point out: a point put in by hand goes, a traced one is left out.
    pub(super) fn remove_point(&mut self, number: usize, p: Point, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        self.change(number, pieces, shapes, |s| {
            match s.added.iter().position(|a| (*a - p).hypot() <= NEAR_POINT) {
                Some(k) => {
                    s.added.remove(k);
                }
                None => s.removed.push(p),
            }
            s.by_hand = true;
        });
    }

    /// Put back a traced point that was taken out.
    pub(super) fn restore_point(&mut self, number: usize, p: Point, pieces: Option<&Pieces>, shapes: Option<&Shapes>) {
        self.change(number, pieces, shapes, |s| s.removed.retain(|r| (*r - p).hypot() > NEAR_POINT));
    }

    pub(super) fn delete(&mut self, number: usize) {
        self.series.retain(|s| s.number != number);
        self.epoch += 1;
    }

    pub(super) fn show(&mut self, number: usize, on: bool) {
        if let Some(s) = self.find_mut(number) {
            s.shown = on;
            self.epoch += 1;
        }
    }

    /// Series `number` as the figures' series of one curve, so that it is written as theirs are. A pattern is written in
    /// the order of its path: a curve that doubles back is not one value of x.
    fn as_series(&self, number: usize) -> Result<(Primitive, Series), ExportError> {
        let found = self.find(number).ok_or(ExportError::NoSuchSeries)?;
        let line = Primitive::new(PrimitiveKind::Stroke, 0, found.points.clone(), Vec::new());
        let sort_by_x = found.trace != Trace::Pattern;
        Ok((line, Series { members: vec![0], sort_by_x, ..Series::new(format!("Series {number}")) }))
    }

    /// Series `number` in plot values, with `calibration`.
    pub fn rows(&self, number: usize, calibration: &Calibration) -> Result<Vec<[f64; 2]>, ExportError> {
        let (line, series) = self.as_series(number)?;
        series.rows(&[line], calibration)
    }

    /// Series `number` as CSV: an `x,y` header and a line for each point.
    pub fn csv(&self, number: usize, calibration: &Calibration) -> Result<String, ExportError> {
        let (line, series) = self.as_series(number)?;
        series.to_csv(&[line], calibration)
    }

    /// What series `number` is: how it is traced, its colours, pieces and points, and its plot ranges once calibrated.
    pub fn meta(
        &self,
        number: usize,
        pieces: Option<&Pieces>,
        map: Option<&ColourMap>,
        calibration: &Calibration,
        meta: MaskMeta,
    ) -> MaskMeta {
        let meta = meta.tags([Tag::Curve, Tag::Series]);
        let Some(series) = self.find(number) else { return meta };
        let how = match series.trace {
            Trace::Line => Tag::Line,
            Trace::Markers => Tag::Markers,
            Trace::Pattern => Tag::Pattern,
        };
        let mut meta = meta.tag(how);
        // the colours it is drawn in
        if let (Some(pieces), Some(map)) = (pieces, map) {
            let inks: BTreeSet<usize> =
                series.pieces.iter().filter_map(|&p| pieces.pieces().get(p)).map(|p| p.ink).collect();
            for (k, ink) in inks.into_iter().enumerate() {
                let key = if k == 0 { "colour".to_string() } else { format!("colour {}", k + 1) };
                if let Some(ink) = map.inks.get(ink) {
                    meta = meta.with(key, Value::Colour(ink.rgb));
                }
            }
        }
        meta = meta
            .with("pieces", Value::Count(series.pieces.len() as u64))
            .with("points", Value::Count(series.points.len() as u64));
        if !series.added.is_empty() {
            meta = meta.with("points put in", Value::Count(series.added.len() as u64));
        }
        if !series.removed.is_empty() {
            meta = meta.with("points taken out", Value::Count(series.removed.len() as u64));
        }
        match self.rows(number, calibration) {
            Ok(rows) => {
                let range = |axis: usize| {
                    rows.iter()
                        .map(|r| r[axis])
                        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)))
                };
                let ((x0, x1), (y0, y1)) = (range(0), range(1));
                meta.tag(Tag::Calibrated)
                    .with("x from", Value::Number(x0))
                    .with("x to", Value::Number(x1))
                    .with("y from", Value::Number(y0))
                    .with("y to", Value::Number(y1))
            }
            Err(_) => meta,
        }
    }
}
