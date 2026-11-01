//! PDF importer.
//!
//! Implements `hayro_interpret::Device` to collect stroked and filled paths (clipping is ignored on purpose), and again
//! to find the raster images the pages draw; draws the page picture with `hayro`.

use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::{Pixmap, RasterizerSettings, RenderContext, Resources, TargetInit};
use hayro::{RenderCache, RenderSettings};
use hayro_interpret::font::GlyphRun;
use hayro_interpret::util::TransformExt;
use hayro_interpret::{
    BlendMode, CacheKey, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageData, ImageDrawProps,
    InterpreterCache, InterpreterSettings, Paint, SoftMask, interpret_page,
};
use hayro_syntax::Pdf;
use kurbo::{Affine, BezPath, Rect};
use plotextract_domain::{Bitmap, Figure, ImportError, OpenOptions, Primitive, Style, Unit};
use plotextract_import::{BEZIER_STEPS, Pieces, PrimitiveBuilder, flatten_path, pixels_for, rect_corners};
use plotextract_ports::{Contents, Embedded, Importer, PictureSource};

use std::collections::HashSet;
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

    fn is_document(&self) -> bool {
        true
    }

    fn contents(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Contents, ImportError> {
        let pdf = open(bytes.clone(), options.password.as_deref())?;
        let mut seen = HashSet::new();
        let mut images = Vec::new();
        for (page, p) in pdf.pages().iter().enumerate() {
            let mut finder = ImageFinder::new(None);
            interpret(&pdf, p, &mut finder);
            // an image drawn on several pages (a logo) is listed once, on the first
            images.extend(finder.found.into_iter().filter(|f| seen.insert(f.key)).map(|f| Embedded {
                page,
                index: f.index,
                width: f.width,
                height: f.height,
            }));
        }
        Ok(Contents { pages: pdf.pages().len(), images })
    }

    fn image(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions, image: &Embedded) -> Result<Vec<u8>, ImportError> {
        let pdf = open(bytes.clone(), options.password.as_deref())?;
        let mut finder = ImageFinder::new(Some(image.index));
        interpret(&pdf, page_of(&pdf, image.page)?, &mut finder);
        let (width, height, rgb) = finder.decoded.ok_or_else(|| ImportError::message("the page has no such image"))?;
        let mut png = Vec::new();
        image::RgbImage::from_raw(width, height, rgb)
            .ok_or_else(|| ImportError::message("the image is damaged"))?
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| ImportError::message(format!("cannot write the image: {e}")))?;
        Ok(png)
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

/// The smallest image listed, in pixels a side: smaller ones are bullets and marks, not figures.
const MIN_IMAGE_SIDE: u32 = 16;

/// Run `device` over a page, as it is drawn.
fn interpret<'a>(pdf: &'a Pdf, page: &'a hayro_syntax::page::Page<'a>, device: &mut impl Device<'a>) {
    let (w, h) = page.base_dimensions();
    let cache = InterpreterCache::new();
    let bounds = Rect::new(0.0, 0.0, f64::from(w), f64::from(h));
    let mut context = Context::new(Affine::IDENTITY, bounds, &cache, pdf.xref(), InterpreterSettings::default());
    interpret_page(page, &mut context, device);
}

/// An image a page draws.
struct Found {
    /// What the image is, so one drawn twice is counted once.
    key: u128,
    index: usize,
    width: u32,
    height: u32,
}

/// Finds the raster images a page draws (each once, the small ones left out) and decodes the one wanted, as RGB on
/// white.
struct ImageFinder {
    wanted: Option<usize>,
    found: Vec<Found>,
    decoded: Option<(u32, u32, Vec<u8>)>,
}

impl ImageFinder {
    fn new(wanted: Option<usize>) -> Self {
        Self { wanted, found: Vec::new(), decoded: None }
    }
}

/// An image's pixels as RGB, laid on white where it is transparent.
fn on_white(image: ImageData, alpha: Option<hayro_interpret::LumaData>) -> (u32, u32, Vec<u8>) {
    let (width, height) = (image.width(), image.height());
    let mut rgb = match image {
        ImageData::Rgb(d) => d.data,
        ImageData::Luma(d) => d.data.iter().flat_map(|&v| [v, v, v]).collect(),
    };
    if let Some(alpha) = alpha.filter(|a| (a.width, a.height) == (width, height)) {
        for (px, &a) in rgb.as_chunks_mut::<3>().0.iter_mut().zip(&alpha.data) {
            for c in px {
                *c = ((u32::from(*c) * u32::from(a) + 255 * u32::from(255 - a)) / 255) as u8;
            }
        }
    }
    (width, height, rgb)
}

