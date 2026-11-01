//! The domain kernel of plotextract: what a figure is, what is made of it, and nothing that reads a file, draws a window
//! or waits on a thread. Every other crate depends on this one, and it depends on none of them.
//!
//! A source file becomes a [`Figure`]: candidate [`Primitive`]s in one coordinate space (the file's own unit, origin top
//! left, y down). What is saved and undone of the work on it is a [`Document`] (calibration, series, hidden curves),
//! kept with a [`History`]. An image is traced into [`raster`] masks, each described by a [`MaskMeta`]. An arXiv paper is
//! named by an [`ArxivId`].

mod arxiv_id;
mod calibration;
mod document;
mod figure;
mod format;
mod history;
mod primitive;
pub mod raster;
mod series;
mod snap;
mod view;

pub use arxiv_id::ArxivId;
pub use calibration::{Axis, AxisCal, AxisMap, CalError, Calibration, Slot, parse_value};
pub use document::{Document, Edit, EditError, Project};
pub use figure::{Bitmap, Figure, ImportError, OpenOptions, Unit};
pub use format::format_g;
pub use history::History;
pub use primitive::{Primitive, PrimitiveKind, Provenance, Style};
pub use raster::mask::Mask;
pub use raster::meta::{MaskMeta, Properties, Tag, Value};
pub use raster::prepare::{Grid, MaskOp, PixelRect, Prepare, Shape, clamp_crop, paint_ops};
pub use series::{ExportError, Series};
pub use snap::{Hit, SnapIndex};
pub use view::{Layer, Layers, Pick, ViewState};
