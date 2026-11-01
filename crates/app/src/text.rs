//! The words the app shows. The core and the workspace report facts (an error kind, a unit); the wording for a person is
//! decided here, in one place.

use plotextract_arxiv::{ArxivError, ArxivId};
use plotextract_core::{
    Axis, AxisCal, CalError, ExportError, ImportError, PrimitiveKind, SessionError, Slot, Unit, format_g,
};
use plotextract_raster::curves::Trace;
use plotextract_raster::shapes::Kind;
use plotextract_raster::{Tag, Value};

use crate::wizard::{MaskId, ShapeClick};
use plotextract_workspace::WorkspaceError;

/// The calibration row's name.
pub fn slot_label(slot: Slot) -> &'static str {
    match slot {
        Slot::XMin => "x min",
        Slot::XMax => "x max",
        Slot::YMin => "y min",
        Slot::YMax => "y max",
    }
}

/// A tick as the name of a mask: "X min".
pub fn tick_name(slot: Slot) -> &'static str {
    match slot {
        Slot::XMin => "X min",
        Slot::XMax => "X max",
        Slot::YMin => "Y min",
        Slot::YMax => "Y max",
    }
}

/// The axis a tick is on, as a letter.
pub fn axis_letter(slot: Slot) -> &'static str {
    match slot.axis() {
        Axis::X => "x",
        Axis::Y => "y",
    }
}

/// The name a mask of the tracing wizard goes by.
pub fn mask_name(mask: MaskId) -> String {
    match mask {
        MaskId::Crop => "Crop".into(),
        MaskId::Exclude => "Exclude".into(),
        MaskId::Ink(i) => format!("Colour {}", i + 1),
        MaskId::Tick(t) => tick_name(t).into(),
        MaskId::Shape(k) => format!("Shape {}", k + 1),
        MaskId::Series(number) => series_name(number),
    }
}

/// What a shape is, as a word.
pub fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Dot => "Dots",
        Kind::Dash => "Dashes",
        Kind::Marker => "Markers",
        Kind::Line => "Lines",
        Kind::Text => "Text",
        Kind::Other => "Other",
    }
}

/// What a kind of shape means for the series.
pub fn kind_tip(kind: Kind) -> &'static str {
    match kind {
        Kind::Dot => "The dots of a dotted or dash-dot curve: a point at each, along the curve",
        Kind::Dash => "The dashes of a dashed or dash-dot curve: points along each, along the curve",
        Kind::Marker => "The markers of a series: a point at each",
        Kind::Line => "Solid lines: a point in every column",
        Kind::Text => "Letters, digits and signs: in no series",
        Kind::Other => "Anything else: in no series",
    }
}

/// A series of the tracing wizard: "Series 2".
pub fn series_name(number: usize) -> String {
    format!("Series {number}")
}

/// How a series is traced, as a word.
pub fn trace_label(trace: Trace) -> &'static str {
    match trace {
        Trace::Line => "Line",
        Trace::Markers => "Markers",
        Trace::Pattern => "Pattern",
    }
}

/// What tracing a series a way does.
pub fn trace_tip(trace: Trace) -> &'static str {
    match trace {
        Trace::Line => "A point in every column of each piece, at the centre of the stroke",
        Trace::Markers => "A point at the centre of each piece: for a scatter plot",
        Trace::Pattern => {
            "A dotted or dashed curve: a point at each dot, points along each dash, in the order of the curve (a curve \
             that doubles back is written as drawn)"
        }
    }
}

/// A mask's tag, as a word.
pub fn tag_label(tag: Tag) -> &'static str {
    match tag {
        Tag::Crop => "crop",
        Tag::Exclude => "exclude",
        Tag::Colour => "colour",
        Tag::Curve => "curve",
        Tag::Fill => "fill",
        Tag::Axes => "axes",
        Tag::Grid => "grid",
        Tag::Tick => "tick",
        Tag::XAxis => "x axis",
        Tag::YAxis => "y axis",
        Tag::XMin => "x min",
        Tag::XMax => "x max",
        Tag::YMin => "y min",
        Tag::YMax => "y max",
        Tag::Calibrated => "calibrated",
        Tag::Series => "series",
        Tag::Line => "line",
        Tag::Markers => "markers",
        Tag::Pattern => "pattern",
        Tag::Shape => "shape",
        Tag::Dot => "dots",
        Tag::Dash => "dashes",
        Tag::Marker => "marker",
        Tag::Text => "text",
        Tag::Other => "other",
        Tag::ByHand => "by hand",
    }
}

