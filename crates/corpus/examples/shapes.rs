//! Find the shapes the dark (axes and text) colour of an image is drawn with, as the wizard's Shapes step does: cut the
//! frame out, break the colour into pieces and group them. Say what was found and how long it took, and write a picture
//! of the pieces coloured by group (the leftovers grey).
//!
//!     cargo run --release -p plotextract-corpus --example shapes -- image.png [out.png]

use std::time::Instant;

use image::{Rgba, RgbaImage};
use plotextract_algo::curves::{self, Source};
use plotextract_algo::lines::{self, Algorithm, Line, Orientation};
use plotextract_algo::shapes;
use plotextract_algo::{ColourParams, analyse_colours, decode};
use plotextract_domain::{Mask, Prepare};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: shapes <image> [out.png]");
    let image = decode::decode(&std::fs::read(path).expect("cannot read the file")).expect("cannot decode the image");
    let map = analyse_colours(&image, &Prepare::default(), &ColourParams::default());
    let dark: Vec<usize> = map
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
    for &ink in &dark {
        for (cell, c) in data.iter_mut().zip(map.mask_of(ink).data()) {
            *cell = (*cell).max(*c);
        }
    }
    let mask = Mask::from_data(crop, data);
    let t = Instant::now();
    // the frame: long lines that meet another at a corner, as the wizard finds it
    let found = lines::detect(&mask, Algorithm::default(), &lines::Params::default());
    let long: Vec<Line> = found
        .iter()
        .copied()
        .filter(|l| {
            let side = if l.orientation == Orientation::Horizontal { crop.width } else { crop.height };
            3.0 * l.length() >= side as f32
        })
        .collect();
    let corner = |a: &Line, b: &Line| {
        let reach = 1.5 + (a.width + b.width) / 2.0;
        let ends_at = |l: &Line, at: f32| (l.from - at).abs() <= reach || (l.to - at).abs() <= reach;
        a.orientation != b.orientation && ends_at(a, b.across) && ends_at(b, a.across)
    };
    let cut: Vec<_> = long.iter().filter(|a| long.iter().any(|b| corner(a, b))).map(Line::band).collect();
    let lines_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let mut pieces = curves::find(&[Source { ink: 0, mask, cut: cut.clone() }], &curves::Params::default());
    let found_ms = t.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let shapes = shapes::classify(&mut pieces, &shapes::Params::default());
    let shapes_ms = t.elapsed().as_secs_f64() * 1e3;
    println!(
        "{path}: {} x {}, frame sides {}, {} pieces; lines {lines_ms:.0} ms, pieces {found_ms:.0} ms, shapes {shapes_ms:.0} ms",
        image.width(),
        image.height(),
        cut.len(),
        pieces.pieces().len()
    );
    for (k, t) in shapes.groups() {
        let l = &t.look;
        println!(
            "  group {:<3} {:12} x{:<4} area {:6.0}  {:5.1} x {:5.1}  filled {:.2}  upright {:.2}  holes {}  stroke {:4.1}  \
             spacing {}",
            k + 1,
            t.form().name(),
            t.members.len(),
            l.area,
            l.length,
            l.breadth,
            l.filled(),
            l.upright,
            l.holes,
            l.stroke,
            t.spacing.map_or("-".into(), |s| format!("{s:.1}"))
        );
    }
    println!("  {} pieces left over", shapes.leftovers().len());
    if let Some(out) = args.get(1) {
        let mut picture = RgbaImage::from_pixel(image.width(), image.height(), Rgba([255, 255, 255, 255]));
        const PALETTE: [[u8; 3]; 6] =
            [[220, 0, 0], [240, 130, 0], [0, 110, 230], [0, 150, 60], [200, 0, 200], [0, 170, 170]];
        for y in crop.y..crop.y + crop.height {
            for x in crop.x..crop.x + crop.width {
                if let Some(p) = pieces.at(x, y) {
                    let [r, g, b] = shapes.type_of[p].map_or([150, 150, 150], |k| PALETTE[k % PALETTE.len()]);
                    picture.put_pixel(x, y, Rgba([r, g, b, 255]));
                }
            }
        }
        picture.save(out).expect("cannot write the picture");
    }
}
