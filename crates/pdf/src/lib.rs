//! PDF importer.
//!
//! Implements `hayro_interpret::Device` to collect stroked and filled paths (clipping is ignored on purpose) and
//! draws the page picture with `hayro`.

use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings};
use hayro_interpret::font::GlyphRun;
use hayro_interpret::util::TransformExt;
use hayro_interpret::{
    BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache,
    InterpreterSettings, Paint, SoftMask, interpret_page,
};
use hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, Rect};
use plotextract_core::{Bitmap, Figure, ImportError, Importer, OpenOptions, PictureSource, Primitive, Style, Unit};
use plotextract_import::{BEZIER_STEPS, Pieces, PrimitiveBuilder, flatten_path, pixels_for, rect_corners};

use std::sync::Arc;

use hayro_syntax::LoadPdfError;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("cannot read the PDF: {0}")]
    Load(String),
    #[error("the PDF has no pages")]
    NoPages,
    #[error("the PDF has no page {page}: it has {count}")]
    NoSuchPage { page: usize, count: usize },
    #[error("the PDF is password protected")]
    PasswordRequired,
    #[error("the password is wrong")]
    WrongPassword,
}

impl From<PdfError> for ImportError {
    fn from(e: PdfError) -> Self {
        match e {
            PdfError::PasswordRequired => ImportError::PasswordRequired,
            PdfError::WrongPassword => ImportError::WrongPassword,
            PdfError::NoSuchPage { page, count } => ImportError::NoSuchPage { page, count },
            other => ImportError::message(other.to_string()),
        }
    }
}

/// A page of a PDF as primitives: stroked paths first, then filled ones.
pub struct PdfPage {
    pub width: f64,
    pub height: f64,
    /// Page points with the origin at the bottom left of the crop box, y up, the page unrotated. The comparison
    /// tools (and the Python prototype) use this frame.
    pub primitives: Vec<Primitive>,
    /// The page held more paths than the importer keeps.
    pub truncated: bool,
}

fn open(data: Arc<Vec<u8>>, password: Option<&str>) -> Result<Pdf, PdfError> {
    let pdf = Pdf::new_with_password(data, password.unwrap_or("")).map_err(|e| match e {
        // hayro does not export the type of the reason, only its name
        LoadPdfError::Decryption(reason) if format!("{reason:?}") == "PasswordProtected" => {
            if password.is_none_or(str::is_empty) { PdfError::PasswordRequired } else { PdfError::WrongPassword }
        }
        LoadPdfError::Decryption(reason) => PdfError::Load(format!("it cannot be decrypted ({reason:?})")),
        LoadPdfError::Invalid => PdfError::Load("it is damaged or not a PDF".into()),
    })?;
    if pdf.pages().is_empty() {
        return Err(PdfError::NoPages);
    }
    Ok(pdf)
}

fn page_of(pdf: &Pdf, index: usize) -> Result<&hayro_syntax::page::Page<'_>, PdfError> {
    let pages = pdf.pages();
    pages.get(index).ok_or(PdfError::NoSuchPage { page: index, count: pages.len() })
}

/// Read the stroked and filled paths of page 1. `steps` is the number of points per cubic Bezier (1 keeps only the
/// end points).
pub fn extract(data: Vec<u8>, steps: usize) -> Result<PdfPage, PdfError> {
    extract_from(&open(Arc::new(data), None)?, 0, steps)
}

fn extract_from(pdf: &Pdf, index: usize, steps: usize) -> Result<PdfPage, PdfError> {
    let page = page_of(pdf, index)?;
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

    let truncated = collector.builder.truncated();
    Ok(PdfPage { width: w as f64, height: h as f64, primitives: collector.builder.finish(), truncated })
}

/// Opens PDF files: the paths of one page.
pub struct PdfImporter;

impl Importer for PdfImporter {
    fn name(&self) -> &'static str {
        "PDF"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["pdf"]
    }

    fn sniff(&self, head: &[u8]) -> bool {
        head.starts_with(b"%PDF")
    }

    fn picture(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Option<Box<dyn PictureSource>> {
        let pdf = open(bytes.clone(), options.password.as_deref()).ok()?;
        page_of(&pdf, options.page).ok()?;
        Some(Box::new(PdfPicture { pdf, page: options.page }))
    }

    fn import(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Figure, ImportError> {
        let pdf = open(bytes.clone(), options.password.as_deref())?;
        let page = page_of(&pdf, options.page)?;
        let mut extracted = extract_from(&pdf, options.page, BEZIER_STEPS)?;

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

        Ok(Figure {
            width: f64::from(width),
            height: f64::from(height),
            unit: Unit::Point,
            primitives: extracted.primitives,
            page: options.page,
            page_count: pdf.pages().len(),
            truncated: extracted.truncated,
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
        if !self.builder.has_room() {
            return;
        }
        let steps = self.steps;
        self.add(|stroked| flatten_path(path, props.transform, steps, stroked), style_of(&props.paint), mode);
    }

    fn draw_rect(&mut self, rect: &Rect, props: DrawProps<'a>, mode: &DrawMode) {
        if !self.builder.has_room() {
            return;
        }
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
struct PdfPicture {
    pdf: Pdf,
    page: usize,
}

impl PictureSource for PdfPicture {
    fn render(&mut self, region: Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let page = page_of(&self.pdf, self.page)?;
        // pixels_for caps at 16384, which fits the renderer's 16-bit sizes
        let side = |len: f64| pixels_for(len, scale) as u16;
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
