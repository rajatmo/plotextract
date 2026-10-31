//! Accuracy on real plots: matplotlib and Mathematica figures (see the corpus crate's `lib.rs` for where they live), each as PDF, SVG and EPS, are opened,
//! calibrated from their tick marks and have their series extracted through the same `Session` calls the app uses.
//! The recovered numbers are compared with the data that drew the plot.
//!
//! matplotlib figures state where every tick landed on the page (in points), which is independent of the importers; an
//! SVG's units are CSS pixels, so those positions are scaled by the file's size relative to the points. Mathematica
//! figures state the tick values; their positions are found from the tick marks and the frame lines in the file.

use kurbo::Point;
use plotextract_core::{Axis, Edit, Figure, Pick, PrimitiveKind, Session, Slot};
use plotextract_corpus::fixture;
use plotextract_workspace::Registry;
use serde::Deserialize;

// --- ground truth files --------------------------------------------------------------------------

#[derive(Deserialize)]
struct SeriesTruth {
    name: String,
    color: [u8; 3],
    kind: String,
    x: Vec<f64>,
    y: Vec<f64>,
}

#[derive(Deserialize)]
struct Frame {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}

#[derive(Deserialize)]
struct Tick {
    value: f64,
    pos: f64,
}

#[derive(Deserialize)]
struct MplAxis {
    scale: String,
    ticks: Vec<Tick>,
    spine: f64,
}

#[derive(Deserialize)]
struct MplAxes {
    #[serde(rename = "box")]
    frame: Frame,
    x: MplAxis,
    y: MplAxis,
    series: Vec<SeriesTruth>,
}

#[derive(Deserialize)]
struct MplFigure {
    width: f64,
    axes: Vec<MplAxes>,
}

#[derive(Deserialize)]
struct MmaAxes {
    xticks: Vec<f64>,
    yticks: Vec<f64>,
    xscale: String,
    yscale: String,
    series: Vec<SeriesTruth>,
}

#[derive(Deserialize)]
struct MmaFigure {
    axes: Vec<MmaAxes>,
}

/// One axes, ready to calibrate and check: tick (value, page position) pairs from the lowest value up, where the axis
/// lines run, and the plot area.
struct AxisSetup {
    label: String,
    frame: Frame,
    x_log: bool,
    y_log: bool,
    x_ticks: Vec<(f64, f64)>,
    y_ticks: Vec<(f64, f64)>,
    /// Page y of the horizontal axis line, and page x of the vertical one.
    x_spine: f64,
    y_spine: f64,
    series: Vec<SeriesTruth>,
}

/// The figure in a generated file, or `None` when the test data is not available.
fn open(rel: &str) -> Option<Figure> {
    let bytes = fixture(rel)?;
    let extension = rel.rsplit('.').next();
    let importer =
        Registry::standard().detect(&bytes, extension).unwrap_or_else(|| panic!("{rel}: no importer recognizes it"));
    Some(plotextract_corpus::import(&*importer, &bytes).unwrap_or_else(|e| panic!("{rel}: {e}")))
}

