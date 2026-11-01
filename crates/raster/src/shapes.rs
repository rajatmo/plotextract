//! The shapes a curve colour is drawn with, found from its pieces alone, with nothing learned beforehand.
//!
//! What a curve mask can hold: curves drawn with a dash pattern (a solid line is one long piece; a dashed, dotted or
//! dash-dot one is many copies of one or two shapes, each a stroke as wide as the line, turned with the path and a few
//! strokes apart), marker series (many copies of one upright shape, as far apart as the data are), and what is not data
//! (letters, which differ from their neighbours and sit close together, and odd pieces). Markers glued onto the line
//! through them are taken off it first, by width.
//!
//! So each piece is described in a way that does not change when it is turned ([`Look`]), pieces that look alike are
//! grouped, and a shape that recurs is a constituent of a curve. Recurring pieces a few strokes apart are linked into
//! chains, the paths of the dash patterns; at a piece with more than two links the straightest pair is kept, so a curve
//! does not run into the dot of an `i` beside it. What recurs but does not chain is a marker; tight rows of different
//! shapes are text.

use std::collections::{BTreeMap, VecDeque};

use kurbo::Point;

use crate::curves::{Piece, Pieces};
use crate::prepare::PixelRect;

/// How many copies of a shape make it recur.
pub const RECUR: usize = 4;
/// How many pieces linked in a row make the path of a dash pattern.
pub const CHAIN_MIN: usize = 6;
/// Holes are looked for in pieces whose bounds are no larger than this, in pixels: markers and letters.
const MAX_HOLE_BOX: u64 = 40_000;

/// What a piece is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    /// A dot of a dotted (or dash-dot) curve.
    Dot,
    /// A dash of a dashed (or dash-dot) curve.
    Dash,
    /// A marker of a series of markers.
    Marker,
    /// A solid line.
    Line,
    /// A letter, a digit, a sign.
    Text,
    /// Anything else: a piece that does not recur and is not a line.
    Other,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Dot, Kind::Dash, Kind::Marker, Kind::Line, Kind::Text, Kind::Other];

    /// Whether data is drawn with it: a series is made of it.
    pub fn is_data(self) -> bool {
        matches!(self, Kind::Dot | Kind::Dash | Kind::Marker | Kind::Line)
    }
}

/// What a piece looks like, in terms that do not change when it is turned (but for [`Look::upright`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub centre: Point,
    /// Its pixels.
    pub area: f64,
    /// The sides of the rectangle with its second moments: a square of side s is s by s however it is turned.
    pub length: f64,
    pub breadth: f64,
    /// The direction of its long side, in radians.
    pub direction: f64,
    /// How much of its upright bounds it fills: 1 for an upright square, 1/2 for a diamond. Markers stand upright; the
    /// dots and dashes of a line turn with it.
    pub upright: f64,
    pub holes: u32,
    /// How thick its strokes are: twice the farthest it is from inside it to its edge.
    pub stroke: f64,
}

impl Look {
    /// How much of the rectangle of its moments it fills: 1 for a square or a bar, about 1.05 for a disc, 0.87 for a
    /// triangle, little for a ring or a cross.
    pub fn filled(&self) -> f64 {
        self.area / (self.length * self.breadth).max(1.0)
    }

    /// Breadth over length: 1 for a dot, small for a dash.
    pub fn roundness(&self) -> f64 {
        self.breadth / self.length.max(1e-9)
    }

    /// Made of thin strokes (a ring, a cross, a letter) rather than solid.
    pub fn thin(&self) -> bool {
        self.stroke <= 0.45 * self.breadth
    }

    /// A solid line: much longer than it is thick.
    pub fn is_line(&self) -> bool {
        self.length >= 30.0 && self.length >= 10.0 * self.stroke.max(1.0)
    }

    /// Half its extent along the unit direction `(ux, uy)`.
    fn reach(&self, ux: f64, uy: f64) -> f64 {
        let (c, s) = (self.direction.cos(), self.direction.sin());
        0.5 * (self.length * (ux * c + uy * s).abs() + self.breadth * (uy * c - ux * s).abs())
    }
}

/// The gap between two pieces on the line through their centres, from their looks.
fn gap(a: &Look, b: &Look) -> f64 {
    let (dx, dy) = (b.centre.x - a.centre.x, b.centre.y - a.centre.y);
    let d = dx.hypot(dy);
    if d < 1e-9 {
        return 0.0;
    }
    d - a.reach(dx / d, dy / d) - b.reach(dx / d, dy / d)
}

/// How the shapes are found.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    /// How unlike two pieces may be and still be copies of one shape ([`unlike`]): 1 by default, less to be stricter.
    pub tolerance: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self { tolerance: 1.0 }
    }
}

/// How unlike two pieces are, as a share of how unlike copies of one shape may be: below 1 they are copies (by default).
/// It is the largest of their differences in size, length, roundness and fill, each over what copies may differ by; a
/// different number of holes is 10.
pub fn unlike(a: &Look, b: &Look) -> f64 {
    let holes = if a.holes == b.holes { 0.0 } else { 10.0 };
    ((a.area / b.area).ln().abs() / 0.3)
        .max((a.length / b.length).ln().abs() / 0.25)
        .max((a.roundness() - b.roundness()).abs() / 0.22)
        .max((a.filled() - b.filled()).abs() / 0.15)
        .max(holes)
}

/// Whether two pieces are copies of one shape, as strictly as `tolerance` says.
fn alike(a: &Look, b: &Look, tolerance: f64) -> bool {
    unlike(a, b) < tolerance
}

/// Pieces of one colour that are one kind of thing: copies of one shape, or the lines, the text or the other pieces of
/// the colour.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeType {
    /// Its colour; [`NO_INK`] for a type made by hand that has no pieces yet.
    pub ink: usize,
    pub kind: Kind,
    /// Its pieces, the largest first.
    pub members: Vec<usize>,
    /// Its most typical piece (the one nearest the average of them all); [`NO_PIECE`] when it has none.
    pub typical: usize,
    /// What its typical piece looks like.
    pub look: Look,
    /// Its pieces, the most unlike the typical one first: where the mistakes of the grouping collect.
    pub odd: Vec<usize>,
    /// Whether it is copies of one shape (rather than the lines, text or other pieces of a colour).
    pub recurs: bool,
    /// The usual distance from one of its pieces to the nearest other, when it recurs.
    pub spacing: Option<f64>,
    /// The number of a type made by hand.
    pub made: Option<usize>,
    /// Whether it was changed by hand: its kind said, pieces put in or taken out.
    pub by_hand: bool,
}

/// The colour of a type that has none yet.
pub const NO_INK: usize = usize::MAX;
/// The typical piece of a type that has none.
pub const NO_PIECE: usize = usize::MAX;

impl ShapeType {
    fn new(ink: usize, kind: Kind, members: Vec<usize>, recurs: bool, looks: &[Look]) -> Self {
        let mut t = Self {
            ink,
            kind,
            members,
            typical: NO_PIECE,
            look: Look::default(),
            odd: Vec::new(),
            recurs,
            spacing: None,
            made: None,
            by_hand: false,
        };
        t.refresh(looks);
        t
    }

