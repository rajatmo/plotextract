//! Step 4, shapes: the curve colours broken into pieces and grouped by how alike they look (searched for together as a
//! job), and the groups fixed by hand. That is all the step does.
//!
//! The user selects pieces (a whole group at a time, or one by one, or by drawing around them) and does one of two things
//! with them: makes them one group, or takes them out of their groups (they are left over). Each is an [`Edit`] that names
//! its pieces by a pixel of each; the edits are kept, made again in order whenever the pieces are found again, and can be
//! undone and redone.

use std::collections::BTreeSet;
use std::sync::Arc;

use plotextract_algo::ColourMap;
use plotextract_algo::curves::{self, Pieces};
use plotextract_algo::shapes::{self, Edit, Seed, Shapes};
use plotextract_domain::{History, MaskMeta, PixelRect, Tag, Value};

use super::intent::Pick;

/// A search for the pieces of the curve colours and their shapes, to be run on a thread: [`PieceSearch::run`].
pub struct PieceSearch {
    map: Arc<ColourMap>,
    /// The curve inks, each with the parts of the image to cut out of it.
    inks: Vec<(usize, Vec<PixelRect>)>,
    params: curves::Params,
    shape_params: shapes::Params,
}

impl PieceSearch {
    /// The slow part: make the mask of each curve colour, break it into pieces and find their shapes. Safe to call on any
    /// thread.
    pub fn run(self) -> Found {
        let sources: Vec<curves::Source> =
            self.inks.into_iter().map(|(ink, cut)| curves::Source { ink, mask: self.map.mask_of(ink), cut }).collect();
        let mut pieces = curves::find(&sources, &self.params);
        let shapes = shapes::classify(&mut pieces, &self.shape_params);
        let centres = pieces.centres();
        // the elongated pieces (lines, dashes) are followed along their stroke, a point every other pixel
        let strokes = shapes
            .looks
            .iter()
            .enumerate()
            .map(|(p, look)| if elongated(look) { pieces.stroke(p, 2.0) } else { Vec::new() })
            .collect();
        Found { pieces, shapes, centres, strokes }
    }
}

/// What a search is made of: the colour map (by address), each curve ink with the parts cut out of it, the settings.
type SearchInputs = (usize, Vec<(usize, Vec<PixelRect>)>, curves::Params, shapes::Params);

/// What asking for a search gives.
pub(super) enum Take {
    /// The search to run.
    Search(PieceSearch),
    /// Nothing changed since the last one: the pieces stay.
    Unchanged,
    /// There is nothing to search (no curve colour): the pieces are gone.
    Cleared,
}

/// The pieces of the curve colours, and the shapes they are.
pub struct Found {
    pub pieces: Pieces,
    pub shapes: Shapes,
    /// Where each piece is, found on the thread so the window does not wait for it.
    pub centres: Vec<Option<kurbo::Point>>,
    /// The points along each elongated piece, from one end to the other; none for the others.
    pub strokes: Vec<Vec<kurbo::Point>>,
}

/// Whether a piece is drawn along (a line, a dash) rather than at a spot (a dot, a marker): its points run along it.
///
/// It is judged by how long its stroke is against how thick: the length drawn (pixels over thickness) at least three
/// thicknesses, so a curve that bends across the plot is elongated however wide its bounds. Small marks of thin strokes
/// (crosses, letters) and marks with a hole (rings) are spots.
pub fn elongated(look: &shapes::Look) -> bool {
    let stroke = look.stroke.max(1.0);
    let drawn = look.area / stroke;
    look.length >= 6.0 && drawn >= 3.0 * stroke && !(look.length < 30.0 && (look.thin() || look.holes > 0))
}

