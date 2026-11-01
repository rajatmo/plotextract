//! What an image is traced into, as data: masks (how much of each pixel belongs to something), what is known about a
//! mask besides its pixels (its ID, tags and properties), and the preparation every trace starts from (the crop and the
//! shapes painted over what is not part of the plot).

pub mod mask;
pub mod meta;
pub mod prepare;