    /// Bring what is said of its pieces up to date with them: their order, the typical one, the odd ones, the spacing.
    fn refresh(&mut self, looks: &[Look]) {
        self.members.sort_by(|&a, &b| looks[b].area.total_cmp(&looks[a].area));
        if self.made.is_some() {
            self.recurs = matches!(self.kind, Kind::Dot | Kind::Dash | Kind::Marker) && self.members.len() >= 2;
        }
        if self.members.is_empty() {
            (self.typical, self.look, self.odd, self.spacing) = (NO_PIECE, Look::default(), Vec::new(), None);
            return;
        }
        // the piece nearest the average, in the measures copies are compared by
        let measure = |l: &Look| {
            [l.area.max(1.0).ln() / 0.3, l.length.max(1.0).ln() / 0.25, l.roundness() / 0.22, l.filled() / 0.15]
        };
        let n = self.members.len() as f64;
        let mut mean = [0.0; 4];
        for &p in &self.members {
            for (m, v) in mean.iter_mut().zip(measure(&looks[p])) {
                *m += v / n;
            }
        }
        let off = |p: usize| measure(&looks[p]).iter().zip(mean).map(|(v, m)| (v - m).powi(2)).sum::<f64>();
        self.typical = self.members.iter().copied().min_by(|&a, &b| off(a).total_cmp(&off(b))).unwrap_or(NO_PIECE);
        self.look = looks[self.typical];
        let look = self.look;
        let mut odd: Vec<(f64, usize)> = self.members.iter().map(|&p| (unlike(&looks[p], &look), p)).collect();
        odd.sort_by(|a, b| b.0.total_cmp(&a.0));
        self.odd = odd.into_iter().map(|(_, p)| p).collect();
        self.spacing = if self.recurs { spacing(&self.members, looks) } else { None };
    }
}

/// A pixel of a piece: how an edit names a piece, so that it names the same piece when the pieces are found again.
pub type Seed = (u32, u32);

/// A shape type as an edit names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// The type of the piece that has this pixel.
    Of(Seed),
    /// A type made by hand, by its number.
    Made(usize),
}

/// A change made by hand to the shapes found. The edits are kept and made again, in order, whenever the shapes are found
/// again; one that no longer applies (its piece is gone) does nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Make an empty type, numbered, of a kind.
    Make(usize, Kind),
    /// Say what kind a type is.
    Kind(Group, Kind),
    /// Put a piece in a type of its colour, or (`None`) among the other pieces of its colour.
    Move(Seed, Option<Group>),
    /// Put all the pieces of one type in another of the same colour.
    Merge(Group, Group),
    /// Cut the path of a dash pattern between two pieces that are next on it.
    Cut(Seed, Seed),
    /// Join two pieces into one path: each the end of a path, or on none.
    Join(Seed, Seed),
}

/// The shapes of the curve colours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shapes {
    /// What each piece looks like, and the type it is of.
    pub looks: Vec<Look>,
    pub type_of: Vec<usize>,
    pub types: Vec<ShapeType>,
    /// The pieces of each dash pattern's path, in order along it.
    pub chains: Vec<Vec<usize>>,
    /// The links between neighbours on those paths; a piece has at most two.
    pub links: Vec<(usize, usize)>,
}

impl Shapes {
    /// The kind of piece `piece`.
    pub fn kind(&self, piece: usize) -> Option<Kind> {
        Some(self.types.get(*self.type_of.get(piece)?)?.kind)
    }

    /// The chain piece `piece` is on.
    pub fn chain_of(&self, piece: usize) -> Option<usize> {
        self.chains.iter().position(|c| c.contains(&piece))
    }

    /// How many neighbours piece `piece` is linked to on its path: 0 on none, 1 at an end, 2 inside.
    pub fn links_at(&self, piece: usize) -> usize {
        self.links.iter().filter(|&&(a, b)| a == piece || b == piece).count()
    }

    /// The type an edit names, among the pieces `pieces`.
    pub fn type_named(&self, pieces: &Pieces, group: Group) -> Option<usize> {
        match group {
            Group::Of((x, y)) => self.type_of.get(pieces.at(x, y)?).copied(),
            Group::Made(n) => self.types.iter().position(|t| t.made == Some(n)),
        }
    }

    /// How an edit names type `k`: by its number if it was made by hand, else by a pixel of its first piece.
    pub fn name_of(&self, pieces: &Pieces, k: usize) -> Option<Group> {
        let t = self.types.get(k)?;
        match t.made {
            Some(n) => Some(Group::Made(n)),
            None => t.members.first().map(|&p| Group::Of(pieces.pieces()[p].seed)),
        }
    }

    /// The pieces of type `k` but `piece` that look like `piece` (as strictly as `tolerance` says) more than they look
    /// like the type's typical piece: those that were in the type for the same mistake as `piece`.
    pub fn alike_in(&self, piece: usize, k: usize, tolerance: f64) -> Vec<usize> {
        let (Some(t), Some(look)) = (self.types.get(k), self.looks.get(piece)) else { return Vec::new() };
        t.members
            .iter()
            .copied()
            .filter(|&q| q != piece)
            .filter(|&q| {
                let near = unlike(&self.looks[q], look);
                near < tolerance && near < unlike(&self.looks[q], &t.look)
            })
            .collect()
    }

    /// Make edit `edit` among the pieces `pieces` (those the shapes were found in). Whether it changed anything.
    pub fn apply(&mut self, pieces: &Pieces, edit: &Edit) -> bool {
        let piece = |(x, y): Seed| pieces.at(x, y).filter(|&p| p < self.looks.len());
        match *edit {
            Edit::Make(n, kind) => {
                if self.types.iter().any(|t| t.made == Some(n)) {
                    return false;
                }
                let mut t = ShapeType::new(NO_INK, kind, Vec::new(), false, &self.looks);
                (t.made, t.by_hand) = (Some(n), true);
                self.types.push(t);
                true
            }
            Edit::Kind(group, kind) => {
                let Some(k) = self.type_named(pieces, group) else { return false };
                let t = &mut self.types[k];
                if t.kind == kind {
                    return false;
                }
                (t.kind, t.by_hand) = (kind, true);
                t.refresh(&self.looks);
                true
            }
            Edit::Move(seed, to) => {
                let Some(p) = piece(seed) else { return false };
                let ink = pieces.pieces()[p].ink;
                let to = match to {
                    Some(group) => self.type_named(pieces, group),
                    None => Some(self.loose(ink)),
                };
                to.is_some_and(|to| self.move_pieces(&[p], to))
            }
            Edit::Merge(from, into) => {
                let (Some(from), Some(into)) = (self.type_named(pieces, from), self.type_named(pieces, into)) else {
                    return false;
                };
                let members = self.types[from].members.clone();
                from != into && self.move_pieces(&members, into)
            }
            Edit::Cut(a, b) => {
                let (Some(a), Some(b)) = (piece(a), piece(b)) else { return false };
                let found =
                    self.chains.iter().enumerate().find_map(|(c, chain)| {
                        chain.windows(2).position(|w| w == [a, b] || w == [b, a]).map(|i| (c, i))
                    });
                let Some((c, i)) = found else { return false };
                let tail = self.chains[c].split_off(i + 1);
                let head = self.chains.remove(c);
                self.chains.extend([head, tail].into_iter().filter(|part| part.len() >= 2));
                self.relink();
                true
            }
            Edit::Join(a, b) => {
                let (Some(a), Some(b)) = (piece(a), piece(b)) else { return false };
                let (ca, cb) = (self.chain_of(a), self.chain_of(b));
                let end = |p: usize, c: Option<usize>| {
                    c.is_none_or(|c| self.chains[c].first() == Some(&p) || self.chains[c].last() == Some(&p))
                };
                if a == b || (ca.is_some() && ca == cb) || !end(a, ca) || !end(b, cb) {
                    return false;
                }
                // a's path ending at a, then b's starting at b
                let mut take = |p: usize, c: Option<usize>, last: bool| -> Vec<usize> {
                    let Some(c) = c else { return vec![p] };
                    let mut chain = std::mem::take(&mut self.chains[c]);
                    if (chain.last() == Some(&p)) != last {
                        chain.reverse();
                    }
                    chain
                };
                let mut joined = take(a, ca, true);
                joined.extend(take(b, cb, false));
                self.chains.retain(|c| !c.is_empty());
                self.chains.push(joined);
                self.relink();
                true
            }
        }
    }

