//! How responsive the tracing wizard is on a big image: how long each background job takes, how long the work done on the
//! window's thread in answer to a click takes, and how long a frame of the canvas takes on each step, fitted and zoomed
//! in, the first frame (pictures made) and the next (pictures kept).
//!
//!     cargo test --release -p plotextract how_responsive -- --ignored --nocapture
//!
//! The image is drawn here (3000 x 2400: a framed plot with ticks, a dotted curve, a dashed curve, markers, a solid line
//! and rows of text-like marks, all in black but the dashes), or read from `PLOTEXTRACT_BENCH_IMAGE` if set.

use std::sync::Arc;
use std::time::Instant;

use egui::{Context, Pos2, RawInput, pos2, vec2};
use image::{Rgba, RgbaImage};
use kurbo::Point;
use plotextract_raster::shapes::Kind;

use crate::view::View;
use crate::wizard::{MaskId, Role, Step, Wizard, WizardIntent};
use crate::wizard_canvas::WizardCanvas;

/// The plot to trace.
fn image() -> RgbaImage {
    if let Ok(path) = std::env::var("PLOTEXTRACT_BENCH_IMAGE") {
        return image::open(path).expect("the image").to_rgba8();
    }
    let (w, h) = (3000u32, 2400u32);
    let mut img = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
    let black = Rgba([0, 0, 0, 255]);
    let put = |img: &mut RgbaImage, x: i64, y: i64, c: Rgba<u8>| {
        if x >= 0 && y >= 0 && x < i64::from(w) && y < i64::from(h) {
            img.put_pixel(x as u32, y as u32, c);
        }
    };
    let (x0, y0, x1, y1) = (300i64, 150i64, 2850i64, 2150i64);
    // the frame, 4 pixels, and ticks inward every 100 pixels
    for t in 0..4 {
        for x in x0..=x1 {
            put(&mut img, x, y0 + t, black);
            put(&mut img, x, y1 - t, black);
        }
        for y in y0..=y1 {
            put(&mut img, x0 + t, y, black);
            put(&mut img, x1 - t, y, black);
        }
    }
    for k in 1..25 {
        for d in 0..18 {
            for t in 0..3 {
                put(&mut img, x0 + 100 * k + t, y1 - d, black);
                put(&mut img, x0 + d, y1 - 80 * k + t, black);
            }
        }
    }
    // a dotted curve: squares of 10, 26 pixels apart along a sine
    let mut s = 0.0f64;
    let mut last = (x0 as f64 + 20.0, 1000.0);
    let mut x = x0 as f64 + 20.0;
    while x < (x1 - 20) as f64 {
        let y = 1000.0 + 500.0 * (x / 300.0).sin();
        s += (x - last.0).hypot(y - last.1);
        last = (x, y);
        if s >= 26.0 {
            s = 0.0;
            for dy in -5..5 {
                for dx in -5..5 {
                    put(&mut img, x as i64 + dx, y as i64 + dy, black);
                }
            }
        }
        x += 0.25;
    }
    // a dashed blue curve
    let blue = Rgba([31, 119, 180, 255]);
    for xi in (x0 + 20)..(x1 - 20) {
        if (xi / 40) % 3 == 2 {
            continue;
        }
        let y = 1600.0 + 300.0 * (xi as f64 / 500.0).cos();
        for t in -3..3 {
            put(&mut img, xi, y as i64 + t, blue);
        }
    }
    // a solid line, markers (rings) and rows of text-like marks
    for xi in (x0 + 20)..(x1 - 20) {
        for t in 0..4 {
            put(&mut img, xi, 400 + (xi - x0) / 10 + t, black);
        }
    }
    for k in 0..40 {
        let (cx, cy) = (x0 as f64 + 60.0 + 60.0 * f64::from(k), 1900.0 - 3.0 * f64::from(k));
        for a in 0..360 {
            let r = (f64::from(a)).to_radians();
            for rr in [12.0, 13.0, 14.0] {
                put(&mut img, (cx + rr * r.cos()) as i64, (cy + rr * r.sin()) as i64, black);
            }
        }
    }
    for row in 0..6 {
        for c in 0..30 {
            let (gx, gy) = (500 + 26 * c as i64, 250 + 40 * row as i64);
            let shape = (c * 7 + row * 3) % 5;
            for dy in 0..20 {
                for dx in 0..14 {
                    let on = match shape {
                        0 => dx < 3 || dy < 3,
                        1 => !(3..=16).contains(&dy) || dx < 3,
                        2 => (dx - 7i64).abs() < 2,
                        3 => !(3..=10).contains(&dx) || (dy - 10i64).abs() < 2,
                        _ => (dx + dy) % 9 < 2,
                    };
                    if on {
                        put(&mut img, gx + dx, gy + dy, black);
                    }
                }
            }
        }
    }
    img
}

