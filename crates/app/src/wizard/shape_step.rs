//! Step 4, shapes: the curve colours broken into pieces and the shapes the pieces are (searched for together as a job),
//! the changes made to them by hand, which shape types are shown, and what a click on the image does.
//!
//! Every change made by hand (a kind said, pieces moved, types merged or made, paths cut or joined) is an [`Edit`] that
//! names pieces by a pixel of theirs; the edits are kept and made again, in order, whenever the pieces are found again, so
//! they survive a new search as long as their pieces do.

use std::collections::BTreeSet;
use std::sync::Arc;

use plotextract_raster::curves::{self, Pieces};
use plotextract_raster::shapes::{self, Edit, Group, Kind, Shapes};
use plotextract_raster::{ColourMap, MaskMeta, PixelRect, Tag, Value};

use super::intent::ShapeClick;

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
        Found { pieces, shapes }
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
}

/// The pieces that look like one just moved, offered to be moved with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub pieces: Vec<usize>,
    /// Where the piece went: a shape type, or out of its own.
    pub to: Option<usize>,
}

/// The tag of a kind of shape.
fn kind_tag(kind: Kind) -> Tag {
    match kind {
        Kind::Dot => Tag::Dot,
        Kind::Dash => Tag::Dash,
        Kind::Marker => Tag::Marker,
        Kind::Line => Tag::Line,
        Kind::Text => Tag::Text,
        Kind::Other => Tag::Other,
    }
}

#[derive(Default)]
pub struct ShapeStep {
    params: curves::Params,
    shape_params: shapes::Params,
    /// The pieces of the curve colours and their shapes, once found (with the edits made).
    pub(super) pieces: Option<Pieces>,
    pub(super) shapes: Option<Shapes>,
    /// They must be found afresh (and are not being looked for yet).
    wanted: bool,
    /// The search running on a thread, by job number.
    running: Option<u64>,
    /// The changes made by hand, in order.
    edits: Vec<Edit>,
    /// The number the next type made by hand gets.
    next_made: usize,
    /// The shape types not shown on the image.
    hidden: BTreeSet<usize>,
    /// What a click on the image does.
    pub(super) click: ShapeClick,
    /// Whether the selected type is shown alone.
    pub(super) solo: bool,
    /// The piece ringed on the image.
    pub(super) focus: Option<usize>,
    /// The pieces that look like the one just moved.
    offer: Option<Offer>,
    /// What the last search was made of (the colour map, by address, and the inks with their cuts), so that a search
    /// asked for again with nothing changed is not run again.
    searched: Option<SearchInputs>,
    /// Counts the changes to which shape types are shown.
    shown_epoch: u64,
    /// Counts the changes to what the shapes are, so that what is made of them is made afresh.
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

    pub fn shapes(&self) -> Option<&Shapes> {
        self.shapes.as_ref()
    }

    /// Counts the changes to what the shapes are.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Counts the changes to how the pieces are drawn on the shapes step: what the shapes are, and which are shown.
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

    /// The pieces that look like the one just moved, while they are offered.
    pub fn offer(&self) -> Option<&Offer> {
        self.offer.as_ref()
    }

    /// Whether piece `piece` was moved by hand to the type it is in.
    pub fn moved_by_hand(&self, piece: usize) -> bool {
        let Some(pieces) = &self.pieces else { return false };
        self.edits.iter().any(|e| matches!(*e, Edit::Move((x, y), _) if pieces.at(x, y) == Some(piece)))
    }

