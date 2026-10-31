//! The words the app shows. The core and the workspace report facts (an error kind, a unit); the wording for a person is
//! decided here, in one place.

use plotextract_core::{AxisCal, CalError, ExportError, ImportError, PrimitiveKind, SessionError, Slot, Unit};
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

pub fn workspace_error(error: &WorkspaceError) -> String {
    match error {
        WorkspaceError::Unsupported { path } => {
            format!("Cannot open {}: plotextract reads PDF, SVG, EPS and PostScript files so far.", path.display())
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
    }
}
