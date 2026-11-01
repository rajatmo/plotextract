//! Drawing any part of an image at any scale: the picture the canvas shows while the image is prepared and traced.

use image::RgbaImage;
use kurbo::Rect;
use plotextract_domain::{Bitmap, ImportError};
use plotextract_import::pixels_for;
use plotextract_ports::PictureSource;
use rayon::prelude::*;

pub struct RasterPicture {
    image: RgbaImage,
}

impl RasterPicture {
    pub fn new(image: RgbaImage) -> Self {
        Self { image }
    }
}

const WHITE: [u8; 4] = [255, 255, 255, 255];

impl PictureSource for RasterPicture {
    /// Zoomed in, every image pixel is a crisp square (so the pixels can be judged); zoomed out, each output pixel is
    /// the average of the image pixels it covers.
    fn render(&mut self, region: Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let (w, h) = (pixels_for(region.width(), scale), pixels_for(region.height(), scale));
        let image = &self.image;
        let (iw, ih) = (i64::from(image.width()), i64::from(image.height()));
        let mut rgba = vec![0u8; w as usize * h as usize * 4];
        rgba.par_chunks_mut(w as usize * 4).enumerate().for_each(|(oy, row)| {
            for (ox, out) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let (x0, x1) = (region.x0 + ox as f64 / scale, region.x0 + (ox + 1) as f64 / scale);
                let (y0, y1) = (region.y0 + oy as f64 / scale, region.y0 + (oy + 1) as f64 / scale);
                *out = if scale >= 1.0 {
                    let (x, y) = (((x0 + x1) / 2.0).floor() as i64, ((y0 + y1) / 2.0).floor() as i64);
                    if (0..iw).contains(&x) && (0..ih).contains(&y) {
                        image.get_pixel(x as u32, y as u32).0
                    } else {
                        WHITE
                    }
                } else {
                    average(image, x0, x1, y0, y1)
                };
            }
        });
        Ok(Bitmap { width: w, height: h, rgba })
    }
}

/// The mean colour of the image pixels whose centres lie in the box, white where the box leaves the image.
fn average(image: &RgbaImage, x0: f64, x1: f64, y0: f64, y1: f64) -> [u8; 4] {
    let (xa, xb) = (x0.floor().max(0.0) as u32, (x1.ceil().min(f64::from(image.width())).max(0.0)) as u32);
    let (ya, yb) = (y0.floor().max(0.0) as u32, (y1.ceil().min(f64::from(image.height())).max(0.0)) as u32);
    if xb <= xa || yb <= ya {
        return WHITE;
    }
    let (mut sum, mut n) = ([0u32; 3], 0u32);
    for y in ya..yb {
        for x in xa..xb {
            let p = image.get_pixel(x, y).0;
            sum[0] += u32::from(p[0]);
            sum[1] += u32::from(p[1]);
            sum[2] += u32::from(p[2]);
            n += 1;
        }
    }
    [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255]
}

#[cfg(test)]
mod tests {
    use image::Rgba;

    use super::*;

    /// 4 x 2 pixels: red, green, blue, black on the top row; white below.
    fn tiny() -> RasterPicture {
        let mut image = RgbaImage::from_pixel(4, 2, Rgba(WHITE));
        for (x, c) in [[255, 0, 0], [0, 255, 0], [0, 0, 255], [0, 0, 0]].into_iter().enumerate() {
            image.put_pixel(x as u32, 0, Rgba([c[0], c[1], c[2], 255]));
        }
        RasterPicture::new(image)
    }

    fn px(b: &Bitmap, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * b.width + x) * 4) as usize;
        b.rgba[i..i + 4].try_into().unwrap()
    }

    #[test]
    fn zoomed_in_each_pixel_is_a_crisp_square() {
        let b = tiny().render(Rect::new(0.0, 0.0, 4.0, 2.0), 3.0).unwrap();
        assert_eq!((b.width, b.height), (12, 6));
        assert_eq!(px(&b, 0, 0), [255, 0, 0, 255]);
        assert_eq!(px(&b, 2, 2), [255, 0, 0, 255], "still the first pixel");
        assert_eq!(px(&b, 3, 0), [0, 255, 0, 255], "the next pixel starts at once, with no blending");
        assert_eq!(px(&b, 11, 2), [0, 0, 0, 255]);
        assert_eq!(px(&b, 0, 5), WHITE);
    }

    #[test]
    fn a_region_starts_at_its_own_corner() {
        let b = tiny().render(Rect::new(2.0, 0.0, 4.0, 1.0), 2.0).unwrap();
        assert_eq!((b.width, b.height), (4, 2));
        assert_eq!(px(&b, 0, 0), [0, 0, 255, 255]);
        assert_eq!(px(&b, 3, 1), [0, 0, 0, 255]);
    }

    #[test]
    fn zoomed_out_each_pixel_is_the_mean_of_the_ones_it_covers() {
        let b = tiny().render(Rect::new(0.0, 0.0, 4.0, 2.0), 0.5).unwrap();
        assert_eq!((b.width, b.height), (2, 1));
        // the top-left block is red, green and two white pixels
        assert_eq!(px(&b, 0, 0), [191, 191, 127, 255]);
        // the right block is blue, black and two white pixels
        assert_eq!(px(&b, 1, 0), [127, 127, 191, 255]);
    }

    #[test]
    fn outside_the_image_is_white() {
        let b = tiny().render(Rect::new(-2.0, -1.0, 0.0, 0.0), 1.0).unwrap();
        assert!(b.rgba.chunks(4).all(|p| p == WHITE));
    }
}
