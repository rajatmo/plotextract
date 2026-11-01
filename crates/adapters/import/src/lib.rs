//! What the file-format importers share.
//!
//! Every importer walks its format's drawing model and hands each painted path to the same two helpers: [`flatten_path`]
//! turns it into vertices (and says where disconnected pieces start), and [`PrimitiveBuilder`] numbers the results as
//! stroked and filled primitives. The constants and sizing helper keep the importers' choices in one place.

mod builder;
mod flatten;

pub use builder::PrimitiveBuilder;
pub use flatten::{Pieces, flatten_path, rect_corners};

/// Points sampled along each cubic Bezier.
pub const BEZIER_STEPS: usize = 8;

/// Most paths an importer keeps from a file.
pub const MAX_PRIMITIVES: usize = 1_000_000;

/// Most vertices, over all paths, an importer keeps from a file: 16 bytes each, plus about 12 in the snap index.
pub const MAX_POINTS: usize = 16_000_000;

/// The largest side of a rendered picture, in pixels.
pub const MAX_PICTURE_PIXELS: u32 = 16_384;

/// The pixels a length of `len` figure units takes at `scale` pixels per unit: at least one, at most
/// [`MAX_PICTURE_PIXELS`].
pub fn pixels_for(len: f64, scale: f64) -> u32 {
    (len * scale).ceil().clamp(1.0, f64::from(MAX_PICTURE_PIXELS)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_at_least_a_pixel_and_never_huge() {
        assert_eq!(pixels_for(0.0, 4.0), 1);
        assert_eq!(pixels_for(10.2, 2.0), 21);
        assert_eq!(pixels_for(1e9, 4.0), MAX_PICTURE_PIXELS);
    }
}
