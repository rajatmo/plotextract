//! What Suggest ticks finds on images: for each, the scale and the two ticks of each axis it sets, printed, and the
//! image with those ticks drawn on it (x in red, y in green, the min tick's line thicker), to check by eye.
//!
//!     cargo run --release -p plotextract-corpus --example suggest -- OUT_DIR image.png ...

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use image::Rgba;
use plotextract_algo::lines::Orientation;
use plotextract_domain::Prepare;
use plotextract_ticks::OnnxTicks;
use plotextract_usecases::wizard::suggest::{SuggestedAxis, TickSearch};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((out, files)) = args.split_first() else {
        eprintln!("usage: suggest OUT_DIR image.png ...");
        std::process::exit(2);
    };
    std::fs::create_dir_all(out).unwrap();
    let models = Arc::new(OnnxTicks::new());
    let (mut both, mut one) = (0, 0);
    for file in files {
        let path = Path::new(file);
        let name = path.file_stem().unwrap().to_string_lossy();
        let mut image = plotextract_algo::decode::decode(&std::fs::read(path).unwrap()).unwrap();
        let start = Instant::now();
        let s = TickSearch::new(models.clone(), Prepare::default()).run(&image).unwrap();
        let took = start.elapsed();
        let axis = |a: &Option<SuggestedAxis>| match a {
            Some(a) => format!(
                "{} {} at {:.1} .. {} at {:.1} ({}/{} agree)",
                if a.log { "log" } else { "lin" },
                a.min.value,
                a.min.line.across,
                a.max.value,
                a.max.line.across,
                a.fit,
                a.of
            ),
            None => "-".into(),
        };
        println!("{name}: {:.0?}\n  x {}\n  y {}", took, axis(&s.axes[0]), axis(&s.axes[1]));
        match s.axes.iter().flatten().count() {
            2 => both += 1,
            1 => one += 1,
            _ => {}
        }
        let (w, h) = image.dimensions();
        for (a, colour) in s.axes.iter().zip([Rgba([230, 0, 0, 255]), Rgba([0, 160, 0, 255])]) {
            let Some(a) = a else { continue };
            for (tick, thick) in [(a.min, 3i64), (a.max, 1)] {
                let at = tick.line.across.round() as i64;
                for d in -thick..=thick {
                    for t in 0..i64::from(w.max(h)) {
                        let (x, y) = match tick.line.orientation {
                            Orientation::Vertical => (at + d, t),
                            Orientation::Horizontal => (t, at + d),
                        };
                        if (0..i64::from(w)).contains(&x) && (0..i64::from(h)).contains(&y) && (t % 6) < 3 {
                            image.put_pixel(x as u32, y as u32, colour);
                        }
                    }
                }
            }
        }
        image.save(Path::new(out).join(format!("{name}.png"))).unwrap();
    }
    println!("{} images: both axes suggested on {both}, one on {one}", files.len());
}