fn json<T: for<'de> Deserialize<'de>>(rel: &str) -> T {
    let bytes = fixture(rel).unwrap_or_else(|| panic!("{rel}: missing"));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// The ground truth of a matplotlib figure, positions converted to the units of the file that was opened.
fn matplotlib_setups(name: &str, ext: &str, figure: &Figure) -> Vec<AxisSetup> {
    let truth: MplFigure = json(&format!("matplotlib/{name}.{ext}.json"));
    let u = figure.width / truth.width; // file units per point
    assert!(
        (u - 1.0).abs() < 1e-3 || (u - 4.0 / 3.0).abs() < 1e-3,
        "{name}: unexpected page width {} for {} pt",
        figure.width,
        truth.width
    );
    truth
        .axes
        .into_iter()
        .enumerate()
        .map(|(i, a)| AxisSetup {
            label: format!("{name} axes {i}"),
            frame: Frame {
                left: a.frame.left * u,
                right: a.frame.right * u,
                top: a.frame.top * u,
                bottom: a.frame.bottom * u,
            },
            x_log: a.x.scale == "log",
            y_log: a.y.scale == "log",
            x_ticks: a.x.ticks.iter().map(|t| (t.value, t.pos * u)).collect(),
            y_ticks: a.y.ticks.iter().map(|t| (t.value, t.pos * u)).collect(),
            x_spine: a.x.spine * u,
            y_spine: a.y.spine * u,
            series: a.series,
        })
        .collect()
}

/// Find the frame and tick marks of a Mathematica plot: the frame is four long axis-aligned lines, the ticks short
/// ones that start on the left and bottom frame lines.
fn mathematica_setup(name: &str, figure: &Figure) -> AxisSetup {
    let truth: MmaFigure = json(&format!("mathematica/{name}.json"));
    let a = truth.axes.into_iter().next().expect("one axes");
    let segments: Vec<(Point, Point)> = figure
        .primitives
        .iter()
        .filter(|p| p.kind == PrimitiveKind::Stroke)
        .flat_map(|p| p.pieces().filter(|piece| piece.len() == 2).map(|piece| (piece[0], piece[1])).collect::<Vec<_>>())
        .collect();
    let horizontal: Vec<_> = segments.iter().filter(|(p, q)| (p.y - q.y).abs() < 1e-6).collect();
    let vertical: Vec<_> = segments.iter().filter(|(p, q)| (p.x - q.x).abs() < 1e-6).collect();
    let len = |s: &&(Point, Point)| (s.1 - s.0).hypot();
    let longest = |v: &[&(Point, Point)]| v.iter().map(len).fold(0.0, f64::max);
    let (lh, lv) = (longest(&horizontal), longest(&vertical));
    let long_h: Vec<_> = horizontal.iter().filter(|s| len(s) >= 0.9 * lh).collect();
    let long_v: Vec<_> = vertical.iter().filter(|s| len(s) >= 0.9 * lv).collect();
    let top = long_h.iter().map(|s| s.0.y).fold(f64::INFINITY, f64::min);
    let bottom = long_h.iter().map(|s| s.0.y).fold(f64::NEG_INFINITY, f64::max);
    let left = long_v.iter().map(|s| s.0.x).fold(f64::INFINITY, f64::min);
    let right = long_v.iter().map(|s| s.0.x).fold(f64::NEG_INFINITY, f64::max);

    let touches = |s: &&&(Point, Point), on_y: Option<f64>, on_x: Option<f64>| {
        [s.0, s.1]
            .iter()
            .any(|p| on_y.is_some_and(|y| (p.y - y).abs() < 0.3) || on_x.is_some_and(|x| (p.x - x).abs() < 0.3))
    };
    let mut x_pos: Vec<f64> =
        vertical.iter().filter(|s| len(s) <= 8.0 && touches(s, Some(bottom), None)).map(|s| s.0.x).collect();
    let mut y_pos: Vec<f64> =
        horizontal.iter().filter(|s| len(s) <= 8.0 && touches(s, None, Some(left))).map(|s| s.0.y).collect();
    x_pos.sort_by(f64::total_cmp);
    x_pos.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    y_pos.sort_by(|a, b| b.total_cmp(a)); // page y grows downward: the lowest value is the largest y
    y_pos.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    assert_eq!(x_pos.len(), a.xticks.len(), "{name}: x tick marks found");
    assert_eq!(y_pos.len(), a.yticks.len(), "{name}: y tick marks found");

    AxisSetup {
        label: name.to_string(),
        frame: Frame { left, right, top, bottom },
        x_log: a.xscale == "log",
        y_log: a.yscale == "log",
        x_ticks: a.xticks.into_iter().zip(x_pos).collect(),
        y_ticks: a.yticks.into_iter().zip(y_pos).collect(),
        x_spine: bottom,
        y_spine: left,
        series: a.series,
    }
}

// --- the user's workflow, through the session ------------------------------------------------------

/// Record the tick at page position `at` (found by snapping, as the user would) into `slot` with its value.
fn record_tick(s: &mut Session, label: &str, near: Point, along_x: bool, slot: Slot, value: f64) {
    let hit =
        s.snap_vertex(near, 4.0).unwrap_or_else(|| panic!("{label}: no vertex near the {slot:?} tick at {near:?}"));
    let (got, want) = if along_x { (hit.pos.x, near.x) } else { (hit.pos.y, near.y) };
    assert!((got - want).abs() < 0.15, "{label}: {slot:?} tick snapped {got} but sits at {want}");
    s.select(Some(Pick { primitive: hit.primitive, vertex: Some(hit.vertex) })).unwrap();
    s.record_selected(slot).unwrap();
    s.edit(Edit::SetValue(slot, Some(value))).unwrap();
}

fn calibrate(s: &mut Session, setup: &AxisSetup) {
    s.edit(Edit::ClearCalibration).unwrap();
    s.edit(Edit::SetLog(Axis::X, setup.x_log)).unwrap();
    s.edit(Edit::SetLog(Axis::Y, setup.y_log)).unwrap();
    let l = &setup.label;
    let (x0, x1) = (setup.x_ticks[0], *setup.x_ticks.last().unwrap());
    let (y0, y1) = (setup.y_ticks[0], *setup.y_ticks.last().unwrap());
    record_tick(s, l, Point::new(x0.1, setup.x_spine), true, Slot::XMin, x0.0);
    record_tick(s, l, Point::new(x1.1, setup.x_spine), true, Slot::XMax, x1.0);
    record_tick(s, l, Point::new(setup.y_spine, y0.1), false, Slot::YMin, y0.0);
    record_tick(s, l, Point::new(setup.y_spine, y1.1), false, Slot::YMax, y1.0);
    assert!(s.calibration().is_calibrated(), "{l}: both axes calibrated");
}

fn color_matches(style: Option<[u8; 4]>, want: [u8; 3]) -> bool {
    style.is_some_and(|c| (0..3).all(|i| c[i].abs_diff(want[i]) <= 2))
}

fn inside(e: kurbo::Rect, f: &Frame, slack: f64) -> bool {
    e.x0 >= f.left - slack && e.x1 <= f.right + slack && e.y0 >= f.top - slack && e.y1 <= f.bottom + slack
}

/// The primitives that draw a series: long strokes of its colour, or small closed shapes of its colour (one per
/// marker; a marker drawn both filled and stroked counts once).
fn members(s: &Session, setup: &AxisSetup, t: &SeriesTruth) -> Vec<usize> {
    let f = &setup.frame;
    let width = f.right - f.left;
    let mut out: Vec<usize> = Vec::new();
    let mut centers: Vec<Point> = Vec::new();
    for (i, p) in s.figure.primitives.iter().enumerate() {
        let Some(e) = p.extent() else { continue };
        if !color_matches(p.style.color, t.color) || !inside(kurbo::Rect::from_points(e.center(), e.center()), f, 1.0) {
            continue;
        }
        match t.kind.as_str() {
            "line" if p.kind == PrimitiveKind::Stroke && p.points.len() >= 3 && e.width() >= 0.3 * width => out.push(i),
            "markers"
                if p.points.len() >= 4 && e.width().max(e.height()) <= 12.0 && e.width() > 0.5 && e.height() > 0.5 =>
            {
                let c = e.center();
                if !centers.iter().any(|q| (*q - c).hypot() < 0.05) {
                    centers.push(c);
                    out.push(i);
                }
            }
            _ => {}
        }
    }
    out
}

fn tx(v: f64, log: bool) -> f64 {
    if log { v.log10() } else { v }
}

fn span(v: &[f64], log: bool) -> f64 {
    let t: Vec<f64> = v.iter().map(|&x| tx(x, log)).collect();
    t.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - t.iter().cloned().fold(f64::INFINITY, f64::min)
}

/// Linear interpolation of the truth curve at `x`, in the (possibly log) plot space.
fn truth_at(t: &SeriesTruth, x: f64, xlog: bool, ylog: bool) -> Option<f64> {
    let xs: Vec<f64> = t.x.iter().map(|&v| tx(v, xlog)).collect();
    let (lo, hi) = (xs[0].min(xs[xs.len() - 1]), xs[0].max(xs[xs.len() - 1]));
    let slack = 1e-4 * (hi - lo); // calibration rounding at the very ends of the data
    let x = tx(x, xlog);
    if x < lo - slack || x > hi + slack {
        return None;
    }
    let x = x.clamp(lo, hi);
    let i = xs.windows(2).position(|w| (w[0] <= x && x <= w[1]) || (w[1] <= x && x <= w[0]))?;
    let (y0, y1) = (tx(t.y[i], ylog), tx(t.y[i + 1], ylog));
    let f = if xs[i + 1] == xs[i] { 0.0 } else { (x - xs[i]) / (xs[i + 1] - xs[i]) };
    Some(y0 + f * (y1 - y0))
}

const TOLERANCE: f64 = 0.004; // of the series' own extent, in plot space

/// Extract every series of an axes through the session and compare with the data. Returns (series name, worst error).
fn check_axes(s: &mut Session, setup: &AxisSetup) -> Vec<(String, f64)> {
    calibrate(s, setup);
    let mut report = Vec::new();
    for t in &setup.series {
        let label = format!("{} / {}", setup.label, t.name);
        let ids = members(s, setup, t);
        assert!(!ids.is_empty(), "{label}: no primitive of colour {:?} found", t.color);
        s.edit(Edit::NewSeries).unwrap();
        for &m in &ids {
            s.edit(Edit::ToggleMember(m)).unwrap();
        }
        let series = s.active_series().unwrap();
        let (xs, ys) = (span(&t.x, setup.x_log), span(&t.y, setup.y_log));

        let worst = if t.kind == "line" {
            let csv = s.csv(series).unwrap_or_else(|e| panic!("{label}: {e}"));
            let rows: Vec<(f64, f64)> = csv
                .lines()
                .skip(1)
                .map(|l| {
                    let (x, y) = l.split_once(',').unwrap();
                    (x.parse().unwrap(), y.parse().unwrap())
                })
                .collect();
            let total: usize = ids.iter().map(|&i| s.figure.primitives[i].points.len()).sum();
            assert_eq!(rows.len(), total, "{label}: one CSV row per vertex");
            let mut worst: f64 = 0.0;
            let mut at = (0.0, 0.0, 0.0);
            for &(x, y) in &rows {
                let want = truth_at(t, x, setup.x_log, setup.y_log)
                    .unwrap_or_else(|| panic!("{label}: x = {x} outside the data"));
                let err = (tx(y, setup.y_log) - want).abs() / ys;
                if err > worst {
                    (worst, at) = (err, (x, y, want));
                }
            }
            if worst > TOLERANCE {
                eprintln!("{label}: worst vertex x = {}, got y = {}, data say {}", at.0, at.1, at.2);
            }
            // the whole visible curve was found, not a piece of it: the plot area clips the data at its edges
            let (x_lo, y_lo) = s.plot_coords(Point::new(setup.frame.left, setup.frame.bottom)).unwrap();
            let (x_hi, y_hi) = s.plot_coords(Point::new(setup.frame.right, setup.frame.top)).unwrap();
            // in plot space (log10 on log axes), with a little slack so data sitting exactly on the plot edge counts
            // despite calibration rounding
            let (x_lo, x_hi) = (tx(x_lo, setup.x_log), tx(x_hi, setup.x_log));
            let (y_lo, y_hi) = (tx(y_lo, setup.y_log), tx(y_hi, setup.y_log));
            let (x_slack, y_slack) = ((x_hi - x_lo).abs() * 1e-3, (y_hi - y_lo).abs() * 1e-3);
            let (x_min, x_max) = (x_lo.min(x_hi) - x_slack, x_lo.max(x_hi) + x_slack);
            let (y_min, y_max) = (y_lo.min(y_hi) - y_slack, y_lo.max(y_hi) + y_slack);
            let in_area = |x: f64, y: f64| {
                let (x, y) = (tx(x, setup.x_log), tx(y, setup.y_log));
                x >= x_min && x <= x_max && y >= y_min && y <= y_max
            };
            let seen: Vec<f64> =
                t.x.iter().zip(&t.y).filter(|&(&x, &y)| in_area(x, y)).map(|(&x, _)| tx(x, setup.x_log)).collect();
            let shown: Vec<f64> = rows.iter().filter(|&&(x, y)| in_area(x, y)).map(|r| tx(r.0, setup.x_log)).collect();
            let lo = shown.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = shown.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let (tlo, thi) = (
                seen.iter().cloned().fold(f64::INFINITY, f64::min),
                seen.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            );
            if (lo - tlo).abs() / xs > TOLERANCE || (hi - thi).abs() / xs > TOLERANCE {
                eprintln!("{label}: extracted x {lo}..{hi}, data inside the plot area {tlo}..{thi}");
            }
            worst = worst.max((lo - tlo).abs() / xs).max((hi - thi).abs() / xs);
            worst
        } else {
            // each marker's centre is a data point
            assert_eq!(ids.len(), t.x.len(), "{label}: one primitive per marker");
            let mut unused: Vec<(f64, f64)> = t.x.iter().copied().zip(t.y.iter().copied()).collect();
            let mut worst: f64 = 0.0;
            for &i in &ids {
                let c = s.figure.primitives[i].extent().unwrap().center();
                let (x, y) = s.plot_coords(c).unwrap();
                let dist = |q: &(f64, f64)| {
                    let (dx, dy) = (
                        (tx(x, setup.x_log) - tx(q.0, setup.x_log)) / xs,
                        (tx(y, setup.y_log) - tx(q.1, setup.y_log)) / ys,
                    );
                    dx.hypot(dy)
                };
                let (k, d) =
                    unused.iter().enumerate().map(|(k, q)| (k, dist(q))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
                unused.swap_remove(k);
                worst = worst.max(d);
            }
            worst
        };
        assert!(worst <= TOLERANCE, "{label}: worst error {worst:.5} of the series extent (limit {TOLERANCE})");
        report.push((t.name.clone(), worst));
    }
    report
}

fn run_matplotlib(name: &str, ext: &str) {
    let Some(figure) = open(&format!("matplotlib/{name}.{ext}")) else { return };
    let setups = matplotlib_setups(name, ext, &figure);
    let mut session = Session::new(figure, None);
    for setup in setups {
        for (series, worst) in check_axes(&mut session, &setup) {
            println!("{}: {series}: worst error {worst:.5}", setup.label);
        }
    }
}

fn run_mathematica(name: &str, ext: &str) {
    let Some(figure) = open(&format!("mathematica/{name}.{ext}")) else { return };
    let setup = mathematica_setup(name, &figure);
    let mut session = Session::new(figure, None);
    for (series, worst) in check_axes(&mut session, &setup) {
        println!("{}: {series}: worst error {worst:.5}", setup.label);
    }
}

macro_rules! corpus {
    ($($figure:ident),* $(,)?) => {
        mod matplotlib {
            use super::run_matplotlib;
            $(
                mod $figure {
                    use super::run_matplotlib;
                    #[test]
                    fn pdf() { run_matplotlib(stringify!($figure), "pdf") }
                    #[test]
                    fn svg() { run_matplotlib(stringify!($figure), "svg") }
                    #[test]
                    fn eps() { run_matplotlib(stringify!($figure), "eps") }
                }
            )*
        }
    };
}

macro_rules! corpus_mathematica {
    ($($figure:ident),* $(,)?) => {
        mod mathematica {
            $(
                mod $figure {
                    use super::super::run_mathematica;
                    #[test]
                    fn pdf() { run_mathematica(stringify!($figure), "pdf") }
                    #[test]
                    fn svg() { run_mathematica(stringify!($figure), "svg") }
                    #[test]
                    fn eps() { run_mathematica(stringify!($figure), "eps") }
                }
            )*
        }
    };
}

corpus!(sin_simple, scatter_markers, loglog, errorbar_semilogy, complex_multi);
corpus_mathematica!(sin_simple, scatter_markers, loglog, complex_multi);
