//! The tracing wizard's state, with no window in it. An image is traced in steps, each of which makes masks: the crop and
//! the shapes to exclude (step 1, [`prepare`]), one mask for every ink colour (step 2, [`colour_step`]), the four ticks the
//! axes are calibrated with and the plot values typed for them (step 3, [`plot_area`]), the groups of pieces the curve
//! colours are drawn with, fixed by hand (step 4, [`shape_step`]), and the pieces of a curve linked in order by hand,
//! whose points are written in plot values (step 5, [`link_step`]). Each step's module owns its state, the changes to
//! it and its masks' metadata; [`Wizard`] holds the steps, which one is shown and which mask is selected, and routes each
//! [`WizardIntent`] to the step it belongs to. What a step reads from the steps before it is passed to it (the frame, the
//! curve colours), so the data flows forward: the lines are looked for in the colours tagged axes, the pieces in those
//! tagged curve. Nothing says what a colour or a group is but the user: the steps measure and group, the user names.
//!
//! What is done with a hand that slips can be undone (Ctrl+Z and Ctrl+Shift+Z): drawing the exclude mask on step 1, the
//! ticks, their values and the scales on step 3 (ticks suggested by the tick networks among them, [`suggest`]), the
//! changes to the groups on step 4 and to the links on step 5. Everything else is a choice that is simply made again, or
//! a mask that is deleted from its row.
//!
//! What the user did is saved with a project ([`Work`]) and brought back on a new wizard ([`Wizard::restore`]): what is
//! found from it is found again.

mod colour_step;
mod intent;
mod link_step;
pub mod masks;
mod plot_area;
mod prepare;
mod shape_step;
mod step;
pub mod suggest;
mod work;

use std::sync::Arc;

use std::collections::BTreeSet;

use plotextract_algo::curves::{self, Pieces};
use plotextract_algo::lines::{self, Algorithm, Line};
use plotextract_algo::shapes::Shapes;
use plotextract_algo::{ColourMap, ColourParams};
use plotextract_domain::{Axis, Calibration, ExportError, Primitive, PrimitiveKind, Series, Slot};
use plotextract_domain::{MaskMeta, PixelRect, Prepare, Tag};
use plotextract_ports::TickModels;

pub use self::colour_step::ColourStep;
pub use self::intent::{Pen, Pick, Step, Tool, WizardIntent};
pub use self::link_step::LinkStep;
pub use self::masks::{MaskId, mask_key, mask_name, tick_name};
pub use self::plot_area::{LineSearch, PlotArea, on_x_axis, orientation_of};
pub use self::prepare::PrepareStep;
pub use self::shape_step::{Found, PieceSearch, ShapeStep};
pub use self::suggest::{Suggestion, TickSearch};
pub(crate) use self::work::rgb_distance;
pub use self::work::{InkWork, Work};
pub use crate::colours::{Colours, Role};

pub struct Wizard {
    size: (u32, u32),
    step: Step,
    /// The selected mask.
    active: MaskId,
    pub tool: Option<Tool>,
    pub pen: Pen,
    /// Brush width in image pixels.
    pub brush: f64,
    prepare: PrepareStep,
    colours: ColourStep,
    plot_area: PlotArea,
    shapes: ShapeStep,
    links: LinkStep,
    /// What the colours of a project brought back were said to be, until the colours are found again.
    restoring: Option<Vec<InkWork>>,
}

impl Wizard {
    pub fn new(width: u32, height: u32) -> Self {
        let brush = (f64::from(width.min(height)) * 0.03).clamp(3.0, 60.0).round();
        Self {
            size: (width, height),
            step: Step::Prepare,
            active: MaskId::Exclude,
            tool: None,
            pen: Pen::Brush,
            brush,
            prepare: PrepareStep::new(width, height),
            colours: ColourStep::default(),
            plot_area: PlotArea::default(),
            shapes: ShapeStep::default(),
            links: LinkStep::default(),
            restoring: None,
        }
    }

    // --- saved and brought back -----------------------------------------------------------------------------------

