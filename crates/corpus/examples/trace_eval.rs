//! Trace generated raster plots through the wizard as a careful user would, and compare with the data they were drawn
//! from. Each plot comes with a JSON file of its truth (made by a script that draws it): the curves' colours and data,
//! and where two ticks of each axis landed in pixels.
//!
//!     cargo run --release -p plotextract-corpus --example trace_eval -- folder
//!
//! The user is simulated: the colours nearest each curve's colour are marked Curve (and Axes for the axes' colour), the
//! line found nearest each tick is chosen and given its value, and on the link step the pieces lying on a curve are
//! linked from left to right. What is measured is what a user would meet: how the pieces of a curve were grouped (how
//! many groups, how many pieces in no group, how many foreign pieces in its groups) and how far the points written are
//! from the truth, as a share of the axis.

use std::sync::Arc;

use plotextract_algo::lines::Orientation;
use plotextract_algo::{analyse_colours, decode};
use plotextract_domain::{Axis, Prepare, Slot};
use plotextract_usecases::wizard::{Role, Step, Wizard, WizardIntent};
use serde_json::Value;

fn settle(w: &mut Wizard) {
    let mut id = 100;
    loop {
        if let Some(search) = w.take_lines_request() {
            id += 1;
            w.begin_lines(id);
            w.finish_lines(id, search.run());
        } else if let Some(search) = w.take_pieces_request() {
            id += 1;
            w.begin_pieces(id);
            w.finish_pieces(id, search.run());
        } else {
            break;
        }
    }
}

fn dist(a: [f64; 3], b: [u8; 3]) -> f64 {
    (0..3).map(|i| (a[i] - f64::from(b[i])).powi(2)).sum::<f64>().sqrt()
}

/// Value to pixel along an axis, from two ticks.
struct AxisMap {
    p0: f64,
    v0: f64,
    p1: f64,
    v1: f64,
    log: bool,
}

impl AxisMap {
    fn t(&self, v: f64) -> f64 {
        if self.log { v.log10() } else { v }
    }

    fn pixel(&self, v: f64) -> f64 {
        let (a, b) = (self.t(self.v0), self.t(self.v1));
        self.p0 + (self.t(v) - a) / (b - a) * (self.p1 - self.p0)
    }
}

/// The distance from `p` to the polyline `line`.
fn to_polyline(p: (f64, f64), line: &[(f64, f64)]) -> f64 {
    line.windows(2)
        .map(|s| {
            let ((ax, ay), (bx, by)) = (s[0], s[1]);
            let (dx, dy) = (bx - ax, by - ay);
            let len2 = (dx * dx + dy * dy).max(1e-12);
            let u = (((p.0 - ax) * dx + (p.1 - ay) * dy) / len2).clamp(0.0, 1.0);
            ((ax + u * dx - p.0).powi(2) + (ay + u * dy - p.1).powi(2)).sqrt()
        })
        .fold(f64::INFINITY, f64::min)
}