impl<'a> Device<'a> for ImageFinder {
    fn draw_image(&mut self, image: Image<'a, '_>, _: ImageDrawProps<'a>) {
        let Image::Raster(raster) = image else { return };
        let (width, height) = (raster.width(), raster.height());
        let key = raster.cache_key();
        if width.min(height) < MIN_IMAGE_SIDE || self.found.iter().any(|f| f.key == key) {
            return;
        }
        let index = self.found.len();
        self.found.push(Found { key, index, width, height });
        if self.wanted == Some(index) {
            raster.with_rgba(|data, alpha| self.decoded = Some(on_white(data, alpha)), None);
        }
    }

    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn draw_rect(&mut self, _: &Rect, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_clip_rect(&mut self, _: &Rect) {}
    fn pop_clip(&mut self) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn pop_transparency_group(&mut self) {}
    fn draw_glyph_run(&mut self, _: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {}
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF of two pages. Page 1 draws a 32 x 16 picture (left half red, right half blue) twice and a 4 x 4 one; page 2
    /// draws the 32 x 16 one again and a 16 x 16 grey one (value 128), half of it transparent.
    fn pdf_with_images() -> Vec<u8> {
        let mut red_blue = Vec::new();
        for _ in 0..16 {
            for x in 0..32 {
                red_blue.extend(if x < 16 { [255, 0, 0] } else { [0, 0, 255] });
            }
        }
        let alpha: Vec<u8> = (0..16 * 16).map(|i| if i % 16 < 8 { 255 } else { 0 }).collect();
        let stream = |dict: &str, data: &[u8]| {
            let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
            out
        };
        let image = |w: u32, h: u32, space: &str, extra: &str, data: &[u8]| {
            let dict = format!(
                "/Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /{space} /BitsPerComponent 8 {extra}"
            );
            stream(&dict, data)
        };
        let page1 = b"q 200 0 0 100 10 10 cm /A Do Q q 100 0 0 50 10 150 cm /A Do Q q 8 0 0 8 0 0 cm /T Do Q";
        let page2 = b"q 100 0 0 100 0 0 cm /G Do Q q 50 0 0 25 100 100 cm /A Do Q";
        let resources = "/Resources << /XObject << /A 5 0 R /T 6 0 R /G 7 0 R >> >>";
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] {resources} /Contents 9 0 R >>").into_bytes(),
            format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] {resources} /Contents 10 0 R >>")
                .into_bytes(),
            image(32, 16, "DeviceRGB", "", &red_blue),
            image(4, 4, "DeviceGray", "", &[0; 16]),
            image(16, 16, "DeviceGray", "/SMask 8 0 R", &[128; 256]),
            image(16, 16, "DeviceGray", "", &alpha),
            stream("", page1),
            stream("", page2),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (n, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", n + 1).into_bytes());
            out.extend(body);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").into_bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).into_bytes(),
        );
        out
    }

    #[test]
    fn the_images_are_listed_once_each_on_the_first_page_that_draws_them_without_the_small_ones() {
        let bytes = Arc::new(pdf_with_images());
        let contents = PdfImporter.contents(&bytes, &OpenOptions::default()).unwrap();
        assert_eq!(contents.pages, 2);
        let listed: Vec<(usize, usize, u32, u32)> =
            contents.images.iter().map(|e| (e.page, e.index, e.width, e.height)).collect();
        // page 2's own images: the grey one first (it is drawn first), the red and blue one already listed
        assert_eq!(listed, vec![(0, 0, 32, 16), (1, 0, 16, 16)]);
    }

    #[test]
    fn an_image_comes_out_as_a_png_of_its_own_pixels_laid_on_white() {
        let bytes = Arc::new(pdf_with_images());
        let contents = PdfImporter.contents(&bytes, &OpenOptions::default()).unwrap();
        let png = PdfImporter.image(&bytes, &OpenOptions::default(), &contents.images[0]).unwrap();
        let picture = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(picture.dimensions(), (32, 16));
        assert_eq!((picture.get_pixel(2, 5).0, picture.get_pixel(27, 5).0), ([255, 0, 0], [0, 0, 255]));

        let grey =
            image::load_from_memory(&PdfImporter.image(&bytes, &OpenOptions::default(), &contents.images[1]).unwrap())
                .unwrap()
                .to_rgb8();
        assert_eq!((grey.get_pixel(2, 2).0, grey.get_pixel(12, 2).0), ([128; 3], [255; 3]), "transparent is white");

        let missing = Embedded { page: 1, index: 5, width: 1, height: 1 };
        assert!(PdfImporter.image(&bytes, &OpenOptions::default(), &missing).is_err());
    }
}
