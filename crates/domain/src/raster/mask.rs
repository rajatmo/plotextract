//! A mask: for every pixel of the crop, how much of it belongs to something (0 to 255). The tracing steps work on masks
//! rather than on the image: the colour step makes one for every ink colour, and the steps after it make the frame, the
//! ticks and the curves from them.
//!
//! The values are coverages, not yes or no, so a thin line's centre can be placed to a fraction of a pixel.

use super::prepare::PixelRect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    crop: PixelRect,
    data: Vec<u8>,
}

impl Mask {
    /// A mask over `crop` that covers nothing.
    pub fn empty(crop: PixelRect) -> Self {
        Self { crop, data: vec![0; crop.width as usize * crop.height as usize] }
    }

    /// A mask over `crop` with the given coverages, row after row.
    pub fn from_data(crop: PixelRect, data: Vec<u8>) -> Self {
        assert_eq!(data.len(), crop.width as usize * crop.height as usize, "one coverage per pixel of the crop");
        Self { crop, data }
    }

    /// The part of the image the mask is about, in image pixels.
    pub fn crop(&self) -> PixelRect {
        self.crop
    }

    /// The coverage at image position `(x, y)`; 0 outside the crop.
    pub fn at(&self, x: u32, y: u32) -> u8 {
        let c = self.crop;
        if x < c.x || y < c.y || x >= c.x + c.width || y >= c.y + c.height {
            return 0;
        }
        self.data[((y - c.y) * c.width + (x - c.x)) as usize]
    }

    /// The coverages, row after row over the crop.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// How many pixels are mostly covered.
    pub fn pixels(&self) -> u64 {
        self.data.iter().filter(|&&v| v >= 128).count() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop() -> PixelRect {
        PixelRect { x: 10, y: 5, width: 8, height: 4 }
    }

    #[test]
    fn a_mask_is_asked_in_image_coordinates_and_is_empty_outside_its_crop() {
        let mut m = Mask::empty(crop());
        assert_eq!((m.crop(), m.data().len(), m.pixels()), (crop(), 32, 0));
        m.data[0] = 200;
        m.data[31] = 255;
        assert_eq!((m.at(10, 5), m.at(17, 8)), (200, 255), "the corners of the crop");
        assert_eq!((m.at(9, 5), m.at(18, 5), m.at(10, 4), m.at(10, 9)), (0, 0, 0, 0), "around it");
        assert_eq!(m.pixels(), 2);
    }

    #[test]
    fn only_mostly_covered_pixels_count() {
        let m = Mask::from_data(PixelRect { x: 0, y: 0, width: 4, height: 1 }, vec![0, 127, 128, 255]);
        assert_eq!(m.pixels(), 2);
    }

    #[test]
    #[should_panic(expected = "one coverage per pixel")]
    fn the_data_must_fit_the_crop() {
        Mask::from_data(crop(), vec![0; 5]);
    }
}