#[derive(Default)]
pub struct ShapeStep {
    params: curves::Params,
    shape_params: shapes::Params,
    /// The pieces of the curve colours and their groups, once found (with the edits made).
    pub(super) pieces: Option<Pieces>,
    /// Where each piece is: the centre of its coverage, found once with the pieces.
    centres: Vec<Option<kurbo::Point>>,
    /// The points along each elongated piece, found with the pieces.
    strokes: Vec<Vec<kurbo::Point>>,
    pub(super) shapes: Option<Shapes>,
    /// The groups as they were found, before any edit: undoing makes the edits again on them.
    found: Option<Shapes>,
    /// They must be found afresh (and are not being looked for yet).
    wanted: bool,
    /// The search running on a thread, by job number.
    running: Option<u64>,
    /// The changes made by hand, in order, and the logs before each change, to undo it.
    edits: Vec<Edit>,
    history: History<Vec<Edit>>,
    /// The groups not shown on the image.
    hidden: BTreeSet<usize>,
    /// The pieces selected.
    selected: BTreeSet<usize>,
    /// What the last search was made of (the colour map, by address, and the inks with their cuts), so that a search
    /// asked for again with nothing changed is not run again.
    searched: Option<SearchInputs>,
    /// Counts the changes to which groups are shown, and to which pieces are selected.
    shown_epoch: u64,
    /// Counts the changes to what the groups are, so that what is made of them is made afresh.
    epoch: u64,
}

impl ShapeStep {
    /// How the curve colours are broken into pieces.
    pub fn params(&self) -> &curves::Params {
        &self.params
    }

    /// How alike pieces must be to be copies of one shape.
    pub fn shape_params(&self) -> &shapes::Params {
        &self.shape_params
    }

    pub fn pieces(&self) -> Option<&Pieces> {
        self.pieces.as_ref()
    }

    /// Where piece `p` is: the centre of its coverage, to a fraction of a pixel.
    pub fn centre(&self, p: usize) -> Option<kurbo::Point> {
        self.centres.get(p).copied().flatten()
    }

    /// The points along piece `p` from one end to the other, if it is elongated; empty if it is a spot.
    pub fn stroke(&self, p: usize) -> &[kurbo::Point] {
        self.strokes.get(p).map_or(&[], Vec::as_slice)
    }

    pub fn shapes(&self) -> Option<&Shapes> {
        self.shapes.as_ref()
    }

    /// Counts the changes to what the groups are.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Counts the changes to how the pieces are drawn: what the groups are, which are shown, which pieces are selected.
    pub fn drawn_epoch(&self) -> u64 {
        self.epoch + self.shown_epoch
    }

    /// Whether the pieces shown are out of date: a search is wanted or running.
    pub fn busy(&self) -> bool {
        self.wanted || self.running.is_some()
    }

    pub fn is_shown(&self, k: usize) -> bool {
        !self.hidden.contains(&k)
    }

    /// The pieces selected.
    pub fn selected(&self) -> &BTreeSet<usize> {
        &self.selected
    }

    /// Where group `k` comes in the order the groups are numbered in: those found by size, the most pieces first (as they
    /// are first listed), then those made by hand, in the order they were made. It does not change with the edits.
    pub fn found_rank(&self, k: usize) -> (bool, std::cmp::Reverse<usize>, usize) {
        match self.found.as_ref().and_then(|f| f.types.get(k)) {
            Some(t) => (false, std::cmp::Reverse(t.members.len()), k),
            None => (true, std::cmp::Reverse(0), k),
        }
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo() && self.shapes.is_some()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo() && self.shapes.is_some()
    }

    /// The changes made by hand, in order.
    pub fn edits(&self) -> &[Edit] {
        &self.edits
    }

    /// Bring back saved settings and changes, with nothing to undo. They are made on the pieces when they are found.
    pub(super) fn restore(&mut self, params: curves::Params, shape_params: shapes::Params, edits: Vec<Edit>) {
        self.params = params;
        self.shape_params = shape_params;
        self.edits = edits;
        self.history = History::default();
    }

    /// The pieces and groups must be found again (on entering the step, or with new settings).
    pub(super) fn find(&mut self) {
        self.wanted = true;
    }

    pub(super) fn set_params(&mut self, params: curves::Params) {
        if params != self.params {
            self.params = params;
            self.wanted = true;
        }
    }

    pub(super) fn set_shape_params(&mut self, params: shapes::Params) {
        if params != self.shape_params {
            self.shape_params = params;
            self.wanted = true;
        }
    }