    /// Put `members` in type `to`, those of its colour (a type with no colour yet takes the first one's). Whether any
    /// moved.
    fn move_pieces(&mut self, members: &[usize], to: usize) -> bool {
        let mut touched = std::collections::BTreeSet::new();
        for &p in members {
            let from = self.type_of[p];
            let ink = self.types[from].ink;
            if from == to || (self.types[to].ink != NO_INK && self.types[to].ink != ink) {
                continue;
            }
            self.types[to].ink = ink;
            self.types[from].members.retain(|&q| q != p);
            self.types[to].members.push(p);
            self.type_of[p] = to;
            touched.extend([from, to]);
        }
        for &k in &touched {
            self.types[k].by_hand = true;
            self.types[k].refresh(&self.looks);
        }
        !touched.is_empty()
    }

    /// The type of the other pieces of colour `ink` (those that are nothing in particular), made if there is none.
    fn loose(&mut self, ink: usize) -> usize {
        let found =
            self.types.iter().position(|t| t.ink == ink && t.kind == Kind::Other && !t.recurs && t.made.is_none());
        found.unwrap_or_else(|| {
            self.types.push(ShapeType::new(ink, Kind::Other, Vec::new(), false, &self.looks));
            self.types.len() - 1
        })
    }

    /// The links, from the paths.
    fn relink(&mut self) {
        self.links = self.chains.iter().flat_map(|c| c.windows(2).map(|w| (w[0], w[1]))).collect();
    }

    /// The curves drawn with dash patterns: the chains drawn with the same shapes together, each in order along its
    /// paths. One curve is found as several chains where it folds onto itself; two chains are one curve when the shape
    /// one is mostly made of is in the other too, a few times at least (dots run together where a curve folds, so a
    /// fold can be mostly double dots that the rest of the curve has here and there).
    pub fn curves(&self) -> Vec<Vec<usize>> {
        let counts: Vec<BTreeMap<usize, usize>> = self
            .chains
            .iter()
            .map(|c| {
                let mut count = BTreeMap::new();
                for &p in c {
                    *count.entry(self.type_of[p]).or_default() += 1;
                }
                count
            })
            .collect();
        let main = |k: usize| counts[k].iter().max_by_key(|&(_, &n)| n).map(|(&t, _)| t);
        let has = |k: usize, t: usize| counts[k].get(&t).is_some_and(|&n| n >= 3.max(self.chains[k].len() / 20));
        let same = |a: usize, b: usize| main(a).is_some_and(|t| has(b, t)) || main(b).is_some_and(|t| has(a, t));
        let mut curve: Vec<usize> = (0..self.chains.len()).collect();
        fn root(curve: &mut [usize], mut k: usize) -> usize {
            while curve[k] != k {
                curve[k] = curve[curve[k]];
                k = curve[k];
            }
            k
        }
        for a in 0..self.chains.len() {
            for b in a + 1..self.chains.len() {
                if same(a, b) {
                    let (ra, rb) = (root(&mut curve, a), root(&mut curve, b));
                    curve[rb] = ra;
                }
            }
        }
        let mut grouped: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for k in 0..self.chains.len() {
            let r = root(&mut curve, k);
            grouped.entry(r).or_default().extend(self.chains[k].iter().copied());
        }
        grouped.into_values().map(|pieces| self.order(&pieces)).collect()
    }

    /// `pieces` in order along their paths: each run of linked pieces from one end to the other, the runs from left to
    /// right, and pieces linked to none placed where they lengthen the path least.
    pub fn order(&self, pieces: &[usize]) -> Vec<usize> {
        let member: BTreeMap<usize, ()> = pieces.iter().map(|&p| (p, ())).collect();
        let mut next: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for &(a, b) in &self.links {
            if member.contains_key(&a) && member.contains_key(&b) {
                next.entry(a).or_default().push(b);
                next.entry(b).or_default().push(a);
            }
        }
        let centre = |p: usize| self.looks.get(p).map_or(Point::ZERO, |l| l.centre);
        let mut seen = BTreeMap::new();
        let mut runs: Vec<Vec<usize>> = Vec::new();
        let mut alone = Vec::new();
        // the ends first, so a run is walked from one end to the other
        let mut starts: Vec<usize> =
            pieces.iter().copied().filter(|p| next.get(p).is_some_and(|n| n.len() == 1)).collect();
        starts.extend(pieces.iter().copied().filter(|p| next.get(p).is_some_and(|n| n.len() > 1)));
        for start in starts {
            if seen.contains_key(&start) {
                continue;
            }
            let mut run = Vec::new();
            let mut at = Some(start);
            while let Some(p) = at {
                seen.insert(p, ());
                run.push(p);
                at = next.get(&p).and_then(|n| n.iter().copied().find(|q| !seen.contains_key(q)));
            }
            // from left to right
            if run.len() > 1 && centre(run[0]).x > centre(*run.last().unwrap_or(&run[0])).x {
                run.reverse();
            }
            runs.push(run);
        }
        for &p in pieces {
            if !seen.contains_key(&p) && !next.contains_key(&p) {
                alone.push(p);
            }
        }
        runs.sort_by(|a, b| centre(a[0]).x.total_cmp(&centre(b[0]).x));
        let mut out: Vec<usize> = runs.concat();
        for p in alone {
            let c = centre(p);
            let cost = |k: usize| {
                let before = k.checked_sub(1).map(|k| centre(out[k]));
                let after = out.get(k).map(|&q| centre(q));
                match (before, after) {
                    (Some(a), Some(b)) => (c - a).hypot() + (b - c).hypot() - (b - a).hypot(),
                    (Some(a), None) | (None, Some(a)) => (c - a).hypot(),
                    (None, None) => 0.0,
                }
            };
            let best = (0..=out.len()).min_by(|&a, &b| cost(a).total_cmp(&cost(b))).unwrap_or(0);
            out.insert(best, p);
        }
        out
    }
}

