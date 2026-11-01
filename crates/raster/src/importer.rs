//! The raster importer: an image is opened as an empty figure the size of the image, with the image as its picture. Its
//! paths come from tracing, which is the wizard's job.

use std::sync::Arc;

use plotextract_core::{Figure, ImportError, Importer, OpenOptions, PictureSource, Unit};

use crate::decode;
use crate::picture::RasterPicture;

/// Opens PNG, JPEG, GIF, BMP, TIFF and WebP images.
pub struct RasterImporter;

impl Importer for RasterImporter {
    fn name(&self) -> &'static str {
        "Image"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["png", "jpg", "jpeg", "jpe", "tif", "tiff", "bmp", "gif", "webp"]
    }

    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"\x89PNG")
            || head.starts_with(&[0xFF, 0xD8, 0xFF])
            || head.starts_with(b"GIF8")
            || head.starts_with(b"BM")
            || head.starts_with(b"II*\0")
            || head.starts_with(b"MM\0*")
            || (head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
    }

    fn needs_tracing(&self) -> bool {
        true
    }

    fn import(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Figure, ImportError> {
        if options.page != 0 {
            return Err(ImportError::NoSuchPage { page: options.page, count: 1 });
        }
        let (width, height) = decode::dimensions(bytes)?;
        Ok(Figure {
            width: f64::from(width),
            height: f64::from(height),
            unit: Unit::Pixel,
            primitives: Vec::new(),
            page: 0,
            page_count: 1,
            truncated: false,
        })
    }

    fn picture(&self, bytes: &Arc<Vec<u8>>, _options: &OpenOptions) -> Option<Box<dyn PictureSource>> {
        Some(Box::new(RasterPicture::new(decode::decode(bytes).ok()?)))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use image::{ImageFormat, Rgba, RgbaImage};
    use kurbo::Rect;

    use super::*;

    fn png(w: u32, h: u32) -> Arc<Vec<u8>> {
        let mut out = Cursor::new(Vec::new());
        RgbaImage::from_pixel(w, h, Rgba([10, 20, 30, 255])).write_to(&mut out, ImageFormat::Png).unwrap();
        Arc::new(out.into_inner())
    }

    #[test]
    fn an_image_opens_as_an_empty_figure_of_its_own_size() {
        let f = RasterImporter.import(&png(30, 20), &OpenOptions::default()).unwrap();
        assert_eq!((f.width, f.height, f.unit), (30.0, 20.0, Unit::Pixel));
        assert!(f.primitives.is_empty() && !f.truncated);
        assert!(RasterImporter.needs_tracing());
        assert!(matches!(
            RasterImporter.import(&png(3, 3), &OpenOptions { page: 1, ..Default::default() }),
            Err(ImportError::NoSuchPage { page: 1, count: 1 })
        ));
    }

    #[test]
    fn the_picture_is_the_image() {
        let mut picture = RasterImporter.picture(&png(30, 20), &OpenOptions::default()).unwrap();
        let b = picture.render(Rect::new(0.0, 0.0, 30.0, 20.0), 1.0).unwrap();
        assert_eq!(&b.rgba[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn the_formats_are_told_apart_by_their_first_bytes() {
        assert!(RasterImporter.sniff(&png(2, 2)));
        assert!(RasterImporter.sniff(&[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(RasterImporter.sniff(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!RasterImporter.sniff(b"RIFF\0\0\0\0WAVEfmt "));
        assert!(!RasterImporter.sniff(b"%PDF-1.7"));
        assert!(!RasterImporter.sniff(b"<svg xmlns"));
        assert!(!RasterImporter.sniff(b"%!PS-Adobe"));
    }
}
