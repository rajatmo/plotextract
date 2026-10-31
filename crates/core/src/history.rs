//! Undo and redo for any cloneable state.

/// Snapshots of past and undone states.
#[derive(Clone, Debug)]
pub struct History<S> {
    undo: Vec<S>,
    redo: Vec<S>,
    limit: usize,
}

impl<S: Clone> History<S> {
    /// An empty history that remembers at most `limit` steps.
    pub fn new(limit: usize) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), limit }
    }

    /// Remember the state from before a change. A new change ends whatever could have been redone.
    pub fn record(&mut self, before: S) {
        self.undo.push(before);
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// The state before the last change, given the current one (which becomes redoable); `None` with nothing to undo.
    pub fn undo(&mut self, current: &S) -> Option<S> {
        let previous = self.undo.pop()?;
        self.redo.push(current.clone());
        Some(previous)
    }

    /// The state that was undone last, given the current one (which becomes undoable again).
    pub fn redo(&mut self, current: &S) -> Option<S> {
        let next = self.redo.pop()?;
        self.undo.push(current.clone());
        Some(next)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_and_redo_walk_back_and_forth() {
        let mut h = History::new(10);
        let mut state = 0;
        for next in [1, 2, 3] {
            h.record(state);
            state = next;
        }
        assert_eq!(h.undo(&state), Some(2));
        state = 2;
        assert_eq!(h.undo(&state), Some(1));
        state = 1;
        assert_eq!(h.redo(&state), Some(2));
        assert!(h.can_undo() && h.can_redo());
    }

    #[test]
    fn a_new_change_ends_the_redo_chain() {
        let mut h = History::new(10);
        h.record(0);
        assert_eq!(h.undo(&1), Some(0));
        assert!(h.can_redo());
        h.record(0);
        assert!(!h.can_redo());
        assert_eq!(h.redo(&5), None);
    }

    #[test]
    fn only_the_last_steps_are_kept() {
        let mut h = History::new(3);
        for s in 0..10 {
            h.record(s);
        }
        let mut seen = Vec::new();
        let mut state = 10;
        while let Some(prev) = h.undo(&state) {
            seen.push(prev);
            state = prev;
        }
        assert_eq!(seen, [9, 8, 7]);
        assert!(!h.can_undo());
        h.clear();
        assert!(!h.can_redo());
    }

    #[test]
    fn nothing_to_undo_changes_nothing() {
        let mut h: History<i32> = History::new(3);
        assert_eq!(h.undo(&1), None);
        assert!(!h.can_redo());
    }
}