/// Find the shapes of `pieces`, taking markers off the lines they are glued to first (which splits those pieces).
pub fn classify(pieces: &mut Pieces, params: &Params) -> Shapes {
    let dist = distances(pieces);
    let first = looks(pieces, &dist);
    let looks = if split_glued(pieces, &dist, &first) { looks(pieces, &dist) } else { first };
    let n = pieces.pieces.len();
    let ink: Vec<usize> = pieces.pieces.iter().map(|p| p.ink).collect();
    let line: Vec<bool> = looks.iter().map(Look::is_line).collect();

    // pieces that look alike, of one colour, the largest first
    let mut by_size: Vec<usize> = (0..n).filter(|&p| !line[p]).collect();
    by_size.sort_by(|&a, &b| looks[b].area.total_cmp(&looks[a].area));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for p in by_size {
        match groups.iter_mut().find(|g| ink[g[0]] == ink[p] && alike(&looks[g[0]], &looks[p], params.tolerance)) {
            Some(g) => g.push(p),
            None => groups.push(vec![p]),
        }
    }
    let groups: Vec<Vec<usize>> = groups.into_iter().flat_map(|g| split_upright(g, &looks)).collect();
    let mut group_of = vec![usize::MAX; n];
    for (k, g) in groups.iter().enumerate() {
        for &p in g {
            group_of[p] = k;
        }
    }
    let recurs = |p: usize| group_of[p] != usize::MAX && groups[group_of[p]].len() >= RECUR;

    // the paths of the dash patterns: recurring solid pieces a few strokes apart
    let near = |a: usize, b: usize| gap(&looks[a], &looks[b]) <= 2.6 * looks[a].stroke.max(looks[b].stroke) + 1.0;
    let candidates: Vec<usize> = (0..n).filter(|&p| recurs(p) && !looks[p].thin()).collect();
    let mut adjacent = link(&candidates, &looks, &ink, near);
    straighten(&mut adjacent, &looks);
    let mut chains: Vec<Vec<usize>> =
        walk(&adjacent, &candidates).into_iter().filter(|c| c.len() >= CHAIN_MIN).collect();
    let mut on_chain = vec![false; n];
    for c in &chains {
        for &p in c {
            on_chain[p] = true;
        }
    }
    // where the path doubles back or runs under something, its dots merge or are cut: take what lies just ahead of an end
    for chain in &mut chains {
        grow(chain, &looks, &ink, &line, &mut on_chain);
    }
    // where it turns sharply (the tip of a spike, a fold), one path is found as several: join ends that nearly meet
    join(&mut chains, &looks, &ink, &group_of);
    for chain in &mut chains {
        grow(chain, &looks, &ink, &line, &mut on_chain);
    }
    let mut links = Vec::new();
    for c in &chains {
        links.extend(c.windows(2).map(|w| (w[0], w[1])));
    }

    // what each piece is
    let mut kind = vec![Kind::Other; n];
    for p in 0..n {
        if line[p] {
            kind[p] = Kind::Line;
        } else if on_chain[p] {
            kind[p] = if looks[p].roundness() >= 0.6 { Kind::Dot } else { Kind::Dash };
        }
    }
    for g in groups.iter().filter(|g| g.len() >= RECUR) {
        let chained = g.iter().filter(|&&p| on_chain[p]).count();
        if 2 * chained < g.len() {
            for &p in g.iter().filter(|&&p| !on_chain[p]) {
                kind[p] = Kind::Marker;
            }
        } else {
            // a copy of a dot that is on no path is a stray: the dot of an i, a full stop
            let k = if looks[g[0]].roundness() >= 0.6 { Kind::Dot } else { Kind::Dash };
            for &p in g.iter().filter(|&&p| on_chain[p]) {
                kind[p] = k;
            }
        }
    }
    // text: rows of different shapes close together (markers are as far apart as the data, dashes alike)
    let rest: Vec<usize> = (0..n).filter(|&p| !line[p] && !on_chain[p]).collect();
    let tight = |a: usize, b: usize| gap(&looks[a], &looks[b]) <= 0.5 * looks[a].length.max(looks[b].length);
    let words = link(&rest, &looks, &ink, tight);
    for word in components(&words, &rest) {
        let shapes: std::collections::BTreeSet<usize> = word.iter().map(|&p| group_of[p]).collect();
        if word.len() >= 3 && shapes.len() >= 2 {
            for p in word {
                kind[p] = Kind::Text;
            }
        }
    }

    // the types: each recurring shape by its kind, then the lines, text and other pieces of each colour
    let mut keyed: BTreeMap<(usize, Kind, usize), Vec<usize>> = BTreeMap::new();
    for p in 0..n {
        let g = if recurs(p) && matches!(kind[p], Kind::Dot | Kind::Dash | Kind::Marker) {
            group_of[p]
        } else {
            usize::MAX
        };
        keyed.entry((ink[p], kind[p], g)).or_default().push(p);
    }
    let mut types = Vec::new();
    let mut type_of = vec![0; n];
    for ((ink, kind, g), members) in keyed {
        for &p in &members {
            type_of[p] = types.len();
        }
        types.push(ShapeType::new(ink, kind, members, g != usize::MAX, &looks));
    }
    Shapes { looks, type_of, types, chains, links }
}

/// A group split in two when its pieces are upright in two ways (squares and diamonds): the dots of a line turn with it
/// and spread evenly, markers do not.
fn split_upright(mut group: Vec<usize>, looks: &[Look]) -> Vec<Vec<usize>> {
    if group.len() < 2 * RECUR {
        return vec![group];
    }
    group.sort_by(|&a, &b| looks[a].upright.total_cmp(&looks[b].upright));
    let (k, widest) = group
        .windows(2)
        .enumerate()
        .map(|(k, w)| (k + 1, looks[w[1]].upright - looks[w[0]].upright))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((0, 0.0));
    let by_size = |mut g: Vec<usize>| {
        g.sort_by(|&a, &b| looks[b].area.total_cmp(&looks[a].area));
        g
    };
    if widest >= 0.25 && k >= RECUR && group.len() - k >= RECUR {
        let rest = group.split_off(k);
        vec![by_size(group), by_size(rest)]
    } else {
        vec![by_size(group)]
    }
}

/// Neighbours among `pieces` (of one colour) for which `close` holds, as lists by piece.
fn link(
    pieces: &[usize],
    looks: &[Look],
    ink: &[usize],
    close: impl Fn(usize, usize) -> bool,
) -> BTreeMap<usize, Vec<usize>> {
    let mut adjacent: BTreeMap<usize, Vec<usize>> = pieces.iter().map(|&p| (p, Vec::new())).collect();
    let mut by_x: Vec<usize> = pieces.to_vec();
    by_x.sort_by(|&a, &b| looks[a].centre.x.total_cmp(&looks[b].centre.x));
    // no two linked pieces are farther apart than the longest piece and the widest gap allowed
    let window = by_x.iter().map(|&p| 2.0 * looks[p].length + 3.0 * looks[p].stroke).fold(0.0, f64::max) + 2.0;
    for (k, &a) in by_x.iter().enumerate() {
        for &b in &by_x[k + 1..] {
            if looks[b].centre.x - looks[a].centre.x > window {
                break;
            }
            if ink[a] == ink[b] && (looks[b].centre.y - looks[a].centre.y).abs() <= window && close(a, b) {
                adjacent.entry(a).or_default().push(b);
                adjacent.entry(b).or_default().push(a);
            }
        }
    }
    adjacent
}

/// Keep at most two links at a piece: the pair that runs on straightest.
fn straighten(adjacent: &mut BTreeMap<usize, Vec<usize>>, looks: &[Look]) {
    loop {
        let Some((&p, near)) = adjacent.iter().find(|(_, n)| n.len() > 2) else { return };
        let c = looks[p].centre;
        let unit = |q: usize| {
            let d = looks[q].centre - c;
            d / d.hypot().max(1e-9)
        };
        let mut best = (near[0], near[1], f64::INFINITY);
        for (i, &a) in near.iter().enumerate() {
            for &b in &near[i + 1..] {
                let (ua, ub) = (unit(a), unit(b));
                // the straighter, the more the two point away from each other
                let cos = ua.x * ub.x + ua.y * ub.y;
                if cos < best.2 {
                    best = (a, b, cos);
                }
            }
        }
        let dropped: Vec<usize> = near.iter().copied().filter(|&q| q != best.0 && q != best.1).collect();
        adjacent.insert(p, vec![best.0, best.1]);
        for q in dropped {
            if let Some(list) = adjacent.get_mut(&q) {
                list.retain(|&r| r != p);
            }
        }
    }
}

