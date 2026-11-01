//! The shapes a colour is drawn with: its pieces grouped by how alike they look, by measure alone. Nothing here says
//! what a group is (the dots of a curve, its markers, a letter); that is the user's to see, and to fix.
//!
//! Each piece is described in a way that does not change when it is turned ([`Look`]). Markers glued onto the line
//! through them are taken off it first, by width. Then a drawing is either a solid line (one long stroke: a group of its
//! own) or a series of shapes (many pieces that look alike: dots, dashes, markers; one group). Pieces that are neither,
//! the few that look like nothing else, are left over and belong to no group.
//!
//! The groups found are fixed by hand with two changes ([`Edit`]): some pieces made one group (two groups that are one
//! drawing, or pieces gathered into a group of their own), and some pieces taken out of their groups (they are left
//! over). An edit names its pieces by a pixel of each, so it can be made again on the pieces found again.

use std::collections::{BTreeSet, VecDeque};

use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::curves::{Piece, Pieces};
use crate::prepare::PixelRect;

/// How many pieces that look alike make a series of shapes; fewer are left over.
pub const RECUR: usize = 4;
/// Holes are looked for in pieces whose bounds are no larger than this, in pixels: markers and letters.
const MAX_HOLE_BOX: u64 = 40_000;

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

    /// What it looks like, in a word, by measure: a line, rings, hollow or open marks, dots, filled marks or dashes. It
    /// says how a piece is drawn, not what it means.
    pub fn form(&self) -> Form {
        if self.is_line() {
            Form::Line
        } else if self.holes > 0 {
            if self.roundness() >= 0.7 { Form::Rings } else { Form::Hollow }
        } else if self.thin() {
            Form::Open
        } else if self.roundness() >= 0.6 {
            if self.length <= 16.0 { Form::Dots } else { Form::Filled }
        } else {
            Form::Dashes
        }
    }
}

/// How a piece is drawn, in a word ([`Look::form`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Form {
    /// A long stroke.
    Line,
    /// Small round pieces.
    Dots,
    /// Pieces much longer than they are wide.
    Dashes,
    /// Round pieces with a hole: circles drawn as outlines.
    Rings,
    /// Other pieces with a hole: squares, triangles, diamonds drawn as outlines.
    Hollow,
    /// Pieces of thin strokes and no hole: crosses, pluses, letters.
    Open,
    /// Larger solid pieces: filled markers.
    Filled,
}

impl Form {
    /// Its name, for a group of pieces drawn so.
    pub fn name(self) -> &'static str {
        match self {
            Form::Line => "Line",
            Form::Dots => "Dots",
            Form::Dashes => "Dashes",
            Form::Rings => "Rings",
            Form::Hollow => "Hollow marks",
            Form::Open => "Open marks",
            Form::Filled => "Filled marks",
        }
    }
}

/// How the shapes are found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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

/// A group of pieces: one drawing, found as copies of one shape or as one long stroke, or put together by hand.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeType {
    /// The colour most of its pieces are drawn in.
    pub ink: usize,
    /// Its pieces, the largest first. Empty once all have been taken out or put elsewhere: it is gone.
    pub members: Vec<usize>,
    /// Its most typical piece (the one nearest the average of them all); [`NO_PIECE`] when it has none.
    pub typical: usize,
    /// What its typical piece looks like.
    pub look: Look,
    /// Its pieces, the most unlike the typical one first: where the mistakes of the grouping collect.
    pub odd: Vec<usize>,
    /// The usual distance from one of its pieces to the nearest other, when it has [`RECUR`] pieces or more.
    pub spacing: Option<f64>,
    /// Whether it was made or changed by hand.
    pub by_hand: bool,
}

/// The typical piece of a group that has none.
pub const NO_PIECE: usize = usize::MAX;

impl ShapeType {
    fn new(members: Vec<usize>, looks: &[Look], ink: &[usize]) -> Self {
        let mut t = Self {
            ink: 0,
            members,
            typical: NO_PIECE,
            look: Look::default(),
            odd: Vec::new(),
            spacing: None,
            by_hand: false,
        };
        t.refresh(looks, ink);
        t
    }

    /// Whether it is a solid line (its typical piece a long stroke), rather than a series of shapes.
    pub fn solid(&self) -> bool {
        !self.members.is_empty() && self.look.is_line()
    }

    /// How it is drawn, in a word: as its typical piece is.
    pub fn form(&self) -> Form {
        self.look.form()
    }

