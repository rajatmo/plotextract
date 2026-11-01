//! The &svg importer on a matplotlib figure (4 x 3 inches: 288 x 216 pt, which &svg counts in CSS pixels: 384 x 288).

use kurbo::Rect;
use plotextract_corpus::{fixture, import, picture};
use plotextract_domain::{PrimitiveKind, Unit};
use plotextract_ports::Importer;
use plotextract_svg::SvgImporter;

/// Pixels per figure unit of the reference picture.
const K: f64 = 6.0;

#[test]
fn recognizes_svg_only() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    assert!(SvgImporter.sniff(&svg));
    assert!(!SvgImporter.sniff(b"%PDF-1.7"));
    assert!(!SvgImporter.sniff(b"%!PS-Adobe-3.0 EPSF-3.0"));
    assert_eq!(SvgImporter.extensions(), ["svg"]);
}

#[test]
fn units_are_css_pixels_and_paths_become_primitives() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = import(&SvgImporter, &svg).unwrap();
    assert_eq!((figure.width.round(), figure.height.round()), (384.0, 288.0));
    assert_eq!(figure.unit, Unit::Pixel);
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
fn the_picture_covers_the_page_on_white() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = import(&SvgImporter, &svg).unwrap();
    let mut picture = picture(&SvgImporter, &svg).expect("SVG draws pictures");
    let back = picture.render(Rect::new(0.0, 0.0, figure.width, figure.height), K).unwrap();
    assert_eq!(back.width, (figure.width * K).ceil() as u32);
    assert_eq!(back.rgba.len(), (back.width * back.height * 4) as usize);
    assert!(back.rgba.chunks(4).any(|p| p[0] < 100), "the plot has dark ink");
    assert!(back.rgba.chunks(4).all(|p| p[3] == 255), "opaque on white");
}

#[test]
fn regions_render_the_same_picture_at_any_scale() {
    let Some(svg) = fixture("matplotlib/sin_simple.svg") else { return };
    let figure = import(&SvgImporter, &svg).unwrap();
    let mut picture = picture(&SvgImporter, &svg).unwrap();
    let back = picture.render(Rect::new(0.0, 0.0, figure.width, figure.height), K).unwrap();

    // 4x on a 40 x 40 patch with part of the curve, box-filtered down, is the page picture's patch
    let region = Rect::new(80.0, 27.0, 120.0, 67.0);
    let zoomed = picture.render(region, 4.0 * K).unwrap();
    let (n, m) = ((40.0 * K) as usize, 4usize);
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
                total += u64::from(((sum / 16) as u8).abs_diff(want));
            }
        }
    }
    let mean = total as f64 / (n * n * 3) as f64;
    assert!(mean < 4.0, "mean difference {mean:.2} of 255");
    assert!(zoomed.rgba.chunks(4).any(|p| p[0] < 100), "the patch has ink");
}
