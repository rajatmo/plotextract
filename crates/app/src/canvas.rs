//! The canvas: the page picture, the curves over it, pan and zoom, snapping, picking, and sharp detail rendering.

use std::time::{Duration, Instant};

use egui::{Color32, Context, CornerRadius, FontId, PointerButton, Pos2, Sense, Shape, Stroke, Ui, vec2};
use kurbo::Point;
use plotextract_core::{Axis, Command, Pick, PrimitiveKind, Slot};

use crate::app::{App, curve_label};
use crate::detail;
use crate::theme::{FILL_COLOR, GUIDE_COLOR, MEMBER_COLOR, SELECT_COLOR, STROKE_COLOR};
use crate::view::{View, marker_pos};

/// Hover snaps to the nearest vertex within this many screen pixels.
const SNAP_PX: f32 = 12.0;
/// A click on a line, away from any vertex, selects it within this many pixels.
const PICK_PX: f32 = 6.0;
/// The view must be still this long before sharp detail is rendered.
const SETTLE: Duration = Duration::from_millis(250);

/// A piece of the backdrop bitmap, as a texture no larger than the GPU allows.
struct Tile {
    /// Pixel position and size within the backdrop bitmap.
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    texture: egui::TextureHandle,
}

/// A bitmap laid on the figure: its top left corner in figure space and its pixels per figure unit.
pub(crate) struct Picture {
    origin: Point,
    scale: f64,
    tiles: Vec<Tile>,
}

impl Picture {
    pub(crate) fn new(ctx: &Context, bitmap: &plotextract_core::Bitmap, origin: Point, scale: f64) -> Self {
        Self { origin, scale, tiles: tiles_of(ctx, bitmap) }
    }

    fn draw(&self, painter: &egui::Painter, view: &View, canvas_origin: Pos2) {
        let k = self.scale;
        let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        for t in &self.tiles {
            let (x0, y0) = (self.origin.x + f64::from(t.x) / k, self.origin.y + f64::from(t.y) / k);
            let area = kurbo::Rect::new(x0, y0, x0 + f64::from(t.w) / k, y0 + f64::from(t.h) / k);
            painter.image(t.texture.id(), view.screen_rect(canvas_origin, area), uv, Color32::WHITE);
        }
    }
}

/// Sharp detail laid over the backdrop.
pub(crate) struct Detail {
    /// The figure-space area the render covers.
    region: kurbo::Rect,
    scale: f64,
    picture: Picture,
}

/// Cut a bitmap into textures that fit the GPU's size limit.
fn tiles_of(ctx: &Context, bitmap: &plotextract_core::Bitmap) -> Vec<Tile> {
    let side = ctx.input(|i| i.max_texture_side).clamp(512, 4096) as u32;
    let mut tiles = Vec::new();
    for y in (0..bitmap.height).step_by(side as usize) {
        for x in (0..bitmap.width).step_by(side as usize) {
            let (w, h) = (side.min(bitmap.width - x), side.min(bitmap.height - y));
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for row in y..y + h {
                let start = ((row * bitmap.width + x) * 4) as usize;
                rgba.extend_from_slice(&bitmap.rgba[start..start + (w * 4) as usize]);
            }
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            let texture = ctx.load_texture(format!("backdrop {x},{y}"), image, egui::TextureOptions::LINEAR);
            tiles.push(Tile { x, y, w, h, texture });
        }
    }
    tiles
}