    /// Whether a search should start now: one is wanted, none is running, and the lines of the axes it cuts out are
    /// known (`lines_busy` is false).
    pub(super) fn ready(&self, lines_busy: bool) -> bool {
        self.wanted && self.running.is_none() && !lines_busy
    }

    /// The search to run, of `inks` of `map` (once: asking again gives nothing until the next change). Without a curve
    /// colour there is nothing to search: the pieces and groups are none. A search asked for with the same colours, cuts
    /// and settings as the one before (whose answer is here) is not run: the pieces are what they were.
    pub(super) fn take(&mut self, map: Option<Arc<ColourMap>>, inks: Vec<(usize, Vec<PixelRect>)>) -> Take {
        self.wanted = false;
        match map.filter(|_| !inks.is_empty()) {
            Some(map) => {
                let inputs = (Arc::as_ptr(&map) as usize, inks.clone(), self.params.clone(), self.shape_params.clone());
                if self.pieces.is_some() && self.searched.as_ref() == Some(&inputs) {
                    return Take::Unchanged;
                }
                self.searched = Some(inputs);
                let (params, shape_params) = (self.params.clone(), self.shape_params.clone());
                Take::Search(PieceSearch { map, inks, params, shape_params })
            }
            None => {
                self.pieces = None;
                self.centres.clear();
                self.strokes.clear();
                self.shapes = None;
                self.found = None;
                self.searched = None;
                self.selected.clear();
                Take::Cleared
            }
        }
    }

    /// The search was started as job `id`.
    pub(super) fn begin(&mut self, id: u64) {
        self.running = Some(id);
    }

    /// The answer of job `id`, with the edits made by hand made again; `false` (and nothing taken) for the answer of any
    /// other job. The pieces selected stay selected, if they are found again.
    pub(super) fn finish(&mut self, id: u64, found: Found) -> bool {
        if self.running != Some(id) {
            return false;
        }
        self.running = None;
        let selected = self.seeds(&self.selected.iter().copied().collect::<Vec<_>>());
        let before = self.shapes.take();
        let Found { pieces, shapes, centres, strokes } = found;
        self.found = Some(shapes.clone());
        let mut shapes = shapes;
        for edit in &self.edits {
            shapes.apply(&pieces, edit);
        }
        self.selected = selected.iter().filter_map(|&(x, y)| pieces.at(x, y)).collect();
        self.centres = centres;
        self.strokes = strokes;
        self.pieces = Some(pieces);
        self.shapes = Some(shapes);
        if self.shapes != before {
            self.epoch += 1;
            if self.shapes.as_ref().map(|s| s.types.len()) != before.map(|s| s.types.len()) {
                self.hidden.clear();
            }
        }
        self.shown_epoch += 1;
        true
    }

    /// The pixels edits name pieces `ps` by.
    fn seeds(&self, ps: &[usize]) -> Vec<Seed> {
        let Some(pieces) = &self.pieces else { return Vec::new() };
        ps.iter().filter_map(|&p| pieces.pieces().get(p)).map(|p| p.seed).collect()
    }

    /// Make `edit` and keep it, if it changes anything. Whether it did.
    fn edit(&mut self, edit: Edit) -> bool {
        let (Some(shapes), Some(pieces)) = (&mut self.shapes, &self.pieces) else { return false };
        if !shapes.apply(pieces, &edit) {
            return false;
        }
        self.history.record(self.edits.clone());
        self.edits.push(edit);
        self.epoch += 1;
        true
    }

    /// The groups as found, with the edits made again on them.
    fn replay(&mut self) {
        let (Some(found), Some(pieces)) = (&self.found, &self.pieces) else { return };
        let mut shapes = found.clone();
        for edit in &self.edits {
            shapes.apply(pieces, edit);
        }
        self.shapes = Some(shapes);
        self.epoch += 1;
    }

    /// Undo the last edit. Whether there was one.
    pub(super) fn undo(&mut self) -> bool {
        if self.shapes.is_none() {
            return false;
        }
        let Some(before) = self.history.undo(&self.edits) else { return false };
        self.edits = before;
        self.replay();
        true
    }

