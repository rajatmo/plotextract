//! The tracing wizard's state, with no window in it. An image is traced in steps, each of which makes masks: the crop and
//! the shapes to exclude (step 1, [`prepare`]), one mask for every ink colour (step 2, [`colour_step`]), the four ticks the
//! axes are calibrated with and the plot values typed for them (step 3, [`plot_area`]), the shapes the curve colours are
//! drawn with (step 4, [`shape_step`]), and the series of data made of them, with stray points taken out and missed ones
//! put in by hand (step 5, [`curve_step`]). Each step's module owns its state, the changes to it and its masks' metadata;
//! [`Wizard`] holds the steps, which one is shown and which mask is selected, and routes each [`WizardIntent`] to the step
//! it belongs to. What a step reads from the steps before it is passed to it (the frame, the curve colours), so the data
//! flows forward: the lines are looked for in the colours tagged axes, the shapes in those tagged curve, a series is
//! proposed for each curve the shapes make, and it is written in plot values with the calibration the ticks make.
//!
//! Only drawing the exclude mask can be undone (Ctrl+Z and Ctrl+Shift+Z, on step 1): it is the one thing done with a hand
//! that slips. Everything else is a choice that is simply made again, or a mask that is deleted from its row; a point put
//! in or taken out by hand is clicked again to undo it.

mod colour_step;
mod curve_step;
mod intent;
pub mod masks;
mod plot_area;
mod prepare;
mod shape_step;

use std::sync::Arc;

use kurbo::Point;
use plotextract_core::{Axis, Calibration, ExportError, Slot};
use plotextract_raster::curves::{self, Pieces, Trace};
use plotextract_raster::lines::{self, Algorithm, Line};
use plotextract_raster::shapes::{Kind, Shapes};
use plotextract_raster::{ColourMap, ColourParams, MaskMeta, PixelRect, Prepare, Tag};

pub use self::colour_step::ColourStep;
pub use self::curve_step::{CurveSeries, CurveStep};
pub use self::intent::{Pen, ShapeClick, Step, Tool, WizardIntent};
pub use self::masks::{MaskId, mask_key};
pub use self::plot_area::{LineSearch, PlotArea, on_x_axis, orientation_of};
pub use self::prepare::PrepareStep;
pub use self::shape_step::{Found, Offer, PieceSearch, ShapeStep};
pub use crate::colours::{Colours, Role};