/// The groups of linked pieces.
fn components(adjacent: &BTreeMap<usize, Vec<usize>>, pieces: &[usize]) -> Vec<Vec<usize>> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for &s in pieces {
        if !seen.insert(s) {
            continue;
        }
        let mut group = vec![s];
        let mut queue = VecDeque::from([s]);
        while let Some(p) = queue.pop_front() {
            for &q in adjacent.get(&p).into_iter().flatten() {
                if seen.insert(q) {
                    group.push(q);
                    queue.push_back(q);
                }
            }
        }
        out.push(group);
    }
    out
}

/// The paths of linked pieces (at most two links each), each from one end to the other.
fn walk(adjacent: &BTreeMap<usize, Vec<usize>>, pieces: &[usize]) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for group in components(adjacent, pieces) {
        let start = group.iter().copied().find(|p| adjacent.get(p).is_none_or(|n| n.len() <= 1)).unwrap_or(group[0]);
        let mut path = vec![start];
        let mut prev = usize::MAX;
        let mut at = start;
        while let Some(&next) = adjacent.get(&at).and_then(|n| n.iter().find(|&&q| q != prev && !path.contains(&q))) {
            path.push(next);
            prev = at;
            at = next;
        }
        out.push(path);
    }
    out
}

/// Lengthen `chain` at both ends by the pieces just ahead of them that are no more than a few dots big (dots run
/// together where the path doubles back, a dot cut by something drawn over it).
fn grow(chain: &mut Vec<usize>, looks: &[Look], ink: &[usize], line: &[bool], on_chain: &mut [bool]) {
    for _ in 0..2 {
        for _ in 0..6 {
            let (end, before) = (chain[chain.len() - 1], chain[chain.len() - 2]);
            let (e, b) = (&looks[end], &looks[before]);
            let ahead = e.centre - b.centre;
            let ahead = ahead / ahead.hypot().max(1e-9);
            let best = (0..looks.len())
                .filter(|&p| !on_chain[p] && !line[p] && ink[p] == ink[end])
                .filter(|&p| looks[p].breadth <= 2.5 * e.breadth && looks[p].area <= 6.0 * e.area && !looks[p].thin())
                .filter(|&p| {
                    let d = looks[p].centre - e.centre;
                    (d.x * ahead.x + d.y * ahead.y) / d.hypot().max(1e-9) >= 0.3
                })
                .map(|p| (gap(e, &looks[p]), p))
                .filter(|&(g, p)| g <= 2.6 * e.stroke.max(looks[p].stroke) + 1.0)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            match best {
                Some((_, p)) => {
                    on_chain[p] = true;
                    chain.push(p);
                }
                None => break,
            }
        }
        chain.reverse();
    }
}

/// The shapes a path is mostly made of: those of a fifth of its pieces or more.
fn signature(chain: &[usize], group_of: &[usize]) -> std::collections::BTreeSet<usize> {
    let mut count: BTreeMap<usize, usize> = BTreeMap::new();
    for &p in chain {
        *count.entry(group_of[p]).or_default() += 1;
    }
    count.into_iter().filter(|&(g, c)| g != usize::MAX && 5 * c >= chain.len()).map(|(g, _)| g).collect()
}

/// Join paths of one colour, drawn with the same shapes, whose ends nearly meet (no farther apart than three gaps of the
/// pattern), the nearest ends first, into one path.
fn join(chains: &mut Vec<Vec<usize>>, looks: &[Look], ink: &[usize], group_of: &[usize]) {
    loop {
        let mut best: Option<(f64, usize, bool, usize, bool)> = None;
        for a in 0..chains.len() {
            for b in a + 1..chains.len() {
                if signature(&chains[a], group_of).is_disjoint(&signature(&chains[b], group_of)) {
                    continue;
                }
                for (a_end, pa) in [(false, chains[a][0]), (true, chains[a][chains[a].len() - 1])] {
                    for (b_end, pb) in [(false, chains[b][0]), (true, chains[b][chains[b].len() - 1])] {
                        if ink[pa] != ink[pb] {
                            continue;
                        }
                        let g = gap(&looks[pa], &looks[pb]);
                        let reach = 3.0 * (2.6 * looks[pa].stroke.max(looks[pb].stroke) + 1.0);
                        if g <= reach && best.is_none_or(|(bg, ..)| g < bg) {
                            best = Some((g, a, a_end, b, b_end));
                        }
                    }
                }
            }
        }
        let Some((_, a, a_end, b, b_end)) = best else { return };
        let mut tail = chains.remove(b);
        let head = &mut chains[a];
        // the ends that meet go together: a's last piece, then b's piece at that end
        if !a_end {
            head.reverse();
        }
        if b_end {
            tail.reverse();
        }
        head.extend(tail);
    }
}

/// The usual distance from one of `members` to the nearest other.
fn spacing(members: &[usize], looks: &[Look]) -> Option<f64> {
    if members.len() < 2 || members.len() > 5000 {
        return None;
    }
    let mut nearest: Vec<f64> = members
        .iter()
        .map(|&a| {
            members
                .iter()
                .filter(|&&b| b != a)
                .map(|&b| (looks[b].centre - looks[a].centre).hypot())
                .fold(f64::INFINITY, f64::min)
        })
        .collect();
    nearest.sort_by(f64::total_cmp);
    Some(nearest[nearest.len() / 2])
}

/// For every pixel of the crop that is in a piece, about how far it is from the nearest pixel that is not (the edge of the
/// crop counts as outside); 0 elsewhere.
fn distances(pieces: &Pieces) -> Vec<f32> {
    let (w, h) = (pieces.crop.width as usize, pieces.crop.height as usize);
    let mut d: Vec<f32> = pieces.labels.iter().map(|&l| if l == 0 { 0.0 } else { f32::INFINITY }).collect();
    let diag = std::f32::consts::SQRT_2;
    let at = |d: &[f32], x: isize, y: isize| {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize { 0.0 } else { d[y as usize * w + x as usize] }
    };
    for y in 0..h as isize {
        for x in 0..w as isize {
            let i = y as usize * w + x as usize;
            if d[i] > 0.0 {
                let best = (at(&d, x - 1, y) + 1.0)
                    .min(at(&d, x, y - 1) + 1.0)
                    .min(at(&d, x - 1, y - 1) + diag)
                    .min(at(&d, x + 1, y - 1) + diag);
                d[i] = d[i].min(best);
            }
        }
    }
    for y in (0..h as isize).rev() {
        for x in (0..w as isize).rev() {
            let i = y as usize * w + x as usize;
            if d[i] > 0.0 {
                let best = (at(&d, x + 1, y) + 1.0)
                    .min(at(&d, x, y + 1) + 1.0)
                    .min(at(&d, x + 1, y + 1) + diag)
                    .min(at(&d, x - 1, y + 1) + diag);
                d[i] = d[i].min(best);
            }
        }
    }
    d
}

