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

/// A raster image one of a file's pages draws, by its page and its place among the images of that page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Embedded {
    /// The page that draws it, from 0.
    pub page: usize,
    /// Which of the page's images it is, from 0, in the order the page draws them (each once).
    pub index: usize,
    /// Its size in pixels, as stored.
    pub width: u32,
    pub height: u32,
}

/// What there is to work on in a file: its pages and the raster images they draw.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contents {
    pub pages: usize,
    /// Each image once, on the first page that draws it, in page order.
    pub images: Vec<Embedded>,
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

    /// Whether a file of the format is a document: what to open of it (a page, or an image a page draws) is chosen from
    /// its [`Importer::contents`] first.
    fn is_document(&self) -> bool {
        false
    }

    /// The pages of the file and the raster images they draw. A format with no images inside has one page and none.
    fn contents(&self, _bytes: &Arc<Vec<u8>>, _options: &OpenOptions) -> Result<Contents, ImportError> {
        Ok(Contents { pages: 1, images: Vec::new() })
    }

    /// One of the images [`Importer::contents`] lists, as the bytes of a PNG file.
    fn image(&self, _bytes: &Arc<Vec<u8>>, _options: &OpenOptions, _image: &Embedded) -> Result<Vec<u8>, ImportError> {
        Err(ImportError::message("the file holds no images"))
    }
}

/// The two small networks that find a plot's ticks and read their labels, behind whatever runs them. What goes in and
/// what comes out is laid down by `plotextract_algo::ticks`, which makes the one and reads the other.
pub trait TickModels: Send + Sync {
    /// The tick finder on a plot fitted into its square: 512 x 512 grey levels, row by row, 0 ink to 1 paper. Its 8 maps of
    /// 128 x 128 cells, map by map, row by row.
    fn maps(&self, square: &[f32]) -> Result<Vec<f32>, ModelError>;

    /// The reader on label crops, one after another, each 32 x 192 grey levels as above. For each crop, its 48 steps of
    /// 14 scores.
    fn read(&self, crops: &[f32]) -> Result<Vec<f32>, ModelError>;
}

/// A network that could not be run.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ModelError(pub String);

/// The files the user opens and saves.
pub trait Files: Send + Sync {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;
}