impl App {
    pub(crate) fn canvas(&mut self, ui: &mut Ui) {
        let Some(session) = self.session.as_mut() else {
            self.welcome(ui);
            return;
        };
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = response.rect;
        let origin = rect.min;
        let figure_size = (session.figure.width, session.figure.height);
        self.view.resized(rect.size(), figure_size);

        let mods = ui.input(|i| i.modifiers);
        let select_tool = mods.command;

        // zoom about the cursor, pan with the hand tool
        if let Some(cursor) = response.hover_pos() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.view.zoom_about(origin, cursor, (scroll * 0.0015).exp(), figure_size);
            }
        }
        let panning =
            (response.dragged_by(PointerButton::Primary) && !select_tool) || response.dragged_by(PointerButton::Middle);
        if panning {
            self.view.pan(response.drag_delta());
        }
        // Other widgets set their own cursor (text boxes, resize edges), and the canvas is drawn last: it may only
        // claim the cursor while the pointer is really over it, or a pan that began there is still under way.
        if panning {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(if select_tool { egui::CursorIcon::Crosshair } else { egui::CursorIcon::Grab });
        }

        self.refine_detail(ui.ctx(), rect);
        let session = self.session.as_mut().expect("checked above");

        // snapping and picking use screen-pixel radii
        let pixel = 1.0 / f64::from(self.view.zoom);
        let pointer = response.hover_pos().map(|p| self.view.to_figure(origin, p));
        self.hover =
            if select_tool { pointer.and_then(|p| session.snap_vertex(p, f64::from(SNAP_PX) * pixel)) } else { None };

        let mut command = None;
        if let (true, Some(p)) = (response.clicked_by(PointerButton::Primary) && select_tool, pointer) {
            let pick = session.pick_at(p, f64::from(SNAP_PX) * pixel, f64::from(PICK_PX) * pixel);
            command = Some(match (mods.shift, pick) {
                (true, Some(pick)) => Command::ToggleMember(pick.primitive),
                _ => Command::Select(pick),
            });
        }
        if response.secondary_clicked() {
            // a right click selects the point under the mouse, as the hand tool does not snap
            if let Some(hit) = pointer.and_then(|p| session.snap_vertex(p, f64::from(SNAP_PX) * pixel)) {
                command = Some(Command::Select(Some(Pick { primitive: hit.primitive, vertex: Some(hit.vertex) })));
            }
        }
        let mut error = None;
        if let Some(Err(e)) = command.map(|c| session.apply(c)) {
            error = Some(e.to_string());
        }

        self.paint(&painter, rect, select_tool);

        // the right-click menu records the selected point as a calibration tick
        let mut chosen = None;
        let session = self.session.as_ref().expect("checked above");
        response.context_menu(|ui| match (session.selection(), session.selected_point()) {
            (Some(pick), Some(point)) => {
                let prim = &session.figure.primitives[pick.primitive];
                let title = format!(
                    "Record point {} of {}",
                    pick.vertex.unwrap_or(0) + 1,
                    curve_label(pick.primitive, prim.kind)
                );
                ui.label(egui::RichText::new(title).strong());
                ui.separator();
                for slot in Slot::ALL {
                    let value = if slot.axis() == Axis::X { point.x } else { point.y };
                    let mut text = format!("{} = {value:.4} pt", slot.label());
                    if let Some(old) = session.calibration().position(slot) {
                        text.push_str(&format!("   (replaces {old:.4})"));
                    }
                    if ui.button(text).clicked() {
                        chosen = Some(slot);
                        ui.close();
                    }
                }
            }
            _ => {
                ui.add_enabled(false, egui::Label::new("Select a point first (Ctrl+click on a curve)"));
            }
        });
        if let Some(slot) = chosen {
            self.slot = slot;
            if self.apply(Command::RecordSelected(slot)) {
                self.info(format!("{} recorded. Type its plot value in the table.", slot.label()));
            }
        }
        if let Some(e) = error {
            self.error(e);
        }
    }

    /// The screen shown before a file is open.
    fn welcome(&mut self, ui: &mut Ui) {
        let mut open = false;
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.28).max(20.0));
            ui.image((self.icon.id(), vec2(104.0, 104.0)));
            ui.add_space(6.0);
            ui.label(egui::RichText::new("plotextract").heading().strong().size(28.0));
            ui.label(egui::RichText::new("Get the numbers out of a plot").weak());
            ui.add_space(14.0);
            if ui.add_sized([180.0, 34.0], crate::theme::primary("Open a plot\u{2026}")).clicked() {
                open = true;
            }
            ui.add_space(8.0);
            crate::theme::hint(ui, "or drop a PDF, SVG, EPS or PostScript file on the window");
        });
        if open {
            self.pending_actions.push(crate::app::Action::OpenDialog);
            ui.ctx().request_repaint();
        }
    }

    /// Receive finished detail renders, and once the view has been still for a moment ask for sharp detail of what is
    /// visible (or drop it when the backdrop is sharp enough).
    fn refine_detail(&mut self, ctx: &Context, canvas: egui::Rect) {
        if let (Some(worker), Some((id, _, _))) = (&self.worker, self.pending)
            && let Some(done) = worker.poll()
            && done.id == id
        {
            let origin = Point::new(done.region.x0, done.region.y0);
            let (w, h) = (f64::from(done.bitmap.width) / done.scale, f64::from(done.bitmap.height) / done.scale);
            let covered = kurbo::Rect::new(origin.x, origin.y, origin.x + w, origin.y + h);
            self.detail = Some(Detail {
                region: covered,
                scale: done.scale,
                picture: Picture::new(ctx, &done.bitmap, origin, done.scale),
            });
            self.pending = None;
        }

        let state = (self.view.zoom, self.view.offset, canvas.size());
        if state != self.last_view {
            self.last_view = state;
            self.changed_at = Instant::now();
        }
        let waited = self.changed_at.elapsed();
        if waited < SETTLE {
            ctx.request_repaint_after(SETTLE - waited);
            return;
        }
        let (Some(session), Some(backdrop)) = (self.session.as_ref(), self.backdrop.as_ref()) else { return };
        let (a, b) = (self.view.to_figure(canvas.min, canvas.min), self.view.to_figure(canvas.min, canvas.max));
        let visible = kurbo::Rect::new(a.x, a.y, b.x, b.y);
        let page = kurbo::Rect::new(0.0, 0.0, session.figure.width, session.figure.height);
        let screen_scale = f64::from(self.view.zoom) * f64::from(ctx.pixels_per_point());
        let wanted = if std::env::var_os("PLOTEXTRACT_NO_DETAIL").is_some() {
            None
        } else {
            detail::plan(visible, page, screen_scale, backdrop.scale)
        };
        match wanted {
            None => {
                self.detail = None;
                self.pending = None;
            }
            Some((region, scale)) => {
                let need = visible.intersect(page);
                let served = self.detail.as_ref().is_some_and(|d| detail::covers(d.region, d.scale, need, scale));
                let asked = self.pending.is_some_and(|(_, r, s)| detail::covers(r, s, need, scale));
                if let (false, false, Some(worker)) = (served, asked, self.worker.as_mut()) {
                    let id = worker.request(region, scale);
                    self.pending = Some((id, region, scale));
                }
            }
        }
    }

    fn paint(&self, painter: &egui::Painter, rect: egui::Rect, select_tool: bool) {
        let session = self.session.as_ref().expect("a figure is open");
        let painter = painter.with_clip_rect(rect);
        let view = &self.view;
        let origin = rect.min;
        let dark = painter.ctx().theme() == egui::Theme::Dark;
        painter.rect_filled(rect, 0.0, Color32::from_gray(if dark { 38 } else { 222 }));

        let page = view.screen_rect(origin, kurbo::Rect::new(0.0, 0.0, session.figure.width, session.figure.height));
        for i in 1..=5 {
            // a soft shadow under the page
            let alpha = if dark { 34 } else { 16 };
            painter.rect_filled(
                page.expand(i as f32 * 1.4).translate(vec2(0.0, 2.0)),
                CornerRadius::same(i as u8 + 1),
                Color32::from_black_alpha(alpha),
            );
        }
        painter.rect_filled(page, 0.0, Color32::WHITE);
        if session.layers().page {
            if let Some(backdrop) = &self.backdrop {
                backdrop.draw(&painter, view, origin);
            }
            if let Some(detail) = &self.detail {
                detail.picture.draw(&painter, view, origin);
            }
        }

        let in_series: Vec<bool> = {
            let mut v = vec![false; session.figure.primitives.len()];
            if let Some(a) = session.active_series() {
                for &m in &session.series()[a].members {
                    v[m] = true;
                }
            }
            v
        };
        let selected = session.selection().map(|p| p.primitive);
        let hovered = self.hover.map(|h| h.primitive);

        let draw = |i: usize, stroke_width: f32, color: Option<Color32>| {
            let prim = &session.figure.primitives[i];
            let color = color.unwrap_or(if prim.kind == PrimitiveKind::Stroke { STROKE_COLOR } else { FILL_COLOR });
            let stroke = Stroke::new(stroke_width, color);
            let mut shapes = Vec::new();
            for piece in prim.pieces() {
                if piece.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = piece.iter().map(|q| view.to_screen(origin, *q)).collect();
                if prim.kind == PrimitiveKind::Fill && color == FILL_COLOR {
                    shapes.extend(Shape::dashed_line(&pts, stroke, 6.0, 4.0));
                } else {
                    shapes.push(Shape::line(pts, stroke));
                }
            }
            painter.extend(shapes);
        };

        for (i, prim) in session.figure.primitives.iter().enumerate() {
            if !session.is_visible(i) {
                continue;
            }
            let Some(extent) = prim.extent() else { continue };
            if !rect.intersects(view.screen_rect(origin, extent).expand(3.0)) {
                continue;
            }
            draw(i, 1.5, None);
        }
        for (i, member) in in_series.iter().enumerate() {
            if *member && session.is_visible(i) {
                draw(i, 3.0, Some(MEMBER_COLOR));
            }
        }
        if let Some(i) = hovered {
            let prim = &session.figure.primitives[i];
            let color = if prim.kind == PrimitiveKind::Stroke {
                Color32::from_rgb(21, 70, 154)
            } else {
                Color32::from_rgb(168, 98, 0)
            };
            draw(i, 3.0, Some(color));
        }
        if let Some(i) = selected {
            draw(i, 3.5, Some(SELECT_COLOR));
        }

        // calibration guide lines at the recorded coordinates
        let guide = Stroke::new(1.5, GUIDE_COLOR);
        for slot in Slot::ALL {
            let Some(v) = session.calibration().position(slot) else { continue };
            let (a, b) = if slot.axis() == Axis::X {
                (Point::new(v, 0.0), Point::new(v, session.figure.height))
            } else {
                (Point::new(0.0, v), Point::new(session.figure.width, v))
            };
            painter.extend(Shape::dashed_line(
                &[view.to_screen(origin, a), view.to_screen(origin, b)],
                guide,
                8.0,
                3.0,
            ));
        }

        if let Some(hit) = self.hover {
            painter.circle_stroke(marker_pos(view.to_screen(origin, hit.pos)), 6.0, Stroke::new(2.0, SELECT_COLOR));
        }
        if let Some(p) = session.selected_point() {
            painter.circle_filled(marker_pos(view.to_screen(origin, p)), 4.0, SELECT_COLOR);
        }

        // which tool is active, as a small pill in the corner
        let text = if select_tool {
            "Select tool \u{b7} click a curve, Shift+click adds it to the series"
        } else {
            "Hand tool \u{b7} drag to pan, wheel to zoom \u{b7} hold Ctrl to select"
        };
        let galley =
            painter.layout_no_wrap(text.to_owned(), FontId::proportional(crate::theme::TEXT_SIZE), Color32::WHITE);
        let pill = egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), galley.size() + vec2(20.0, 10.0));
        painter.rect_filled(pill, CornerRadius::same(12), Color32::from_black_alpha(150));
        painter.galley(pill.min + vec2(10.0, 5.0), galley, Color32::WHITE);
    }
}
