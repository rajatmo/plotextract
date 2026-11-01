//! The ports: the traits through which the use cases reach what is outside them (file formats, arXiv, the file system),
//! so that they depend on no format, network or disk. The adapters implement them, and the composition root chooses
//! which.

pub mod papers;

use std::path::Path;
use std::sync::Arc;

use plotextract_domain::{Bitmap, Figure, ImportError, OpenOptions};

/// Draws any part of a figure at any scale: the one way a figure's picture reaches the screen, from the whole page at a
/// base resolution to a sharp close-up.
pub trait PictureSource {
    /// Pixels covering `region` (figure space) at `scale` pixels per figure unit. Pixel (0, 0) is at the region's top
    /// left corner and the bitmap is at least as large as the region.
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError>;
}

/// A file format: recognizes its files and turns them into a [`Figure`].
pub trait Importer: Send + Sync {
    fn name(&self) -> &'static str;

    /// The file extensions of the format, lower case and without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Whether the file's leading bytes look like this format.
    fn sniff(&self, head: &[u8]) -> bool;

    fn import(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Figure, ImportError>;

    /// Whether the file holds pixels rather than paths: its figure has no primitives until the image is traced.
    fn needs_tracing(&self) -> bool {
        false
    }

    /// A source of pictures of the same page, created on the thread that will use it (the renderers of some formats
    /// cannot move between threads). `None` when the format has no picture to draw.
    fn picture(&self, _bytes: &Arc<Vec<u8>>, _options: &OpenOptions) -> Option<Box<dyn PictureSource>> {
        None
    }
}

/// The files the user opens and saves.
pub trait Files: Send + Sync {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
}
