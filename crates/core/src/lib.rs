//! Format-independent core of plotextract.
//!
//! A source file becomes a [`Figure`]: a backdrop picture plus candidate [`Primitive`]s in one coordinate space
//! (figure space: source units, origin top left, y down). A [`Session`] holds everything the user does with a figure
//! (calibration, series, selection) and changes only through [`Command`]s, so the UI is a thin shell and the
//! whole workflow can be driven and tested without a window.

mod builder;
mod calibration;
mod figure;
mod flatten;
mod format;
mod primitive;
mod series;
mod session;
mod snap;

pub use builder::PrimitiveBuilder;
pub use calibration::{Axis, AxisCal, AxisMap, CalError, Calibration, Slot, parse_value};
pub use figure::{Backdrop, Bitmap, Figure, ImportError, Importer, RegionRenderer};
pub use flatten::{Pieces, flatten_path, rect_corners};
pub use format::format_g;
pub use primitive::{Primitive, PrimitiveKind, Provenance, Style};
pub use series::{ExportError, Series};
pub use session::{Command, Layer, Layers, Pick, Project, Session, SessionError};
pub use snap::{Hit, SnapIndex};
