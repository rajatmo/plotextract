//! SVG importer.
//!
//! `usvg` resolves the document (units, transforms, `use`, basic shapes) into a tree of paths; those become primitives
//! (clipping and text are ignored on purpose). `resvg` draws the page picture and any region of it.
//!
//! Figure units are the SVG's user units: CSS pixels, so a `288pt` wide image is 384 units wide.

use plotextract_core::{
    Backdrop, Bitmap, Figure, ImportError, Importer, PrimitiveBuilder, RegionRenderer, Style, flatten_path,
};
use resvg::tiny_skia;
use usvg::{Node, Paint};

/// Points sampled per cubic Bezier.
pub const BEZIER_STEPS: usize = 8;
/// Longest side of the backdrop bitmap, in pixels.
const BACKDROP_MAX_PX: f64 = 4096.0;
/// Most pixels per figure unit in the backdrop.
const BACKDROP_MAX_SCALE: f64 = 6.0;

fn parse(bytes: &[u8]) -> Result<usvg::Tree, ImportError> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts(); // plots often draw their labels as text
    usvg::Tree::from_data(bytes, &options).map_err(|e| ImportError(format!("cannot read the SVG: {e}")))
}

/// The paths of a tree: stroked ones, then filled ones (each in drawing order).
fn collect(tree: &usvg::Tree) -> Vec<plotextract_core::Primitive> {
    let mut builder = PrimitiveBuilder::default();
    walk(tree.root(), &mut builder);
    builder.finish()
}

fn walk(group: &usvg::Group, builder: &mut PrimitiveBuilder) {
    for node in group.children() {
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

/// Draw `region` (figure units) of the tree at `scale` pixels per unit onto white.
fn draw(tree: &usvg::Tree, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError> {
    let side = |len: f64| (len * scale).ceil().clamp(1.0, 16384.0) as u32;
    let (w, h) = (side(region.width()), side(region.height()));
    let mut pixmap =
        tiny_skia::Pixmap::new(w, h).ok_or_else(|| ImportError("the picture is too large to draw".into()))?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let k = scale as f32;
    let transform =
        tiny_skia::Transform::from_scale(k, k).post_translate((-region.x0 * scale) as f32, (-region.y0 * scale) as f32);
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(Bitmap { width: w, height: h, rgba: pixmap.data().to_vec() }) // opaque on white: premultiplied = straight
}

/// Opens SVG files.
pub struct SvgImporter;

impl Importer for SvgImporter {
    fn name(&self) -> &'static str {
        "SVG"
    }

    fn sniff(&self, head: &[u8]) -> bool {
        let text = String::from_utf8_lossy(head);
        text.contains("<svg")
    }

    fn import(&self, bytes: Vec<u8>) -> Result<Figure, ImportError> {
        let tree = parse(&bytes)?;
        let size = tree.size();
        let (width, height) = (f64::from(size.width()), f64::from(size.height()));
        let scale = (BACKDROP_MAX_PX / width.max(height)).min(BACKDROP_MAX_SCALE);
        let bitmap = draw(&tree, kurbo::Rect::new(0.0, 0.0, width, height), scale)?;
        Ok(Figure { width, height, backdrop: Some(Backdrop { bitmap, scale }), primitives: collect(&tree) })
    }

    fn region_renderer(&self, bytes: &[u8]) -> Option<Box<dyn RegionRenderer>> {
        parse(bytes).ok().map(|tree| Box::new(SvgRegionRenderer { tree }) as Box<dyn RegionRenderer>)
    }
}

/// Draws parts of an SVG at any scale, in figure units.
pub struct SvgRegionRenderer {
    tree: usvg::Tree,
}

impl RegionRenderer for SvgRegionRenderer {
    fn render(&mut self, region: kurbo::Rect, scale: f64) -> Result<Bitmap, ImportError> {
        draw(&self.tree, region, scale)
    }
}
