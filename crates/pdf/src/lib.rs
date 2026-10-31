//! PDF importer.
//!
//! Implements `hayro_interpret::Device` to collect stroked and filled paths (clipping is ignored on purpose) and
//! renders the page backdrop with `hayro`.

use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use hayro_interpret::font::GlyphRun;
use hayro_interpret::util::TransformExt;
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache,
    InterpreterSettings, Paint, SoftMask, interpret_page,
};
use hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, Rect};
use plotextract_core::{
    Backdrop, Bitmap, Figure, ImportError, Importer, Pieces, Primitive, PrimitiveBuilder, RegionRenderer, Style,
    flatten_path, rect_corners,
};

/// Points sampled per cubic Bezier.
pub const BEZIER_STEPS: usize = 8;

/// Longest side of the backdrop bitmap, in pixels.
const BACKDROP_MAX_PX: f64 = 4096.0;
/// Most pixels per page point in the backdrop.
const BACKDROP_MAX_SCALE: f64 = 6.0;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("cannot read the PDF: {0}")]
    Load(String),
    #[error("the PDF has no pages")]
    NoPages,
}

/// Page 1 of a PDF as primitives: stroked paths first, then filled ones.
pub struct PdfPage {
    pub width: f64,
    pub height: f64,
    /// Page points with the origin at the bottom left of the crop box, y up, the page unrotated. The comparison
    /// tools (and the Python prototype) use this frame.
    pub primitives: Vec<Primitive>,
}

fn open(data: Vec<u8>) -> Result<Pdf, PdfError> {
    let pdf = Pdf::new(data).map_err(|e| PdfError::Load(format!("{e:?}")))?;
    if pdf.pages().iter().next().is_none() {
        return Err(PdfError::NoPages);
    }
    Ok(pdf)
}

/// Read the stroked and filled paths of page 1. `steps` is the number of points per cubic Bezier (1 keeps only the
/// end points).
pub fn extract(data: Vec<u8>, steps: usize) -> Result<PdfPage, PdfError> {
    extract_from(&open(data)?, steps)
}

fn extract_from(pdf: &Pdf, steps: usize) -> Result<PdfPage, PdfError> {
    let page = pdf.pages().iter().next().ok_or(PdfError::NoPages)?;
    let crop = page.intersected_crop_box();
    let (w, h) = page.base_dimensions();

    let initial = Affine::translate((-crop.x0, -crop.y0));
    let cache = InterpreterCache::new();
    let mut context = Context::new(
        initial,
        Rect::new(0.0, 0.0, w as f64, h as f64),
        &cache,
        pdf.xref(),
        InterpreterSettings::default(),
    );
    let mut collector = Collector { steps: steps.max(1), builder: PrimitiveBuilder::default() };
    interpret_page(page, &mut context, &mut collector);

    Ok(PdfPage { width: w as f64, height: h as f64, primitives: collector.builder.finish() })
}

/// Opens PDF files: page 1 as a backdrop picture plus its paths.
pub struct PdfImporter;

impl Importer for PdfImporter {
    fn name(&self) -> &'static str {
        "PDF"
    }

    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"%PDF")
    }

    fn region_renderer(&self, bytes: &[u8]) -> Option<Box<dyn RegionRenderer>> {
        open(bytes.to_vec()).ok().map(|pdf| Box::new(PdfRegionRenderer { pdf }) as Box<dyn RegionRenderer>)
    }

    fn import(&self, bytes: Vec<u8>) -> Result<Figure, ImportError> {
        let err = |e: PdfError| ImportError(e.to_string());
        let pdf = open(bytes).map_err(err)?;
        let page = pdf.pages().iter().next().expect("checked in open");
        let mut extracted = extract_from(&pdf, BEZIER_STEPS).map_err(err)?;

        // Figure space is what the renderer draws: origin top left, y down, the page rotation applied. The points
        // were extracted unrotated, y up, relative to the crop box; carry them to the rendered frame.
        let (width, height) = page.render_dimensions();
        let crop = page.intersected_crop_box();
        let m = page.initial_transform(true).to_kurbo() * Affine::translate((-crop.x0, -crop.y0)).inverse();
        for prim in &mut extracted.primitives {
            for p in &mut prim.points {
                *p = m * *p;
            }
        }

        let scale = (BACKDROP_MAX_PX / f64::from(width.max(height))).min(BACKDROP_MAX_SCALE) as f32;
        let pixmap = hayro::render(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE },
        );
        let bitmap = Bitmap {
            width: u32::from(pixmap.width()),
            height: u32::from(pixmap.height()),
            rgba: pixmap.data_as_u8_slice().to_vec(), // opaque on a white background: premultiplied = straight
        };
        Ok(Figure {
            width: f64::from(width),
            height: f64::from(height),
            backdrop: Some(Backdrop { bitmap, scale: f64::from(scale) }),
            primitives: extracted.primitives,
        })
    }
}