/// Run `f` and say how long it took, in milliseconds.
fn time<T>(what: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let out = f();
    println!("  {what:<52} {:>8.1} ms", t.elapsed().as_secs_f64() * 1e3);
    out
}

/// Run the jobs the wizard asks for, timing each.
fn settle(w: &mut Wizard) {
    let mut id = 1000;
    loop {
        if let Some(search) = w.take_lines_request() {
            id += 1;
            w.begin_lines(id);
            let found = time("job: straight lines", || search.run());
            w.finish_lines(id, found);
        } else if let Some(search) = w.take_pieces_request() {
            id += 1;
            w.begin_pieces(id);
            let found = time("job: pieces and shapes", || search.run());
            time("window: take the pieces (propose, trace)", || w.finish_pieces(id, found));
        } else {
            break;
        }
    }
}

/// One frame of the canvas; how long it took.
fn frame(ctx: &Context, canvas: &mut WizardCanvas, view: &View, wizard: &Wizard) -> f64 {
    let input = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), vec2(1500.0, 1000.0))),
        ..Default::default()
    };
    let t = Instant::now();
    let mut output = ctx.run_ui(input, |ui| {
        let rect = ui.max_rect();
        canvas.paint(ui.painter(), view, rect.min, rect, wizard);
    });
    let took = t.elapsed().as_secs_f64() * 1e3;
    output.textures_delta.clear();
    took
}

/// The canvas on the wizard's step, fitted and zoomed in: the first frame and the median of the next ten.
fn frames(wizard: &Wizard, label: &str) {
    let ctx = Context::default();
    let size = wizard.size();
    let figure = (f64::from(size.0), f64::from(size.1));
    let mut fitted = View::default();
    fitted.fit(vec2(1500.0, 1000.0), figure);
    let mut zoomed = View::default();
    zoomed.fit(vec2(1500.0, 1000.0), figure);
    zoomed.zoom_about(Pos2::ZERO, pos2(750.0, 500.0), 8.0, figure);
    for (name, view) in [("fitted", fitted), ("zoomed", zoomed)] {
        let mut canvas = WizardCanvas::default();
        let first = frame(&ctx, &mut canvas, &view, wizard);
        let mut next: Vec<f64> = (0..10).map(|_| frame(&ctx, &mut canvas, &view, wizard)).collect();
        next.sort_by(f64::total_cmp);
        println!("  frame, {label}, {name:<6}: first {first:>7.1} ms, then {:>6.2} ms", next[5]);
    }
}

