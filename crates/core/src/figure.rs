//! A loaded source file: a picture to look at and the geometry found in it.

use crate::Primitive;

/// RGBA pixels, straight alpha, row after row from the top.
#[derive(Clone, Debug)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// The picture shown behind the primitives.
#[derive(Clone, Debug)]
pub struct Backdrop {
    pub bitmap: Bitmap,
    /// Pixels per figure unit.
    pub scale: f64,
}

/// A figure in figure space: source units, origin at the top left, y down.
#[derive(Clone, Debug)]
pub struct Figure {
    pub width: f64,
    pub height: f64,
    pub backdrop: Option<Backdrop>,
    pub primitives: Vec<Primitive>,
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ImportError(pub String);

/// Draws any part of a figure at any scale, so the picture stays sharp when zoomed in.
pub trait RegionRenderer {
    /// Pixels covering `region` (figure space) at `scale` pixels per figure unit. Pixel (0, 0) is at the region's top
    /// left corner and the bitmap is at least as large as the region.
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError>;
}

/// Turns the bytes of one file format into a [`Figure`].
pub trait Importer: Send + Sync {
    fn name(&self) -> &'static str;
    /// Whether the file's leading bytes look like this format.
    fn sniff(&self, head: &[u8]) -> bool;
    fn import(&self, bytes: Vec<u8>) -> Result<Figure, ImportError>;

    /// A renderer for sharp detail of the same file, created on the thread that will use it. `None` when the format has
    /// no vector picture to re-render (the backdrop is then all there is).
    fn region_renderer(&self, _bytes: &[u8]) -> Option<Box<dyn RegionRenderer>> {
        None
    }
}