    /// What the user did, to save.
    pub fn work(&self) -> Work {
        let colours = &self.colours.colours;
        // what the colours were said to be stands even while they are to be looked at again
        let inks = colours.map().map_or_else(Vec::new, |map| {
            map.inks.iter().enumerate().map(|(i, ink)| InkWork { rgb: ink.rgb, roles: colours.roles(i) }).collect()
        });
        Work {
            step: self.step,
            crop: self.prepare.prepare().crop,
            exclude: self.prepare.prepare().mask.clone(),
            tolerance: colours.tolerance,
            // brought back but not yet found again: what was said is kept
            inks: self.restoring.clone().unwrap_or(inks),
            algorithm: self.plot_area.algorithm(),
            line_params: self.plot_area.params().clone(),
            ticks: Slot::ALL.map(|s| self.plot_area.tick(s)),
            values: Slot::ALL.map(|s| self.plot_area.value(s)),
            log: [self.plot_area.is_log(Axis::X), self.plot_area.is_log(Axis::Y)],
            curve_params: self.shapes.params().clone(),
            shape_params: self.shapes.shape_params().clone(),
            edits: self.shapes.edits().to_vec(),
            link_group: self.links.group,
            links: self.links.seeds().to_vec(),
            turned: self.links.turned_seeds().collect(),
        }
    }

    /// Bring back saved work on a wizard just made for the same image, on the step it was saved on. The colours are
    /// looked at again (from the step after the first), each takes back what it was said to be, and then the lines and
    /// the pieces are looked for, with the edits and the links made again on them.
    pub fn restore(&mut self, work: Work) {
        let Work {
            step,
            crop,
            exclude,
            tolerance,
            inks,
            algorithm,
            line_params,
            ticks,
            values,
            log,
            curve_params,
            shape_params,
            edits,
            link_group,
            links,
            turned,
        } = work;
        self.prepare.restore(crop, exclude);
        self.colours.colours.tolerance = tolerance.clamp(2.0, 40.0);
        self.restoring = Some(inks);
        self.plot_area.restore(ticks, values, log, algorithm, line_params);
        self.shapes.restore(curve_params, shape_params, edits);
        self.links.restore(links, turned, link_group);
        self.step = step;
        self.tool = None;
        self.active = match step {
            Step::PlotArea => MaskId::Tick(self.plot_area.first_unset().unwrap_or(Slot::XMin)),
            _ => MaskId::Exclude,
        };
        if step >= Step::Colours {
            self.colours.restore();
        }
    }

    // --- what is selected and shown -------------------------------------------------------------------------------

    /// The size of the image, in pixels.
    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    pub fn step(&self) -> Step {
        self.step
    }

    /// The selected mask.
    pub fn active(&self) -> MaskId {
        self.active
    }

    /// The masks that exist, in the order the steps make them: the crop once there is one, the exclude mask once
    /// something is excluded, a mask for every colour that is not ignored, each tick that has a line, and each group of
    /// pieces. They are what the left panel lists (but for the groups, listed on their step), so it grows as the steps are
    /// taken.
    pub fn visible_masks(&self) -> Vec<MaskId> {
        let mut masks = Vec::new();
        if self.prepare.prepare().crop.is_some() {
            masks.push(MaskId::Crop);
        }
        if !self.prepare.prepare().mask.is_empty() {
            masks.push(MaskId::Exclude);
        }
        if let Some(map) = self.colours.colours.map().filter(|_| self.colours.current()) {
            masks.extend((0..map.inks.len()).filter(|&i| !self.colours.colours.is_ignored(i)).map(MaskId::Ink));
        }
        masks.extend(Slot::ALL.into_iter().filter(|&t| self.plot_area.tick(t).is_some()).map(MaskId::Tick));
        masks.extend(self.shape_order().into_iter().map(MaskId::Shape));
        masks
    }

    /// The masks that exist, each with its tags and properties.
    pub fn masks(&self) -> Vec<(MaskId, MaskMeta)> {
        self.visible_masks().into_iter().map(|mask| (mask, self.meta(mask))).collect()
    }

    /// The masks that exist and have every one of `tags`.
    pub fn find_masks(&self, tags: &[Tag]) -> Vec<MaskId> {
        self.masks().into_iter().filter(|(_, meta)| tags.iter().all(|&t| meta.has(t))).map(|(mask, _)| mask).collect()
    }

