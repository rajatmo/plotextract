//! Cursors that can be seen on any background.
//!
//! The system's cursors follow the user's cursor scheme: white on a white plot is invisible, and so is black on a black
//! screenshot. Over the canvas, where the background is whatever image was opened, the app supplies its own: a black core
//! with a white halo, which shows on light and dark alike.
//!
//! The cursors are pictures drawn in TikZ (`tools/cursors`, rendered to `assets/cursors` and compiled in) and handed to the
//! window system as real cursors, sized for the display's scaling. Where the window system does not take them, egui falls
//! back to the system cursor it was asked for.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::{Context, CursorIcon, CustomCursorImage};

/// The system cursors the canvas asks for, and the file name of the picture that replaces each.
fn file_for(icon: CursorIcon) -> Option<&'static str> {
    match icon {
        CursorIcon::Crosshair => Some("cross"),
        CursorIcon::Move | CursorIcon::Grab | CursorIcon::Grabbing => Some("move"),
        CursorIcon::ResizeHorizontal => Some("resize_h"),
        CursorIcon::ResizeVertical => Some("resize_v"),
        CursorIcon::ResizeNwSe => Some("resize_nwse"),
        CursorIcon::ResizeNeSw => Some("resize_nesw"),
        _ => None,
    }
}

/// The sizes the pictures are made in, in pixels: 100%, 150%, 200% and 300% display scaling.
const SIZES: [u32; 4] = [32, 48, 64, 96];

macro_rules! pictures {
    ($($name:literal),*) => {
        &[$(($name, [
            include_bytes!(concat!("../assets/cursors/", $name, "-32.png")).as_slice(),
            include_bytes!(concat!("../assets/cursors/", $name, "-48.png")).as_slice(),
            include_bytes!(concat!("../assets/cursors/", $name, "-64.png")).as_slice(),
            include_bytes!(concat!("../assets/cursors/", $name, "-96.png")).as_slice(),
        ])),*]
    };
}

/// The compiled-in PNG files of every cursor, one per entry of [`SIZES`].
const PICTURES: &[(&str, [&[u8]; 4])] =
    pictures!("cross", "move", "resize_h", "resize_v", "resize_nwse", "resize_nesw");
/// `name x y` per line: where each cursor points, in pixels of its 32 pixel picture.
const HOTSPOTS: &str = include_str!("../assets/cursors/hotspots.txt");

/// The size of picture for a display scaling: the nearest one.
fn size_for(pixels_per_point: f32) -> usize {
    let wanted = 32.0 * pixels_per_point;
    (0..SIZES.len())
        .min_by(|&a, &b| (SIZES[a] as f32 - wanted).abs().total_cmp(&(SIZES[b] as f32 - wanted).abs()))
        .unwrap_or(0)
}

/// Where a cursor points, in pixels of its 32 pixel picture.
fn hotspot(name: &str) -> [u32; 2] {
    HOTSPOTS
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            if words.next()? != name {
                return None;
            }
            Some([words.next()?.parse().ok()?, words.next()?.parse().ok()?])
        })
        .unwrap_or([16, 16])
}

/// Every cursor picture, decoded once; the same buffers are given to the window system every frame, so that it can tell
/// it already has them.
fn pictures() -> &'static HashMap<&'static str, Vec<CustomCursorImage>> {
    static DECODED: OnceLock<HashMap<&'static str, Vec<CustomCursorImage>>> = OnceLock::new();
    DECODED.get_or_init(|| {
        let mut all = HashMap::new();
        for (name, files) in PICTURES {
            let [hx, hy] = hotspot(name);
            let images = files
                .iter()
                .zip(SIZES)
                .filter_map(|(png, size)| {
                    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png).ok()?.to_rgba8();
                    let (w, h) = image.dimensions();
                    // the hot spot is given for the 32 pixel picture and scales with it
                    let scaled = |v: u32| (v * size / 32) as u16;
                    Some(CustomCursorImage {
                        rgba: Arc::from(image.into_raw()),
                        size: [w as u16, h as u16],
                        hotspot: [scaled(hx), scaled(hy)],
                    })
                })
                .collect();
            all.insert(*name, images);
        }
        all
    })
}

/// If the canvas asked for a cursor this frame that the app has a picture of, give the window system that picture.
/// Otherwise nothing is set and the system cursor stays.
pub fn replace_system_cursor(ctx: &Context) {
    let icon = ctx.output(|o| o.cursor_icon);
    let Some(name) = file_for(icon) else { return };
    if let Some(image) = pictures().get(name).and_then(|sizes| sizes.get(size_for(ctx.pixels_per_point()))) {
        ctx.set_cursor_image(Some(image.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursors_the_canvas_uses_have_pictures_and_the_rest_are_left_to_the_system() {
        for icon in [
            CursorIcon::Crosshair,
            CursorIcon::Grab,
            CursorIcon::Grabbing,
            CursorIcon::Move,
            CursorIcon::ResizeHorizontal,
            CursorIcon::ResizeVertical,
            CursorIcon::ResizeNwSe,
            CursorIcon::ResizeNeSw,
        ] {
            let name = file_for(icon).unwrap_or_else(|| panic!("{icon:?}"));
            assert!(pictures().contains_key(name), "{name}");
        }
        for icon in [CursorIcon::Default, CursorIcon::Text, CursorIcon::PointingHand, CursorIcon::None] {
            assert_eq!(file_for(icon), None, "{icon:?}");
        }
    }

    #[test]
    fn every_cursor_has_a_picture_at_every_size_with_its_hot_spot_inside() {
        for (name, _) in PICTURES {
            let images = &pictures()[name];
            assert_eq!(images.len(), SIZES.len(), "{name}");
            for (image, size) in images.iter().zip(SIZES) {
                assert_eq!(image.size, [size as u16, size as u16], "{name}");
                assert_eq!(image.rgba.len(), (size * size * 4) as usize);
                assert!(image.hotspot[0] < image.size[0] && image.hotspot[1] < image.size[1], "{name} at {size}");
                assert!(image.rgba.chunks(4).any(|p| p[3] == 255 && p[0] < 110), "{name} at {size} has a dark core");
                assert!(image.rgba.chunks(4).any(|p| p[3] == 255 && p[0] > 215), "{name} at {size} has a white halo");
                assert!(image.rgba.chunks(4).any(|p| p[3] == 0), "{name} is transparent around it");
            }
        }
    }

    #[test]
    fn the_hot_spot_scales_with_the_picture_and_the_crosshair_points_at_its_gap() {
        let images = &pictures()["cross"];
        assert_eq!(images[0].hotspot, [16, 16]);
        assert_eq!(images[2].hotspot, [32, 32]);
        // the pixel the crosshair points at is inside its gap: nothing is drawn there
        let alpha_at_hotspot = |i: &CustomCursorImage| {
            let (x, y) = (i.hotspot[0] as usize, i.hotspot[1] as usize);
            i.rgba[(y * i.size[0] as usize + x) * 4 + 3]
        };
        assert_eq!(alpha_at_hotspot(&images[0]), 0);
        assert_eq!(alpha_at_hotspot(&images[3]), 0);
        assert_eq!(hotspot("no such cursor"), [16, 16]);
    }

    #[test]
    fn the_picture_follows_the_display_scaling() {
        let sizes = [1.0, 1.25, 1.5, 2.0, 2.5, 3.0, 4.0].map(|scale| SIZES[size_for(scale)]);
        assert_eq!(sizes, [32, 32, 48, 64, 64, 96, 96]);
    }
}
