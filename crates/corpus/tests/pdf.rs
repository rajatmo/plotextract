//! The sin plot made by Mathematica, checked against what the Python prototype extracted from it.

use plotextract_core::{Importer, PrimitiveKind};
use plotextract_corpus::fixture;
use plotextract_pdf::{BEZIER_STEPS, PdfImporter, extract};

#[test]
fn page_size_and_curve_counts() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let page = extract(sin.clone(), BEZIER_STEPS).unwrap();
    assert_eq!((page.width, page.height), (260.0, 166.0));
    let count = |kind| page.primitives.iter().filter(|c| c.kind == kind).count();
    assert_eq!(count(PrimitiveKind::Stroke), 18);
    assert_eq!(count(PrimitiveKind::Fill), 1);
}

#[test]
fn strokes_come_first_and_are_indexed_per_kind() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let page = extract(sin.clone(), BEZIER_STEPS).unwrap();
    let names: Vec<_> = page.primitives.iter().map(|c| c.name()).collect();
    assert_eq!(names.first().map(String::as_str), Some("stroke[0]"));
    assert_eq!(names.last().map(String::as_str), Some("fill[0]"));
}

#[test]
fn breaks_point_inside_the_primitive() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    for c in &extract(sin.clone(), BEZIER_STEPS).unwrap().primitives {
        assert!(c.breaks.iter().all(|&b| b > 0 && b < c.points.len()), "{}", c.name());
    }
}

#[test]
fn importer_recognizes_pdf_only() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    assert!(PdfImporter.sniff(&sin));
    assert!(!PdfImporter.sniff(b"<svg xmlns="));
}

#[test]
fn imported_figure_is_y_down_and_has_a_backdrop() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = PdfImporter.import(sin.clone()).unwrap();
    assert_eq!((figure.width, figure.height), (260.0, 166.0));
    assert_eq!(figure.primitives.len(), 19);

    for c in &figure.primitives {
        let e = c.extent().unwrap();
        assert!(
            e.x0 >= -1.0 && e.x1 <= figure.width + 1.0 && e.y0 >= -1.0 && e.y1 <= figure.height + 1.0,
            "{}",
            c.name()
        );
    }

    let back = figure.backdrop.expect("the page is rendered");
    assert_eq!(back.bitmap.rgba.len(), (back.bitmap.width * back.bitmap.height * 4) as usize);
    assert_eq!(back.bitmap.width, (figure.width * back.scale).floor() as u32);
    assert!(back.bitmap.rgba.chunks(4).all(|px| px[3] == 255), "opaque on white");
    // the page is not blank: the plot has dark strokes on white
    assert!(back.bitmap.rgba.chunks(4).any(|px| px[0] < 128));
    assert!(back.bitmap.rgba.chunks(4).any(|px| px[0] == 255));
}

#[test]
fn imported_strokes_line_up_with_the_rendered_page() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    // The first stroke is dark ink on the page, so the pixel under one of its vertices must not be white.
    let figure = PdfImporter.import(sin.clone()).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let dark_under = |p: &plotextract_core::Primitive| {
        p.points.iter().any(|v| {
            let (x, y) = ((v.x * back.scale) as u32, (v.y * back.scale) as u32);
            (x < back.bitmap.width && y < back.bitmap.height) && {
                // look in a 3-pixel neighbourhood: strokes are antialiased and a vertex sits on their centre line
                (y.saturating_sub(2)..(y + 3).min(back.bitmap.height)).any(|yy| {
                    (x.saturating_sub(2)..(x + 3).min(back.bitmap.width)).any(|xx| {
                        let i = ((yy * back.bitmap.width + xx) * 4) as usize;
                        back.bitmap.rgba[i] < 200
                    })
                })
            }
        })
    };
    let strokes = figure.primitives.iter().filter(|p| p.kind == PrimitiveKind::Stroke);
    let aligned = strokes.clone().filter(|p| dark_under(p)).count();
    assert!(aligned * 4 >= strokes.count() * 3, "only {aligned} strokes sit on ink");
}

fn crop(bitmap: &plotextract_core::Bitmap, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for row in y..y + h {
        let start = ((row * bitmap.width + x) * 4) as usize;
        out.extend_from_slice(&bitmap.rgba[start..start + (w * 4) as usize]);
    }
    out
}

#[test]
fn region_render_at_the_backdrop_scale_is_the_backdrop() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = PdfImporter.import(sin.clone()).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let mut renderer = PdfImporter.region_renderer(&sin).expect("PDF can re-render regions");

    let whole = renderer.render(kurbo::Rect::new(0.0, 0.0, figure.width, figure.height), back.scale).unwrap();
    assert_eq!((whole.width, whole.height), (back.bitmap.width, back.bitmap.height));
    assert!(whole.rgba == back.bitmap.rgba, "the whole page renders the same as the backdrop");

    // a region starting on a pixel boundary is exactly that crop of the page
    let k = back.scale;
    let sub = renderer.render(kurbo::Rect::new(100.0, 50.0, 160.0, 100.0), k).unwrap();
    let want = crop(&back.bitmap, (100.0 * k) as u32, (50.0 * k) as u32, sub.width, sub.height);
    // The rasterizer may differ in the last bit of an antialiased edge; the picture must be the same.
    let differing = sub.rgba.iter().zip(&want).filter(|(a, b)| a.abs_diff(**b) > 8).count();
    eprintln!("sub-region: {differing} of {} bytes differ by more than 8", want.len());
    assert!(differing * 1000 < want.len(), "a sub-region renders as the matching part of the page");
}

#[test]
fn zoomed_region_is_the_same_picture_in_more_pixels() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = PdfImporter.import(sin.clone()).unwrap();
    let back = figure.backdrop.as_ref().unwrap();
    let k = back.scale;
    let mut renderer = PdfImporter.region_renderer(&sin).unwrap();

    // 4x the backdrop's resolution on a 40 x 40 pt patch that holds part of the curve and an axis
    let region = kurbo::Rect::new(30.0, 120.0, 70.0, 160.0);
    let zoomed = renderer.render(region, 4.0 * k).unwrap();
    assert_eq!((zoomed.width, zoomed.height), (960, 960));

    // box-filter it down 4x and compare with the backdrop's pixels for the same patch
    let reference = crop(&back.bitmap, (region.x0 * k) as u32, (region.y0 * k) as u32, 240, 240);
    let mut total = 0u64;
    for y in 0..240usize {
        for x in 0..240usize {
            for c in 0..3usize {
                let mut sum = 0u32;
                for dy in 0..4usize {
                    for dx in 0..4usize {
                        sum += u32::from(zoomed.rgba[(((y * 4 + dy) * 960 + x * 4 + dx) * 4) + c]);
                    }
                }
                let down = (sum / 16) as i32;
                total += (down - i32::from(reference[((y * 240 + x) * 4) + c])).unsigned_abs() as u64;
            }
        }
    }
    let mean = total as f64 / (240.0 * 240.0 * 3.0);
    assert!(mean < 4.0, "mean difference {mean:.2} of 255: the zoomed render must show the same picture");
    // and it is not blank: the patch has ink
    assert!(zoomed.rgba.chunks(4).any(|px| px[0] < 100));
}