    /// What mask `mask` is, as tags, and what else is known about it, as properties: each step describes its own.
    pub fn meta(&self, mask: MaskId) -> MaskMeta {
        let meta = MaskMeta::new(mask_key(mask));
        match mask {
            MaskId::Crop => self.prepare.crop_meta(meta),
            MaskId::Exclude => self.prepare.exclude_meta(meta),
            MaskId::Ink(i) => self.colours.meta(i, meta),
            MaskId::Tick(slot) => self.plot_area.meta(slot, meta),
            MaskId::Shape(k) => self.shapes.meta(k, self.colours.colours.map().map(Arc::as_ref), meta),
        }
    }

    pub fn is_shown(&self, mask: MaskId) -> bool {
        match mask {
            MaskId::Crop => self.prepare.show_crop,
            MaskId::Exclude => self.prepare.show_exclude,
            MaskId::Ink(i) => self.colours.shown.contains(&i),
            MaskId::Tick(t) => self.plot_area.show_ticks[plot_area::index(t)],
            MaskId::Shape(k) => self.shapes.is_shown(k),
        }
    }

    // --- step 1 ---------------------------------------------------------------------------------------------------

    /// The crop and the exclude mask.
    pub fn prepare(&self) -> &Prepare {
        self.prepare.prepare()
    }

    /// See [`PrepareStep::mask_epoch`].
    pub fn mask_epoch(&self) -> u64 {
        self.prepare.mask_epoch()
    }

    /// The crop in whole pixels: the whole image until one is set.
    pub fn crop_pixels(&self) -> PixelRect {
        self.prepare.crop_pixels()
    }

    /// The step on screen, as a step.
    fn current(&self) -> &dyn step::WizardStep {
        match self.step {
            Step::Prepare => &self.prepare,
            Step::Colours => &self.colours,
            Step::PlotArea => &self.plot_area,
            Step::Shapes => &self.shapes,
            Step::Links => &self.links,
        }
    }

    fn current_mut(&mut self) -> &mut dyn step::WizardStep {
        match self.step {
            Step::Prepare => &mut self.prepare,
            Step::Colours => &mut self.colours,
            Step::PlotArea => &mut self.plot_area,
            Step::Shapes => &mut self.shapes,
            Step::Links => &mut self.links,
        }
    }

