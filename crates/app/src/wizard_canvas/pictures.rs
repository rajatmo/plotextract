//! The pictures drawn over the image and kept between frames: the exclude mask, the ink colours, the pieces of the
//! curves and outlines, each made for the part of the image in view and made again only when what it shows changes.

use super::*;

/// How strongly everything but the lit colour is dimmed (0 to 255).
pub(in crate::wizard_canvas) const DIM: u8 = 165;

/// The most pixels of mask picture made at once.
pub(in crate::wizard_canvas) const MAX_OVERLAY_PIXELS: usize = 6_000_000;

/// The mask as a picture over the part of the image in view, kept so that a new shape can be drawn into it.
pub(in crate::wizard_canvas) struct Overlay {
    pub(in crate::wizard_canvas) zoom: u32,
    /// The wizard's mask epoch when it was made.
    pub(in crate::wizard_canvas) epoch: u64,
    /// How many of the mask's shapes are in the picture.
    pub(in crate::wizard_canvas) shapes: usize,
    /// The part of the image the picture covers, in image pixels.
    pub(in crate::wizard_canvas) covers: Rect,
    pub(in crate::wizard_canvas) grid: Grid,
    /// The mask on `grid`: 255 where masked.
    pub(in crate::wizard_canvas) mask: Vec<u8>,
    pub(in crate::wizard_canvas) texture: TextureHandle,
}

/// The edge of a mask is drawn black, with white just outside it, so that it shows on a black background and on a white one.
pub(in crate::wizard_canvas) const EDGE: Color32 = Color32::BLACK;

pub(in crate::wizard_canvas) const EDGE_HALO: Color32 = Color32::WHITE;

/// What a pixel on the edge of a mask, or just outside it, is drawn as; `None` anywhere else. `inside` is whether the pixel
/// is in the mask and `around(dx, dy)` whether the pixel beside it is.
pub(in crate::wizard_canvas) fn edge(inside: bool, around: impl Fn(i64, i64) -> bool) -> Option<Color32> {
    let beside = [(1, 0), (-1, 0), (0, 1), (0, -1)].map(|(dx, dy)| around(dx, dy));
    if inside && beside.contains(&false) {
        Some(EDGE)
    } else if !inside && beside.contains(&true) {
        Some(EDGE_HALO)
    } else {
        None
    }
}

/// A pixel of the exclude mask's picture: red where masked, with the edge drawn.
pub(in crate::wizard_canvas) fn exclude_pixel(mask: &[u8], width: usize, height: usize, x: usize, y: usize) -> Color32 {
    let masked = |x: usize, y: usize| mask[y * width + x] == 255;
    let inside = masked(x, y);
    // beyond the picture a pixel is taken to be like this one: no edge at the border of what is drawn
    let around = |dx: i64, dy: i64| {
        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
        if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
            inside
        } else {
            masked(nx as usize, ny as usize)
        }
    };
    edge(inside, around).unwrap_or(if inside { MASK_FILL } else { Color32::TRANSPARENT })
}

/// How an ink's mask is drawn over the image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::wizard_canvas) enum Look {
    /// The ink's pixels stay as they are, the rest of the image is dimmed, and the edge is drawn.
    Spotlight,
    /// The ink's pixels are tinted.
    Tint(Color32),
    /// The ink's pixels are tinted and the edge is drawn: the mask the tools work on.
    OutlinedTint(Color32),
}

/// A mask of an ink drawn at the image's resolution (or the screen's, zoomed out) over the part of the image in view.
pub(in crate::wizard_canvas) struct InkPicture {
    pub(in crate::wizard_canvas) ink: usize,
    /// Which analysis it was made from (the address of the colour map).
    pub(in crate::wizard_canvas) map: usize,
    pub(in crate::wizard_canvas) zoom: u32,
    pub(in crate::wizard_canvas) look: Look,
    pub(in crate::wizard_canvas) covers: Rect,
    pub(in crate::wizard_canvas) texture: TextureHandle,
}

