//! Step 3, plot area: the straight lines of the axes colour (searched for as a job), the four ticks chosen from them with
//! the plot values typed for them (the calibration), and the frame the later steps keep to.

use plotextract_algo::lines::{self, Algorithm, Line, Orientation};
use plotextract_domain::{Axis, Calibration, Slot};
use plotextract_domain::{MaskMeta, PixelRect, Tag, Value};

use crate::colours::AxesSource;

/// A search for straight lines in the axes colour, to be run on a thread: [`LineSearch::run`].
pub struct LineSearch {
    source: AxesSource,
    algorithm: Algorithm,
    params: lines::Params,
}

impl LineSearch {
    /// The slow part: make the axes mask and look for lines in it. Safe to call on any thread.
    pub fn run(self) -> Vec<Line> {
        lines::detect(&self.source.mask(), self.algorithm, &self.params)
    }
}

/// The direction of the line that is tick `slot`: a vertical line on the horizontal axis, and the other way round.
pub fn orientation_of(slot: Slot) -> Orientation {
    if on_x_axis(slot) { Orientation::Vertical } else { Orientation::Horizontal }
}

/// Whether tick `slot` is on the x axis.
pub fn on_x_axis(slot: Slot) -> bool {
    slot.axis() == Axis::X
}

pub(super) fn index(slot: Slot) -> usize {
    slot as usize
}

/// The tags that say which tick a slot is.
fn tick_tags(slot: Slot) -> [Tag; 2] {
    match slot {
        Slot::XMin => [Tag::XAxis, Tag::XMin],
        Slot::XMax => [Tag::XAxis, Tag::XMax],
        Slot::YMin => [Tag::YAxis, Tag::YMin],
        Slot::YMax => [Tag::YAxis, Tag::YMax],
    }
}

pub struct PlotArea {
    /// The line chosen for each tick, in the order of [`Slot::ALL`].
    ticks: [Option<Line>; 4],
    pub(super) show_ticks: [bool; 4],
    /// The plot value typed for each tick, in the order of [`Slot::ALL`].
    values: [Option<f64>; 4],
    /// Whether the x and the y axis are logarithmic.
    log: [bool; 2],
    algorithm: Algorithm,
    params: lines::Params,
    /// The straight lines found in the axes colour.
    pub(super) lines: Vec<Line>,
    /// The lines must be looked for afresh (and are not being looked for yet).
    wanted: bool,
    /// The search for lines that is running on a thread, by job number.
    running: Option<u64>,
}

impl Default for PlotArea {
    fn default() -> Self {
        Self {
            ticks: [None; 4],
            show_ticks: [true; 4],
            values: [None; 4],
            log: [false; 2],
            algorithm: Algorithm::default(),
            params: lines::Params::default(),
            lines: Vec::new(),
            wanted: false,
            running: None,
        }
    }
}

impl PlotArea {
    /// How the straight lines are looked for.
    pub fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// The settings of the search for straight lines.
    pub fn params(&self) -> &lines::Params {
        &self.params
    }

    /// The straight lines found in the axes colour. Empty until the step is entered.
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// The line chosen as tick `tick`.
    pub fn tick(&self, tick: Slot) -> Option<Line> {
        self.ticks[index(tick)]
    }

    /// The first tick that has no line yet.
    pub fn first_unset(&self) -> Option<Slot> {
        Slot::ALL.into_iter().find(|&t| self.tick(t).is_none())
    }

    /// The plot value typed for tick `tick`.
    pub fn value(&self, tick: Slot) -> Option<f64> {
        self.values[index(tick)]
    }

    /// Whether `axis` is logarithmic.
    pub fn is_log(&self, axis: Axis) -> bool {
        self.log[usize::from(axis == Axis::Y)]
    }

    /// The calibration the chosen ticks and the typed values make: the positions are those of the ticks' lines, in image
    /// pixels.
    pub fn calibration(&self) -> Calibration {
        let mut calibration = Calibration::default();
        for slot in Slot::ALL {
            let axis = calibration.axis_mut(slot.axis());
            axis.pos[slot.which()] = self.tick(slot).map(|l| f64::from(l.across));
            axis.value[slot.which()] = self.value(slot);
        }
        calibration.x.log = self.is_log(Axis::X);
        calibration.y.log = self.is_log(Axis::Y);
        calibration
    }

    /// Make `line` tick `tick`, or take its line away. Whether it was done: a line of the wrong direction is not a tick.
    pub(super) fn set_tick(&mut self, tick: Slot, line: Option<Line>) -> bool {
        if line.is_some_and(|l| l.orientation != orientation_of(tick)) {
            return false;
        }
        self.ticks[index(tick)] = line;
        true
    }

