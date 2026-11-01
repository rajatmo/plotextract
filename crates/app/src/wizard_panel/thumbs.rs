//! Small pictures of pieces for the shapes step: each piece drawn from its own pixels in its colour, all at one scale so
//! that their sizes compare, and kept until the shapes change.

use std::collections::HashMap;

use egui::{Color32, CornerRadius, Rect, Sense, Stroke, TextureHandle, Ui, vec2};
use plotextract_raster::shapes::Kind;

use crate::wizard::Wizard;

/// The most pixels a side of a piece's picture has: a larger piece is drawn from every few of its pixels.
const MAX_SIDE: u32 = 160;

/// The pictures made so far, by piece, for the shapes as they were at `epoch`.
#[derive(Default)]
pub(super) struct Thumbs {
    epoch: Option<u64>,
    made: HashMap<usize, TextureHandle>,
}

/// The scale the pieces are drawn at, in points per image pixel: the largest typical piece of the data shapes fills a
/// cell `cell` points wide.
pub(super) fn shared_scale(wizard: &Wizard, cell: f32) -> f32 {
    let (Some(shapes), Some(pieces)) = (wizard.shapes(), wizard.pieces()) else { return 1.0 };
    let largest = wizard
        .shape_order()
        .into_iter()
        .filter_map(|k| shapes.types.get(k))
        .filter(|t| matches!(t.kind, Kind::Dot | Kind::Dash | Kind::Marker))
        .filter_map(|t| pieces.pieces().get(t.typical))
        .map(|p| p.bounds.width.max(p.bounds.height))
        .max()
        .unwrap_or(0);
    if largest == 0 { 2.0 } else { ((cell - 4.0) / largest as f32).clamp(0.25, 6.0) }
}

impl Thumbs {
    /// Draw piece `piece` in a square cell `cell` points wide, at `scale` points per image pixel (smaller if it would not
    /// fit, and then said so on hover); a solid line is drawn as a short stroke as thick as it. `None` draws an empty
    /// cell.
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        wizard: &Wizard,
        piece: Option<usize>,
        cell: f32,
        scale: f32,
    ) -> egui::Response {
        let (rect, response) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
        let (Some(pieces), Some(shapes), Some(p)) = (wizard.pieces(), wizard.shapes(), piece) else {
            return response;
        };
        let Some(info) = pieces.pieces().get(p) else { return response };
        let rgb = wizard.colours().map().and_then(|m| m.inks.get(info.ink)).map_or([0; 3], |i| i.rgb);
        let ink = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        // the tile is paper-like behind dark ink, dark behind light ink
        let light = 0.299 * f32::from(rgb[0]) + 0.587 * f32::from(rgb[1]) + 0.114 * f32::from(rgb[2]) < 150.0;
        let tile = if light { Color32::from_gray(250) } else { Color32::from_gray(40) };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(3), tile);
        painter.rect_stroke(
            rect,
            CornerRadius::same(3),
            Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
        let look = shapes.looks.get(p).copied().unwrap_or_default();
        if shapes.kind(p) == Some(Kind::Line) {
            let width = (look.stroke as f32 * scale).clamp(1.0, cell / 4.0);
            let y = rect.center().y;
            painter.line_segment([egui::pos2(rect.left() + 4.0, y), egui::pos2(rect.right() - 4.0, y)], (width, ink));
            return response.on_hover_text(format!("A solid line, {:.0} px long", look.length));
        }
        if self.epoch != Some(wizard.shapes_epoch()) {
            self.made.clear();
            self.epoch = Some(wizard.shapes_epoch());
        }
        let texture = self.made.entry(p).or_insert_with(|| {
            let b = info.bounds;
            let step = b.width.max(b.height).div_ceil(MAX_SIDE).max(1);
            let (w, h) = (b.width.div_ceil(step) as usize, b.height.div_ceil(step) as usize);
            let mut pixels = Vec::with_capacity(w * h);
            for j in 0..h as u32 {
                for i in 0..w as u32 {
                    let on = pieces.at(b.x + i * step, b.y + j * step) == Some(p);
                    pixels.push(if on { ink } else { Color32::TRANSPARENT });
                }
            }
            let image = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
            ui.ctx().load_texture("piece", image, egui::TextureOptions::NEAREST)
        });
        let size = vec2(info.bounds.width as f32, info.bounds.height as f32) * scale;
        let room = cell - 4.0;
        let fit = (room / size.x.max(size.y)).min(1.0);
        let shown = Rect::from_center_size(rect.center(), size * fit);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(texture.id(), shown, uv, Color32::WHITE);
        let size = format!("{} \u{d7} {} px", info.bounds.width, info.bounds.height);
        if fit < 1.0 {
            response.on_hover_text(format!("{size}, drawn smaller than the others"))
        } else {
            response.on_hover_text(size)
        }
    }
}