/// Whether `inner` lies within `outer`.
fn within(outer: PixelRect, inner: PixelRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

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
    curves: CurveStep,
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
            curves: CurveStep::default(),
        }
    }

    /// Step 5, for what shows it.
    pub fn curve_step(&self) -> &CurveStep {
        &self.curves
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
    /// something is excluded, a mask for every colour that is not ignored, each tick that has a line, each shape type, and
    /// each series that has pieces. They are what the left panel lists, so it grows as the steps are taken.
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
        masks.extend(self.curves.series().iter().filter(|s| s.exists()).map(|s| MaskId::Series(s.number())));
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
            MaskId::Series(number) => self.curves.meta(
                number,
                self.shapes.pieces(),
                self.colours.colours.map().map(Arc::as_ref),
                &self.calibration(),
                meta,
            ),
        }
    }

    pub fn is_shown(&self, mask: MaskId) -> bool {
        match mask {
            MaskId::Crop => self.prepare.show_crop,
            MaskId::Exclude => self.prepare.show_exclude,
            MaskId::Ink(i) => self.colours.shown.contains(&i),
            MaskId::Tick(t) => self.plot_area.show_ticks[plot_area::index(t)],
            MaskId::Shape(k) => self.shapes.is_shown(k),
            MaskId::Series(number) => self.curves.find(number).is_some_and(|s| s.shown),
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

    /// Whether drawing on the exclude mask can be undone now: only on the first step, where it is drawn.
    pub fn can_undo(&self) -> bool {
        self.step == Step::Prepare && self.prepare.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.step == Step::Prepare && self.prepare.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let done = self.can_undo() && self.prepare.undo();
        if done {
            self.colours_outdated();
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.can_redo() && self.prepare.redo();
        if done {
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
        self.colours.finish(id, result);
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

    /// The part of the image inside the frame; `None` without one.
    #[cfg(test)]
    fn frame(&self) -> Option<kurbo::Rect> {
        self.plot_area.frame(self.crop_pixels())
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

    /// The shape type selected, if one is.
    pub fn active_shape(&self) -> Option<usize> {
        match self.active {
            MaskId::Shape(k) if self.shapes().is_some_and(|s| k < s.types.len()) => Some(k),
            _ => None,
        }
    }

    /// The shape types listed, in the order they are listed: by what they are (dots, dashes, markers, lines, text,
    /// other), the most pieces first. A type with no pieces (one made by hand, or emptied into another) is listed only
    /// while it is selected.
    pub fn shape_order(&self) -> Vec<usize> {
        let Some(shapes) = self.shapes() else { return Vec::new() };
        let selected = if let MaskId::Shape(k) = self.active { Some(k) } else { None };
        let mut order: Vec<usize> =
            (0..shapes.types.len()).filter(|&k| !shapes.types[k].members.is_empty() || selected == Some(k)).collect();
        order.sort_by_key(|&k| (shapes.types[k].kind, std::cmp::Reverse(shapes.types[k].members.len()), k));
        order
    }

    /// The first shape type listed.
    fn first_shape(&self) -> Option<MaskId> {
        self.shape_order().first().map(|&k| MaskId::Shape(k))
    }

    /// How alike pieces must be to be copies of one shape.
    pub fn shape_params(&self) -> &plotextract_raster::shapes::Params {
        self.shapes.shape_params()
    }

    /// What a click on the image does on the shapes step.
    pub fn shape_click(&self) -> ShapeClick {
        self.shapes.click
    }

    /// Whether the selected shape type is shown alone.
    pub fn solo(&self) -> bool {
        self.shapes.solo
    }

    /// The shape type shown alone, the others faint: the one selected, while solo is on.
    pub fn solo_shape(&self) -> Option<usize> {
        self.active_shape().filter(|_| self.shapes.solo)
    }

    /// The piece ringed on the image.
    pub fn focus_piece(&self) -> Option<usize> {
        self.shapes.focus.filter(|&p| self.pieces().is_some_and(|ps| p < ps.pieces().len()))
    }

    /// The pieces that look like the one just moved, while they are offered to be moved too.
    pub fn shape_offer(&self) -> Option<&Offer> {
        self.shapes.offer()
    }

    /// Whether piece `piece` was moved by hand to the shape type it is in.
    pub fn moved_by_hand(&self, piece: usize) -> bool {
        self.shapes.moved_by_hand(piece)
    }

    /// Counts the changes to what the shapes are.
    pub fn shapes_epoch(&self) -> u64 {
        self.shapes.epoch()
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
            shape_step::Take::Unchanged => None,
            shape_step::Take::Cleared => {
                self.curves.retrace(None, None);
                None
            }
        }
    }

    /// The search was started as job `id`.
    pub fn begin_pieces(&mut self, id: u64) {
        self.shapes.begin(id);
    }

    /// The answer of job `id`; the answer of any other is dropped. On the curves step the series are proposed for what
    /// the shapes make.
    pub fn finish_pieces(&mut self, id: u64, found: Found) {
        if !self.shapes.finish(id, found) {
            return;
        }
        if self.step == Step::Curves {
            self.propose_series();
        }
        self.curves.retrace(self.shapes.pieces(), self.shapes.shapes());
        match self.step {
            Step::Curves if self.active_series().is_none() => {
                if let Some(first) = self.curves.series().first() {
                    self.active = MaskId::Series(first.number());
                }
            }
            Step::Shapes if self.active_shape().is_none() => {
                if let Some(first) = self.first_shape() {
                    self.active = first;
                }
            }
            _ => {}
        }
    }

    // --- step 5 ---------------------------------------------------------------------------------------------------

    /// The series, in the order they were made.
    pub fn series(&self) -> &[CurveSeries] {
        self.curves.series()
    }

    /// The series a click on the image adds pieces to: the one selected.
    pub fn active_series(&self) -> Option<usize> {
        match self.active {
            MaskId::Series(number) => self.curves.find(number).map(|s| s.number()),
            _ => None,
        }
    }

    /// The series piece `piece` is in, if any.
    pub fn series_of(&self, piece: usize) -> Option<usize> {
        self.curves.series_of(piece)
    }

    /// Whether a click on the curves step puts points in and takes them out, rather than pieces.
    pub fn edit_points(&self) -> bool {
        self.curves.edit_points()
    }

    /// Counts the changes to how the pieces are drawn (what the shapes are and which are shown, which series has which
    /// pieces and which are shown), so that a picture of them knows when it is out of date. Points put in or taken out do
    /// not count: they are drawn on their own.
    pub fn curves_epoch(&self) -> u64 {
        self.shapes.drawn_epoch() + self.curves.epoch()
    }

    /// Series `number` in plot values, with the calibration the ticks make, from left to right.
    pub fn series_rows(&self, number: usize) -> Result<Vec<[f64; 2]>, ExportError> {
        self.curves.rows(number, &self.calibration())
    }

    /// Series `number` as CSV: an `x,y` header and a line for each point.
    pub fn series_csv(&self, number: usize) -> Result<String, ExportError> {
        self.curves.csv(number, &self.calibration())
    }

    /// Propose the series of each curve colour's shapes, of the pieces inside the frame and not on its ticks (for a
    /// colour that is the axes too).
    fn propose_series(&mut self) {
        let crop = self.crop_pixels();
        let (frame, ticks) = (self.plot_area.frame(crop), self.plot_area.tick_bands(crop));
        let (Some(pieces), Some(shapes), Some(map)) =
            (self.shapes.pieces(), self.shapes.shapes(), self.colours.colours.map())
        else {
            return;
        };
        let colours = &self.colours.colours;
        let keep = |p: usize| {
            let Some(piece) = pieces.pieces().get(p) else { return false };
            let centre = shapes.looks.get(p).map_or(Point::ZERO, |l| l.centre);
            frame.is_none_or(|f| f.contains(centre))
                && !(colours.has_role(piece.ink, Role::Axes) && ticks.iter().any(|&band| within(band, piece.bounds)))
        };
        self.curves.propose(pieces, shapes, map, self.shapes.epoch(), keep);
        if self.active_series().is_none() && matches!(self.active, MaskId::Series(_)) {
            self.active = self.curves.series().first().map_or(MaskId::Exclude, |s| MaskId::Series(s.number()));
        }
    }

    // --- changes ----------------------------------------------------------------------------------------------------

    pub fn apply(&mut self, intent: WizardIntent) {
        let (pieces, shapes) = (self.shapes.pieces.as_ref(), self.shapes.shapes.as_ref());
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
                // a shape type is not deleted but said to be nothing: its pieces are in no series proposed
                MaskId::Shape(k) => self.apply(WizardIntent::SetKind(k, Kind::Other)),
                MaskId::Series(number) => {
                    self.curves.delete(number);
                    if self.active == mask {
                        self.active =
                            self.curves.series().first().map_or(MaskId::Exclude, |s| MaskId::Series(s.number()));
                    }
                }
            },
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
                MaskId::Series(number) => self.curves.show(number, on),
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
            WizardIntent::SetTick(tick, line) => {
                // on to the next tick that has no line yet
                if self.plot_area.set_tick(tick, line) && line.is_some() {
                    self.active = MaskId::Tick(self.plot_area.first_unset().unwrap_or(tick));
                }
            }
            WizardIntent::SetCurveParams(params) => self.shapes.set_params(params),
            WizardIntent::SetShapeParams(params) => self.shapes.set_shape_params(params),
            WizardIntent::SetKind(k, kind) => {
                self.shapes.set_kind(k, kind);
            }
            WizardIntent::SetShapeClick(click) => self.shapes.click = click,
            WizardIntent::MovePieces(moved, to) => self.shapes.move_pieces(&moved, to),
            WizardIntent::LookAlikes(yes) => self.shapes.take_offer(yes),
            WizardIntent::MergeShape(from, into) => {
                if self.shapes.merge(from, into) {
                    self.active = MaskId::Shape(into);
                }
            }
            WizardIntent::NewShape => {
                if let Some(k) = self.shapes.new_type() {
                    self.active = MaskId::Shape(k);
                    self.shapes.click = ShapeClick::Move;
                }
            }
            WizardIntent::CutLink(a, b) => {
                self.shapes.link(a, b, false);
            }
            WizardIntent::JoinPieces(a, b) => {
                self.shapes.link(a, b, true);
            }
            WizardIntent::SetSolo(on) => self.shapes.solo = on,
            WizardIntent::FocusPiece(piece) => self.shapes.focus = piece,
            WizardIntent::NewSeries(piece) => {
                let trace = match piece.and_then(|p| shapes?.kind(p)) {
                    Some(Kind::Dot | Kind::Dash) => Trace::Pattern,
                    Some(Kind::Marker) => Trace::Markers,
                    _ => Trace::Line,
                };
                let number = self.curves.add(trace);
                self.active = MaskId::Series(number);
                if let Some(piece) = piece {
                    self.apply(WizardIntent::SetPiece(piece, Some(number)));
                }
            }
            WizardIntent::SetPiece(piece, series) => {
                if let Some(pieces) = pieces {
                    self.curves.set_piece(pieces, shapes, piece, series);
                }
            }
            WizardIntent::SetTrace(number, trace) => self.curves.set_trace(number, trace, pieces, shapes),
            WizardIntent::SetEditPoints(on) => self.curves.edit_points = on,
            WizardIntent::AddPoint(number, p) => self.curves.add_point(number, p, pieces, shapes),
            WizardIntent::RemovePoint(number, p) => self.curves.remove_point(number, p, pieces, shapes),
            WizardIntent::RestorePoint(number, p) => self.curves.restore_point(number, p, pieces, shapes),
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
        if matches!(self.active, MaskId::Tick(_)) && step != Step::PlotArea
            || matches!(self.active, MaskId::Shape(_)) && step != Step::Shapes
            || matches!(self.active, MaskId::Series(_)) && step != Step::Curves
        {
            self.active = MaskId::Exclude;
        }
        match step {
            Step::PlotArea => {
                self.plot_area.enter();
                self.active = MaskId::Tick(self.plot_area.first_unset().unwrap_or(Slot::XMin));
            }
            // the colours or the lines may have changed since: the pieces and their shapes are found again, and the
            // series follow them by their pixels
            Step::Shapes => {
                if let Some(first) = self.first_shape() {
                    self.active = first;
                }
                self.shapes.find();
            }
            Step::Curves => {
                self.propose_series();
                self.curves.retrace(self.shapes.pieces(), self.shapes.shapes());
                if let Some(first) = self.curves.series().first() {
                    self.active = MaskId::Series(first.number());
                }
                self.shapes.find();
            }
            Step::Prepare | Step::Colours => {}
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