    /// Bring what is said of its pieces up to date with them: their colour, their order, the typical one, the odd ones,
    /// the spacing.
    fn refresh(&mut self, looks: &[Look], ink: &[usize]) {
        self.members.sort_by(|&a, &b| looks[b].area.total_cmp(&looks[a].area).then(a.cmp(&b)));
        if self.members.is_empty() {
            (self.typical, self.look, self.odd, self.spacing) = (NO_PIECE, Look::default(), Vec::new(), None);
            return;
        }
        let mut count = std::collections::BTreeMap::new();
        for &p in &self.members {
            *count.entry(ink[p]).or_insert(0usize) += 1;
        }
        self.ink = count.into_iter().max_by_key(|&(i, n)| (n, std::cmp::Reverse(i))).map_or(0, |(i, _)| i);
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
        self.spacing = if self.members.len() >= RECUR { spacing(&self.members, looks) } else { None };
    }
}

/// A pixel of a piece: how an edit names a piece, so that it names the same piece when the pieces are found again.
pub type Seed = (u32, u32);

/// A change made by hand to the groups found. The edits are kept and made again, in order, whenever the groups are found
/// again; the pieces an edit names that are gone are skipped.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Edit {
    /// The pieces become one group, and nothing else is in it: they leave the groups they were in. A group all of whose
    /// pieces are among them is the one they become (the largest such), so a group keeps its place when others are
    /// merged into it.
    Join(Vec<Seed>),
    /// The pieces are taken out of their groups: they are left over.
    Remove(Vec<Seed>),
}

/// The groups of the pieces of the curve colours.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shapes {
    /// What each piece looks like, and the group it is in (none if it is left over).
    pub looks: Vec<Look>,
    pub type_of: Vec<Option<usize>>,
    /// The groups, by number. A group emptied by an edit stays, empty, so the numbers of the others do not change.
    pub types: Vec<ShapeType>,
    /// The colour of each piece.
    pub inks: Vec<usize>,
}

impl Shapes {
    /// The pieces in no group, the largest first.
    pub fn leftovers(&self) -> Vec<usize> {
        let mut out: Vec<usize> = (0..self.type_of.len()).filter(|&p| self.type_of[p].is_none()).collect();
        out.sort_by(|&a, &b| self.looks[b].area.total_cmp(&self.looks[a].area).then(a.cmp(&b)));
        out
    }

    /// The groups that have pieces, by number.
    pub fn groups(&self) -> impl Iterator<Item = (usize, &ShapeType)> {
        self.types.iter().enumerate().filter(|(_, t)| !t.members.is_empty())
    }

    /// The pieces not among `selected` that look like one of them, as strictly as `tolerance` says.
    pub fn similar(&self, selected: &[usize], tolerance: f64) -> Vec<usize> {
        let chosen: BTreeSet<usize> = selected.iter().copied().filter(|&p| p < self.looks.len()).collect();
        // a few of each look are enough to compare with: the selection may be thousands of copies of one shape
        let mut samples: Vec<Look> = Vec::new();
        for &p in &chosen {
            let l = self.looks[p];
            if !samples.iter().any(|s| unlike(s, &l) < 0.25 * tolerance) {
                samples.push(l);
            }
        }
        (0..self.looks.len())
            .filter(|p| !chosen.contains(p))
            .filter(|&p| samples.iter().any(|s| alike(s, &self.looks[p], tolerance)))
            .collect()
    }

    /// Make edit `edit` among the pieces `pieces` (those the groups were found in). Whether it changed anything.
    pub fn apply(&mut self, pieces: &Pieces, edit: &Edit) -> bool {
        let named = |seeds: &[Seed]| -> Vec<usize> {
            let set: BTreeSet<usize> =
                seeds.iter().filter_map(|&(x, y)| pieces.at(x, y)).filter(|&p| p < self.looks.len()).collect();
            set.into_iter().collect()
        };
        match edit {
            Edit::Join(seeds) => {
                let chosen = named(seeds);
                if chosen.is_empty() {
                    return false;
                }
                let set: BTreeSet<usize> = chosen.iter().copied().collect();
                // the group they become: the largest one wholly among them, else a new one
                let whole = self
                    .groups()
                    .filter(|(_, t)| t.members.iter().all(|p| set.contains(p)))
                    .max_by_key(|(k, t)| (t.members.len(), std::cmp::Reverse(*k)))
                    .map(|(k, _)| k);
                if let Some(k) = whole
                    && self.types[k].members.len() == set.len()
                {
                    return false;
                }
                let to = whole.unwrap_or_else(|| {
                    self.types.push(ShapeType::new(Vec::new(), &self.looks, &self.inks));
                    self.types.len() - 1
                });
                self.put(&chosen, Some(to))
            }
            Edit::Remove(seeds) => {
                let chosen = named(seeds);
                self.put(&chosen, None)
            }
        }
    }