/// What every piece looks like, from one pass over the labels (the moments and the stroke of every piece at once, so a
/// long line's bounds are not gone over on their own).
fn looks(pieces: &Pieces, dist: &[f32]) -> Vec<Look> {
    let n = pieces.pieces.len();
    let c = pieces.crop;
    let w = c.width as usize;
    // per piece: pixels, sums of x, y, xx, yy, xy (about the piece's first pixel, to keep the sums small), and the
    // farthest from the edge
    let mut sums = vec![[0.0f64; 6]; n];
    let mut stroke = vec![0.0f32; n];
    for (i, &label) in pieces.labels.iter().enumerate() {
        if label == 0 {
            continue;
        }
        let k = label as usize - 1;
        let (sx, sy) = pieces.pieces[k].seed;
        let (x, y) = ((c.x + (i % w) as u32) as f64 - f64::from(sx), (c.y + (i / w) as u32) as f64 - f64::from(sy));
        let m = &mut sums[k];
        m[0] += 1.0;
        m[1] += x;
        m[2] += y;
        m[3] += x * x;
        m[4] += y * y;
        m[5] += x * y;
        stroke[k] = stroke[k].max(dist[i]);
    }
    (0..n)
        .map(|p| {
            let piece = pieces.pieces[p];
            let [count, sx, sy, sxx, syy, sxy] = sums[p];
            if count == 0.0 {
                return Look::default();
            }
            let (mx, my) = (sx / count, sy / count);
            let (vxx, vyy, vxy) =
                (sxx / count - mx * mx + 1.0 / 12.0, syy / count - my * my + 1.0 / 12.0, sxy / count - mx * my);
            let (half, root) = ((vxx + vyy) / 2.0, (((vxx - vyy) / 2.0).powi(2) + vxy * vxy).sqrt());
            let (seed_x, seed_y) = piece.seed;
            let b = piece.bounds;
            Look {
                centre: Point::new(f64::from(seed_x) + mx + 0.5, f64::from(seed_y) + my + 0.5),
                area: count,
                length: (12.0 * (half + root)).sqrt(),
                breadth: (12.0 * (half - root).max(0.0)).sqrt(),
                direction: 0.5 * (2.0 * vxy).atan2(vxx - vyy),
                upright: count / (f64::from(b.width) * f64::from(b.height)),
                holes: holes(pieces, p),
                stroke: 2.0 * f64::from(stroke[p]),
            }
        })
        .collect()
}

/// How many holes piece `p` has (of a few pixels or more): a ring has one, a hollow square one, an `e` one.
fn holes(pieces: &Pieces, p: usize) -> u32 {
    let b = pieces.pieces[p].bounds;
    if u64::from(b.width) * u64::from(b.height) > MAX_HOLE_BOX {
        return 0;
    }
    let (w, h) = (b.width as usize + 2, b.height as usize + 2);
    let label = p as u32 + 1;
    // the bounds with a border of background: 1 for the piece, 0 for background, 2 for background reached from outside
    let mut grid = vec![0u8; w * h];
    for y in 0..b.height as usize {
        for x in 0..b.width as usize {
            if pieces.index(b.x + x as u32, b.y + y as u32).is_some_and(|i| pieces.labels[i] == label) {
                grid[(y + 1) * w + x + 1] = 1;
            }
        }
    }
    let fill = |grid: &mut Vec<u8>, start: usize, mark: u8| -> usize {
        let mut queue = VecDeque::from([start]);
        grid[start] = mark;
        let mut count = 0;
        while let Some(i) = queue.pop_front() {
            count += 1;
            let (x, y) = (i % w, i / w);
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < w && ny < h && grid[ny * w + nx] == 0 {
                    grid[ny * w + nx] = mark;
                    queue.push_back(ny * w + nx);
                }
            }
        }
        count
    };
    fill(&mut grid, 0, 2);
    let mut count = 0;
    for i in 0..grid.len() {
        if grid[i] == 0 && fill(&mut grid, i, 3) >= 3 {
            count += 1;
        }
    }
    count
}

