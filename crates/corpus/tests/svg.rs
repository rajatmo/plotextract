//! The &svg importer on a matplotlib figure (4 x 3 inches: 288 x 216 pt, which &svg counts in CSS pixels: 384 x 288).

use plotextract_core::{Importer, PrimitiveKind};
use plotextract_corpus::fixture;
use plotextract_svg::SvgImporter;

#[test]
fn recognizes_svg_only() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    assert!(SvgImporter.sniff(&svg));
    assert!(!SvgImporter.sniff(b"%PDF-1.7"));
    assert!(!SvgImporter.sniff(b"%!PS-Adobe-3.0 EPSF-3.0"));
}

#[test]
fn units_are_css_pixels_and_paths_become_primitives() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = SvgImporter.import(svg.clone()).unwrap();
    assert_eq!((figure.width.round(), figure.height.round()), (384.0, 288.0));
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
    // the sine curve is the one long blue stroke
    let curve = figure.primitives.iter().find(|p| p.style.color == Some([31, 119, 180, 255]) && p.points.len() > 20);
    assert!(curve.is_some(), "the data curve, in its colour");
}

#[test]
fn backdrop_covers_the_page() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = SvgImporter.import(svg.clone()).unwrap();
    let back = figure.backdrop.unwrap();
    assert_eq!(back.bitmap.width, (figure.width * back.scale).ceil() as u32);
    assert_eq!(back.bitmap.rgba.len(), (back.bitmap.width * back.bitmap.height * 4) as usize);
    assert!(back.bitmap.rgba.chunks(4).any(|p| p[0] < 100), "the plot has dark ink");
    assert!(back.bitmap.rgba.chunks(4).all(|p| p[3] == 255), "opaque on white");
}

#[test]
fn regions_render_the_same_picture_at_any_scale() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = SvgImporter.import(svg.clone()).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let mut renderer = SvgImporter.region_renderer(&svg).expect("&svg can re-render regions");

    let whole = renderer.render(kurbo::Rect::new(0.0, 0.0, figure.width, figure.height), back.scale).unwrap();
    assert!(whole.rgba == back.bitmap.rgba, "the whole page at the backdrop's scale is the backdrop");

    // 4x on a 40 x 40 patch with part of the curve, box-filtered down, is the backdrop's patch
    let k = back.scale;
    let region = kurbo::Rect::new(80.0, 27.0, 120.0, 67.0);
    let zoomed = renderer.render(region, 4.0 * k).unwrap();
    let (n, m) = ((40.0 * k) as usize, 4usize);
    assert_eq!((zoomed.width as usize, zoomed.height as usize), (n * m, n * m));
    let (px, py) = ((region.x0 * k) as usize, (region.y0 * k) as usize);
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
                let want = back.bitmap.rgba[(((py + y) * back.bitmap.width as usize + px + x) * 4) + c];
                total += u64::from(((sum / 16) as u8).abs_diff(want));
            }
        }
    }
    let mean = total as f64 / (n * n * 3) as f64;
    assert!(mean < 4.0, "mean difference {mean:.2} of 255");
    assert!(zoomed.rgba.chunks(4).any(|p| p[0] < 100), "the patch has ink");
}
