//! SVG importer.
//!
//! `usvg` resolves the document (units, transforms, `use`, basic shapes) into a tree of paths; those become primitives
//! (clipping and text are ignored on purpose). `resvg` draws the picture and any region of it.
//!
//! Figure units are the SVG's user units: CSS pixels, so a `288pt` wide image is 384 units wide.

use std::sync::Arc;

use plotextract_domain::{Bitmap, Figure, ImportError, OpenOptions, Primitive, Style, Unit};
use plotextract_import::{BEZIER_STEPS, PrimitiveBuilder, flatten_path, pixels_for};
use plotextract_ports::{Importer, PictureSource};
use resvg::tiny_skia;
use usvg::{Node, Paint};

fn parse(bytes: &[u8]) -> Result<usvg::Tree, ImportError> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts(); // plots often draw their labels as text
    usvg::Tree::from_data(bytes, &options).map_err(|e| ImportError::message(format!("cannot read the SVG: {e}")))
}

/// The paths of a tree: stroked ones, then filled ones (each in drawing order), and whether some were dropped.
fn collect(tree: &usvg::Tree) -> (Vec<Primitive>, bool) {
    let mut builder = PrimitiveBuilder::default();
    walk(tree.root(), &mut builder);
    let truncated = builder.truncated();
    (builder.finish(), truncated)
}

fn walk(group: &usvg::Group, builder: &mut PrimitiveBuilder) {
    for node in group.children() {
        if !builder.has_room() {
            return;
        }
        match node {
            Node::Group(g) => walk(g, builder),
            Node::Path(path) if path.is_visible() => {
                let t = path.abs_transform();
                let transform = kurbo::Affine::new([
                    f64::from(t.sx),
                    f64::from(t.ky),
                    f64::from(t.kx),
                    f64::from(t.sy),
                    f64::from(t.tx),
                    f64::from(t.ty),
                ]);
                let bez = to_bez(path.data());
                let style = |paint: &Paint| match paint {
                    Paint::Color(c) => Style { color: Some([c.red, c.green, c.blue, 255]) },
                    _ => Style::default(),
                };
                if let Some(stroke) = path.stroke() {
                    builder.stroke(flatten_path(&bez, transform, BEZIER_STEPS, true), style(stroke.paint()));
                }
                if let Some(fill) = path.fill() {
                    builder.fill(flatten_path(&bez, transform, BEZIER_STEPS, false), style(fill.paint()));
                }
            }
            // text, images and the rest carry no curves to pick
            _ => {}
        }
    }
}

fn to_bez(path: &tiny_skia::Path) -> kurbo::BezPath {
    use tiny_skia::PathSegment;
    let pt = |p: tiny_skia::Point| kurbo::Point::new(f64::from(p.x), f64::from(p.y));
    let mut bez = kurbo::BezPath::new();
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => bez.move_to(pt(p)),
            PathSegment::LineTo(p) => bez.line_to(pt(p)),
            PathSegment::QuadTo(c, p) => bez.quad_to(pt(c), pt(p)),
            PathSegment::CubicTo(a, b, p) => bez.curve_to(pt(a), pt(b), pt(p)),
            PathSegment::Close => bez.close_path(),
        }
    }
    bez
}

/// Opens SVG files.
pub struct SvgImporter;

impl Importer for SvgImporter {
    fn name(&self) -> &'static str {
        "SVG"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["svg"]
    }

    fn sniff(&self, head: &[u8]) -> bool {
        String::from_utf8_lossy(head).contains("<svg")
    }

    fn import(&self, bytes: &Arc<Vec<u8>>, options: &OpenOptions) -> Result<Figure, ImportError> {
        if options.page != 0 {
            return Err(ImportError::NoSuchPage { page: options.page, count: 1 });
        }
        let tree = parse(bytes)?;
        let size = tree.size();
        let (primitives, truncated) = collect(&tree);
        Ok(Figure {
            width: f64::from(size.width()),
            height: f64::from(size.height()),
            unit: Unit::Pixel,
            primitives,
            page: 0,
            truncated,
        })
    }

    fn picture(&self, bytes: &Arc<Vec<u8>>, _options: &OpenOptions) -> Option<Box<dyn PictureSource>> {
        parse(bytes).ok().map(|tree| Box::new(SvgPicture { tree }) as Box<dyn PictureSource>)
    }
}

/// Draws parts of an SVG at any scale, in figure units.
struct SvgPicture {
    tree: usvg::Tree,
}

impl PictureSource for SvgPicture {
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError> {
        let (w, h) = (pixels_for(region.width(), scale), pixels_for(region.height(), scale));
        let mut pixmap =
            tiny_skia::Pixmap::new(w, h).ok_or_else(|| ImportError::message("the picture is too large to draw"))?;
        pixmap.fill(tiny_skia::Color::WHITE);
        let k = scale as f32;
        let transform = tiny_skia::Transform::from_scale(k, k)
            .post_translate((-region.x0 * scale) as f32, (-region.y0 * scale) as f32);
        resvg::render(&self.tree, transform, &mut pixmap.as_mut());
        Ok(Bitmap { width: w, height: h, rgba: pixmap.data().to_vec() }) // opaque on white: premultiplied = straight
    }
}