    /// Whether something can be undone now, on the step on screen: drawing on the exclude mask, the changes to the
    /// groups, the changes to the links. Ctrl+Z on another step must not quietly change what it was built from.
    pub fn can_undo(&self) -> bool {
        self.current().can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.current().can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let done = self.current_mut().undo();
        if done && self.step == Step::Prepare {
            self.colours_outdated();
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.current_mut().redo();
        if done && self.step == Step::Prepare {
            self.colours_outdated();
        }
        done
    }

    // --- step 2 ---------------------------------------------------------------------------------------------------

    pub fn colours(&self) -> &Colours {
        self.colours.colours()
    }

    /// Whether the colours found match the crop and the exclude mask now (they do not while those have changed since).
    pub fn inks_current(&self) -> bool {
        self.colours.current()
    }

    /// If the colours are to be analysed, the parameters to do it with (once).
    pub fn take_analysis_request(&mut self) -> Option<ColourParams> {
        self.colours.take_request()
    }

    /// The analysis was started as job `id`.
    pub fn begin_analysis(&mut self, id: u64) {
        self.colours.begin(id);
        if matches!(self.active, MaskId::Ink(_)) {
            self.active = MaskId::Exclude;
        }
    }

    /// The answer of job `id`.
    pub fn finish_analysis(&mut self, id: u64, result: Result<Arc<ColourMap>, ()>) {
        if !self.colours.finish(id, result) {
            return;
        }
        // work brought back: the colours take back what they were said to be, and what the step shown is built from
        // is looked for
        if let Some(inks) = self.restoring.take() {
            self.colours.colours.restore_roles(&inks);
            if self.step >= Step::PlotArea {
                self.plot_area.enter();
            }
            if self.step >= Step::Shapes {
                self.shapes.find();
            }
        }
    }

    /// The crop, the exclude mask or the tolerance changed: the colours are looked at again now if they are in view, else
    /// on return.
    fn colours_outdated(&mut self) {
        self.colours.outdated(self.step == Step::Colours);
    }

    /// A colour that is nothing has no mask: it is not shown, and not selected.
    fn forget_if_ignored(&mut self, ink: usize) {
        if self.colours.forget_if_ignored(ink) && self.active == MaskId::Ink(ink) {
            self.active = MaskId::Exclude;
        }
    }

    /// Whether any ink is the axes.
    pub fn has_axes(&self) -> bool {
        !self.find_masks(&[Tag::Colour, Tag::Axes]).is_empty()
    }

    /// Whether any colour is a curve.
    pub fn has_curves(&self) -> bool {
        !self.find_masks(&[Tag::Colour, Tag::Curve]).is_empty()
    }

    // --- step 3 ---------------------------------------------------------------------------------------------------

    /// How the straight lines are looked for.
    pub fn algorithm(&self) -> Algorithm {
        self.plot_area.algorithm()
    }

    /// The settings of the search for straight lines.
    pub fn line_params(&self) -> &lines::Params {
        self.plot_area.params()
    }

    /// The straight lines found in the axes colour. Empty until the plot area step is entered.
    pub fn lines(&self) -> &[Line] {
        self.plot_area.lines()
    }

    /// The line chosen as tick `tick`.
    pub fn tick(&self, tick: Slot) -> Option<Line> {
        self.plot_area.tick(tick)
    }

    /// Where tick `tick` is, in image pixels: x for an x tick, y for a y tick, to a fraction of a pixel.
    pub fn tick_position(&self, tick: Slot) -> Option<f32> {
        self.tick(tick).map(|l| l.across)
    }

    /// The plot value typed for tick `tick`.
    pub fn tick_value(&self, tick: Slot) -> Option<f64> {
        self.plot_area.value(tick)
    }

    /// Whether `axis` is logarithmic.
    pub fn is_log(&self, axis: Axis) -> bool {
        self.plot_area.is_log(axis)
    }

    /// The calibration the chosen ticks and the typed values make, in image pixels.
    pub fn calibration(&self) -> Calibration {
        self.plot_area.calibration()
    }

    /// If the lines are to be looked for and no search is running, the search to run (once).
    pub fn take_lines_request(&mut self) -> Option<LineSearch> {
        let source = self.colours.colours.axes_source();
        self.plot_area.take_request(source)
    }

    /// The search was started as job `id`.
    pub fn begin_lines(&mut self, id: u64) {
        self.plot_area.begin(id);
    }

    /// The answer of job `id`; the answer of any other is dropped.
    pub fn finish_lines(&mut self, id: u64, found: Vec<Line>) {
        self.plot_area.finish(id, found);
    }

    /// Whether the lines shown are out of date: a search is wanted or running.
    pub fn lines_busy(&self) -> bool {
        self.plot_area.busy()
    }

    /// Give the wizard the tick networks, so it can suggest the ticks (or take them away).
    pub fn set_tick_models(&mut self, models: Option<Arc<dyn TickModels>>) {
        self.plot_area.suggest.models = models;
    }

    /// Whether the ticks can be suggested: the program has the tick networks.
    pub fn can_suggest_ticks(&self) -> bool {
        self.plot_area.suggest.models.is_some()
    }

    /// Whether ticks are being suggested.
    pub fn suggesting(&self) -> bool {
        self.plot_area.suggest.busy()
    }

    /// If ticks were asked for and none are being looked for, the search to run (once).
    pub fn take_ticks_request(&mut self) -> Option<TickSearch> {
        let prepare = self.prepare.prepare().clone();
        self.plot_area.suggest.take(&prepare)
    }

    /// The search for ticks was started as job `id`.
    pub fn begin_ticks(&mut self, id: u64) {
        self.plot_area.suggest.begin(id);
    }

    /// The answer of job `id`, taken (as one change to undo) and given back; `None` for the answer of any other job,
    /// which is dropped. The tick selected is the first left without a line.
    pub fn finish_ticks(&mut self, id: u64, suggestion: Suggestion) -> Option<Suggestion> {
        if !self.plot_area.suggest.finish(id) {
            return None;
        }
        self.plot_area.take_suggestion(&suggestion);
        if self.step == Step::PlotArea {
            self.active = MaskId::Tick(self.plot_area.first_unset().unwrap_or(Slot::XMin));
        }
        Some(suggestion)
    }

    /// The search for ticks, job `id`, failed: it is no longer waited for.
    pub fn fail_ticks(&mut self, id: u64) {
        self.plot_area.suggest.finish(id);
    }

    // --- step 4 ---------------------------------------------------------------------------------------------------

    /// How the curve colours are broken into pieces.
    pub fn curve_params(&self) -> &curves::Params {
        self.shapes.params()
    }

    /// The pieces of the curve colours, once found.
    pub fn pieces(&self) -> Option<&Pieces> {
        self.shapes.pieces()
    }

    /// The shapes of the pieces, once found.
    pub fn shapes(&self) -> Option<&Shapes> {
        self.shapes.shapes()
    }

    /// The groups listed, in the order they are listed: the series of shapes (dots, dashes, markers: many small pieces),
    /// then the solid lines, the most pieces first. A group emptied by an edit is not listed.
    pub fn shape_order(&self) -> Vec<usize> {
        let Some(shapes) = self.shapes() else { return Vec::new() };
        let mut order: Vec<usize> = shapes.groups().map(|(k, _)| k).collect();
        let t = |k: usize| &shapes.types[k];
        order.sort_by_key(|&k| (t(k).solid(), std::cmp::Reverse(t(k).members.len()), k));
        order
    }

    /// The name group `k` is listed by: how its pieces look ("Dots", "Dashes", "Line"), numbered when several groups
    /// look so ("Dots 2"): those found by size, then those made by hand, so a name does not change with the edits.
    pub fn group_name(&self, k: usize) -> String {
        let Some(shapes) = self.shapes() else { return String::new() };
        let Some(form) = shapes.types.get(k).map(|t| t.form()) else { return String::new() };
        let mut same: Vec<usize> = shapes.groups().filter(|(_, t)| t.form() == form).map(|(o, _)| o).collect();
        same.sort_by_key(|&o| self.shapes.found_rank(o));
        match same.iter().position(|&o| o == k) {
            Some(0) | None => form.name().to_string(),
            Some(i) => format!("{} {}", form.name(), i + 1),
        }
    }

    /// How alike pieces must be to be copies of one shape.
    pub fn shape_params(&self) -> &plotextract_algo::shapes::Params {
        self.shapes.shape_params()
    }

    /// The pieces selected.
    pub fn selected(&self) -> &BTreeSet<usize> {
        self.shapes.selected()
    }

    /// Counts the changes to what the groups are.
    pub fn shapes_epoch(&self) -> u64 {
        self.shapes.epoch()
    }

    /// Counts the changes to how the pieces are drawn (what the groups are, which are shown, which pieces are selected),
    /// so that a picture of them knows when it is out of date.
    pub fn pieces_epoch(&self) -> u64 {
        self.shapes.drawn_epoch()
    }

    /// Whether the pieces shown are out of date: a search is wanted or running.
    pub fn pieces_busy(&self) -> bool {
        self.shapes.busy()
    }

    /// If the pieces are to be found, nothing is being searched, and the lines of the axes are known, the search to run
    /// (once). Without a curve colour there are no pieces.
    pub fn take_pieces_request(&mut self) -> Option<PieceSearch> {
        if !self.shapes.ready(self.plot_area.busy()) {
            return None;
        }
        let colours = &self.colours.colours;
        let map = colours.map().filter(|_| self.colours.current()).cloned();
        // a colour that is the axes too loses the frame, which would join every curve that touches it into one piece
        let cut: Vec<PixelRect> = self.plot_area.frame_lines(self.crop_pixels()).iter().map(Line::band).collect();
        let inks = map
            .as_ref()
            .map(|m| (0..m.inks.len()).filter(|&i| colours.has_role(i, Role::Curve)).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .map(|i| (i, if colours.has_role(i, Role::Axes) { cut.clone() } else { Vec::new() }))
            .collect();
        match self.shapes.take(map, inks) {
            shape_step::Take::Search(search) => Some(search),
            shape_step::Take::Unchanged | shape_step::Take::Cleared => None,
        }
    }

    /// The search was started as job `id`.
    pub fn begin_pieces(&mut self, id: u64) {
        self.shapes.begin(id);
    }

    /// The answer of job `id`; the answer of any other is dropped.
    pub fn finish_pieces(&mut self, id: u64, found: Found) {
        if self.shapes.finish(id, found) && self.step == Step::Links {
            self.choose_link_group();
            self.auto_link();
        }
    }

    // --- step 5 ---------------------------------------------------------------------------------------------------

    /// The group whose pieces are shown to be linked (`None` for all of them).
    pub fn link_group(&self) -> Option<usize> {
        self.links
            .group
            .filter(|&k| self.shapes().is_some_and(|s| s.types.get(k).is_some_and(|t| !t.members.is_empty())))
    }

    /// Whether piece `p` is shown to be linked: it is of the group chosen, or there is none chosen.
    pub fn linkable(&self, p: usize) -> bool {
        match self.link_group() {
            Some(k) => self.shapes().is_some_and(|s| s.type_of.get(p).copied().flatten() == Some(k)),
            None => true,
        }
    }

    /// The links, in the order their points are written, each its pieces in order.
    pub fn links(&self) -> Vec<Vec<usize>> {
        self.pieces().map(|p| self.links.resolve(p)).unwrap_or_default()
    }

    /// The link being made, if one is.
    pub fn open_link(&self) -> Option<usize> {
        self.links.open()
    }

    /// Where piece `p` is: the centre of its coverage, in image pixels.
    pub fn piece_centre(&self, p: usize) -> Option<kurbo::Point> {
        self.shapes.centre(p)
    }

    /// The points of the links in image pixels, link after link: the centre of each piece's coverage, to a fraction of a
    /// pixel.
    ///
    /// A spot (a dot, a marker) is one point; an elongated piece (a line, a dash) gives points along it, in the
    /// direction the link runs: from the end nearer the piece before it, or toward the piece after it. A link of one
    /// piece runs from left to right.
    pub fn link_points(&self) -> Vec<kurbo::Point> {
        self.link_strokes().into_iter().flatten().collect()
    }

    /// The points of each link, as [`Wizard::link_points`] gives them, one list a link.
    pub fn link_strokes(&self) -> Vec<Vec<kurbo::Point>> {
        let turned = self.links.turned();
        self.links()
            .into_iter()
            .enumerate()
            .map(|(i, link)| {
                let mut out: Vec<kurbo::Point> = Vec::new();
                for (k, &p) in link.iter().enumerate() {
                    let stroke = self.shapes.stroke(p);
                    let (Some(&first), Some(&end)) = (stroke.first(), stroke.last()) else {
                        out.extend(self.shapes.centre(p));
                        continue;
                    };
                    let next = link.get(k + 1).and_then(|&q| self.shapes.centre(q));
                    let reverse = match (out.last(), next) {
                        (Some(&l), _) => (end - l).hypot() < (first - l).hypot(),
                        (None, Some(n)) => (first - n).hypot() < (end - n).hypot(),
                        (None, None) => (first.x > end.x) != turned.get(i).copied().unwrap_or(false),
                    };
                    if reverse {
                        out.extend(stroke.iter().rev());
                    } else {
                        out.extend_from_slice(stroke);
                    }
                }
                out
            })
            .collect()
    }

    /// The points piece `p` gives when linked: along its stroke if it is elongated, else its centre.
    pub fn piece_points(&self, p: usize) -> Vec<kurbo::Point> {
        let stroke = self.shapes.stroke(p);
        if stroke.is_empty() { self.shapes.centre(p).into_iter().collect() } else { stroke.to_vec() }
    }

    /// Whether piece `p` is drawn along (its points run along it) rather than at a spot.
    pub fn is_stroke(&self, p: usize) -> bool {
        !self.shapes.stroke(p).is_empty()
    }

    /// A group of one piece needs no linking: it is linked as soon as it is chosen, unless it is linked already.
    fn auto_link(&mut self) {
        let Some(k) = self.link_group() else { return };
        let Some(&[p]) = self.shapes().and_then(|s| s.types.get(k)).map(|t| t.members.as_slice()) else { return };
        if self.links().iter().flatten().any(|&q| q == p) {
            return;
        }
        if let Some(seed) = self.seed(p) {
            self.links.end();
            self.links.link(seed);
            self.links.end();
        }
    }

    /// The points of the links in plot values, with the calibration the ticks make, link after link.
    pub fn link_rows(&self) -> Result<Vec<[f64; 2]>, ExportError> {
        self.links_series().rows(&[self.links_primitive()], &self.calibration())
    }

    /// The points of the links as CSV: an `x,y` header and a line for each point, link after link.
    pub fn links_csv(&self) -> Result<String, ExportError> {
        self.links_series().to_csv(&[self.links_primitive()], &self.calibration())
    }

    /// The points of the links as one curve, so they are written as a vector figure's series is.
    fn links_primitive(&self) -> Primitive {
        Primitive::new(PrimitiveKind::Stroke, 0, self.link_points(), Vec::new())
    }

    fn links_series(&self) -> Series {
        let mut series = Series::new("links");
        series.members.push(0);
        series
    }

    /// The group to link when the step is entered: the one chosen before, else the one most of the pieces selected on
    /// the shapes step are in, else the first listed.
    fn choose_link_group(&mut self) {
        if self.link_group().is_some() {
            return;
        }
        let Some(shapes) = self.shapes() else { return };
        let mut count = std::collections::BTreeMap::new();
        for &p in self.selected() {
            if let Some(k) = shapes.type_of.get(p).copied().flatten() {
                *count.entry(k).or_insert(0usize) += 1;
            }
        }
        let most = count.into_iter().max_by_key(|&(k, n)| (n, std::cmp::Reverse(k))).map(|(k, _)| k);
        self.links.group = most.or_else(|| self.shape_order().first().copied());
    }

    /// The pixel a link names piece `p` by.
    fn seed(&self, p: usize) -> Option<plotextract_algo::shapes::Seed> {
        self.pieces()?.pieces().get(p).map(|p| p.seed)
    }

    // --- changes ----------------------------------------------------------------------------------------------------

    pub fn apply(&mut self, intent: WizardIntent) {
        match intent {
            WizardIntent::SetTool(tool) => {
                if tool.is_none() || self.step.has_tools() {
                    self.tool = tool;
                }
            }
            WizardIntent::SetPen(pen) => self.pen = pen,
            WizardIntent::SetBrush(width) => self.brush = width.clamp(1.0, 500.0),
            WizardIntent::SetCrop(rect) => {
                if self.prepare.set_crop(rect) {
                    self.colours_outdated();
                }
            }
            WizardIntent::Exclude(op) => {
                if self.prepare.exclude(op) {
                    self.colours_outdated();
                }
            }
            WizardIntent::Delete(mask) => match mask {
                MaskId::Crop => self.apply(WizardIntent::SetCrop(None)),
                MaskId::Exclude => {
                    if self.prepare.clear() {
                        self.colours_outdated();
                    }
                }
                MaskId::Ink(ink) => self.apply(WizardIntent::IgnoreColour(ink)),
                MaskId::Tick(tick) => self.apply(WizardIntent::SetTick(tick, None)),
                // a group is not deleted: its pieces are taken out of it, and are left over
                MaskId::Shape(k) => {
                    self.shapes.dissolve(k);
                }
            },
            // a group is not a mask to select: its pieces are
            WizardIntent::SetActive(MaskId::Shape(k)) => {
                let members = self.shapes().and_then(|s| s.types.get(k)).map(|t| t.members.clone());
                self.shapes.select(&members.unwrap_or_default(), Pick::Replace);
            }
            WizardIntent::SetActive(mask) => {
                self.active = mask;
                if let MaskId::Ink(i) = mask {
                    self.colours.colours.select(Some(i));
                }
            }
            WizardIntent::Show(mask, on) => match mask {
                MaskId::Crop => self.prepare.show_crop = on,
                MaskId::Exclude => self.prepare.show_exclude = on,
                MaskId::Ink(i) => self.colours.show(i, on),
                MaskId::Tick(t) => self.plot_area.show_ticks[plot_area::index(t)] = on,
                MaskId::Shape(k) => self.shapes.show(k, on),
            },
            WizardIntent::GoTo(step) => self.go_to(step),
            WizardIntent::SetTolerance(tolerance) => {
                self.colours.colours.tolerance = tolerance.clamp(2.0, 40.0);
                self.colours_outdated();
            }
            WizardIntent::SetRole(ink, role, on) => {
                self.colours.colours.set_role(ink, role, on);
                self.forget_if_ignored(ink);
            }
            WizardIntent::IgnoreColour(ink) => {
                self.colours.colours.ignore(ink);
                self.forget_if_ignored(ink);
            }
            WizardIntent::Select(ink) => self.colours.colours.select(ink),
            WizardIntent::SetAlgorithm(algorithm) => self.plot_area.set_algorithm(algorithm),
            WizardIntent::SetLineParams(params) => self.plot_area.set_params(params),
            WizardIntent::SetTickValue(tick, value) => self.plot_area.set_value(tick, value),
            WizardIntent::SetLog(axis, on) => self.plot_area.set_log(axis, on),
            WizardIntent::SuggestTicks => self.plot_area.suggest.want(),
            WizardIntent::SetTick(tick, line) => {
                // on to the next tick that has no line yet
                if self.plot_area.set_tick(tick, line) && line.is_some() {
                    self.active = MaskId::Tick(self.plot_area.first_unset().unwrap_or(tick));
                }
            }
            WizardIntent::SetCurveParams(params) => self.shapes.set_params(params),
            WizardIntent::SetShapeParams(params) => self.shapes.set_shape_params(params),
            WizardIntent::SelectPieces(pieces, pick) => self.shapes.select(&pieces, pick),
            WizardIntent::SelectSimilar => self.shapes.select_similar(),
            WizardIntent::JoinSelected => {
                self.shapes.join_selected();
            }
            WizardIntent::RemoveSelected => {
                self.shapes.remove_selected();
            }
            WizardIntent::SetLinkGroup(group) => {
                self.links.group = group;
                self.auto_link();
            }
            WizardIntent::LinkPiece(p) => {
                if let Some(seed) = self.seed(p).filter(|_| self.linkable(p)) {
                    self.links.link(seed);
                }
            }
            WizardIntent::EndLink => self.links.end(),
            WizardIntent::LinkBack => {
                self.links.back();
            }
            WizardIntent::UnlinkPiece(p) => {
                if let Some(seed) = self.seed(p) {
                    self.links.unlink(seed);
                }
            }
            WizardIntent::MoveLink(i, up) => {
                self.links.move_link(i, up);
            }
            WizardIntent::ReverseLink(i) => {
                self.links.reverse(i);
            }
            WizardIntent::DeleteLink(i) => {
                self.links.delete(i);
            }
        }
    }

    /// Show step `step`: what it works on is looked for afresh where the steps before may have changed it, and the mask
    /// selected is one of its own.
    fn go_to(&mut self, step: Step) {
        self.step = step;
        if !step.has_tools() {
            self.tool = None;
        }
        // coming back to the colours keeps what was found and said, unless the crop or the mask changed since
        if step == Step::Colours {
            self.colours.enter();
        } else {
            self.colours.leave();
        }
        if matches!(self.active, MaskId::Tick(_)) && step != Step::PlotArea || matches!(self.active, MaskId::Shape(_)) {
            self.active = MaskId::Exclude;
        }
        match step {
            Step::PlotArea => {
                self.plot_area.enter();
                self.active = MaskId::Tick(self.plot_area.first_unset().unwrap_or(Slot::XMin));
            }
            // the colours or the lines may have changed since: the pieces and their groups are found again, with the
            // edits made again on them
            Step::Shapes => self.shapes.find(),
            // the links name their pieces by pixels: they follow them when found again
            Step::Links => {
                self.shapes.find();
                self.choose_link_group();
                self.auto_link();
            }
            Step::Prepare | Step::Colours => {}
        }
    }
}

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;
#[cfg(test)]
mod tests;
