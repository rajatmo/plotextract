//! Step 5, links: the pieces of a curve put in the order the curve runs, by hand. A click on a piece starts a link and
//! each next click (or the pieces a drag passes through) adds to it; a double-click ends it, and the next click starts
//! another. The links are in an order of their own, changed on the panel, and the points written are the centres of the
//! pieces, link after link, each link in its own order.
//!
//! A link names its pieces by a pixel of each, so it keeps them when the pieces are found again. Every change to the
//! links can be undone and redone.

use plotextract_algo::curves::Pieces;
use plotextract_algo::shapes::Seed;
use plotextract_domain::History;

/// The links and whether one is being made.
#[derive(Clone, Debug, Default, PartialEq)]
struct Links {
    /// The links, in order, each its pieces in order.
    links: Vec<Vec<Seed>>,
    /// The pieces whose link of that one piece runs from right to left (a link of several runs as its pieces are
    /// ordered).
    turned: std::collections::BTreeSet<Seed>,
    /// The link being made: a click adds to it.
    open: Option<usize>,
}

impl Links {
    /// Drop the links left empty, keeping the open one's place right.
    fn tidy(&mut self) {
        let open = self.open.filter(|&i| self.links.get(i).is_some_and(|l| !l.is_empty()));
        let before = open.map_or(0, |i| self.links[..i].iter().filter(|l| l.is_empty()).count());
        self.links.retain(|l| !l.is_empty());
        self.open = open.map(|i| i - before);
    }

    /// Take `seed` out of every link.
    fn take(&mut self, seed: Seed) {
        for link in &mut self.links {
            link.retain(|&s| s != seed);
        }
    }
}

#[derive(Default)]
pub struct LinkStep {
    now: Links,
    history: History<Links>,
    /// The group whose pieces are shown to be linked; `None` for all.
    pub(super) group: Option<usize>,
}

impl LinkStep {
    /// Whether a link is being made, and which.
    pub fn open(&self) -> Option<usize> {
        self.now.open
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The links as pieces of `pieces`, in order: a piece no longer found is left out.
    pub fn resolve(&self, pieces: &Pieces) -> Vec<Vec<usize>> {
        self.now.links.iter().map(|l| l.iter().filter_map(|&(x, y)| pieces.at(x, y)).collect()).collect()
    }

    /// The links as they are kept: each its pieces, by a pixel of each, in order.
    pub fn seeds(&self) -> &[Vec<Seed>] {
        &self.now.links
    }

    /// The pieces whose link of that one piece is turned round.
    pub fn turned_seeds(&self) -> impl Iterator<Item = Seed> + '_ {
        self.now.turned.iter().copied()
    }

    /// Bring back saved links, with nothing to undo and none being made.
    pub(super) fn restore(&mut self, links: Vec<Vec<Seed>>, turned: Vec<Seed>, group: Option<usize>) {
        let mut now = Links { links, turned: turned.into_iter().collect(), open: None };
        now.tidy();
        *self = Self { now, history: History::default(), group };
    }

    /// For each link, whether it is one piece turned round: run from right to left.
    pub fn turned(&self) -> Vec<bool> {
        self.now.links.iter().map(|l| matches!(l.as_slice(), [only] if self.now.turned.contains(only))).collect()
    }

    /// Change the links with `f`, keeping what they were to undo it, if it changes anything. Whether it did.
    fn change(&mut self, f: impl FnOnce(&mut Links)) -> bool {
        let mut next = self.now.clone();
        f(&mut next);
        next.tidy();
        if next == self.now {
            return false;
        }
        self.history.record(std::mem::replace(&mut self.now, next));
        true
    }

    /// Add the piece named by `seed` to the link being made (taking it out of any other), or start a link with it. With
    /// none being made, a click on an end of a link goes on with that link from that end, as a pen does with an open
    /// path.
    pub(super) fn link(&mut self, seed: Seed) -> bool {
        let reopened = self.now.open.is_none() && {
            let end = self.now.links.iter().position(|l| l.last() == Some(&seed));
            let start = self.now.links.iter().position(|l| l.len() > 1 && l.first() == Some(&seed));
            match (end, start) {
                (Some(i), _) => {
                    self.now.open = Some(i);
                    true
                }
                (None, Some(i)) => {
                    // going on from its first piece: turned round, so that piece is last
                    self.change(|l| {
                        l.links[i].reverse();
                        l.open = Some(i);
                    })
                }
                (None, None) => false,
            }
        };
        if reopened {
            return true;
        }
        self.change(|l| match l.open {
            Some(i) if l.links[i].last() == Some(&seed) => {}
            Some(i) => {
                l.take(seed);
                l.links[i].push(seed);
            }
            None => {
                l.take(seed);
                l.links.push(vec![seed]);
                l.open = Some(l.links.len() - 1);
            }
        })
    }

    /// Take the last piece off the link being made.
    pub(super) fn back(&mut self) -> bool {
        self.change(|l| {
            if let Some(i) = l.open {
                l.links[i].pop();
            }
        })
    }

    /// End the link being made: the next click starts another. Not a change to undo.
    pub(super) fn end(&mut self) {
        self.now.open = None;
    }

    /// Take the piece named by `seed` out of its link.
    pub(super) fn unlink(&mut self, seed: Seed) -> bool {
        self.change(|l| l.take(seed))
    }

    /// Move link `i` one place up (towards the first) or down.
    pub(super) fn move_link(&mut self, i: usize, up: bool) -> bool {
        self.change(|l| {
            let j = if up { i.checked_sub(1) } else { Some(i + 1) };
            let Some(j) = j.filter(|&j| j < l.links.len() && i < l.links.len()) else { return };
            l.links.swap(i, j);
            l.open = l.open.map(|o| {
                if o == i {
                    j
                } else if o == j {
                    i
                } else {
                    o
                }
            });
        })
    }

    /// Turn link `i` round: its last piece first.
    pub(super) fn reverse(&mut self, i: usize) -> bool {
        self.change(|l| {
            if let Some(link) = l.links.get_mut(i) {
                if let [only] = link.as_slice() {
                    if !l.turned.remove(only) {
                        l.turned.insert(*only);
                    }
                } else {
                    link.reverse();
                }
            }
        })
    }

    /// Delete link `i`: its pieces are linked no more.
    pub(super) fn delete(&mut self, i: usize) -> bool {
        self.change(|l| {
            if let Some(link) = l.links.get_mut(i) {
                link.clear();
            }
        })
    }

    pub(super) fn undo(&mut self) -> bool {
        let Some(before) = self.history.undo(&self.now) else { return false };
        self.now = before;
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let Some(after) = self.history.redo(&self.now) else { return false };
        self.now = after;
        true
    }
}
