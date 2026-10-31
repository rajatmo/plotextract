//! The PostScript importer on a matplotlib figure (4 x 3 inches: an &eps with a 288 x 216 pt bounding box).

use plotextract_core::{Importer, PrimitiveKind};
use plotextract_corpus::fixture;
use plotextract_ps::PsImporter;

#[test]
fn recognizes_postscript_only() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    assert!(PsImporter.sniff(&eps));
    assert!(PsImporter.sniff(b"%!PS\n0 0 moveto"));
    assert!(!PsImporter.sniff(b"%PDF-1.7"));
    assert!(!PsImporter.sniff(b"<svg xmlns="));
}

#[test]
fn page_is_the_bounding_box_and_paths_become_primitives() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    let figure = PsImporter.import(eps.clone()).unwrap();
    assert_eq!((figure.width.round(), figure.height.round()), (288.0, 216.0));
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

#[test]
fn page_y_runs_down_and_lines_up_with_the_picture() {
    let Some(eps) = fixture("matplotlib/sin_simple.eps") else { return };
    // the x axis is at the bottom of the page: the curve's start is at the left edge of the axes, vertically mid-plot
    let figure = PsImporter.import(eps.clone()).unwrap();
    let curve =
        figure.primitives.iter().find(|p| p.style.color == Some([31, 119, 180, 255]) && p.points.len() > 20).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let blue_near = |x: f64, y: f64| {
        let (px, py) = ((x * back.scale) as i64, (y * back.scale) as i64);
        (-3..=3).any(|dy| {
            (-3..=3).any(|dx| {
                let (xx, yy) = (px + dx, py + dy);
                if xx < 0 || yy < 0 || xx >= i64::from(back.bitmap.width) || yy >= i64::from(back.bitmap.height) {
                    return false;
                }
                let i = ((yy as usize * back.bitmap.width as usize) + xx as usize) * 4;
                let p = &back.bitmap.rgba[i..i + 3];
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
    let figure = PsImporter.import(eps.clone()).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let mut renderer = PsImporter.region_renderer(&eps).expect("PostScript can re-render regions");

    // the whole page at the backdrop's scale is the backdrop, up to antialiasing
    let whole = renderer.render(kurbo::Rect::new(0.0, 0.0, figure.width, figure.height), back.scale).unwrap();
    assert_eq!((whole.width, whole.height), (back.bitmap.width, back.bitmap.height));
    let total: u64 = whole.rgba.iter().zip(&back.bitmap.rgba).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    let mean = total as f64 / whole.rgba.len() as f64;
    assert!(mean < 1.0, "mean difference {mean:.3} of 255 between the region render and the backdrop");

    // zoomed in 3x, a patch with part of the curve still has ink where the backdrop has it
    let zoomed = renderer.render(kurbo::Rect::new(60.0, 20.0, 120.0, 80.0), 3.0 * back.scale).unwrap();
    assert_eq!((zoomed.width, zoomed.height), (720, 720));
    assert!(zoomed.rgba.chunks(4).any(|p| p[0] < 100));
    assert!(zoomed.rgba.chunks(4).all(|p| p[3] == 255));
}