    /// Make the last edit undone again. Whether there was one.
    pub(super) fn redo(&mut self) -> bool {
        if self.shapes.is_none() {
            return false;
        }
        let Some(after) = self.history.redo(&self.edits) else { return false };
        self.edits = after;
        self.replay();
        true
    }

    /// Change which pieces are selected.
    pub(super) fn select(&mut self, pieces: &[usize], pick: Pick) {
        let n = self.pieces.as_ref().map_or(0, |p| p.pieces().len());
        let given: BTreeSet<usize> = pieces.iter().copied().filter(|&p| p < n).collect();
        let before = self.selected.clone();
        match pick {
            Pick::Replace => self.selected = given,
            Pick::Add => self.selected.extend(given),
            Pick::Subtract => self.selected.retain(|p| !given.contains(p)),
            Pick::Toggle => {
                if !given.is_empty() && given.is_subset(&self.selected) {
                    self.selected.retain(|p| !given.contains(p));
                } else {
                    self.selected.extend(given);
                }
            }
        }
        if self.selected != before {
            self.shown_epoch += 1;
        }
    }

    /// Add the pieces that look like one of those selected.
    pub(super) fn select_similar(&mut self) {
        let Some(shapes) = &self.shapes else { return };
        let selected: Vec<usize> = self.selected.iter().copied().collect();
        let similar = shapes.similar(&selected, self.shape_params.tolerance);
        self.select(&similar, Pick::Add);
    }

    /// Make the pieces selected one group. Whether anything changed.
    pub(super) fn join_selected(&mut self) -> bool {
        let seeds = self.seeds(&self.selected.iter().copied().collect::<Vec<_>>());
        !seeds.is_empty() && self.edit(Edit::Join(seeds))
    }

    /// Take the pieces selected out of their groups. Whether anything changed.
    pub(super) fn remove_selected(&mut self) -> bool {
        let seeds = self.seeds(&self.selected.iter().copied().collect::<Vec<_>>());
        !seeds.is_empty() && self.edit(Edit::Remove(seeds))
    }

    /// Take all the pieces of group `k` out of it: they are left over. Whether any moved.
    pub(super) fn dissolve(&mut self, k: usize) -> bool {
        let members = self.shapes.as_ref().and_then(|s| s.types.get(k)).map(|t| t.members.clone()).unwrap_or_default();
        let seeds = self.seeds(&members);
        !seeds.is_empty() && self.edit(Edit::Remove(seeds))
    }

    pub(super) fn show(&mut self, k: usize, on: bool) {
        let changed = if on { self.hidden.remove(&k) } else { self.hidden.insert(k) };
        self.shown_epoch += u64::from(changed);
    }

    /// What group `k` is: a solid line or a series of shapes, its colour, how many pieces, and for copies of one shape
    /// their size and spacing.
    pub fn meta(&self, k: usize, map: Option<&ColourMap>, meta: MaskMeta) -> MaskMeta {
        let meta = meta.tag(Tag::Shape);
        let Some(t) = self.shapes.as_ref().and_then(|s| s.types.get(k)) else { return meta };
        let mut meta = if t.solid() { meta.tag(Tag::Line) } else { meta };
        if let Some(ink) = map.and_then(|m| m.inks.get(t.ink)) {
            meta = meta.with("colour", Value::Colour(ink.rgb));
        }
        meta = meta.with("pieces", Value::Count(t.members.len() as u64));
        if !t.solid() && !t.members.is_empty() {
            let l = &t.look;
            meta = meta
                .with("length", Value::Pixels(l.length))
                .with("breadth", Value::Pixels(l.breadth))
                .with("stroke", Value::Pixels(l.stroke));
            if l.holes > 0 {
                meta = meta.with("holes", Value::Count(u64::from(l.holes)));
            }
            if let Some(spacing) = t.spacing {
                meta = meta.with("spacing", Value::Pixels(spacing));
            }
        }
        if t.by_hand {
            meta = meta.tag(Tag::ByHand);
        }
        meta
    }
}
