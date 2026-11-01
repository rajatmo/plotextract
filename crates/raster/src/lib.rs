//! Raster images.
//!
//! An image is opened as a figure with no paths and traced in steps (the tracing wizard). This crate holds what the
//! steps are built from, with no UI dependency, so each can be tested on generated images: decoding, drawing any part of
//! the image at any scale, the preparation the wizard starts with (crop and mask), the ink colours, masks, the straight
//! lines of a mask, the pieces of the curves and the points traced along them, and what is known about a mask besides its
//! pixels (its ID, tags and properties).

pub mod colours;
pub mod curves;
pub mod decode;
mod importer;
pub mod lines;
pub mod mask;
pub mod meta;
mod picture;
pub mod prepare;
pub mod shapes;

pub use colours::{ColourMap, Ink, Params as ColourParams, analyse as analyse_colours};
pub use importer::RasterImporter;
pub use mask::Mask;
pub use meta::{MaskMeta, Properties, Tag, Value};
pub use picture::RasterPicture;
pub use prepare::{Grid, MaskOp, PixelRect, Prepare, Shape, clamp_crop, paint_ops};