    /// Bring back saved ticks, values and settings. The lines are not looked for: that waits for the colours.
    pub(super) fn restore(
        &mut self,
        ticks: [Option<Line>; 4],
        values: [Option<f64>; 4],
        log: [bool; 2],
        algorithm: Algorithm,
        params: lines::Params,
    ) {
        for (slot, line) in Slot::ALL.into_iter().zip(ticks) {
            self.set_tick(slot, line);
        }
        for (slot, value) in Slot::ALL.into_iter().zip(values) {
            self.set_value(slot, value);
        }
        self.log = log;
        self.algorithm = algorithm;
        self.params = params;
    }

    pub(super) fn set_value(&mut self, tick: Slot, value: Option<f64>) {
        self.values[index(tick)] = value.filter(|v| v.is_finite());
    }

    pub(super) fn set_log(&mut self, axis: Axis, on: bool) {
        self.log[usize::from(axis == Axis::Y)] = on;
    }

    pub(super) fn set_algorithm(&mut self, algorithm: Algorithm) {
        self.algorithm = algorithm;
        self.wanted = true;
    }

    pub(super) fn set_params(&mut self, params: lines::Params) {
        if params != self.params {
            self.params = params;
            self.wanted = true;
        }
    }

    /// The step is entered: the lines are looked for afresh, and those found before are not shown meanwhile.
    pub(super) fn enter(&mut self) {
        self.lines.clear();
        self.wanted = true;
    }

    /// If the lines are to be looked for and no search is running, the search to run (once: asking again gives nothing
    /// until the next change). Without an axes colour (`source`) there is nothing to search and no lines.
    pub(super) fn take_request(&mut self, source: Option<AxesSource>) -> Option<LineSearch> {
        if self.running.is_some() || !std::mem::take(&mut self.wanted) {
            return None;
        }
        let Some(source) = source else {
            self.lines.clear();
            return None;
        };
        Some(LineSearch { source, algorithm: self.algorithm, params: self.params.clone() })
    }

    /// The search was started as job `id`.
    pub(super) fn begin(&mut self, id: u64) {
        self.running = Some(id);
    }

    /// The answer of job `id`; the answer of any other is dropped.
    pub(super) fn finish(&mut self, id: u64, found: Vec<Line>) {
        if self.running == Some(id) {
            self.running = None;
            self.lines = found;
        }
    }

    /// Whether the lines shown are out of date: a search is wanted or running.
    pub fn busy(&self) -> bool {
        self.wanted || self.running.is_some()
    }

    /// The sides of the frame, or the two axes when only those are drawn: the long straight lines of the axes colour that
    /// meet another at a corner. A curve of the axes colour can be as long and as straight as a side of the frame, but it
    /// does not end where a side does.
    pub fn frame_lines(&self, crop: PixelRect) -> Vec<Line> {
        let long: Vec<Line> = self
            .lines
            .iter()
            .copied()
            .filter(|l| {
                let side = match l.orientation {
                    Orientation::Horizontal => crop.width,
                    Orientation::Vertical => crop.height,
                };
                3.0 * l.length() >= side as f32
            })
            .collect();
        let corner = |a: &Line, b: &Line| {
            let reach = 1.5 + (a.width + b.width) / 2.0;
            let ends_at = |l: &Line, at: f32| (l.from - at).abs() <= reach || (l.to - at).abs() <= reach;
            a.orientation != b.orientation && ends_at(a, b.across) && ends_at(b, a.across)
        };
        long.iter().copied().filter(|a| long.iter().any(|b| corner(a, b))).collect()
    }

    /// Which tick `slot` is, where its line is, its value, and whether its axis is calibrated.
    pub fn meta(&self, slot: Slot, meta: MaskMeta) -> MaskMeta {
        let mut meta = meta.tags([Tag::Tick, Tag::Axes]).tags(tick_tags(slot));
        if let Some(line) = self.tick(slot) {
            meta = meta
                .with("position", Value::Pixels(f64::from(line.across)))
                .with("length", Value::Pixels(f64::from(line.length())))
                .with("thickness", Value::Pixels(f64::from(line.width)));
        }
        if let Some(value) = self.value(slot) {
            meta = meta.with("value", Value::Number(value));
        }
        meta = meta.with("log scale", Value::Flag(self.is_log(slot.axis())));
        if self.calibration().axis(slot.axis()).map().is_ok() { meta.tag(Tag::Calibrated) } else { meta }
    }
}
