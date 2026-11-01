//! Find the straight lines in the axes colour of an image with each algorithm, say what was found and how long it took,
//! and write a picture of the lines found.
//!
//!     cargo run --release -p plotextract-corpus --example lines -- image.png [outdir] [--list]

use std::time::Instant;

use image::Rgba;
use plotextract_algo::lines::{self, Algorithm, Orientation};
use plotextract_algo::{ColourParams, analyse_colours, decode};
use plotextract_domain::{Mask, Prepare};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: lines <image> [outdir] [--list]");
    let list = args.iter().any(|a| a == "--list");
    let out = args.iter().skip(1).find(|a| !a.starts_with("--"));
    let image = decode::decode(&std::fs::read(path).expect("cannot read the file")).expect("cannot decode the image");
    let map = analyse_colours(&image, &Prepare::default(), &ColourParams::default());
    // the inks that are dark greys: what the wizard first calls axes and text
    let axes: Vec<usize> = map
        .inks
        .iter()
        .enumerate()
        .filter(|(_, ink)| {
            let [r, g, b] = ink.rgb;
            r.max(g).max(b) - r.min(g).min(b) < 40 && (u16::from(r) + u16::from(g) + u16::from(b)) / 3 < 110
        })
        .map(|(i, _)| i)
        .collect();
    let crop = map.crop;
    let mut data = vec![0u8; crop.width as usize * crop.height as usize];
    for &ink in &axes {
        let m = map.mask_of(ink);
        for (cell, c) in data.iter_mut().zip(m.data()) {
            *cell = (*cell).max(*c);
        }
    }
    let mask = Mask::from_data(crop, data);
    println!("{path}: {} x {}, axes inks {axes:?}, {} px", image.width(), image.height(), mask.pixels());
    for algorithm in Algorithm::ALL {
        let t = Instant::now();
        let found = lines::detect(&mask, algorithm, &lines::Params::default());
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let h = found.iter().filter(|l| l.orientation == Orientation::Horizontal).count();
        let longest = found.iter().map(|l| l.length()).fold(0.0, f32::max);
        println!(
            "  {:20} {:4} lines ({h} horizontal), longest {longest:.0}, {ms:.0} ms",
            algorithm.label(),
            found.len()
        );
        if list {
            for l in &found {
                println!("    {:?} {:.1}..{:.1} at {:.2} w{:.1}", l.orientation, l.from, l.to, l.across, l.width);
            }
        }
        if let Some(dir) = out {
            let mut picture = image.clone();
            for l in &found {
                let b = l.band();
                for y in b.y..(b.y + b.height).min(picture.height()) {
                    for x in b.x..(b.x + b.width).min(picture.width()) {
                        picture.put_pixel(x, y, Rgba([255, 0, 0, 255]));
                    }
                }
            }
            let name = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
            let file = format!("{dir}/{name}_{}.png", format!("{algorithm:?}").to_lowercase());
            picture.save(&file).expect("cannot write the picture");
        }
    }
}
