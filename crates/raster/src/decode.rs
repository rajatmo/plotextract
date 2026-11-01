//! Reading an image file into pixels.

use std::io::Cursor;

use image::{ImageReader, Limits, RgbaImage};
use plotextract_core::ImportError;

/// Most pixels an image may have (a 7,000 x 7,000 image): 4 bytes each once decoded, and a copy of that is made.
pub const MAX_PIXELS: u64 = 50_000_000;

fn reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, ImportError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| ImportError::message(format!("cannot read the image: {e}")))?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(1 << 30);
    reader.limits(limits);
    Ok(reader)
}

fn unreadable(e: impl std::fmt::Display) -> ImportError {
    ImportError::message(format!("cannot read the image: {e}"))
}

/// The size of the image, from its header, without decoding it.
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32), ImportError> {
    let (w, h) = reader(bytes)?.into_dimensions().map_err(unreadable)?;
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(ImportError::message(format!(
            "the image is {w} x {h} pixels: more than the {} million plotextract opens",
            MAX_PIXELS / 1_000_000
        )));
    }
    Ok((w, h))
}

/// The pixels, with transparency laid over white paper.
pub fn decode(bytes: &[u8]) -> Result<RgbaImage, ImportError> {
    dimensions(bytes)?;
    let mut image = reader(bytes)?.decode().map_err(unreadable)?.into_rgba8();
    for px in image.pixels_mut() {
        let a = u32::from(px.0[3]);
        if a != 255 {
            for c in &mut px.0[..3] {
                *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            px.0[3] = 255;
        }
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageFormat, Rgba};

    use super::*;

    fn encode(image: &DynamicImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, format).unwrap();
        out.into_inner()
    }

    #[test]
    fn png_jpeg_gif_bmp_and_tiff_decode_to_the_same_size() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(7, 5, Rgba([200, 30, 30, 255])));
        for format in [ImageFormat::Png, ImageFormat::Bmp, ImageFormat::Gif, ImageFormat::Tiff] {
            let bytes = encode(&image, format);
            assert_eq!(dimensions(&bytes).unwrap(), (7, 5), "{format:?}");
            assert_eq!(decode(&bytes).unwrap().dimensions(), (7, 5), "{format:?}");
        }
        // JPEG has no transparency
        let jpeg = encode(&DynamicImage::ImageRgb8(image.to_rgb8()), ImageFormat::Jpeg);
        assert_eq!(decode(&jpeg).unwrap().dimensions(), (7, 5));
    }

    #[test]
    fn transparency_is_laid_over_white() {
        let mut image = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 0, Rgba([0, 0, 0, 128]));
        let out = decode(&encode(&DynamicImage::ImageRgba8(image), ImageFormat::Png)).unwrap();
        assert_eq!(out.get_pixel(0, 0).0, [255, 255, 255, 255]);
        let half = out.get_pixel(1, 0).0;
        assert!((126..=129).contains(&half[0]) && half[3] == 255, "{half:?}");
    }

    #[test]
    fn damaged_and_unknown_files_are_refused_with_a_reason() {
        for bytes in [&b""[..], b"not an image at all", b"\x89PNG\r\n\x1a\n garbage"] {
            let e = decode(bytes).unwrap_err();
            assert!(e.to_string().starts_with("cannot read the image"), "{e}");
        }
    }

    #[test]
    fn an_image_with_too_many_pixels_is_refused_before_it_is_decoded() {
        // a BMP whose header claims 20,000 x 20,000 pixels (400 million); a PNG's header is checksummed, a BMP's is not
        let mut bmp =
            encode(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]))), ImageFormat::Bmp);
        bmp[18..22].copy_from_slice(&20_000i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&20_000i32.to_le_bytes());
        let e = dimensions(&bmp).unwrap_err();
        assert!(e.to_string().contains("more than the 50 million"), "{e}");
    }
}
