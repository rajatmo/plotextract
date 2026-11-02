//! The tick networks on generated plots: the Rust pipeline must find what plotML's Python pipeline finds, and find the
//! true ticks.
//!
//! The plots, their truth and what the Python pipeline found are written by plotML's `eval/parity.py` into
//! `testdata/ticks/` (see [`plotextract_corpus::data_dir`]); without them the test says so and passes.

use std::sync::Arc;

use plotextract_domain::{ArxivId, Prepare, Slot};
use plotextract_model::{Effect, Intent, Model};
use plotextract_ports::papers::{ArxivError, Paper, PaperSource, Progress};
use plotextract_ticks::OnnxTicks;
use plotextract_usecases::wizard::suggest::{SuggestedAxis, TickSearch};
use plotextract_usecases::wizard::{Step, Wizard, WizardIntent};
use serde_json::Value;

/// How far a tick may be from where the Python pipeline put it, in pixels: the two scale the image a little differently.
const PLACE: f64 = 0.15;
/// An axis is right if, at every true tick, its calibration gives the true value to within this share of the span.
const SPAN: f64 = 0.005;

fn axis_error(s: &SuggestedAxis, truth: &Value, axis: usize) -> f64 {
    let name = ["x", "y"][axis];
    let log = truth[format!("{name}log")].as_bool().unwrap();
    if s.log != log {
        return f64::INFINITY;
    }
    let u = |v: f64| if log { v.log10() } else { v };
    let (p0, p1) = (f64::from(s.min.line.across), f64::from(s.max.line.across));
    let (u0, u1) = (u(s.min.value), u(s.max.value));
    let ticks = truth["ticks"][name].as_array().unwrap();
    let values: Vec<f64> = ticks.iter().map(|t| u(t["value"].as_f64().unwrap())).collect();
    let span = values.iter().copied().fold(f64::MIN, f64::max) - values.iter().copied().fold(f64::MAX, f64::min);
    ticks
        .iter()
        .zip(&values)
        .map(|(t, v)| ((u0 + (u1 - u0) * (t["at"][axis].as_f64().unwrap() - p0) / (p1 - p0)) - v).abs() / span)
        .fold(0.0, f64::max)
}

#[test]
fn the_tick_networks_find_what_plotml_finds_and_the_true_ticks() {
    let dir = plotextract_corpus::data_dir().join("ticks");
    let Some(expected) = plotextract_corpus::fixture("ticks/expected.json") else { return };
    let expected: Value = serde_json::from_slice(&expected).unwrap();
    let models = Arc::new(OnnxTicks::new());
    let (mut axes, mut agree, mut right, mut plots_right) = (0, 0, 0, 0);
    let mut differ = Vec::new();
    for (stem, want) in expected.as_object().unwrap() {
        let bytes = std::fs::read(dir.join(format!("{stem}.png"))).unwrap();
        let truth: Value = serde_json::from_slice(&std::fs::read(dir.join(format!("{stem}.json"))).unwrap()).unwrap();
        let image = plotextract_algo::decode::decode(&bytes).unwrap();
        let got = TickSearch::new(models.clone(), Prepare::default()).run(&image).unwrap();
        let mut plot_right = true;
        for (k, name) in ["x", "y"].into_iter().enumerate() {
            axes += 1;
            let same = match (&got.axes[k], &want[name]) {
                (None, Value::Null) => true,
                (Some(s), w) if !w.is_null() => {
                    let end = |e: &Value| (e[0].as_f64().unwrap(), e[1].as_f64().unwrap());
                    let ((p0, v0), (p1, v1)) = (end(&w["min"]), end(&w["max"]));
                    s.log == w["log"].as_bool().unwrap()
                        && s.fit as u64 == w["fit"].as_u64().unwrap()
                        && s.of as u64 == w["of"].as_u64().unwrap()
                        && (f64::from(s.min.line.across) - p0).abs() <= PLACE
                        && (f64::from(s.max.line.across) - p1).abs() <= PLACE
                        && (s.min.value - v0).abs() <= 1e-9 * v0.abs()
                        && (s.max.value - v1).abs() <= 1e-9 * v1.abs()
                }
                _ => false,
            };
            if same {
                agree += 1;
            } else {
                differ.push(format!("{stem} {name}: Rust {:?}, Python {}", got.axes[k], want[name]));
            }
            let ok = got.axes[k].as_ref().is_some_and(|s| axis_error(s, &truth, k) <= SPAN);
            right += usize::from(ok);
            plot_right &= ok;
        }
        plots_right += usize::from(plot_right);
    }
    let plots = expected.as_object().unwrap().len();
    eprintln!(
        "{plots} plots: {agree} of {axes} axes as plotML finds them, {right} right; {plots_right} plots right ({:.1}%)",
        100.0 * plots_right as f64 / plots as f64
    );
    for d in &differ {
        eprintln!("  differs: {d}");
    }
    // the same networks and the same steps: an axis may differ only where a label is read differently from an image
    // scaled a little differently
    assert!(agree * 100 >= axes * 97, "{agree} of {axes} axes agree with plotML");
    // plotML measures 97.6% of plots right on its held-out plots
    assert!(plots_right * 100 >= plots * 93, "{plots_right} of {plots} plots right");
}