/// The pieces of the curves, drawn over the part of the image in view: on the shapes step each shape type's in its
/// colour; else each series' in its colour, on the curves step the others pale.
pub(in crate::wizard_canvas) struct PiecesPicture {
    /// The wizard's curves epoch, and the step and whether it was the curves step, when it was made.
    pub(in crate::wizard_canvas) epoch: u64,
    pub(in crate::wizard_canvas) step: Step,
    pub(in crate::wizard_canvas) loose: bool,
    /// The shape type shown alone, the others faint.
    pub(in crate::wizard_canvas) solo: Option<usize>,
    pub(in crate::wizard_canvas) zoom: u32,
    pub(in crate::wizard_canvas) covers: Rect,
    pub(in crate::wizard_canvas) texture: TextureHandle,
}

/// The outline of some pieces: the one under the pointer, or a series whose row is under it.
pub(in crate::wizard_canvas) struct Outline {
    pub(in crate::wizard_canvas) pieces: Vec<usize>,
    pub(in crate::wizard_canvas) epoch: u64,
    pub(in crate::wizard_canvas) zoom: u32,
    pub(in crate::wizard_canvas) covers: Rect,
    pub(in crate::wizard_canvas) texture: TextureHandle,
}

/// The part of the image in view, and the part a picture of it is made for: a little beyond the view so that panning does
/// not redo it at every step, the whole image when zoomed out, and never more than [`MAX_OVERLAY_PIXELS`]. `None` when
/// nothing of the image is in view.
pub(in crate::wizard_canvas) fn picture_region(
    view: &View,
    origin: Pos2,
    canvas: egui::Rect,
    size: (u32, u32),
) -> Option<(Rect, Rect)> {
    let image = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
    let (a, b) = (view.to_figure(origin, canvas.min), view.to_figure(origin, canvas.max));
    let visible = Rect::new(a.x, a.y, b.x, b.y).intersect(image);
    if visible.width() <= 0.0 || visible.height() <= 0.0 {
        return None;
    }
    let scale = f64::from(view.zoom).min(1.0);
    let mut covers = if scale < 1.0 {
        image
    } else {
        visible.inflate(visible.width() / 4.0, visible.height() / 4.0).intersect(image)
    };
    covers = Rect::new(covers.x0.floor(), covers.y0.floor(), covers.x1.ceil(), covers.y1.ceil());
    let pixels = |r: Rect| (r.width() * scale).ceil() as usize * (r.height() * scale).ceil() as usize;
    if pixels(covers) > MAX_OVERLAY_PIXELS {
        covers = Rect::new(visible.x0.floor(), visible.y0.floor(), visible.x1.ceil(), visible.y1.ceil());
    }
    Some((visible, covers))
}

/// A picture of the image region `covers` at `scale`, each of its pixels coloured by `colour(x, y, step)` for the image
/// pixel `(x, y)` it shows (`step` image pixels apart, zoomed out); and the region it covers exactly.
pub(in crate::wizard_canvas) fn make_picture(
    ctx: &Context,
    name: &str,
    covers: Rect,
    scale: f64,
    colour: impl Fn(u32, u32, i64) -> Color32,
) -> (TextureHandle, Rect) {
    let (w, h) =
        ((covers.width() * scale).ceil().max(1.0) as usize, (covers.height() * scale).ceil().max(1.0) as usize);
    let step = (1.0 / scale).round().max(1.0) as i64;
    let mut pixels = Vec::with_capacity(w * h);
    for j in 0..h {
        let y = (covers.y0 + (j as f64 + 0.5) / scale).floor().max(0.0) as u32;
        for i in 0..w {
            let x = (covers.x0 + (i as f64 + 0.5) / scale).floor().max(0.0) as u32;
            pixels.push(colour(x, y, step));
        }
    }
    let picture = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
    let texture = ctx.load_texture(name, picture, egui::TextureOptions::NEAREST);
    (texture, Rect::new(covers.x0, covers.y0, covers.x0 + w as f64 / scale, covers.y0 + h as f64 / scale))
}

