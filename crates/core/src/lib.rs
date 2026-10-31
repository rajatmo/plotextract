//! The domain of plotextract: what a figure is and what the user does with it.
//!
//! A source file becomes a [`Figure`]: candidate [`Primitive`]s in one coordinate space (the file's own unit, origin top
//! left, y down). A [`Session`] works on a figure. It keeps apart the [`Document`] (calibration, series, hidden curves:
//! what is saved and undone), the [`History`] and the [`ViewState`] (layers and selection), so a UI is a thin shell that
//! turns input into calls on it. File formats plug in through the [`Importer`] and [`PictureSource`] ports; nothing
//! here knows a format, a window or a piece of user-facing wording.

mod calibration;
mod document;
mod figure;
mod format;
mod history;
mod primitive;
mod series;
mod session;
mod snap;
mod view;

pub use calibration::{Axis, AxisCal, AxisMap, CalError, Calibration, Slot, parse_value};
pub use document::{Document, Edit, EditError, Project};
pub use figure::{Bitmap, Figure, ImportError, Importer, OpenOptions, PictureSource, Unit};
pub use format::format_g;
pub use history::History;
pub use primitive::{Primitive, PrimitiveKind, Provenance, Style};
pub use series::{ExportError, Series};
pub use session::{Session, SessionError};
pub use snap::{Hit, SnapIndex};
pub use view::{Layer, Layers, Pick, ViewState};
