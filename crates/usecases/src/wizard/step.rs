//! What every step of the wizard is to the wizard: something whose hand-made changes may be undone and redone. The
//! wizard routes Ctrl+Z to the step on screen through this trait, so a step with an undo of its own (the exclude mask,
//! the groups, the links, the ticks) needs no arm in the wizard, and one without (the colours, where everything is a
//! choice made again) says so by not overriding it. Every history is a [`History`](plotextract_domain::History) of the
//! step's own state.

use super::{ColourStep, LinkStep, PlotArea, PrepareStep, ShapeStep};

pub(super) trait WizardStep {
    fn can_undo(&self) -> bool {
        false
    }

    fn can_redo(&self) -> bool {
        false
    }

    /// Undo the last change. Whether there was one.
    fn undo(&mut self) -> bool {
        false
    }

    /// Make the last change undone again. Whether there was one.
    fn redo(&mut self) -> bool {
        false
    }
}

impl WizardStep for PrepareStep {
    fn can_undo(&self) -> bool {
        PrepareStep::can_undo(self)
    }

    fn can_redo(&self) -> bool {
        PrepareStep::can_redo(self)
    }

    fn undo(&mut self) -> bool {
        PrepareStep::undo(self)
    }

    fn redo(&mut self) -> bool {
        PrepareStep::redo(self)
    }
}

impl WizardStep for ColourStep {}

impl WizardStep for PlotArea {
    fn can_undo(&self) -> bool {
        PlotArea::can_undo(self)
    }

    fn can_redo(&self) -> bool {
        PlotArea::can_redo(self)
    }

    fn undo(&mut self) -> bool {
        PlotArea::undo(self)
    }

    fn redo(&mut self) -> bool {
        PlotArea::redo(self)
    }
}

impl WizardStep for ShapeStep {
    fn can_undo(&self) -> bool {
        ShapeStep::can_undo(self)
    }

    fn can_redo(&self) -> bool {
        ShapeStep::can_redo(self)
    }

    fn undo(&mut self) -> bool {
        ShapeStep::undo(self)
    }

    fn redo(&mut self) -> bool {
        ShapeStep::redo(self)
    }
}

impl WizardStep for LinkStep {
    fn can_undo(&self) -> bool {
        LinkStep::can_undo(self)
    }

    fn can_redo(&self) -> bool {
        LinkStep::can_redo(self)
    }

    fn undo(&mut self) -> bool {
        LinkStep::undo(self)
    }

    fn redo(&mut self) -> bool {
        LinkStep::redo(self)
    }
}