fn main() {
    let dir = std::env::args().nth(1).expect("usage: trace_eval <folder>");
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    names.sort();
    println!(
        "{:<22} {:<8} {:<7} {:>4} {:>3} {:>4} {:>4} {:>4} {:>5} {:>5} {:>5} {:>6} {:>6}",
        "plot", "curve", "kind", "on", "grp", "left", "frgn", "link", "pts", "cov%", "ord%", "med%", "max%"
    );
    for path in names {
        let truth: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let bytes = std::fs::read(path.with_file_name(truth["image"].as_str().unwrap())).unwrap();
        let image = match decode::decode(&bytes) {
            Ok(i) => i,
            Err(e) => {
                println!("{stem}: cannot decode: {e}");
                continue;
            }
        };
        let mut w = Wizard::new(image.width(), image.height());
        w.apply(WizardIntent::GoTo(Step::Colours));
        let params = w.take_analysis_request().unwrap_or_default();
        w.begin_analysis(1);
        w.finish_analysis(1, Ok(Arc::new(analyse_colours(&image, &Prepare::default(), &params))));
        let inks: Vec<[u8; 3]> = w.colours().map().unwrap().inks.iter().map(|i| i.rgb).collect();
        let rgb = |v: &Value| [0, 1, 2].map(|k| v[k].as_f64().unwrap());
        let axes = rgb(&truth["axes_rgb"]);
        let curves = truth["curves"].as_array().unwrap();
        // the ink nearest each curve's colour is the curve; the one nearest the axes' colour the axes
        let nearest = |c: [f64; 3]| (0..inks.len()).min_by(|&a, &b| dist(c, inks[a]).total_cmp(&dist(c, inks[b])));
        let curve_inks: Vec<Option<usize>> =
            curves.iter().map(|c| nearest(rgb(&c["rgb"])).filter(|&i| dist(rgb(&c["rgb"]), inks[i]) < 80.0)).collect();
        let axes_ink = nearest(axes).filter(|&i| dist(axes, inks[i]) < 80.0);
        for i in curve_inks.iter().flatten() {
            w.apply(WizardIntent::SetRole(*i, Role::Curve, true));
        }
        if let Some(i) = axes_ink {
            w.apply(WizardIntent::SetRole(i, Role::Axes, true));
        }
        w.apply(WizardIntent::GoTo(Step::PlotArea));
        settle(&mut w);
        let log = [truth["log"][0].as_bool().unwrap(), truth["log"][1].as_bool().unwrap()];
        let mut missing = Vec::new();
        for (slot, key, orientation) in [
            (Slot::XMin, "xmin", Orientation::Vertical),
            (Slot::XMax, "xmax", Orientation::Vertical),
            (Slot::YMin, "ymin", Orientation::Horizontal),
            (Slot::YMax, "ymax", Orientation::Horizontal),
        ] {
            let (at, value) = (truth["ticks"][key][0].as_f64().unwrap(), truth["ticks"][key][1].as_f64().unwrap());
            let line = w
                .lines()
                .iter()
                .copied()
                .filter(|l| l.orientation == orientation)
                .min_by(|a, b| (f64::from(a.across) - at).abs().total_cmp(&(f64::from(b.across) - at).abs()))
                .filter(|l| (f64::from(l.across) - at).abs() < 2.5);
            match line {
                Some(l) => {
                    w.apply(WizardIntent::SetTick(slot, Some(l)));
                    w.apply(WizardIntent::SetTickValue(slot, Some(value)));
                }
                None => missing.push(key),
            }
        }
        w.apply(WizardIntent::SetLog(Axis::X, log[0]));
        w.apply(WizardIntent::SetLog(Axis::Y, log[1]));
        if !missing.is_empty() {
            println!("{stem}: no line found for ticks {missing:?} (inks {}: axes {:?})", inks.len(), axes_ink);
        }
        w.apply(WizardIntent::GoTo(Step::Shapes));
        settle(&mut w);
        let tick = |k: &str| (truth["ticks"][k][0].as_f64().unwrap(), truth["ticks"][k][1].as_f64().unwrap());
        let (xa, xb, ya, yb) = (tick("xmin"), tick("xmax"), tick("ymin"), tick("ymax"));
        let xm = AxisMap { p0: xa.0, v0: xa.1, p1: xb.0, v1: xb.1, log: log[0] };
        let ym = AxisMap { p0: ya.0, v0: ya.1, p1: yb.0, v1: yb.1, log: log[1] };
        w.apply(WizardIntent::GoTo(Step::Links));
        w.apply(WizardIntent::SetLinkGroup(None));
        let groups_total = w.shapes().map_or(0, |s| s.groups().count());
        for (c, ink) in curves.iter().zip(&curve_inks) {
            let name = c["name"].as_str().unwrap();
            let kind = c["kind"].as_str().unwrap();
            let (xs, ys): (Vec<f64>, Vec<f64>) = (
                c["x"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect(),
                c["y"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect(),
            );
            let (Some(ink), Some(pieces), Some(shapes)) = (*ink, w.pieces(), w.shapes()) else {
                println!("{stem:<22} {name:<10} {kind:<8} no ink of its colour, or no pieces");
                continue;
            };
            let line: Vec<(f64, f64)> = xs.iter().zip(&ys).map(|(&x, &y)| (xm.pixel(x), ym.pixel(y))).collect();
            // the pieces of its colour that lie on it: what the user links
            let near = |p: usize| {
                let Some(centre) = w.piece_centre(p) else { return false };
                let extent = f64::from(pieces.pieces()[p].extent());
                let d = if kind == "markers" {
                    line.iter().map(|q| (q.0 - centre.x).hypot(q.1 - centre.y)).fold(f64::INFINITY, f64::min)
                } else {
                    to_polyline((centre.x, centre.y), &line)
                };
                d <= extent / 2.0 + 3.0
            };
            let mut on: Vec<usize> =
                (0..pieces.pieces().len()).filter(|&p| pieces.pieces()[p].ink == ink && near(p)).collect();
            let groups: std::collections::BTreeSet<usize> = on.iter().filter_map(|&p| shapes.type_of[p]).collect();
            let left = on.iter().filter(|&&p| shapes.type_of[p].is_none()).count();
            let foreign: usize =
                groups.iter().map(|&k| shapes.types[k].members.iter().filter(|p| !on.contains(p)).count()).sum();
            on.sort_by(|&a, &b| {
                let (ca, cb) = (w.piece_centre(a).unwrap(), w.piece_centre(b).unwrap());
                ca.x.total_cmp(&cb.x)
            });
            let (on_count, group_count) = (on.len(), groups.len());
            let single = (groups.len() == 1 && left == 0)
                .then(|| *groups.iter().next().unwrap())
                .filter(|&k| shapes.types[k].members.len() == 1);
            let how = if let Some(k) = single {
                w.apply(WizardIntent::SetLinkGroup(Some(k)));
                "auto"
            } else {
                w.apply(WizardIntent::SetLinkGroup(None));
                for &p in &on {
                    w.apply(WizardIntent::LinkPiece(p));
                }
                w.apply(WizardIntent::EndLink);
                "click"
            };
            let rows = w.link_rows().unwrap_or_default();
            while !w.links().is_empty() {
                w.apply(WizardIntent::DeleteLink(0));
            }
            w.apply(WizardIntent::SetLinkGroup(None));
            let t = |v: f64, log: bool| if log { v.log10() } else { v };
            let yrange = (t(ya.1, log[1]) - t(yb.1, log[1])).abs();
            let xrange = (t(xa.1, log[0]) - t(xb.1, log[0])).abs();
            let truth_pts: Vec<(f64, f64)> =
                xs.iter().zip(&ys).map(|(&a, &b)| (t(a, log[0]) / xrange, t(b, log[1]) / yrange)).collect();
            let nearest = |x: f64, y: f64| {
                truth_pts
                    .iter()
                    .enumerate()
                    .map(|(k, q)| (k, (q.0 - x).hypot(q.1 - y)))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap()
            };
            let found: Vec<(usize, f64)> =
                rows.iter().map(|r| nearest(t(r[0], log[0]) / xrange, t(r[1], log[1]) / yrange)).collect();
            let mut errs: Vec<f64> = found.iter().map(|f| f.1 * 100.0).collect();
            errs.sort_by(f64::total_cmp);
            // the share of the truth within 1% of the axis of a point written (markers: of their own point)
            let cov = if kind == "markers" {
                let hit: std::collections::BTreeSet<usize> = found.iter().filter(|f| f.1 < 0.01).map(|f| f.0).collect();
                hit.len() as f64 / truth_pts.len() as f64 * 100.0
            } else {
                let pts: Vec<(f64, f64)> =
                    rows.iter().map(|r| (t(r[0], log[0]) / xrange, t(r[1], log[1]) / yrange)).collect();
                let near = truth_pts.iter().filter(|q| pts.iter().any(|p| (p.0 - q.0).hypot(p.1 - q.1) < 0.01)).count();
                near as f64 / truth_pts.len() as f64 * 100.0
            };
            // the share of steps that go the same way along the truth as most do
            let steps: Vec<i64> = found.windows(2).map(|w| w[1].0 as i64 - w[0].0 as i64).collect();
            let (up, down) = (steps.iter().filter(|&&s| s >= 0).count(), steps.iter().filter(|&&s| s <= 0).count());
            let ord = if steps.is_empty() { 100.0 } else { up.max(down) as f64 / steps.len() as f64 * 100.0 };
            let pct = |q: f64| errs.get(((errs.len() as f64 - 1.0) * q) as usize).copied().unwrap_or(f64::NAN);
            println!(
                "{stem:<22} {name:<8} {kind:<7} {:>4} {:>3} {:>4} {:>4} {:>4} {:>5} {:>5.0} {:>5.0} {:>6.2} {:>6.2}",
                on_count,
                group_count,
                left,
                foreign,
                how,
                rows.len(),
                cov,
                ord,
                pct(0.5),
                pct(1.0)
            );
        }
        println!("{stem:<22} ({} inks, {} groups in all)", inks.len(), groups_total);
    }
}
