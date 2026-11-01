//! The algorithms that trace an image, each pure: given pixels or a mask, they measure and group, and keep no state and
//! do no I/O. The wizard's steps (in the use cases) call them on a thread; their results are plain data.
//!
//! - [`decode`]: the bytes of an image file into pixels, within limits.
//! - [`colours`]: the ink colours of an image, and a mask for each.
//! - [`lines`]: the straight lines of a mask (the Hough transform or the line segment detector).
//! - [`curves`]: the pieces a mask breaks into, with the lines of the axes cut out.
//! - [`shapes`]: the pieces grouped by how they look, and the edits that fix the groups.

pub mod colours;
pub mod curves;
pub mod decode;
pub mod lines;
pub mod shapes;

// the masks are domain types; the algorithms name them as before
use plotextract_domain::raster::{mask, prepare};

pub use colours::{ColourMap, Ink, Params as ColourParams, analyse as analyse_colours};