    /// The pieces and shapes must be found again (on entering a step that shows them, or with new settings).
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
    /// colour there is nothing to search: the pieces and shapes are none. A search asked for with the same colours, cuts
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
                self.shapes = None;
                self.searched = None;
                self.focus = None;
                self.offer = None;
                Take::Cleared
            }
        }
    }

    /// The search was started as job `id`.
    pub(super) fn begin(&mut self, id: u64) {
        self.running = Some(id);
    }

    /// The answer of job `id`, with the edits made by hand made again; `false` (and nothing taken) for the answer of any
    /// other job.
    pub(super) fn finish(&mut self, id: u64, found: Found) -> bool {
        if self.running != Some(id) {
            return false;
        }
        self.running = None;
        let before = self.shapes.take();
        let Found { pieces, mut shapes } = found;
        for edit in &self.edits {
            shapes.apply(&pieces, edit);
        }
        self.pieces = Some(pieces);
        self.shapes = Some(shapes);
        if self.shapes != before {
            self.epoch += 1;
            self.focus = None;
            self.offer = None;
            if self.shapes.as_ref().map(|s| s.types.len()) != before.map(|s| s.types.len()) {
                self.hidden.clear();
            }
        }
        true
    }

    /// Make `edit` and keep it, if it changes anything. Whether it did.
    fn edit(&mut self, edit: Edit) -> bool {
        let (Some(shapes), Some(pieces)) = (&mut self.shapes, &self.pieces) else { return false };
        if !shapes.apply(pieces, &edit) {
            return false;
        }
        self.edits.push(edit);
        self.epoch += 1;
        true
    }

    /// How an edit names type `k`.
    fn name(&self, k: usize) -> Option<Group> {
        self.shapes.as_ref()?.name_of(self.pieces.as_ref()?, k)
    }

    /// The pixel an edit names piece `p` by.
    fn seed(&self, p: usize) -> Option<(u32, u32)> {
        self.pieces.as_ref()?.pieces().get(p).map(|p| p.seed)
    }

    /// Say shape type `k` is `kind`. Whether anything changed.
    pub(super) fn set_kind(&mut self, k: usize, kind: Kind) -> bool {
        self.offer = None;
        self.name(k).is_some_and(|g| self.edit(Edit::Kind(g, kind)))
    }

    /// Put `pieces` in type `to` (those of its colour), or take them out of theirs. A single piece moved offers the pieces
    /// of the type it left that look like it.
    pub(super) fn move_pieces(&mut self, moved: &[usize], to: Option<usize>) {
        self.offer = None;
        let group = match to {
            Some(k) => match self.name(k) {
                Some(g) => Some(g),
                None => return,
            },
            None => None,
        };
        let from = moved.first().and_then(|&p| self.shapes.as_ref()?.type_of.get(p).copied());
        let mut any = false;
        for &p in moved {
            if let Some(seed) = self.seed(p) {
                any |= self.edit(Edit::Move(seed, group));
            }
        }
        if let ([piece], true, Some(from), Some(shapes)) = (moved, any, from, &self.shapes) {
            let alike = shapes.alike_in(*piece, from, self.shape_params.tolerance);
            if !alike.is_empty() {
                self.offer = Some(Offer { pieces: alike, to });
            }
        }
    }

    /// Move the pieces offered too, or not.
    pub(super) fn take_offer(&mut self, yes: bool) {
        if let Some(offer) = self.offer.take().filter(|_| yes) {
            self.move_pieces(&offer.pieces, offer.to);
            self.offer = None;
        }
    }

    /// Put all the pieces of type `from` in `into`. Whether any moved.
    pub(super) fn merge(&mut self, from: usize, into: usize) -> bool {
        self.offer = None;
        match (self.name(from), self.name(into)) {
            (Some(a), Some(b)) => self.edit(Edit::Merge(a, b)),
            _ => false,
        }
    }

    /// Make an empty type to put pieces in; its place in the list.
    pub(super) fn new_type(&mut self) -> Option<usize> {
        self.offer = None;
        self.next_made += 1;
        let n = self.next_made;
        self.edit(Edit::Make(n, Kind::Marker)).then(|| {
            let (shapes, pieces) = (self.shapes.as_ref()?, self.pieces.as_ref()?);
            shapes.type_named(pieces, Group::Made(n))
        })?
    }

    /// Cut the path between pieces `a` and `b`, or join them.
    pub(super) fn link(&mut self, a: usize, b: usize, join: bool) -> bool {
        self.offer = None;
        let (Some(sa), Some(sb)) = (self.seed(a), self.seed(b)) else { return false };
        self.edit(if join { Edit::Join(sa, sb) } else { Edit::Cut(sa, sb) })
    }

    pub(super) fn show(&mut self, k: usize, on: bool) {
        let changed = if on { self.hidden.remove(&k) } else { self.hidden.insert(k) };
        self.shown_epoch += u64::from(changed);
    }

    /// What shape type `k` is: its kind, colour, how many pieces, and for copies of one shape its size and spacing.
    pub fn meta(&self, k: usize, map: Option<&ColourMap>, meta: MaskMeta) -> MaskMeta {
        let meta = meta.tag(Tag::Shape);
        let Some(t) = self.shapes.as_ref().and_then(|s| s.types.get(k)) else { return meta };
        let mut meta = meta.tag(kind_tag(t.kind));
        if let Some(ink) = map.and_then(|m| m.inks.get(t.ink)) {
            meta = meta.with("colour", Value::Colour(ink.rgb));
        }
        meta = meta.with("pieces", Value::Count(t.members.len() as u64));
        if t.recurs {
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
