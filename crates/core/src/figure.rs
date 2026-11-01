//! A loaded source file: the geometry found in it, and the ports through which formats plug in.

use std::sync::Arc;

use crate::Primitive;

/// RGBA pixels, straight alpha, row after row from the top.
#[derive(Clone, Debug)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// What a figure's coordinates are measured in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// PostScript points (1/72 inch): PDF, PostScript and EPS.
    Point,
    /// CSS pixels (1/96 inch): SVG.
    Pixel,
}

impl Unit {
    /// The unit's usual abbreviation.
    pub fn symbol(self) -> &'static str {
        match self {
            Unit::Point => "pt",
            Unit::Pixel => "px",
        }
    }
}

/// A figure in figure space: the unit of its source file, origin at the top left, y down.
#[derive(Clone, Debug)]
pub struct Figure {
    pub width: f64,
    pub height: f64,
    pub unit: Unit,
    pub primitives: Vec<Primitive>,
    /// Which page of the file this is (from 0) and how many it has.
    pub page: usize,
    pub page_count: usize,
    /// The file holds more paths than were kept: the importer stopped at its limits.
    pub truncated: bool,
}

/// What to open of a file.
#[derive(Clone, Debug, Default)]
pub struct OpenOptions {
    /// The page to read, from 0, for formats with several.
    pub page: usize,
    /// The password of a protected file.
    pub password: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ImportError {
    #[error("the file is password protected")]
    PasswordRequired,
    #[error("the password is wrong")]
    WrongPassword,
    #[error("the file has no page {page}: it has {count}")]
    NoSuchPage { page: usize, count: usize },
    #[error("{0}")]
    Message(String),
}

impl ImportError {
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

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
