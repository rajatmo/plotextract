//! What the wizard is told: the steps, the tools, and every change the user can make ([`WizardIntent`]).

use kurbo::{Point, Rect};
use plotextract_core::{Axis, Slot};
use plotextract_raster::MaskOp;
use plotextract_raster::curves::{self, Trace};
use plotextract_raster::lines::{self, Algorithm, Line};
use plotextract_raster::shapes::{self, Kind};

use super::{MaskId, Role};

/// The wizard's steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// Crop the image to the plot and mask what is not part of it.
    Prepare,
    /// Find the ink colours and say what each one is.
    Colours,
    /// Find the straight lines of the axes and choose the four ticks the calibration is made from.
    PlotArea,
    /// Break the curve colours into pieces and find the shapes they are drawn with.
    Shapes,
    /// Make series of the shapes, take stray points out and put missed ones in.
    Curves,
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
            Step::Curves => "Step 5 \u{b7} Curves",
        }
    }

    /// What the step is for.
    pub fn about(self) -> &'static str {
        match self {
            Step::Prepare => "Choose the plot and cover what should be ignored.",
            Step::Colours => "Find the colours the plot is drawn in and say what each is.",
            Step::PlotArea => "Find the straight lines of the axes and choose the four ticks.",
            Step::Shapes => "Find the shapes the curves are drawn with: dots, dashes, markers, lines, and the text.",
            Step::Curves => "Make series of the shapes; take stray points out and put missed ones in.",
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

/// What a click on the image does on the shapes step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShapeClick {
    /// Selects the shape type of the piece clicked.
    #[default]
    Select,
    /// Puts the piece clicked in the selected type, or takes it out; a drag around pieces does it to them all.
    Move,
    /// Cuts the path of dots or dashes where its link is clicked; a click on one end and then another joins them.
    Links,
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
    /// Delete a mask: take the crop away, empty the exclude mask (undoable), ignore a colour, take a tick's line away, say
    /// a shape type is nothing, or remove a series.
    Delete(MaskId),
    /// Select a mask: the one a tick's line goes to, the colour lit, the series a click adds pieces to.
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
    /// How the curve colours are broken into pieces.
    SetCurveParams(curves::Params),
    /// How alike pieces must be to be copies of one shape.
    SetShapeParams(shapes::Params),
    /// Say what kind of thing a shape type is (it is kept when the shapes are found again, as every change to the shapes
    /// is).
    SetKind(usize, Kind),
    /// What a click on the image does on the shapes step.
    SetShapeClick(ShapeClick),
    /// Put pieces in a shape type (those of its colour), or (`None`) take them out of theirs, among the other pieces of
    /// their colour. Moving one piece offers to move the pieces that look like it too.
    MovePieces(Vec<usize>, Option<usize>),
    /// Move the pieces that look like the one just moved too (true), or not.
    LookAlikes(bool),
    /// Put all the pieces of one shape type in another of the same colour, and select that one.
    MergeShape(usize, usize),
    /// Make an empty shape type to put pieces in, select it, and make a click put pieces in it.
    NewShape,
    /// Cut the path of a dash pattern between two pieces next on it.
    CutLink(usize, usize),
    /// Join two pieces into one path: each the end of one, or on none.
    JoinPieces(usize, usize),
    /// Show the selected shape type alone, the others faint.
    SetSolo(bool),
    /// Ring a piece on the image (one of the odd pieces of a type, being looked at), or none.
    FocusPiece(Option<usize>),
    /// Make a new series and select it; with a piece, the piece goes to it.
    NewSeries(Option<usize>),
    /// Put a piece in a series (taking it out of any other), or in none.
    SetPiece(usize, Option<usize>),
    /// How a series is traced.
    SetTrace(usize, Trace),
    /// Whether a click on the curves step works on points (true) or on pieces.
    SetEditPoints(bool),
    /// Put a point in a series by hand, in image pixels.
    AddPoint(usize, Point),
    /// Take a point out of a series: a point put in by hand goes, a traced one is left out.
    RemovePoint(usize, Point),
    /// Put back a traced point that was taken out.
    RestorePoint(usize, Point),
}
