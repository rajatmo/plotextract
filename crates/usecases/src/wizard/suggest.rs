//! Suggested ticks: the tick networks find the four ticks of the plot area step and read their values, for the user to
//! check. The search runs as a job, on the crop with what is excluded painted over as paper; an axis whose labels do not
//! agree on a line is not suggested.

use std::sync::Arc;

use image::RgbaImage;
use plotextract_algo::lines::{Line, Orientation};
use plotextract_algo::ticks::{self, CLASSES, STEPS};
use plotextract_domain::{Axis, Prepare};
use plotextract_ports::{ModelError, TickModels};

/// A search for the four ticks, to be run on a thread: [`TickSearch::run`].
pub struct TickSearch {
    models: Arc<dyn TickModels>,
    prepare: Prepare,
}

/// A tick suggested: its line (its mark, at the place the axis' fit gives it) and its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuggestedTick {
    pub line: Line,
    pub value: f64,
}

/// The ticks suggested for one axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SuggestedAxis {
    pub log: bool,
    /// The smaller value's tick and the larger's.
    pub min: SuggestedTick,
    pub max: SuggestedTick,
    /// How many of the ticks found agree, and of how many.
    pub fit: usize,
    pub of: usize,
}

/// What the networks suggest: an axis each, x then y, where its ticks agree.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Suggestion {
    pub axes: [Option<SuggestedAxis>; 2],
}

impl TickSearch {
    /// A search with `models` in the part of the image `prepare` keeps (the wizard makes one with
    /// [`Wizard::take_ticks_request`](super::Wizard::take_ticks_request)).
    pub fn new(models: Arc<dyn TickModels>, prepare: Prepare) -> Self {
        Self { models, prepare }
    }

    /// The slow part: find the ticks in `image` and read their labels. Safe to call on any thread.
    pub fn run(self, image: &RgbaImage) -> Result<Suggestion, ModelError> {
        let (crop, unknown) = self.prepare.unknown_in_crop(image.width(), image.height());
        let mut grey = ticks::grey(image, crop);
        for (p, &u) in grey.pixels_mut().zip(&unknown) {
            if u != 0 {
                p.0[0] = 255;
            }
        }
        let (square, scale) = ticks::fit(&grey);
        let maps = self.models.maps(&square)?;
        let mut axes = [None, None];
        for (k, axis) in [Axis::X, Axis::Y].into_iter().enumerate() {
            let found = ticks::candidates(&maps, scale, axis);
            let crops: Vec<f32> =
                found.iter().filter_map(|c| c.label).flat_map(|b| ticks::label_crop(&grey, b)).collect();
            let scores = self.models.read(&crops)?;
            let mut texts = scores.as_chunks::<{ STEPS * CLASSES }>().0.iter().map(|s| ticks::decode(s));
            let read: Vec<String> =
                found.iter().map(|c| c.label.and_then(|_| texts.next()).unwrap_or_default()).collect();
            let powers = 2 * read.iter().filter(|t| t.contains('^')).count() > found.len();
            let along = |c: &ticks::Candidate| if axis == Axis::X { c.x } else { c.y };
            let points: Vec<(f64, Option<f64>)> =
                found.iter().zip(&read).map(|(c, t)| (along(c), ticks::value_of(t))).collect();
            axes[k] = ticks::calibrate(&points, powers).map(|fit| {
                let tick = |end: ticks::End| {
                    let c = &found[end.index];
                    let [x0, y0, x1, y1] = ticks::mark(&grey, c.x, c.y, axis).map(|v| v as f32);
                    let (ox, oy) = (crop.x as f32, crop.y as f32);
                    let line = match axis {
                        Axis::X => Line {
                            orientation: Orientation::Vertical,
                            from: oy + y0,
                            to: oy + y1,
                            across: ox + end.position as f32,
                            width: x1 - x0,
                        },
                        Axis::Y => Line {
                            orientation: Orientation::Horizontal,
                            from: ox + x0,
                            to: ox + x1,
                            across: oy + end.position as f32,
                            width: y1 - y0,
                        },
                    };
                    SuggestedTick { line, value: end.value }
                };
                SuggestedAxis { log: fit.log, min: tick(fit.min), max: tick(fit.max), fit: fit.fit, of: fit.of }
            });
        }
        Ok(Suggestion { axes })
    }
}

/// Whether ticks can be suggested, and the search for them.
#[derive(Default)]
pub(super) struct Suggest {
    /// The networks, if the program has them.
    pub(super) models: Option<Arc<dyn TickModels>>,
    /// A search was asked for and not started yet.
    wanted: bool,
    /// The search running, by job number.
    running: Option<u64>,
}

impl Suggest {
    pub(super) fn want(&mut self) {
        self.wanted = self.models.is_some();
    }

    /// The search to run, once, if one is wanted and none is running.
    pub(super) fn take(&mut self, prepare: &Prepare) -> Option<TickSearch> {
        if self.running.is_some() || !std::mem::take(&mut self.wanted) {
            return None;
        }
        Some(TickSearch::new(self.models.clone()?, prepare.clone()))
    }

    pub(super) fn begin(&mut self, id: u64) {
        self.running = Some(id);
    }

    /// Whether `id` is the search waited for; it is no longer running.
    pub(super) fn finish(&mut self, id: u64) -> bool {
        let ours = self.running == Some(id);
        if ours {
            self.running = None;
        }
        ours
    }

    pub(super) fn busy(&self) -> bool {
        self.wanted || self.running.is_some()
    }
}
