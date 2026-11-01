//! PostScript and EPS importer.
//!
//! `stet` interprets the program and records what it painted as a display list of device-space fills and strokes. Those
//! become primitives (text glyphs and clipping are ignored on purpose), and the same display list is what draws the
//! picture of the page and of any region of it.
//!
//! Figure units are PostScript points, origin at the top left of the page (of the bounding box, for EPS).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use plotextract_domain::{Bitmap, Figure, ImportError, OpenOptions, Style, Unit};
use plotextract_import::{BEZIER_STEPS, PrimitiveBuilder, flatten_path, pixels_for};
use plotextract_ports::{Importer, PictureSource};
use stet::{DisplayElement, IccCache, Interpreter, PreparedDisplayList, PsDisplayList, RegionRender};
use stet_fonts::geometry::{PathSegment, PsPath};

/// The page is interpreted once at this many pixels per point; its display list is in these pixels.
const SCALE: f64 = 4.0;
const DPI: f64 = 72.0 * SCALE;
/// A PostScript program is a program: one that is still running after this long is stopped.
const TIME_LIMIT: Duration = Duration::from_secs(20);

fn interpreter() -> Interpreter {
    Interpreter::builder().suppress_output().build()
}

fn fail(interpreter: &Interpreter, what: &str) -> ImportError {
    let mut text = what.to_string();
    for w in interpreter.warnings() {
        text.push_str(&format!(" ({w}: {})", w.hint()));
    }
    ImportError::message(text)
}

/// Run something on the interpreter under the time limit. Whether the limit stopped it comes second.
fn run_limited<T>(interpreter: &mut Interpreter, run: impl FnOnce(&mut Interpreter) -> T) -> (T, bool) {
    let stop = Arc::new(AtomicBool::new(false));
    interpreter.context().interrupt_flag = Some(stop.clone());
    let (finished, waiting) = channel::<()>();
    let watchdog = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            if waiting.recv_timeout(TIME_LIMIT) == Err(RecvTimeoutError::Timeout) {
                stop.store(true, Ordering::Relaxed);
            }
        })
    };
    let out = run(interpreter);
    let _ = finished.send(());
    let _ = watchdog.join();
    (out, stop.load(Ordering::Relaxed))
}

fn timed_out() -> ImportError {
    ImportError::message(format!(
        "the PostScript program was still running after {} seconds and was stopped",
        TIME_LIMIT.as_secs()
    ))
}

/// Opens PostScript and EPS files: the first page (EPS: its bounding box).
pub struct PsImporter;

impl Importer for PsImporter {
    fn name(&self) -> &'static str {
        "PostScript"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ps", "eps", "epsf"]
    }

    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"%!") || head.starts_with(&[0xC5, 0xD0, 0xD3, 0xC6]) // or a DOS EPS binary header
    }

    fn import(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Figure, ImportError> {
        let mut interp = interpreter();
        let (result, stopped) = run_limited(&mut interp, |i| i.render_to_display_list(bytes, DPI));
        if stopped {
            return Err(timed_out());
        }
        let pages = result.map_err(|e| ImportError::message(e.to_string()))?;
        let page_count = pages.len();
        if page_count == 0 {
            return Err(fail(&interp, "the program painted no page (it never called showpage)"));
        }
        let Some(page) = pages.into_iter().nth(options.page) else {
            return Err(ImportError::NoSuchPage { page: options.page, count: page_count });
        };
        let mut builder = PrimitiveBuilder::default();
        walk(&page.display_list, &mut builder);
        let truncated = builder.truncated();
        Ok(Figure {
            width: f64::from(page.width) / SCALE,
            height: f64::from(page.height) / SCALE,
            unit: Unit::Point,
            primitives: builder.finish(),
            page: options.page,
            page_count,
            truncated,
        })
    }

    fn picture(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Option<Box<dyn PictureSource>> {
        let mut interp = interpreter();
        let (result, stopped) = run_limited(&mut interp, |i| i.render_to_display_list(bytes, DPI));
        if stopped {
            return None;
        }
        let page = result.ok()?.into_iter().nth(options.page)?;
        let bake = &interp.context().icc_cache;
        let icc = stet::build_icc_cache_for_list_with_bpc(
            &page.display_list,
            bake.system_cmyk_bytes(),
            false,
            bake.bpc_mode(),
        );
        let prepared = stet::prepare_display_list(&page.display_list);
        Some(Box::new(PsPicture { list: page.display_list, prepared, icc }))
    }
}

/// Draws parts of the page at any scale from the display list, in figure units.
struct PsPicture {
    list: PsDisplayList,
    prepared: PreparedDisplayList,
    icc: IccCache,
}

impl PictureSource for PsPicture {
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let (w, h) = (pixels_for(region.width(), scale), pixels_for(region.height(), scale));
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
        if !builder.has_room() {
            return;
        }
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
