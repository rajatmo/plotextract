//! What the wizard is told: the steps, the tools, and every change the user can make ([`WizardIntent`]).

use kurbo::Rect;
use plotextract_algo::curves;
use plotextract_algo::lines::{self, Algorithm, Line};
use plotextract_algo::shapes;
use plotextract_domain::MaskOp;
use plotextract_domain::{Axis, Slot};

use super::{MaskId, Role};

/// The wizard's steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Step {
    /// Crop the image to the plot and mask what is not part of it.
    Prepare,
    /// Find the ink colours and say what each one is.
    Colours,
    /// Find the straight lines of the axes and choose the four ticks the calibration is made from.
    PlotArea,
    /// Break the curve colours into pieces, group them by how alike they look, and fix the groups by hand.
    Shapes,
    /// Link the pieces of a curve in the order it runs, by hand, and write their points.
    Links,
}

impl Step {
    /// Whether the step has tools to draw with (only the first does: the later steps choose with the pointer).
    pub fn has_tools(self) -> bool {
        self == Step::Prepare
    }

    /// The step's name, with its number.
    pub fn title(self) -> &'static str {
        match self {
            Step::Prepare => "Step 1 \u{b7} Crop and mask",
            Step::Colours => "Step 2 \u{b7} Colours",
            Step::PlotArea => "Step 3 \u{b7} Plot area",
            Step::Shapes => "Step 4 \u{b7} Shapes",
            Step::Links => "Step 5 \u{b7} Link",
        }
    }

    /// What the step is for, in a line.
    pub fn about(self) -> &'static str {
        match self {
            Step::Prepare => "Crop to the plot; paint over anything to ignore.",
            Step::Colours => "Say what each colour is.",
            Step::PlotArea => "Pick four ticks and type their values.",
            Step::Shapes => "Join what is one curve; remove what is not a curve.",
            Step::Links => "Click a curve's pieces in order; double-click to end.",
        }
    }
}

/// What the pointer draws on the image. With no tool, it moves the image (and on the later steps lights what it is on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Drag out the crop, or drag its handles.
    Crop,
    /// Draw on the exclude mask with the pen chosen; with Alt held, erase from it.
    Exclude,
}

/// What the exclude tool draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pen {
    /// A stroke as wide as the brush.
    Brush,
    /// A rectangle dragged out.
    Rectangle,
    /// A polygon clicked corner by corner; double-click or Enter closes it.
    Polygon,
}

/// How a choice of pieces changes the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// They are the selection.
    Replace,
    /// They are added to it.
    Add,
    /// They are taken out of it.
    Subtract,
    /// They are taken out of it if all are in it, else added.
    Toggle,
}

/// A change the user makes in the wizard.
#[derive(Clone, Debug)]
pub enum WizardIntent {
    /// Choose a tool, or none to move the image.
    SetTool(Option<Tool>),
    /// What the exclude tool draws.
    SetPen(Pen),
    /// The brush width, in image pixels.
    SetBrush(f64),
    /// Set the crop (made valid first), or `None` for the whole image.
    SetCrop(Option<Rect>),
    /// Add a shape to the exclude mask (painted on it, or erased from it). Undoable.
    Exclude(MaskOp),
    /// Delete a mask: take the crop away, empty the exclude mask (undoable), ignore a colour, take a tick's line away, or
    /// take the pieces of a group out of it (they are left over).
    Delete(MaskId),
    /// Select a mask: the one a tick's line goes to, the colour lit; for a group, its pieces are selected.
    SetActive(MaskId),
    /// Show or hide a mask on the image.
    Show(MaskId, bool),
    /// Go to a step. Going to the colours looks at the image afresh.
    GoTo(Step),
    /// How different two colours must be to count as two inks.
    SetTolerance(f32),
    /// Say whether an ink is something (it can be several).
    SetRole(usize, Role, bool),
    /// Say an ink is nothing: it has no mask.
    IgnoreColour(usize),
    /// Select an ink (or none).
    Select(Option<usize>),
    /// How the straight lines are looked for.
    SetAlgorithm(Algorithm),
    /// The settings of the search for straight lines.
    SetLineParams(lines::Params),
    /// Make a line the tick, or take the tick's line away.
    SetTick(Slot, Option<Line>),
    /// The plot value of a tick (`None` for none).
    SetTickValue(Slot, Option<f64>),
    /// Whether an axis is logarithmic.
    SetLog(Axis, bool),
    /// Have the tick networks find the four ticks and read their values (a job); an axis they can read is set, as one
    /// change to undo.
    SuggestTicks,
    /// How the curve colours are broken into pieces.
    SetCurveParams(curves::Params),
    /// How alike pieces must be to be copies of one shape.
    SetShapeParams(shapes::Params),
    /// Change which pieces are selected on the shapes step.
    SelectPieces(Vec<usize>, Pick),
    /// Add to the selection the pieces that look like one of those selected.
    SelectSimilar,
    /// Make the pieces selected one group: nothing else is in it. Kept when the pieces are found again; undoable.
    JoinSelected,
    /// Take the pieces selected out of their groups: they are left over. Kept when the pieces are found again; undoable.
    RemoveSelected,
    /// Which group's pieces are shown to be linked on the links step (`None` for all).
    SetLinkGroup(Option<usize>),
    /// Add a piece to the link being made, or start a link with it (or go on with a link from its end). Undoable.
    LinkPiece(usize),
    /// End the link being made: the next piece clicked starts another.
    EndLink,
    /// Take the last piece off the link being made. Undoable.
    LinkBack,
    /// Take a piece out of its link. Undoable.
    UnlinkPiece(usize),
    /// Move a link one place up (true) or down in the order the points are written. Undoable.
    MoveLink(usize, bool),
    /// Turn a link round: its last piece first. Undoable.
    ReverseLink(usize),
    /// Delete a link. Undoable.
    DeleteLink(usize),
}