    /// Put `members` in group `to`, or in none. Whether any moved.
    fn put(&mut self, members: &[usize], to: Option<usize>) -> bool {
        let mut touched = BTreeSet::new();
        for &p in members {
            let from = self.type_of[p];
            if from == to {
                continue;
            }
            if let Some(k) = from {
                self.types[k].members.retain(|&q| q != p);
                touched.insert(k);
            }
            if let Some(k) = to {
                self.types[k].members.push(p);
                touched.insert(k);
            }
            self.type_of[p] = to;
        }
        for &k in &touched {
            self.types[k].by_hand = true;
            self.types[k].refresh(&self.looks, &self.inks);
        }
        !touched.is_empty()
    }
}

/// Find the groups of `pieces`, taking markers off the lines they are glued to first (which splits those pieces): each
/// long stroke a group of its own, each set of [`RECUR`] or more pieces of one colour that look alike a group, and the
/// rest left over.
pub fn classify(pieces: &mut Pieces, params: &Params) -> Shapes {
    let dist = distances(pieces);
    let first = looks(pieces, &dist);
    let looks = if split_glued(pieces, &dist, &first) { looks(pieces, &dist) } else { first };
    let n = pieces.pieces.len();
    let inks: Vec<usize> = pieces.pieces.iter().map(|p| p.ink).collect();

    // pieces that look alike, of one colour, the largest first; a line is one drawing however like another it is
    let mut by_size: Vec<usize> = (0..n).collect();
    by_size.sort_by(|&a, &b| looks[b].area.total_cmp(&looks[a].area));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for p in by_size {
        let line = looks[p].is_line();
        let found = if line {
            None
        } else {
            groups.iter_mut().find(|g| {
                inks[g[0]] == inks[p] && !looks[g[0]].is_line() && alike(&looks[g[0]], &looks[p], params.tolerance)
            })
        };
        match found {
            Some(g) => g.push(p),
            None => groups.push(vec![p]),
        }
    }
    let groups = groups.into_iter().flat_map(|g| split_upright(g, &looks));
    let mut types = Vec::new();
    let mut type_of = vec![None; n];
    for members in groups.filter(|g| looks[g[0]].is_line() || g.len() >= RECUR) {
        for &p in &members {
            type_of[p] = Some(types.len());
        }
        types.push(ShapeType::new(members, &looks, &inks));
    }
    Shapes { looks, type_of, types, inks }
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
    use crate::curves::{Source, find};
    use crate::mask::Mask;

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

    /// The piece nearest to `(x, y)`.
    fn piece_at(shapes: &Shapes, x: f64, y: f64) -> usize {
        (0..shapes.looks.len())
            .min_by(|&a, &b| {
                let d = |p: usize| (shapes.looks[p].centre - Point::new(x, y)).hypot();
                d(a).total_cmp(&d(b))
            })
            .unwrap()
    }

    /// The group of the rings (the pieces with a hole).
    fn rings_of(shapes: &Shapes) -> usize {
        shapes.groups().find(|(_, t)| t.look.holes == 1).unwrap().0
    }

    /// The pixels an edit names pieces `ps` by.
    fn seeds(found: &Pieces, ps: &[usize]) -> Vec<Seed> {
        ps.iter().map(|&p| found.pieces()[p].seed).collect()
    }

    #[test]
    fn alike_pieces_are_one_group_whatever_their_turn_and_they_are_named_by_how_they_look() {
        let mut found = pieces(plot);
        let shapes = classify(&mut found, &Params::default());
        let rings = &shapes.types[rings_of(&shapes)];
        assert_eq!(rings.members.len(), 6);
        assert_eq!(rings.form(), Form::Rings);
        assert!((rings.spacing.unwrap() - 55.0).abs() < 1.0);
        // the lone square is as big as the dots: it is put with them, and only a person can say it is not one
        let dots = shapes.type_of[piece_at(&shapes, 20.0, 20.0)].unwrap();
        assert_eq!(shapes.types[dots].members.len(), 15);
        assert_eq!(shapes.type_of[piece_at(&shapes, 350.0, 40.0)], Some(dots));
        assert_eq!(shapes.types[dots].form(), Form::Dots);
        assert!(shapes.leftovers().is_empty());
    }

    #[test]
    fn pieces_like_too_few_others_are_left_over_and_each_line_is_a_group_of_its_own() {
        let mut found = pieces(|put| {
            ring(put, 100.0, 100.0, 7.0);
            for y in 92..108 {
                for dx in 0..3 {
                    put(112 + dx, y);
                }
            }
            square(put, 125.0, 103.0, 9.0, 0.0);
            // two lines alike in every way
            for x in 20..380 {
                put(x, 200);
                put(x, 250);
            }
        });
        let shapes = classify(&mut found, &Params::default());
        assert_eq!(shapes.leftovers().len(), 3, "{:?}", shapes.types);
        let lines: Vec<&ShapeType> = shapes.groups().map(|(_, t)| t).collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|t| t.solid() && t.members.len() == 1 && t.form() == Form::Line));
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
        let squares = shapes.types.iter().find(|t| t.members.len() == 6);
        let lines = shapes.looks.iter().filter(|l| l.is_line()).count();
        assert!(squares.is_some() && lines == 1, "{:?}", shapes.types);
    }

    #[test]
    fn a_group_knows_its_typical_piece_and_its_oddest() {
        let mut found = pieces(|put| {
            plot(put);
            // one ring a little bigger than the others, far from them
            ring(put, 360.0, 200.0, 9.5);
        });
        let shapes = classify(&mut found, &Params::default());
        let rings = &shapes.types[rings_of(&shapes)];
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
        let ring_groups = |tolerance: f64| {
            let shapes = classify(&mut found.clone(), &Params { tolerance });
            shapes.groups().filter(|(_, t)| t.look.holes == 1).count()
        };
        assert_eq!(ring_groups(1.0), 1, "rings of 8 and 9 are copies");
        assert_eq!(ring_groups(0.3), 2, "but not to a strict eye");
    }

    #[test]
    fn two_groups_made_one_keep_the_larger_ones_number_and_pieces_made_one_are_a_new_group() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let rings = rings_of(&shapes);
        let dots = shapes.type_of[piece_at(&shapes, 20.0, 20.0)].unwrap();
        // the dots and the rings, all of them: one group, the dots' (the larger)
        let mut both = shapes.types[dots].members.clone();
        both.extend(&shapes.types[rings].members);
        assert!(shapes.apply(&found, &Edit::Join(seeds(&found, &both))));
        assert_eq!(shapes.types[dots].members.len(), 21);
        assert!(shapes.types[rings].members.is_empty() && shapes.types[dots].by_hand);
        assert!(!shapes.apply(&found, &Edit::Join(seeds(&found, &both))), "already one group");
        // three of its pieces made one: a new group of just those, the rest stay
        let three = shapes.types[dots].members[..3].to_vec();
        assert!(shapes.apply(&found, &Edit::Join(seeds(&found, &three))));
        let new = shapes.types.len() - 1;
        assert_eq!(shapes.types[new].members.len(), 3);
        assert_eq!(shapes.types[dots].members.len(), 18);
        assert!(three.iter().all(|&p| shapes.type_of[p] == Some(new)));
    }

    #[test]
    fn pieces_taken_out_are_left_over_and_can_be_put_back() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let stray = piece_at(&shapes, 350.0, 40.0);
        let dots = shapes.type_of[stray].unwrap();
        assert!(shapes.apply(&found, &Edit::Remove(seeds(&found, &[stray]))));
        assert_eq!(shapes.type_of[stray], None);
        assert_eq!(shapes.leftovers(), vec![stray]);
        assert_eq!(shapes.types[dots].members.len(), 14);
        assert!(!shapes.apply(&found, &Edit::Remove(seeds(&found, &[stray]))), "already left over");
        // a leftover made one with a whole group goes into it
        let mut back = shapes.types[dots].members.clone();
        back.push(stray);
        assert!(shapes.apply(&found, &Edit::Join(seeds(&found, &back))));
        assert_eq!(shapes.type_of[stray], Some(dots));
    }

    #[test]
    fn edits_made_again_on_pieces_found_again_mean_the_same() {
        let mut found = pieces(plot);
        let mut shapes = classify(&mut found, &Params::default());
        let rings = rings_of(&shapes);
        let stray = piece_at(&shapes, 350.0, 40.0);
        let mut joined = shapes.types[rings].members.clone();
        joined.push(stray);
        let edits = [Edit::Remove(seeds(&found, &[stray])), Edit::Join(seeds(&found, &joined))];
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
    fn the_pieces_similar_to_a_selection_are_those_like_one_of_it() {
        let mut found = pieces(|put| {
            for k in 0..6 {
                ring(put, 40.0 + 55.0 * f64::from(k), 100.0, 8.0);
                square(put, 40.0 + 55.0 * f64::from(k), 200.0, 6.0, 0.0);
            }
        });
        let shapes = classify(&mut found, &Params::default());
        let rings = rings_of(&shapes);
        let one = shapes.types[rings].members[0];
        let similar = shapes.similar(&[one], 1.0);
        assert_eq!(similar.len(), 5, "{similar:?}");
        assert!(similar.iter().all(|&p| shapes.type_of[p] == Some(rings)));
    }
}