/// Papers that never come.
struct NoPapers;

impl PaperSource for NoPapers {
    fn fetch(&self, id: &ArxivId, _: &dyn Fn(&str) -> bool, _: &Progress) -> Result<Paper, ArxivError> {
        Err(ArxivError::NotFound(id.to_string()))
    }
}

/// Carry out an intent and the jobs it starts, here, as the window would on its threads.
fn run(m: &mut Model, intent: Intent) {
    let mut todo = m.dispatch(intent);
    while !todo.is_empty() {
        let mut next = Vec::new();
        for effect in todo {
            if let Effect::Run(job) = effect {
                next.extend(m.finish(job.run()));
            }
        }
        todo = next;
    }
}

#[test]
fn suggest_ticks_on_step_3_calibrates_the_image_and_ctrl_z_takes_it_back() {
    let dir = plotextract_corpus::data_dir().join("ticks");
    let Some(expected) = plotextract_corpus::fixture("ticks/expected.json") else { return };
    let expected: Value = serde_json::from_slice(&expected).unwrap();
    let (stem, want) =
        expected.as_object().unwrap().iter().find(|(_, w)| !w["x"].is_null() && !w["y"].is_null()).unwrap();
    let mut m =
        Model::new(plotextract_corpus::library(), Arc::new(NoPapers)).with_tick_models(Arc::new(OnnxTicks::new()));
    run(&mut m, Intent::Open(dir.join(format!("{stem}.png"))));
    assert!(m.wizard().is_some_and(Wizard::can_suggest_ticks), "an image opens in the wizard, which can suggest ticks");
    run(&mut m, Intent::Wizard(WizardIntent::GoTo(Step::PlotArea)));
    run(&mut m, Intent::Wizard(WizardIntent::SuggestTicks));

    let w = m.wizard().unwrap();
    assert!(!w.suggesting());
    let cal = w.calibration();
    assert!(cal.is_calibrated(), "{:?}", m.message());
    for (axis, name) in [(&cal.x, "x"), (&cal.y, "y")] {
        let w = &want[name];
        assert_eq!(axis.log, w["log"].as_bool().unwrap());
        for (i, end) in ["min", "max"].into_iter().enumerate() {
            assert!((axis.pos[i].unwrap() - w[end][0].as_f64().unwrap()).abs() <= PLACE, "{name} {end}");
            assert_eq!(axis.value[i], w[end][1].as_f64());
        }
    }
    assert!(m.message().unwrap().text.starts_with("Suggested the ticks of both axes"), "{:?}", m.message());

    run(&mut m, Intent::Undo);
    assert!(Slot::ALL.iter().all(|&s| m.wizard().unwrap().tick(s).is_none()), "Ctrl+Z takes the suggestion back");
}
