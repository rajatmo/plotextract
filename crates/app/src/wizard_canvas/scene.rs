//! What the image shows on each step, decided in one place.

use super::*;

/// How the pieces of the curves are coloured on the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PieceLayer {
    /// Not drawn.
    None,
    /// Each in its shape type's colour (the shapes step).
    ByShape,
    /// Each in its series' colour, and the pieces in no series pale so they can be clicked (the curves step).
    BySeries,
}

/// What the image shows, beside the image itself: the one place that says what is drawn on which step. A mask is drawn
/// when its eye is open, on its own step and the steps after it (it is part of what they build on), never on a step
/// before it (it would be out of date there, or not yet made); what a step chooses from (the lines found, the pieces) is
/// drawn on that step only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Scene {
    pub crop: bool,
    pub exclude: bool,
    /// The colours whose masks are tinted.
    pub inks: Vec<usize>,
    /// The straight lines found, to choose the ticks from.
    pub lines: bool,
    /// The ticks chosen.
    pub ticks: Vec<Slot>,
    pub pieces: PieceLayer,
    /// The paths of the dots and dashes, their links drawn between them, to see where they break and cut or join them.
    pub chains: bool,
    /// The points the series are traced into.
    pub points: bool,
}

/// What the image shows for `wizard` as it is.
pub(crate) fn scene(wizard: &Wizard) -> Scene {
    let step = wizard.step();
    let shown = |mask: MaskId| wizard.is_shown(mask);
    let inks = wizard
        .visible_masks()
        .into_iter()
        .filter_map(|m| if let MaskId::Ink(i) = m { Some(i) } else { None })
        .filter(|&i| step >= Step::Colours && wizard.inks_current() && shown(MaskId::Ink(i)))
        .collect();
    let ticks = Slot::ALL
        .into_iter()
        .filter(|&t| step >= Step::PlotArea && wizard.tick(t).is_some() && shown(MaskId::Tick(t)))
        .collect();
    let pieces = match step {
        Step::Shapes if wizard.shapes().is_some() => PieceLayer::ByShape,
        Step::Curves if wizard.pieces().is_some() => PieceLayer::BySeries,
        _ => PieceLayer::None,
    };
    Scene {
        crop: shown(MaskId::Crop) || wizard.tool == Some(Tool::Crop),
        exclude: shown(MaskId::Exclude),
        inks,
        lines: step == Step::PlotArea,
        ticks,
        chains: pieces == PieceLayer::ByShape,
        pieces,
        points: step == Step::Curves,
    }
}