/// Take markers off the lines they are glued to: in a piece much thicker in places than its line is, what a disc a little
/// wider than the line fits in (the markers) becomes pieces of its own, and the rest stays the line. Whether any piece
/// was split. Only a long piece thicker somewhere than half again its mean width (area over length) is looked at
/// closely: a plain line or curve is not, however long.
fn split_glued(pieces: &mut Pieces, dist: &[f32], looks: &[Look]) -> bool {
    let crop = pieces.crop;
    let width = crop.width as usize;
    let count = pieces.pieces.len();
    let mut split = false;
    for (p, l) in looks.iter().enumerate().take(count) {
        let Piece { bounds: b, pixels, ink, .. } = pieces.pieces[p];
        let mean = l.area / l.length.max(1.0);
        if pixels < 300 || l.length < 6.0 * mean || l.stroke / 2.0 <= 0.75 * mean + 0.5 {
            continue;
        }
        let label = p as u32 + 1;
        let at = |x: i64, y: i64| -> f32 {
            if x < i64::from(crop.x) || y < i64::from(crop.y) {
                return 0.0;
            }
            pieces.index(x as u32, y as u32).filter(|&i| pieces.labels[i] == label).map_or(0.0, |i| dist[i])
        };
        // the half-width of the line: the usual distance to the edge along its middle
        let (mut ridge, mut top) = (Vec::new(), 0.0f32);
        for y in i64::from(b.y)..i64::from(b.y + b.height) {
            for x in i64::from(b.x)..i64::from(b.x + b.width) {
                let d = at(x, y);
                if d == 0.0 {
                    continue;
                }
                top = top.max(d);
                let peak = (-1..=1).all(|dy| (-1..=1).all(|dx| at(x + dx, y + dy) <= d));
                if peak {
                    ridge.push(d);
                }
            }
        }
        if ridge.is_empty() {
            continue;
        }
        ridge.sort_by(f32::total_cmp);
        let half = ridge[ridge.len() / 2];
        let long = f64::from(b.width.max(b.height)) >= 12.0 * f64::from(half);
        if !long || top <= 2.2 * half + 1.0 {
            continue;
        }
        // the markers: within a few pixels of where a disc wider than the line fits
        let r = half + 1.5;
        let (bw, bh) = (b.width as usize, b.height as usize);
        let local =
            |x: usize, y: usize| (b.y as usize - crop.y as usize + y) * width + (b.x as usize - crop.x as usize + x);
        let mut depth = vec![u32::MAX; bw * bh];
        let mut queue = VecDeque::new();
        for y in 0..bh {
            for x in 0..bw {
                let i = local(x, y);
                if pieces.labels[i] == label && dist[i] > r {
                    depth[y * bw + x] = 0;
                    queue.push_back((x, y));
                }
            }
        }
        let steps = r.ceil() as u32;
        while let Some((x, y)) = queue.pop_front() {
            let d = depth[y * bw + x];
            if d >= steps {
                continue;
            }
            for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                    continue;
                }
                let (nx, ny) = (nx as usize, ny as usize);
                if pieces.labels[local(nx, ny)] == label && depth[ny * bw + nx] == u32::MAX {
                    depth[ny * bw + nx] = d + 1;
                    queue.push_back((nx, ny));
                }
            }
        }
        // each blob of them a piece
        let mut blob = vec![0u32; bw * bh];
        let mut blobs: Vec<Vec<usize>> = Vec::new();
        for s in 0..bw * bh {
            if depth[s] == u32::MAX || blob[s] != 0 {
                continue;
            }
            let mut cells = vec![s];
            blob[s] = blobs.len() as u32 + 1;
            let mut k = 0;
            while k < cells.len() {
                let (x, y) = (cells[k] % bw, cells[k] / bw);
                k += 1;
                for (dx, dy) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1), (-1, -1), (1, -1), (-1, 1), (1, 1)] {
                    let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                    if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                        continue;
                    }
                    let j = ny as usize * bw + nx as usize;
                    if depth[j] != u32::MAX && blob[j] == 0 {
                        blob[j] = blob[s];
                        cells.push(j);
                    }
                }
            }
            blobs.push(cells);
        }
        let big = (2.0 * r).powi(2) as usize / 2;
        let markers: Vec<&Vec<usize>> = blobs.iter().filter(|c| c.len() >= big).collect();
        if markers.len() < 2 {
            continue;
        }
        for cells in markers {
            pieces.pieces.push(Piece {
                ink,
                bounds: PixelRect { x: 0, y: 0, width: 0, height: 0 },
                pixels: 0,
                seed: (0, 0),
            });
            let new = pieces.pieces.len() as u32;
            for &c in cells {
                pieces.labels[local(c % bw, c / bw)] = new;
            }
        }
        split = true;
    }
    if split {
        pieces.recount();
    }
    split
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mask;
    use crate::curves::{Source, find};

    const CROP: PixelRect = PixelRect { x: 0, y: 0, width: 400, height: 300 };

    /// The pieces of what `draw` puts in one colour.
    fn pieces(draw: impl Fn(&mut dyn FnMut(i64, i64))) -> Pieces {
        let mut data = vec![0u8; (CROP.width * CROP.height) as usize];
        draw(&mut |x, y| {
            if x >= 0 && y >= 0 && x < i64::from(CROP.width) && y < i64::from(CROP.height) {
                data[(y as u32 * CROP.width + x as u32) as usize] = 255;
            }
        });
        find(
            &[Source { ink: 0, mask: Mask::from_data(CROP, data), cut: Vec::new() }],
            &crate::curves::Params::default(),
        )
    }

    /// A square of side `s` centred on `(cx, cy)`, turned by `angle`.
    fn square(put: &mut dyn FnMut(i64, i64), cx: f64, cy: f64, s: f64, angle: f64) {
        let (c, si) = (angle.cos(), angle.sin());
        let r = s.ceil() as i64;
        for y in cy as i64 - r..=cy as i64 + r {
            for x in cx as i64 - r..=cx as i64 + r {
                let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                let (u, v) = (dx * c + dy * si, -dx * si + dy * c);
                if u.abs() <= s / 2.0 && v.abs() <= s / 2.0 {
                    put(x, y);
                }
            }
        }
    }

    fn ring(put: &mut dyn FnMut(i64, i64), cx: f64, cy: f64, r: f64) {
        for y in (cy - r) as i64 - 1..=(cy + r) as i64 + 1 {
            for x in (cx - r) as i64 - 1..=(cx + r) as i64 + 1 {
                let d = (x as f64 + 0.5 - cx).hypot(y as f64 + 0.5 - cy);
                if (r - 1.5..=r).contains(&d) {
                    put(x, y);
                }
            }
        }
    }

    /// Dots of side 6 every 16 pixels along a diagonal (turned with it), rings far apart, and a lone square.
    fn plot(put: &mut dyn FnMut(i64, i64)) {
        let angle = 0.5f64;
        for k in 0..14 {
            let t = 16.0 * f64::from(k);
            square(put, 20.0 + t * angle.cos(), 20.0 + t * angle.sin(), 6.0, angle);
        }
        for k in 0..6 {
            ring(put, 40.0 + 55.0 * f64::from(k), 250.0, 8.0);
        }
        square(put, 350.0, 40.0, 6.0, 0.0);
    }

    #[test]
    fn the_dots_of_a_dotted_line_chain_whatever_their_turn_and_rings_far_apart_are_markers() {
        let mut found = pieces(plot);
        let shapes = classify(&mut found, &Params::default());
        assert_eq!(shapes.chains.len(), 1, "{:?}", shapes.chains);
        assert_eq!(shapes.chains[0].len(), 14);
        let kinds = |k: Kind| (0..shapes.looks.len()).filter(|&p| shapes.kind(p) == Some(k)).count();
        assert_eq!((kinds(Kind::Dot), kinds(Kind::Marker), kinds(Kind::Other)), (14, 6, 1));
        let rings = shapes.types.iter().find(|t| t.kind == Kind::Marker).unwrap();
        assert!(rings.recurs && rings.look.holes == 1 && rings.look.thin());
        assert!((rings.spacing.unwrap() - 55.0).abs() < 1.0);
        // the chain runs from one end to the other, in order
        let xs: Vec<f64> = shapes.chains[0].iter().map(|&p| shapes.looks[p].centre.x).collect();
        assert!(xs.windows(2).all(|w| w[1] > w[0]) || xs.windows(2).all(|w| w[1] < w[0]), "{xs:?}");
        // traced as a pattern, a dot is its centre
        let points = found.trace(&shapes.order(&shapes.chains[0]), crate::curves::Trace::Pattern);
        assert_eq!(points.len(), 14);
        assert!(points.windows(2).all(|w| w[1].x > w[0].x));
    }

    #[test]
    fn a_dot_beside_the_line_is_not_taken_into_it() {
        let mut found = pieces(|put| {
            plot(put);
            // the dot of an i, just off the line between two of its dots
            let (c, s) = (0.5f64.cos(), 0.5f64.sin());
            square(put, 20.0 + 88.0 * c + 9.0 * s, 20.0 + 88.0 * s - 9.0 * c, 6.0, 0.0);
        });
        let shapes = classify(&mut found, &Params::default());
        assert_eq!(shapes.chains.len(), 1);
        assert_eq!(shapes.chains[0].len(), 14, "the dot beside the line stays out of it");
    }

    #[test]
    fn dashes_and_dots_alternating_are_one_path() {
        let mut found = pieces(|put| {
            let mut x = 10i64;
            while x < 380 {
                for dx in 0..20 {
                    for dy in 0..3 {
                        put(x + dx, 100 + dy);
                    }
                }
                for dx in 26..29 {
                    for dy in 0..3 {
                        put(x + dx, 100 + dy);
                    }
                }
                x += 35;
            }
        });
        let shapes = classify(&mut found, &Params::default());
        assert_eq!(shapes.chains.len(), 1, "{:?}", shapes.chains);
        let kinds: Vec<Kind> = shapes.chains[0].iter().filter_map(|&p| shapes.kind(p)).collect();
        assert!(kinds.windows(2).all(|w| w[0] != w[1]), "dash, dot, dash, dot: {kinds:?}");
        // a dash 20 by 3 gives a point every 3 pixels or so along it, a dot one; all in order along the line
        let points = found.trace(&shapes.order(&shapes.chains[0]), crate::curves::Trace::Pattern);
        assert_eq!(points.len(), 11 * 7 + 11, "{points:?}");
        assert!(points.windows(2).all(|w| w[1].x > w[0].x) && points.iter().all(|p| (p.y - 101.5).abs() < 1e-9));
    }

    #[test]
    fn markers_glued_onto_their_line_are_taken_off_it() {
        let mut found = pieces(|put| {
            for x in 20..380 {
                for dy in 0..2 {
                    put(x, 150 + dy);
                }
            }
            for k in 0..6 {
                square(put, 40.0 + 60.0 * f64::from(k), 151.0, 10.0, 0.0);
            }
        });
        assert_eq!(found.pieces().len(), 1, "one piece before");
        let shapes = classify(&mut found, &Params::default());
        let markers = (0..shapes.looks.len()).filter(|&p| shapes.kind(p) == Some(Kind::Marker)).count();
        let lines = (0..shapes.looks.len()).filter(|&p| shapes.kind(p) == Some(Kind::Line)).count();
        assert_eq!((markers, lines), (6, 1), "{:?}", shapes.types);
    }

    #[test]
    fn a_type_knows_its_typical_piece_and_its_oddest() {
        let mut found = pieces(|put| {
            plot(put);
            // one ring a little bigger than the others, far from them
            ring(put, 360.0, 200.0, 9.5);
        });
        let shapes = classify(&mut found, &Params::default());
        let rings = shapes.types.iter().find(|t| t.kind == Kind::Marker).unwrap();
        assert_eq!(rings.members.len(), 7);
        let areas: Vec<f64> = rings.members.iter().map(|&p| shapes.looks[p].area).collect();
        assert!(areas.windows(2).all(|w| w[0] >= w[1]), "the largest first: {areas:?}");
        let odd = rings.odd[0];
        assert!((shapes.looks[odd].centre.x - 360.0).abs() < 1.0, "the bigger ring is the oddest");
        assert_ne!(rings.typical, odd);
        assert_eq!(rings.look, shapes.looks[rings.typical]);
    }

    #[test]
    fn a_stricter_tolerance_splits_what_a_looser_one_groups() {
        let found = pieces(|put| {
            for k in 0..6 {
                ring(put, 40.0 + 55.0 * f64::from(k), 100.0, 8.0);
                ring(put, 40.0 + 55.0 * f64::from(k), 200.0, 9.0);
            }
        });
        let markers = |tolerance: f64| {
            let shapes = classify(&mut found.clone(), &Params { tolerance });
            shapes.types.iter().filter(|t| t.kind == Kind::Marker).count()
        };
        assert_eq!(markers(1.0), 1, "rings of 8 and 9 are copies");
        assert_eq!(markers(0.3), 2, "but not to a strict eye");
    }

    /// The pixel an edit names piece `p` by.
    fn seed(found: &Pieces, p: usize) -> Seed {
        found.pieces()[p].seed
    }

    #[test]
    fn pieces_are_moved_between_types_and_types_merged() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let rings = shapes.types.iter().position(|t| t.kind == Kind::Marker).unwrap();
        let stray = (0..shapes.looks.len()).find(|&p| shapes.kind(p) == Some(Kind::Other)).unwrap();
        let to_rings = Edit::Move(seed(&found, stray), shapes.name_of(&found, rings));
        assert!(shapes.apply(&found, &to_rings));
        assert_eq!(shapes.kind(stray), Some(Kind::Marker));
        assert_eq!(shapes.types[rings].members.len(), 7);
        assert!(shapes.types[rings].by_hand);
        assert!(!shapes.apply(&found, &to_rings), "already there");
        // taken out again, it is among the other pieces of its colour
        assert!(shapes.apply(&found, &Edit::Move(seed(&found, stray), None)));
        assert_eq!(shapes.kind(stray), Some(Kind::Other));

        // a type made by hand takes the pieces put in it, and a merge empties one type into another
        assert!(shapes.apply(&found, &Edit::Make(1, Kind::Dash)));
        let made = shapes.type_named(&found, Group::Made(1)).unwrap();
        assert_eq!(shapes.types[made].ink, NO_INK);
        let ring = shapes.types[rings].members[0];
        assert!(shapes.apply(&found, &Edit::Move(seed(&found, ring), Some(Group::Made(1)))));
        assert_eq!((shapes.types[made].ink, shapes.kind(ring)), (0, Some(Kind::Dash)));
        assert!(shapes.apply(&found, &Edit::Merge(shapes.name_of(&found, rings).unwrap(), Group::Made(1))));
        assert!(shapes.types[rings].members.is_empty());
        assert_eq!(shapes.types[made].members.len(), 6);
        assert!(shapes.types[made].recurs && shapes.types[made].spacing.is_some());
    }

    #[test]
    fn paths_are_cut_and_joined_by_hand() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let chain = shapes.chains[0].clone();
        let (a, b) = (chain[5], chain[6]);
        assert!(shapes.apply(&found, &Edit::Cut(seed(&found, a), seed(&found, b))));
        assert_eq!(shapes.chains.len(), 2);
        assert_eq!(shapes.links.len(), 12);
        assert_eq!((shapes.links_at(a), shapes.links_at(b)), (1, 1));
        assert!(!shapes.apply(&found, &Edit::Cut(seed(&found, a), seed(&found, b))), "no longer linked");
        // the middle of a path cannot be joined to; its ends can
        assert!(!shapes.apply(&found, &Edit::Join(seed(&found, chain[2]), seed(&found, b))));
        assert!(shapes.apply(&found, &Edit::Join(seed(&found, b), seed(&found, a))));
        assert_eq!(shapes.chains.len(), 1);
        assert_eq!(shapes.chains[0].len(), 14);
        // a lone piece joined to an end lengthens the path
        let stray = (0..shapes.looks.len()).find(|&p| shapes.kind(p) == Some(Kind::Other)).unwrap();
        let end = *shapes.chains[0].last().unwrap();
        assert!(shapes.apply(&found, &Edit::Join(seed(&found, end), seed(&found, stray))));
        assert_eq!(shapes.chains[0].len(), 15);
        assert_eq!(*shapes.chains[0].last().unwrap(), stray);
    }

    #[test]
    fn edits_made_again_on_pieces_found_again_mean_the_same() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let rings = shapes.types.iter().position(|t| t.kind == Kind::Marker).unwrap();
        let stray = (0..shapes.looks.len()).find(|&p| shapes.kind(p) == Some(Kind::Other)).unwrap();
        let edits = [
            Edit::Make(1, Kind::Marker),
            Edit::Move(seed(&found, stray), Some(Group::Made(1))),
            Edit::Kind(shapes.name_of(&found, rings).unwrap(), Kind::Text),
        ];
        for e in &edits {
            assert!(shapes.apply(&found, e));
        }
        let mut again = pieces(plot);
        let mut redone = classify(&mut again, &Params::default());
        for e in &edits {
            redone.apply(&again, e);
        }
        assert_eq!(redone, shapes);
    }

    #[test]
    fn the_look_alikes_of_a_piece_are_those_more_like_it_than_like_the_rest() {
        let mut found = pieces(|put| {
            for k in 0..6 {
                ring(put, 40.0 + 55.0 * f64::from(k), 100.0, 8.0);
            }
            for k in 0..3 {
                ring(put, 40.0 + 55.0 * f64::from(k), 200.0, 9.0);
            }
        });
        let shapes = classify(&mut found, &Params::default());
        let rings = shapes.types.iter().position(|t| t.kind == Kind::Marker).unwrap();
        assert_eq!(shapes.types[rings].members.len(), 9, "one shape");
        let big = |p: usize| shapes.looks[p].centre.y > 150.0;
        let one = shapes.types[rings].members.iter().copied().find(|&p| big(p)).unwrap();
        let alike = shapes.alike_in(one, rings, 1.0);
        assert_eq!(alike.len(), 2, "{alike:?}");
        assert!(alike.iter().all(|&p| big(p)));
    }

    #[test]
    fn letters_close_together_are_text() {
        let mut found = pieces(|put| {
            // a word: a ring, a bar, a square, a ring, a bar, close together
            ring(put, 100.0, 100.0, 7.0);
            for y in 92..108 {
                for dx in 0..3 {
                    put(112 + dx, y);
                }
            }
            square(put, 125.0, 103.0, 9.0, 0.0);
            ring(put, 140.0, 100.0, 7.0);
            for y in 92..108 {
                for dx in 0..3 {
                    put(152 + dx, y);
                }
            }
        });
        let shapes = classify(&mut found, &Params::default());
        assert!((0..shapes.looks.len()).all(|p| shapes.kind(p) == Some(Kind::Text)), "{:?}", shapes.types);
    }
}
