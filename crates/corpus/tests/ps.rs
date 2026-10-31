//! The PostScript importer on a matplotlib figure (4 x 3 inches: an &eps with a 288 x 216 pt bounding box).

use kurbo::Rect;
use plotextract_core::{Bitmap, Importer, PrimitiveKind, Unit};
use plotextract_corpus::{fixture, import, picture};
use plotextract_ps::PsImporter;

/// Pixels per point of the reference picture.
const K: f64 = 4.0;

#[test]
fn recognizes_postscript_only() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    assert!(PsImporter.sniff(&eps));
    assert!(PsImporter.sniff(b"%!PS\n0 0 moveto"));
    assert!(!PsImporter.sniff(b"%PDF-1.7"));
    assert!(!PsImporter.sniff(b"<svg xmlns="));
    assert_eq!(PsImporter.extensions(), ["ps", "eps", "epsf"]);
}

#[test]
fn page_is_the_bounding_box_and_paths_become_primitives() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    let figure = import(&PsImporter, &eps).unwrap();
    assert_eq!((figure.width.round(), figure.height.round()), (288.0, 216.0));
    assert_eq!(figure.unit, Unit::Point);
    let strokes = figure.primitives.iter().filter(|p| p.kind == PrimitiveKind::Stroke).count();
    assert!(strokes >= 10, "ticks, spines and the curve: {strokes}");
    for p in &figure.primitives {
        let e = p.extent().unwrap();
        assert!(
            e.x0 >= -1.0 && e.x1 <= figure.width + 1.0 && e.y0 >= -1.0 && e.y1 <= figure.height + 1.0,
            "{}",
            p.name()
        );
    }
    let curve = figure.primitives.iter().find(|p| p.style.color == Some([31, 119, 180, 255]) && p.points.len() > 20);
    assert!(curve.is_some(), "the data curve, in its colour");
}

fn whole_page(eps: &[u8], width: f64, height: f64) -> Bitmap {
    let mut picture = picture(&PsImporter, eps).expect("PostScript draws pictures");
    picture.render(Rect::new(0.0, 0.0, width, height), K).unwrap()
}

#[test]
fn page_y_runs_down_and_lines_up_with_the_picture() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    // the x axis is at the bottom of the page: the curve's start is at the left edge of the axes, vertically mid-plot
    let figure = import(&PsImporter, &eps).unwrap();
    let curve =
        figure.primitives.iter().find(|p| p.style.color == Some([31, 119, 180, 255]) && p.points.len() > 20).unwrap();
    let back = whole_page(&eps, figure.width, figure.height);
    let blue_near = |x: f64, y: f64| {
        let (px, py) = ((x * K) as i64, (y * K) as i64);
        (-3..=3).any(|dy| {
            (-3..=3).any(|dx| {
                let (xx, yy) = (px + dx, py + dy);
                if xx < 0 || yy < 0 || xx >= i64::from(back.width) || yy >= i64::from(back.height) {
                    return false;
                }
                let i = ((yy as usize * back.width as usize) + xx as usize) * 4;
                let p = &back.rgba[i..i + 3];
                p[2] > 120 && p[0] < 100 // blue ink
            })
        })
    };
    let on_ink = curve.points.iter().filter(|p| blue_near(p.x, p.y)).count();
    assert!(on_ink * 10 >= curve.points.len() * 9, "{on_ink} of {} curve vertices sit on blue ink", curve.points.len());
}

#[test]
fn regions_render_the_same_picture_at_any_scale() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    let figure = import(&PsImporter, &eps).unwrap();
    let back = whole_page(&eps, figure.width, figure.height);
    assert_eq!((back.width, back.height), ((figure.width * K).ceil() as u32, (figure.height * K).ceil() as u32));
    assert!(back.rgba.chunks(4).all(|p| p[3] == 255), "opaque on white");
    assert!(back.rgba.chunks(4).any(|p| p[0] < 100), "the plot has dark ink");

    // 3x on a 60 x 60 pt patch with part of the curve, box-filtered down, is the page picture's patch
    let mut picture = picture(&PsImporter, &eps).unwrap();
    let region = Rect::new(60.0, 20.0, 120.0, 80.0);
    let zoomed = picture.render(region, 3.0 * K).unwrap();
    let (n, m) = ((60.0 * K) as usize, 3usize);
    assert_eq!((zoomed.width as usize, zoomed.height as usize), (n * m, n * m));
    let (px, py) = ((region.x0 * K) as usize, (region.y0 * K) as usize);
    let mut total = 0u64;
    for y in 0..n {
        for x in 0..n {
            for c in 0..3 {
                let mut sum = 0u32;
                for dy in 0..m {
                    for dx in 0..m {
                        sum += u32::from(zoomed.rgba[(((y * m + dy) * n * m + x * m + dx) * 4) + c]);
                    }
                }
                let want = back.rgba[(((py + y) * back.width as usize + px + x) * 4) + c];
                total += u64::from(((sum / 9) as u8).abs_diff(want));
            }
        }
    }
    let mean = total as f64 / (n * n * 3) as f64;
    assert!(mean < 4.0, "mean difference {mean:.2} of 255: the zoomed render must show the same picture");
    assert!(zoomed.rgba.chunks(4).any(|p| p[0] < 100), "the patch has ink");
}