impl WizardCanvas {
    /// Draw ink `ink`'s mask, as `look` says, for the part of the image in view and a little beyond it. `slot` keeps the
    /// picture between frames.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::wizard_canvas) fn paint_ink(
        slot: &mut Option<InkPicture>,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        map: &Arc<ColourMap>,
        ink: usize,
        look: Look,
    ) {
        let size = wizard.size();
        let image = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let (a, b) = (view.to_figure(origin, canvas.min), view.to_figure(origin, canvas.max));
        let visible = Rect::new(a.x, a.y, b.x, b.y).intersect(image);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            return;
        }
        let colours = wizard.colours();
        let scale = f64::from(view.zoom).min(1.0);
        let (map_id, zoom_bits) = (Arc::as_ptr(map) as usize, view.zoom.to_bits());
        let current = slot.as_ref().is_some_and(|p| {
            p.ink == ink && p.map == map_id && p.zoom == zoom_bits && p.look == look && contains(p.covers, visible)
        });
        if !current {
            let mut covers = if scale < 1.0 {
                image
            } else {
                visible.inflate(visible.width() / 4.0, visible.height() / 4.0).intersect(image)
            };
            covers = Rect::new(covers.x0.floor(), covers.y0.floor(), covers.x1.ceil(), covers.y1.ceil());
            let pixels = |r: Rect| (r.width() * scale).ceil() as usize * (r.height() * scale).ceil() as usize;
            if pixels(covers) > MAX_OVERLAY_PIXELS {
                covers = Rect::new(visible.x0.floor(), visible.y0.floor(), visible.x1.ceil(), visible.y1.ceil());
            }
            let (w, h) = ((covers.width() * scale).ceil() as usize, (covers.height() * scale).ceil() as usize);
            let mut pixels = Vec::with_capacity(w * h);
            // zoomed out, a pixel of the picture is several of the image's: the edge is one pixel of the picture wide
            let step = (1.0 / scale).round().max(1.0) as i64;
            for j in 0..h {
                let y = (covers.y0 + (j as f64 + 0.5) / scale).floor().max(0.0) as u32;
                for i in 0..w {
                    let x = (covers.x0 + (i as f64 + 0.5) / scale).floor().max(0.0) as u32;
                    let coverage = u32::from(colours.coverage(ink, x, y));
                    let outlined = !matches!(look, Look::Tint(_));
                    let edge = if outlined {
                        // the pixels beside this one, a pixel of the picture away
                        let around = |dx: i64, dy: i64| {
                            let (nx, ny) = (i64::from(x) + dx * step, i64::from(y) + dy * step);
                            nx >= 0 && ny >= 0 && colours.coverage(ink, nx as u32, ny as u32) >= 128
                        };
                        edge(coverage >= 128, around)
                    } else {
                        None
                    };
                    pixels.push(match (edge, look) {
                        (Some(colour), _) => colour,
                        // the ink's own pixels stay lit, and the less of a pixel is the ink the more it is dimmed
                        (None, Look::Spotlight) => {
                            Color32::from_black_alpha((u32::from(DIM) * (255 - coverage) / 255) as u8)
                        }
                        (None, Look::Tint(c) | Look::OutlinedTint(c)) => Color32::from_rgba_unmultiplied(
                            c.r(),
                            c.g(),
                            c.b(),
                            (u32::from(TINT) * coverage / 255) as u8,
                        ),
                    });
                }
            }
            let picture = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
            let texture = painter.ctx().load_texture("ink mask", picture, egui::TextureOptions::NEAREST);
            let covers = Rect::new(covers.x0, covers.y0, covers.x0 + w as f64 / scale, covers.y0 + h as f64 / scale);
            *slot = Some(InkPicture { ink, map: map_id, zoom: zoom_bits, look, covers, texture });
        }
        if let Some(p) = slot {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(p.texture.id(), view.screen_rect(origin, p.covers), uv, Color32::WHITE);
        }
    }

    /// The mask as a red tint, drawn at the image's own resolution (or the screen's, when zoomed out) for the part of
    /// the image that is in view, a little beyond it so that panning does not redo it at every step.
    pub(in crate::wizard_canvas) fn paint_mask(
        &mut self,
        ctx: &Context,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
    ) {
        if wizard.prepare().mask.is_empty() {
            self.overlay = None;
            return;
        }
        let size = wizard.size();
        let image = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let (a, b) = (view.to_figure(origin, canvas.min), view.to_figure(origin, canvas.max));
        let visible = Rect::new(a.x, a.y, b.x, b.y).intersect(image);
        if visible.width() <= 0.0 || visible.height() <= 0.0 {
            return;
        }
        let zoom = f64::from(view.zoom);
        let scale = zoom.min(1.0);
        let zoom_bits = view.zoom.to_bits();
        let ops = &wizard.prepare().mask;
        let current = self
            .overlay
            .as_ref()
            .is_some_and(|o| o.zoom == zoom_bits && o.epoch == wizard.mask_epoch() && contains(o.covers, visible));
        if !current {
            // a new picture of everything: the view moved out of the old one, or the mask changed other than by growing
            let mut covers = if scale < 1.0 {
                image
            } else {
                visible.inflate(visible.width() / 4.0, visible.height() / 4.0).intersect(image)
            };
            covers = Rect::new(covers.x0.floor(), covers.y0.floor(), covers.x1.ceil(), covers.y1.ceil());
            let pixels = |r: Rect| (r.width() * scale).ceil() as usize * (r.height() * scale).ceil() as usize;
            if pixels(covers) > MAX_OVERLAY_PIXELS {
                covers = Rect::new(visible.x0.floor(), visible.y0.floor(), visible.x1.ceil(), visible.y1.ceil());
            }
            let grid = Grid {
                origin: Point::new(covers.x0, covers.y0),
                scale,
                width: (covers.width() * scale).ceil() as usize,
                height: (covers.height() * scale).ceil() as usize,
            };
            let mask = wizard.prepare().rasterize_mask(&grid);
            let image = egui::ColorImage {
                size: [grid.width, grid.height],
                source_size: vec2(grid.width as f32, grid.height as f32),
                pixels: (0..grid.height)
                    .flat_map(|y| (0..grid.width).map(move |x| (x, y)))
                    .map(|(x, y)| exclude_pixel(&mask, grid.width, grid.height, x, y))
                    .collect(),
            };
            let texture = ctx.load_texture("mask", image, egui::TextureOptions::NEAREST);
            let covers = Rect::new(
                covers.x0,
                covers.y0,
                covers.x0 + grid.width as f64 / scale,
                covers.y0 + grid.height as f64 / scale,
            );
            self.overlay = Some(Overlay {
                zoom: zoom_bits,
                epoch: wizard.mask_epoch(),
                shapes: ops.len(),
                covers,
                grid,
                mask,
                texture,
            });
        } else if let Some(o) = self.overlay.as_mut().filter(|o| o.shapes < ops.len()) {
            // shapes were added at the end of the mask: draw just those into the picture, and send the window system only
            // the part of the picture they touch
            let new = &ops[o.shapes..];
            plotextract_raster::paint_ops(new, &o.grid, &mut o.mask);
            o.shapes = ops.len();
            let touched = new.iter().map(MaskOp::bounds).reduce(|a, b| a.union(b));
            if let Some(b) = touched {
                let to_pixels = |v: f64, from: f64, limit: usize| {
                    (((v - from) * o.grid.scale).floor().max(0.0) as usize).min(limit)
                };
                let x0 = to_pixels(b.x0, o.grid.origin.x, o.grid.width).saturating_sub(1);
                let y0 = to_pixels(b.y0, o.grid.origin.y, o.grid.height).saturating_sub(1);
                let x1 = (to_pixels(b.x1, o.grid.origin.x, o.grid.width) + 2).min(o.grid.width);
                let y1 = (to_pixels(b.y1, o.grid.origin.y, o.grid.height) + 2).min(o.grid.height);
                if x1 > x0 && y1 > y0 {
                    let (w, h) = (x1 - x0, y1 - y0);
                    let mut pixels = Vec::with_capacity(w * h);
                    for row in y0..y1 {
                        pixels
                            .extend((x0..x1).map(|col| exclude_pixel(&o.mask, o.grid.width, o.grid.height, col, row)));
                    }
                    let part = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
                    o.texture.set_partial([x0, y0], part, egui::TextureOptions::NEAREST);
                }
            }
        }
        if let Some(o) = &self.overlay {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(o.texture.id(), view.screen_rect(origin, o.covers), uv, Color32::WHITE);
        }
    }
}
