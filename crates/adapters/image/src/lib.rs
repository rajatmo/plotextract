//! Raster images as a file format behind the [`Importer`](plotextract_ports::Importer) port: a PNG, JPEG, GIF, BMP, TIFF
//! or WebP file opens as a figure with no paths, to be traced, and draws any part of itself at any scale.

mod importer;
mod picture;

pub use importer::RasterImporter;
pub use picture::RasterPicture;
