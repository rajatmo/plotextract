//! Find the ink colours of an image the way the tracing wizard does, and say what was found and how long it took.
//!
//!     cargo run --release -p plotextract-corpus --example colours -- image.png

use std::time::Instant;

use plotextract_raster::{ColourParams, Prepare, analyse_colours, decode};

fn main() {
    let path = std::env::args().nth(1).expect("usage: colours <image>");
    let bytes = std::fs::read(&path).expect("cannot read the file");
    let t = Instant::now();
    let image = decode::decode(&bytes).expect("cannot decode the image");
    let decoded = t.elapsed();
    let t = Instant::now();
    let map = analyse_colours(&image, &Prepare::default(), &ColourParams::default());
    let known = map.known_pixels() as f64;
    println!(
        "{path}: {} x {}, decoded in {:.0} ms, analysed in {:.0} ms; background {:?}",
        image.width(),
        image.height(),
        decoded.as_secs_f64() * 1e3,
        t.elapsed().as_secs_f64() * 1e3,
        map.background
    );
    for (i, ink) in map.inks.iter().enumerate() {
        println!("  {:2}  {:3?}  {:5.2}% of the image", i + 1, ink.rgb, 100.0 * ink.pixels as f64 / known);
    }
}