/// What a click does on the shapes step, as a word.
pub fn shape_click_label(click: ShapeClick) -> &'static str {
    match click {
        ShapeClick::Select => "Select",
        ShapeClick::Move => "Move",
        ShapeClick::Links => "Links",
    }
}

/// What a click does on the shapes step, said in full.
pub fn shape_click_tip(click: ShapeClick) -> &'static str {
    match click {
        ShapeClick::Select => "A click on a piece selects its shape",
        ShapeClick::Move => {
            "A click on a piece puts it in the selected shape, or takes it out; a drag around pieces does it to them all \
             (with Alt held, takes them out). Drag with the middle button to move the image"
        }
        ShapeClick::Links => {
            "A click on a link between dots or dashes cuts the path there; a click on the end of one path (or a piece on \
             none) and then on another joins them"
        }
    }
}

/// A mask's property value, as text.
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Count(n) => n.to_string(),
        Value::Pixels(p) => format!("{p:.1} px"),
        Value::Percent(p) => format!("{p:.2}%"),
        Value::Number(v) => format_g(*v, 10),
        Value::Colour([r, g, b]) => format!("#{r:02x}{g:02x}{b:02x}"),
        Value::Text(t) => t.clone(),
        Value::Flag(on) => (if *on { "yes" } else { "no" }).into(),
    }
}

/// "Stroke" or "Fill".
pub fn kind_name(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::Stroke => "Stroke",
        PrimitiveKind::Fill => "Fill",
    }
}

/// How a curve is named in messages: its type and its 1-based ID, as the curve table shows them.
pub fn curve_label(index: usize, kind: PrimitiveKind) -> String {
    format!("{} #{}", kind_name(kind), index + 1)
}

/// A length in the figure's unit, such as `12.500 pt`.
pub fn measure(value: f64, unit: Unit, decimals: usize) -> String {
    format!("{value:.decimals$} {}", unit.symbol())
}

/// Why an axis is or is not usable, for a tooltip.
pub fn axis_status(axis: &AxisCal) -> String {
    match axis.map() {
        Ok(_) => "calibrated".into(),
        Err(CalError::Incomplete) => "needs two ticks and their values".into(),
        Err(CalError::SamePosition) => "the two ticks are at the same position".into(),
        Err(CalError::SameValue) => "the two values are the same".into(),
        Err(CalError::NonPositiveLog) => "a log axis needs positive values".into(),
    }
}

pub fn session_error(error: &SessionError) -> String {
    match error {
        SessionError::NoPointSelected => "Select a point first: hold Ctrl and click on a curve.".into(),
        SessionError::Hidden => "That curve is hidden.".into(),
        SessionError::OutOfRange | SessionError::Edit(_) => "No such curve or series.".into(),
        SessionError::Export(e) => export_error(e),
    }
}

fn export_error(error: &ExportError) -> String {
    match error {
        ExportError::NoSuchSeries => "Make a series first: select a curve, then Add curve.".into(),
        ExportError::NotCalibrated => "Calibrate both axes first: record four ticks and type their values.".into(),
        ExportError::Empty => "The series has no points. Add curves to it first.".into(),
        ExportError::NonFinite { bad, total } => format!(
            "{bad} of {total} points map outside the range of representable numbers. \
             The two ticks of an axis are probably too close together."
        ),
    }
}

/// Why a series of the tracing wizard cannot be written.
pub fn series_error(error: &ExportError) -> String {
    match error {
        ExportError::NoSuchSeries => "Select a series first.".into(),
        ExportError::NotCalibrated => {
            "Calibrate both axes first: on step 3, choose the four ticks and type their values.".into()
        }
        ExportError::Empty => "The series has no pieces. Click pieces of a curve in the image to add them.".into(),
        other => export_error(other),
    }
}