#[test]
#[ignore]
fn how_responsive_the_wizard_is() {
    let img = image();
    println!("image {} x {}", img.width(), img.height());
    let mut w = Wizard::new(img.width(), img.height());
    w.apply(WizardIntent::GoTo(Step::Colours));
    let params = w.take_analysis_request().unwrap();
    w.begin_analysis(1);
    let map = time("job: colours", || plotextract_raster::analyse_colours(&img, w.prepare(), &params));
    w.finish_analysis(1, Ok(Arc::new(map)));
    let inks = w.colours().map().unwrap().inks.len();
    for i in 0..inks {
        let rgb = w.colours().map().unwrap().inks[i].rgb;
        if rgb.iter().all(|&c| c < 110) || rgb == [31, 119, 180] {
            w.apply(WizardIntent::SetRole(i, Role::Curve, true));
        }
        w.apply(WizardIntent::Show(MaskId::Ink(i), true));
    }
    frames(&w, "colours");
    {
        let map = w.colours().map().unwrap().clone();
        let curve: Vec<usize> = (0..inks).filter(|&i| w.colours().has_role(i, Role::Curve)).collect();
        let masks = time("job part: the masks of the curve colours", || {
            curve.iter().map(|&i| map.mask_of(i)).collect::<Vec<_>>()
        });
        let sources: Vec<_> = curve
            .iter()
            .zip(masks)
            .map(|(&ink, mask)| plotextract_raster::curves::Source { ink, mask, cut: Vec::new() })
            .collect();
        let mut pieces =
            time("job part: the pieces", || plotextract_raster::curves::find(&sources, &Default::default()));
        time("job part: the shapes", || plotextract_raster::shapes::classify(&mut pieces, &Default::default()));
    }
    time("window: go to the plot area", || w.apply(WizardIntent::GoTo(Step::PlotArea)));
    settle(&mut w);
    println!("  {} lines found", w.lines().len());
    frames(&w, "plot area");
    time("window: go to the shapes", || w.apply(WizardIntent::GoTo(Step::Shapes)));
    settle(&mut w);
    let shapes = w.shapes().unwrap();
    println!(
        "  {} pieces, {} shape types, {} chains",
        w.pieces().unwrap().pieces().len(),
        shapes.types.len(),
        shapes.chains.len()
    );
    frames(&w, "shapes");
    time("window: the masks panel's list (each frame)", || w.masks().len());
    time("window: the masks panel's views (each frame)", || {
        w.visible_masks().into_iter().map(|m| w.view(m)).collect::<Vec<_>>().len()
    });
    let dots = w.shapes().unwrap().types.iter().position(|t| t.kind == Kind::Dot);
    if let Some(k) = dots {
        time("window: say a shape is markers", || w.apply(WizardIntent::SetKind(k, Kind::Marker)));
        time("window: and dots again", || w.apply(WizardIntent::SetKind(k, Kind::Dot)));
    }
    time("window: go to the curves (propose, trace)", || w.apply(WizardIntent::GoTo(Step::Curves)));
    settle(&mut w);
    println!("  {} series, {} points", w.series().len(), w.series().iter().map(|s| s.points().len()).sum::<usize>());
    frames(&w, "curves");
    time("window: the masks panel's list (each frame)", || w.masks().len());
    let biggest =
        w.series().iter().max_by_key(|s| s.points().len()).map(|s| (s.number(), s.points()[s.points().len() / 2]));
    if let Some((number, p)) = biggest {
        w.apply(WizardIntent::SetActive(MaskId::Series(number)));
        // the frame after a click: a point (drawn on its own) and a piece (the picture of the pieces is made again)
        let ctx = Context::default();
        let mut view = View::default();
        view.fit(vec2(1500.0, 1000.0), (f64::from(w.size().0), f64::from(w.size().1)));
        let mut canvas = WizardCanvas::default();
        frame(&ctx, &mut canvas, &view, &w);
        w.apply(WizardIntent::AddPoint(number, Point::new(p.x + 5.0, p.y)));
        println!("  frame after a point is put in:    {:>7.2} ms", frame(&ctx, &mut canvas, &view, &w));
        let piece = w.series().iter().find(|s| s.number() == number).and_then(|s| s.pieces().last().copied());
        if let Some(piece) = piece {
            w.apply(WizardIntent::SetPiece(piece, None));
            println!("  frame after a piece is taken out: {:>7.2} ms", frame(&ctx, &mut canvas, &view, &w));
            w.apply(WizardIntent::SetPiece(piece, Some(number)));
        }
        time("window: take a point out of the biggest series", || w.apply(WizardIntent::RemovePoint(number, p)));
        time("window: put a point in", || w.apply(WizardIntent::AddPoint(number, Point::new(p.x + 3.0, p.y))));
        let piece = w.series().iter().find(|s| s.number() == number).and_then(|s| s.pieces().first().copied());
        if let Some(piece) = piece {
            time("window: take a piece out", || w.apply(WizardIntent::SetPiece(piece, None)));
        }
        time("window: write the series as CSV", || w.series_csv(number).map(|c| c.len()).unwrap_or(0));
    }
}