struct Collector {
    steps: usize,
    builder: PrimitiveBuilder,
}

fn style_of(paint: &Paint<'_>) -> Style {
    match paint {
        Paint::Color(c) => Style { color: Some(c.to_rgba().to_rgba8()) },
        Paint::Pattern(_) => Style::default(),
    }
}

impl Collector {
    /// `build(stroked)` gives the path's pieces; it differs only in the corner order of non-rectangular quads.
    fn add(&mut self, build: impl Fn(bool) -> Pieces, style: Style, mode: &DrawMode) {
        let (stroke, fill) = match mode {
            DrawMode::Fill(_) => (false, true),
            DrawMode::Stroke(_) => (true, false),
            DrawMode::FillAndStroke(..) => (true, true),
            DrawMode::Invisible => return,
        };
        if stroke {
            self.builder.stroke(build(true), style);
        }
        if fill {
            self.builder.fill(build(false), style);
        }
    }
}

impl<'a> Device<'a> for Collector {
    fn draw_path(&mut self, path: &BezPath, props: DrawProps<'a>, mode: &DrawMode) {
        let steps = self.steps;
        self.add(|stroked| flatten_path(path, props.transform, steps, stroked), style_of(&props.paint), mode);
    }

    fn draw_rect(&mut self, rect: &Rect, props: DrawProps<'a>, mode: &DrawMode) {
        let corners = rect_corners(props.transform.transform_rect_bbox(*rect));
        self.add(|_| (corners.clone(), Vec::new()), style_of(&props.paint), mode);
    }

    // Clipping paths are ignored on purpose: ticks and lines that the PDF clips away can still be picked.
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_clip_rect(&mut self, _: &Rect) {}
    fn pop_clip(&mut self) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
}

/// Renders parts of page 1 at any scale, in figure space.
pub struct PdfRegionRenderer {
    pdf: Pdf,
}

impl RegionRenderer for PdfRegionRenderer {
    fn render(&mut self, region: Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let page = self.pdf.pages().iter().next().ok_or_else(|| ImportError("the PDF has no pages".into()))?;
        let side = |len: f64| (len * scale).ceil().clamp(1.0, f64::from(u16::MAX)) as u16;
        let mut ctx = RenderContext::new(side(region.width()), side(region.height()));
        // figure space -> device pixels of the region
        let transform = Affine::translate((-region.x0 * scale, -region.y0 * scale))
            * Affine::scale(scale)
            * page.initial_transform(true).to_kurbo();
        hayro::render_into(
            page,
            &RenderCache::new(),
            &InterpreterSettings::default(),
            &RenderSettings::default(),
            &mut ctx,
            transform,
        );
        ctx.flush();
        let mut pixmap = Pixmap::new(ctx.width(), ctx.height());
        ctx.render_with(
            &mut pixmap,
            &mut Resources::default(),
            RasterizerSettings { target_init: TargetInit::Clear(WHITE), ..Default::default() },
        );
        Ok(Bitmap {
            width: u32::from(pixmap.width()),
            height: u32::from(pixmap.height()),
            rgba: pixmap.data_as_u8_slice().to_vec(),
        })
    }
}