pub fn not_an_arxiv_number(text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return "Type the number of an arXiv paper, such as 2301.01234, or paste a link to it.".into();
    }
    format!(
        "\u{201c}{text}\u{201d} is not an arXiv number: it looks like 2301.01234, or hep-th/9901001 for older papers."
    )
}

pub fn arxiv_error(paper: &ArxivId, error: &ArxivError) -> String {
    match error {
        ArxivError::NotFound(_) => format!("arXiv has no paper {paper}. Check the number."),
        ArxivError::Status { status: 403 | 429 | 503, .. } => {
            "arXiv is turning requests away for now. Wait a minute and try again.".into()
        }
        ArxivError::Network(e) => format!("Cannot reach arXiv: {e}. Check the connection and try again."),
        ArxivError::NotAnArchive => format!("arXiv sent something for {paper} that is not a paper's source."),
        // the remaining errors already say what failed
        other => format!("Cannot get the figures of {paper}: {other}."),
    }
}

pub fn workspace_error(error: &WorkspaceError) -> String {
    match error {
        WorkspaceError::Unsupported { path } => {
            format!("Cannot open {}: plotextract reads PDF, SVG, EPS and PostScript files and images.", path.display())
        }
        WorkspaceError::Project { path, .. } => {
            format!("Cannot read {}: it is not a plotextract project.", path.display())
        }
        WorkspaceError::Import(ImportError::NoSuchPage { page, count }) => {
            format!("There is no page {}: the file has {count}.", page + 1)
        }
        WorkspaceError::Import(e) => format!("Cannot read the file: {e}"),
        WorkspaceError::Session(e) => session_error(e),
        // the remaining errors already say what failed and on which path
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn mask_properties_read_as_text() {
        assert_eq!(value_text(&Value::Count(12)), "12");
        assert_eq!(value_text(&Value::Pixels(120.04)), "120.0 px");
        assert_eq!(value_text(&Value::Percent(2.5)), "2.50%");
        assert_eq!(value_text(&Value::Number(0.001)), "0.001");
        assert_eq!(value_text(&Value::Colour([31, 119, 180])), "#1f77b4");
        assert_eq!(value_text(&Value::Flag(true)), "yes");
        assert_eq!(tag_label(Tag::XMin), "x min");
        assert_eq!(mask_name(MaskId::Ink(2)), "Colour 3");
        assert_eq!(mask_name(MaskId::Series(2)), "Series 2");
        assert_eq!(tag_label(Tag::Markers), "markers");
    }

    #[test]
    fn curves_are_labelled_by_type_and_one_based_id() {
        assert_eq!(curve_label(0, PrimitiveKind::Stroke), "Stroke #1");
        assert_eq!(curve_label(11, PrimitiveKind::Fill), "Fill #12");
    }

    #[test]
    fn lengths_carry_the_figures_unit() {
        assert_eq!(measure(12.5, Unit::Point, 3), "12.500 pt");
        assert_eq!(measure(96.0, Unit::Pixel, 1), "96.0 px");
    }

    #[test]
    fn an_axis_says_what_it_lacks() {
        let mut axis = AxisCal::default();
        assert_eq!(axis_status(&axis), "needs two ticks and their values");
        axis.pos = [Some(1.0), Some(1.0)];
        axis.value = [Some(0.0), Some(1.0)];
        assert_eq!(axis_status(&axis), "the two ticks are at the same position");
        axis.pos = [Some(0.0), Some(1.0)];
        assert_eq!(axis_status(&axis), "calibrated");
        axis.log = true;
        assert_eq!(axis_status(&axis), "a log axis needs positive values");
    }

    #[test]
    fn errors_get_wording_that_tells_the_user_what_to_do() {
        let session = SessionError::Export(ExportError::NotCalibrated);
        assert!(session_error(&session).starts_with("Calibrate both axes"));
        let unsupported = WorkspaceError::Unsupported { path: PathBuf::from("notes.txt") };
        let text = workspace_error(&unsupported);
        assert!(text.contains("notes.txt") && text.contains("PDF"));
        assert!(series_error(&ExportError::NotCalibrated).contains("step 3"));
        assert!(series_error(&ExportError::NonFinite { bad: 1, total: 2 }).starts_with("1 of 2"));
    }
}
