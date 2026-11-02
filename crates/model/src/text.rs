//! The words the app shows. The core and the workspace report facts (an error kind, a unit); the wording for a person is
//! decided here, in one place.

use plotextract_domain::ArxivId;
use plotextract_domain::{Axis, AxisCal, CalError, ExportError, ImportError, PrimitiveKind, Slot, Unit, format_g};
use plotextract_domain::{Tag, Value};
use plotextract_ports::papers::ArxivError;
use plotextract_usecases::SessionError;

use plotextract_usecases::WorkspaceError;

/// The calibration row's name.
pub fn slot_label(slot: Slot) -> &'static str {
    match slot {
        Slot::XMin => "x min",
        Slot::XMax => "x max",
        Slot::YMin => "y min",
        Slot::YMax => "y max",
    }
}

/// The axis a tick is on, as a letter.
pub fn axis_letter(slot: Slot) -> &'static str {
    match slot.axis() {
        Axis::X => "x",
        Axis::Y => "y",
    }
}

pub use plotextract_usecases::wizard::{mask_name, tick_name};

/// What the tick networks found: which axes they set, and what is left to the user.
pub fn suggested(s: &plotextract_usecases::wizard::Suggestion) -> String {
    let scale = |log: bool| if log { "log" } else { "linear" };
    match &s.axes {
        [Some(x), Some(y)] => format!(
            "Suggested the ticks of both axes (x {}, y {}): check them before going on. Ctrl+Z takes them back.",
            scale(x.log),
            scale(y.log)
        ),
        [Some(x), None] => format!(
            "Suggested the x axis' ticks ({}); the y axis' labels could not be read: choose its ticks by hand.",
            scale(x.log)
        ),
        [None, Some(y)] => format!(
            "Suggested the y axis' ticks ({}); the x axis' labels could not be read: choose its ticks by hand.",
            scale(y.log)
        ),
        [None, None] => "No tick labels could be read: choose the ticks by hand.".into(),
    }
}

/// Why the points of the links of the tracing wizard cannot be written.
pub fn links_error(error: &ExportError) -> String {
    match error {
        ExportError::NotCalibrated => "Calibrate the axes on step 3 first.".into(),
        ExportError::Empty | ExportError::NoSuchSeries => "Nothing linked yet.".into(),
        other => export_error(other),
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
        Tag::Line => "line",
        Tag::Shape => "shape",
        Tag::ByHand => "by hand",
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
        Err(CalError::Incomplete) => "2 ticks and values needed".into(),
        Err(CalError::SamePosition) => "both ticks at one position".into(),
        Err(CalError::SameValue) => "both values the same".into(),
        Err(CalError::NonPositiveLog) => "log axis needs values above 0".into(),
    }
}

pub fn session_error(error: &SessionError) -> String {
    match error {
        SessionError::NoPointSelected => "Ctrl+click a curve point first.".into(),
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
        assert_eq!(mask_name(plotextract_usecases::wizard::MaskId::Ink(2)), "Colour 3");
        assert_eq!(tag_label(Tag::ByHand), "by hand");
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
        assert_eq!(axis_status(&axis), "2 ticks and values needed");
        axis.pos = [Some(1.0), Some(1.0)];
        axis.value = [Some(0.0), Some(1.0)];
        assert_eq!(axis_status(&axis), "both ticks at one position");
        axis.pos = [Some(0.0), Some(1.0)];
        assert_eq!(axis_status(&axis), "calibrated");
        axis.log = true;
        assert_eq!(axis_status(&axis), "log axis needs values above 0");
    }

    #[test]
    fn errors_get_wording_that_tells_the_user_what_to_do() {
        let session = SessionError::Export(ExportError::NotCalibrated);
        assert!(session_error(&session).starts_with("Calibrate both axes"));
        let unsupported = WorkspaceError::Unsupported { path: PathBuf::from("notes.txt") };
        let text = workspace_error(&unsupported);
        assert!(text.contains("notes.txt") && text.contains("PDF"));
    }
}
