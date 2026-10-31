//! PostScript and EPS importer.
//!
//! `stet` interprets the program and records what it painted as a display list of device-space fills and strokes. Those
//! become primitives (text glyphs and clipping are ignored on purpose), and the same display list is what draws the
//! page picture and any region of it.
//!
//! Figure units are PostScript points, origin at the top left of the page (of the bounding box, for EPS).

use plotextract_core::{
    Backdrop, Bitmap, Figure, ImportError, Importer, PrimitiveBuilder, RegionRenderer, Style, flatten_path,
};
use stet::{DisplayElement, IccCache, Interpreter, PreparedDisplayList, PsDisplayList, RegionRender};
use stet_fonts::geometry::{PathSegment, PsPath};

/// Points sampled per cubic Bezier.
pub const BEZIER_STEPS: usize = 8;
/// The page is interpreted once at this many pixels per point; its picture and its display list are in these pixels.
const SCALE: f64 = 4.0;
const DPI: f64 = 72.0 * SCALE;

fn interpreter() -> Interpreter {
    Interpreter::builder().suppress_output().build()
}

fn fail(interpreter: &Interpreter, what: &str) -> ImportError {
    let mut text = what.to_string();
    for w in interpreter.warnings() {
        text.push_str(&format!("\n{w}: {}", w.hint()));
    }
    ImportError(text)
}

/// Opens PostScript and EPS files: the first page (EPS: its bounding box).
pub struct PsImporter;

impl Importer for PsImporter {
    fn name(&self) -> &'static str {
        "PostScript"
    }

    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"%!") || head.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) // or a DOS EPS binary header
    }

    fn import(&self, bytes: Vec<u8>) -> Result<Figure, ImportError> {
        let mut interp = interpreter();
        let pages = interp.render(&bytes, DPI).map_err(|e| ImportError(e.to_string()))?;
        let Some(page) = pages.into_iter().next() else {
            return Err(fail(&interp, "the program painted no page (it never called showpage)"));
        };
        let (width, height) = (f64::from(page.width) / SCALE, f64::from(page.height) / SCALE);
        let mut builder = PrimitiveBuilder::default();
        walk(&page.display_list, &mut builder);
        let primitives = builder.finish();
        let bitmap = Bitmap { width: page.width, height: page.height, rgba: page.rgba };
        Ok(Figure { width, height, backdrop: Some(Backdrop { bitmap, scale: SCALE }), primitives })
    }

    fn region_renderer(&self, bytes: &[u8]) -> Option<Box<dyn RegionRenderer>> {
        let mut interp = interpreter();
        let page = interp.render_to_display_list(bytes, DPI).ok()?.into_iter().next()?;
        let bake = &interp.context().icc_cache;
        let icc = stet::build_icc_cache_for_list_with_bpc(
            &page.display_list,
            bake.system_cmyk_bytes(),
            false,
            bake.bpc_mode(),
        );
        let prepared = stet::prepare_display_list(&page.display_list);
        Some(Box::new(PsRegionRenderer { list: page.display_list, prepared, icc }))
    }
}

/// Draws parts of the page at any scale from the display list, in figure units.
pub struct PsRegionRenderer {
    list: PsDisplayList,
    prepared: PreparedDisplayList,
    icc: IccCache,
}

impl RegionRenderer for PsRegionRenderer {
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let side = |len: f64| (len * scale).ceil().clamp(1.0, 16384.0) as u32;
        let (w, h) = (side(region.width()), side(region.height()));
        // the viewport is in the list's device pixels; its size is the pixel size over the zoom, so pixel (0, 0) sits
        // exactly at the region's corner
        let viewport =
            [region.x0 * SCALE, region.y0 * SCALE, f64::from(w) / scale * SCALE, f64::from(h) / scale * SCALE];
        let rgba = RegionRender::new(&self.list, &self.prepared, viewport, w, h, DPI).icc(&self.icc).render();
        Ok(Bitmap { width: w, height: h, rgba: over_white(rgba) })
    }
}

/// Straight-alpha RGBA onto white paper.
fn over_white(mut rgba: Vec<u8>) -> Vec<u8> {
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            px[3] = 255;
        }
    }
    rgba
}

/// Feed the strokes and fills of a display list to `builder`, in drawing order.
fn walk(list: &PsDisplayList, builder: &mut PrimitiveBuilder) {
    // device pixels back to points
    let to_figure = kurbo::Affine::scale(1.0 / SCALE);
    let style = |r: f64, g: f64, b: f64| {
        let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        Style { color: Some([byte(r), byte(g), byte(b), 255]) }
    };
    for element in list.elements() {
        match element {
            DisplayElement::Fill { path, params } if !params.is_text_glyph => {
                let pieces = flatten_path(&to_bez(path), to_figure, BEZIER_STEPS, false);
                builder.fill(pieces, style(params.color.r, params.color.g, params.color.b));
            }
            DisplayElement::Stroke { path, params } if !params.is_text_glyph => {
                let pieces = flatten_path(&to_bez(path), to_figure, BEZIER_STEPS, true);
                builder.stroke(pieces, style(params.color.r, params.color.g, params.color.b));
            }
            DisplayElement::Group { elements, .. } | DisplayElement::OcgGroup { elements, .. } => {
                walk(elements, builder)
            }
            DisplayElement::SoftMasked { content, .. } => walk(content, builder),
            // glyphs, images, shadings, clips: nothing to pick
            _ => {}
        }
    }
}

fn to_bez(path: &PsPath) -> kurbo::BezPath {
    let mut bez = kurbo::BezPath::new();
    for seg in &path.segments {
        match *seg {
            PathSegment::MoveTo(x, y) => bez.move_to((x, y)),
            PathSegment::LineTo(x, y) => bez.line_to((x, y)),
            PathSegment::CurveTo { x1, y1, x2, y2, x3, y3 } => bez.curve_to((x1, y1), (x2, y2), (x3, y3)),
            PathSegment::ClosePath => bez.close_path(),
        }
    }
    bez
}
