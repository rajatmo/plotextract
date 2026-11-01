//! The sin plot made by Mathematica, checked against what the Python prototype extracted from it.

use kurbo::Rect;
use plotextract_corpus::{fixture, import, picture};
use plotextract_domain::{Bitmap, Figure, PrimitiveKind, Unit};
use plotextract_import::BEZIER_STEPS;
use plotextract_pdf::{PdfImporter, extract};
use plotextract_ports::Importer;

/// Pixels per point of the reference picture.
const K: f64 = 6.0;

fn whole_page(pdf: &[u8], figure: &Figure) -> Bitmap {
    let mut picture = picture(&PdfImporter, pdf).expect("PDF draws pictures");
    picture.render(Rect::new(0.0, 0.0, figure.width, figure.height), K).unwrap()
}

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
    assert_eq!(PdfImporter.extensions(), ["pdf"]);
}

#[test]
fn imported_figure_is_y_down_in_points() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = import(&PdfImporter, &sin).unwrap();
    assert_eq!((figure.width, figure.height), (260.0, 166.0));
    assert_eq!(figure.unit, Unit::Point);
    assert_eq!(figure.primitives.len(), 19);

    for c in &figure.primitives {
        let e = c.extent().unwrap();
        assert!(
            e.x0 >= -1.0 && e.x1 <= figure.width + 1.0 && e.y0 >= -1.0 && e.y1 <= figure.height + 1.0,
            "{}",
            c.name()
        );
    }
}

#[test]
fn the_page_picture_covers_the_page_on_white() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = import(&PdfImporter, &sin).unwrap();
    let back = whole_page(&sin, &figure);
    assert_eq!(back.rgba.len(), (back.width * back.height * 4) as usize);
    assert_eq!((back.width, back.height), ((figure.width * K).ceil() as u32, (figure.height * K).ceil() as u32));
    assert!(back.rgba.chunks(4).all(|px| px[3] == 255), "opaque on white");
    // the page is not blank: the plot has dark strokes on white
    assert!(back.rgba.chunks(4).any(|px| px[0] < 128));
    assert!(back.rgba.chunks(4).any(|px| px[0] == 255));
}

#[test]
fn imported_strokes_line_up_with_the_rendered_page() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    // The first stroke is dark ink on the page, so the pixel under one of its vertices must not be white.
    let figure = import(&PdfImporter, &sin).unwrap();
    let back = whole_page(&sin, &figure);
    let dark_under = |p: &plotextract_domain::Primitive| {
        p.points.iter().any(|v| {
            let (x, y) = ((v.x * K) as u32, (v.y * K) as u32);
            (x < back.width && y < back.height) && {
                // look in a 3-pixel neighbourhood: strokes are antialiased and a vertex sits on their centre line
                (y.saturating_sub(2)..(y + 3).min(back.height)).any(|yy| {
                    (x.saturating_sub(2)..(x + 3).min(back.width)).any(|xx| {
                        let i = ((yy * back.width + xx) * 4) as usize;
                        back.rgba[i] < 200
                    })
                })
            }
        })
    };
    let strokes = figure.primitives.iter().filter(|p| p.kind == PrimitiveKind::Stroke);
    let aligned = strokes.clone().filter(|p| dark_under(p)).count();
    assert!(aligned * 4 >= strokes.count() * 3, "only {aligned} strokes sit on ink");
}

fn crop(bitmap: &Bitmap, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    for row in y..y + h {
        let start = ((row * bitmap.width + x) * 4) as usize;
        out.extend_from_slice(&bitmap.rgba[start..start + (w * 4) as usize]);
    }
    out
}

#[test]
fn a_sub_region_renders_as_the_matching_part_of_the_page() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = import(&PdfImporter, &sin).unwrap();
    let back = whole_page(&sin, &figure);
    let mut picture = picture(&PdfImporter, &sin).unwrap();

    // a region starting on a pixel boundary is exactly that crop of the page
    let sub = picture.render(Rect::new(100.0, 50.0, 160.0, 100.0), K).unwrap();
    let want = crop(&back, (100.0 * K) as u32, (50.0 * K) as u32, sub.width, sub.height);
    // The rasterizer may differ in the last bit of an antialiased edge; the picture must be the same.
    let differing = sub.rgba.iter().zip(&want).filter(|(a, b)| a.abs_diff(**b) > 8).count();
    eprintln!("sub-region: {differing} of {} bytes differ by more than 8", want.len());
    assert!(differing * 1000 < want.len(), "a sub-region renders as the matching part of the page");
}

#[test]
fn zoomed_region_is_the_same_picture_in_more_pixels() {
    let Some(sin) = fixture("sin_mathematica.pdf") else { return };
    let figure = import(&PdfImporter, &sin).unwrap();
    let back = whole_page(&sin, &figure);
    let mut picture = picture(&PdfImporter, &sin).unwrap();

    // 4x the resolution on a 40 x 40 pt patch that holds part of the curve and an axis
    let region = Rect::new(30.0, 120.0, 70.0, 160.0);
    let zoomed = picture.render(region, 4.0 * K).unwrap();
    assert_eq!((zoomed.width, zoomed.height), (960, 960));

    // box-filter it down 4x and compare with the page picture's pixels for the same patch
    let reference = crop(&back, (region.x0 * K) as u32, (region.y0 * K) as u32, 240, 240);
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
