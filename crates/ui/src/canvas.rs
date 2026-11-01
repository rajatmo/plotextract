//! The canvas: the page picture, the curves over it, pan and zoom, snapping and picking. It reads the session, keeps only
//! its own state (view, pictures, hover) and reports what the user did as [`Intent`]s.

use std::time::{Duration, Instant};

use egui::{Color32, Context, CornerRadius, FontId, PointerButton, Pos2, Sense, Shape, Stroke, Ui, vec2};
use kurbo::Point;
use plotextract_domain::{Axis, Bitmap, Edit, Figure, Hit, Pick, PrimitiveKind, Slot};
use plotextract_usecases::Session;
use plotextract_usecases::Source;

use crate::cursor;
use crate::detail::{self, Kind, Worker};
use crate::theme::{FILL_COLOR, GUIDE_COLOR, MEMBER_COLOR, SELECT_COLOR, STROKE_COLOR, TEXT_SIZE};
use crate::view::{View, marker_pos};
use crate::wizard_canvas::WizardCanvas;
use plotextract_model::Intent;
use plotextract_model::text::{curve_label, measure, slot_label};
use plotextract_usecases::wizard::{MaskId, Pen, Step, Tool, Wizard};

/// Hover snaps to the nearest vertex within this many screen pixels.
const SNAP_PX: f32 = 12.0;
/// A click on a line, away from any vertex, selects it within this many pixels.
const PICK_PX: f32 = 6.0;
/// Curves smaller than this on screen (their longer side, in pixels) are not outlined: the picture shows them.
const MIN_OUTLINE_PX: f32 = 1.0;
/// With more curves than this in view none is outlined: every one costs a frame its share, and the picture shows them.
const MAX_OUTLINED: usize = 10_000;
/// Points closer than this many pixels to the last one drawn are skipped.
const MIN_STEP_PX: f32 = 1.0;
/// The view must be still this long before sharp detail is rendered.
const SETTLE: Duration = Duration::from_millis(250);

/// A piece of a bitmap, as a texture no larger than the GPU allows.
struct Tile {
    /// Pixel position and size within the bitmap.
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    texture: egui::TextureHandle,
}

/// A bitmap laid on the figure: its top left corner in figure space and its pixels per figure unit.
struct Picture {
    origin: Point,
    scale: f64,
    tiles: Vec<Tile>,
}

impl Picture {
    fn new(ctx: &Context, bitmap: &Bitmap, origin: Point, scale: f64) -> Self {
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

/// Sharp detail laid over the base picture.
struct Detail {
    /// The figure-space area the render covers.
    region: kurbo::Rect,
    scale: f64,
    picture: Picture,
}

/// The detail render asked for and not yet delivered.
#[derive(Clone, Copy)]
struct Pending {
    id: u64,
    region: kurbo::Rect,
    scale: f64,
}

/// Cut a bitmap into textures that fit the GPU's size limit.
fn tiles_of(ctx: &Context, bitmap: &Bitmap) -> Vec<Tile> {
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
            let texture = ctx.load_texture(format!("picture {x},{y}"), image, egui::TextureOptions::LINEAR);
            tiles.push(Tile { x, y, w, h, texture });
        }
    }
    tiles
}

/// The screen points of one piece of a curve, keeping only what can show: the stretches outside `clip` are dropped and
/// points within a pixel of the last one kept are skipped. A curve of millions of vertices becomes a few thousand
/// points, whatever the zoom.
fn screen_runs(piece: &[Point], view: &View, origin: Pos2, clip: egui::Rect) -> Vec<Vec<Pos2>> {
    let mut runs = Vec::new();
    let mut run: Vec<Pos2> = Vec::new();
    let mut skipped: Option<Pos2> = None; // the latest point left out of the run, kept if the run ends there
    let mut previous: Option<Pos2> = None;
    for q in piece {
        let p = view.to_screen(origin, *q);
        if let Some(a) = previous {
            if !clip.intersects(egui::Rect::from_two_pos(a, p)) {
                // this segment is off screen: close the run
                run.extend(skipped.take());
                if run.len() >= 2 {
                    runs.push(std::mem::take(&mut run));
                }
                run.clear();
                previous = Some(p);
                continue;
            }
            if run.is_empty() {
                run.push(a);
            }
        }
        match run.last() {
            Some(&last) if (p - last).length_sq() < MIN_STEP_PX * MIN_STEP_PX => skipped = Some(p),
            _ => {
                run.push(p);
                skipped = None;
            }
        }
        previous = Some(p);
    }
    run.extend(skipped);
    if run.len() >= 2 {
        runs.push(run);
    }
    runs
}

/// Everything the canvas remembers between frames.
pub struct CanvasState {
    pub view: View,
    /// The wizard's half-finished gestures and the mask picture.
    wizard: WizardCanvas,
    hover: Option<Hit>,
    base: Option<Picture>,
    detail: Option<Detail>,
    pending: Option<Pending>,
    worker: Option<Worker>,
    base_scale: f64,
    /// View state seen last frame, and when it last differed.
    last_view: (f32, egui::Vec2, egui::Vec2),
    changed_at: Instant,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            view: View::default(),
            wizard: WizardCanvas::default(),
            hover: None,
            base: None,
            detail: None,
            pending: None,
            worker: None,
            base_scale: 1.0,
            last_view: (0.0, egui::Vec2::ZERO, egui::Vec2::ZERO),
            changed_at: Instant::now(),
        }
    }
}

impl CanvasState {
    /// A figure was opened: forget the old one's pictures and ask for the new one's.
    pub fn open(&mut self, ctx: &Context, figure: &Figure, source: Option<Source>) {
        *self = Self::default();
        self.base_scale = detail::base_scale(figure.width, figure.height);
        self.worker = source.map(|s| Worker::spawn(ctx.clone(), s));
        if let Some(worker) = &mut self.worker {
            let page = kurbo::Rect::new(0.0, 0.0, figure.width, figure.height);
            worker.request(Kind::Base, page, self.base_scale);
        }
    }

    /// The colour under the pointer, in the wizard's colour step.
    pub fn hovered_ink(&self) -> Option<usize> {
        self.wizard.hovered()
    }

    /// A colour's row in the wizard's panel is under the pointer: light that colour on the image.
    pub fn set_panel_hover(&mut self, mask: Option<MaskId>) {
        self.wizard.set_panel_hover(mask);
    }

    /// The vertex under the pointer, while the select tool is held.
    pub fn hover(&self) -> Option<Hit> {
        self.hover
    }
}

/// Draw the canvas for a session and return what the user did on it. While an image is being prepared (`wizard`) the
/// pointer crops and masks instead of picking curves.
pub fn show(ui: &mut Ui, session: &Session, state: &mut CanvasState, wizard: Option<&Wizard>) -> Vec<Intent> {
    let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
    let rect = response.rect;
    let origin = rect.min;
    let figure_size = (session.figure.width, session.figure.height);
    state.view.resized(rect.size(), figure_size);

    let mods = ui.input(|i| i.modifiers);
    let select_tool = mods.command && wizard.is_none();

    // zoom about the cursor, pan with the hand tool
    if let Some(cursor) = response.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            state.view.zoom_about(origin, cursor, (scroll * 0.0015).exp(), figure_size);
        }
    }
    let hand = match wizard {
        Some(w) => WizardCanvas::pans(w),
        None => !select_tool,
    };
    let panning = (response.dragged_by(PointerButton::Primary) && hand) || response.dragged_by(PointerButton::Middle);
    if panning {
        state.view.pan(response.drag_delta());
    }
    // Other widgets set their own cursor (text boxes, resize edges), and the canvas is drawn last: it may only
    // claim the cursor while the pointer is really over it, or a pan that began there is still under way.
    if panning {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() && (wizard.is_none() || hand) {
        ui.ctx().set_cursor_icon(if select_tool { egui::CursorIcon::Crosshair } else { egui::CursorIcon::Grab });
    }

    state.refine_detail(ui.ctx(), rect, session);

    // snapping and picking use screen-pixel radii
    let pixel = 1.0 / f64::from(state.view.zoom);
    let pointer = response.hover_pos().map(|p| state.view.to_figure(origin, p));
    state.hover =
        if select_tool { pointer.and_then(|p| session.snap_vertex(p, f64::from(SNAP_PX) * pixel)) } else { None };

    let mut intents = Vec::new();
    if let (true, Some(p)) = (response.clicked_by(PointerButton::Primary) && select_tool, pointer) {
        let pick = session.pick_at(p, f64::from(SNAP_PX) * pixel, f64::from(PICK_PX) * pixel);
        intents.push(match (mods.shift, pick) {
            (true, Some(pick)) => Intent::Edit(Edit::ToggleMember(pick.primitive)),
            _ => Intent::Select(pick),
        });
    }
    if response.secondary_clicked() {
        // a right click selects the point under the mouse, as the hand tool does not snap
        if let Some(hit) = pointer.and_then(|p| session.snap_vertex(p, f64::from(SNAP_PX) * pixel)) {
            intents.push(Intent::Select(Some(Pick { primitive: hit.primitive, vertex: Some(hit.vertex) })));
        }
    }

    if let Some(w) = wizard
        && !panning
    {
        intents.extend(state.wizard.interact(ui, &response, &state.view, origin, w));
    }

    state.paint(&painter, rect, select_tool, session, wizard);
    if let Some(w) = wizard {
        state.wizard.paint(&painter.with_clip_rect(rect), &state.view, origin, rect, w);
        state.wizard.context_menu(&response, w, &mut intents);
        if response.hovered() || response.dragged() {
            cursor::replace_system_cursor(ui.ctx());
        }
        return intents;
    }

    // the right-click menu records the selected point as a calibration tick, and hides the selected curve
    response.context_menu(|ui| {
        let Some(pick) = session.selection() else {
            ui.add_enabled(false, egui::Label::new("Select a curve first (Ctrl+click on it)"));
            return;
        };
        let prim = &session.figure.primitives[pick.primitive];
        if let Some(point) = session.selected_point() {
            let title =
                format!("Record point {} of {}", pick.vertex.unwrap_or(0) + 1, curve_label(pick.primitive, prim.kind));
            ui.label(egui::RichText::new(title).strong());
            ui.separator();
            for slot in Slot::ALL {
                let value = if slot.axis() == Axis::X { point.x } else { point.y };
                let mut label = format!("{} = {}", slot_label(slot), measure(value, session.figure.unit, 4));
                if let Some(old) = session.calibration().position(slot) {
                    label.push_str(&format!("   (replaces {old:.4})"));
                }
                if ui.button(label).clicked() {
                    intents.push(Intent::RecordAt(slot));
                    ui.close();
                }
            }
            ui.separator();
        } else {
            ui.label(egui::RichText::new(curve_label(pick.primitive, prim.kind)).strong());
            ui.separator();
        }
        // a curve in the way (grid lines over the data, minor ticks beside the major ones) is hidden, and is then
        // neither drawn nor snapped to
        let hide = format!("Hide {}", curve_label(pick.primitive, prim.kind));
        if ui.button(hide).on_hover_text("Hide this curve: it is no longer drawn, snapped to or picked").clicked() {
            intents.push(Intent::Edit(Edit::SetHidden(pick.primitive, true)));
            ui.close();
        }
    });
    if response.hovered() || response.dragged() {
        cursor::replace_system_cursor(ui.ctx());
    }
    intents
}

impl CanvasState {
    /// Receive finished renders, and once the view has been still for a moment ask for sharp detail of what is visible
    /// (or drop it when the base picture is sharp enough).
    fn refine_detail(&mut self, ctx: &Context, canvas: egui::Rect, session: &Session) {
        if let Some(worker) = &self.worker {
            for done in worker.poll_all() {
                let origin = Point::new(done.region.x0, done.region.y0);
                let picture = Picture::new(ctx, &done.bitmap, origin, done.scale);
                match done.kind {
                    Kind::Base => self.base = Some(picture),
                    Kind::Detail if self.pending.is_some_and(|p| p.id == done.id) => {
                        let (w, h) =
                            (f64::from(done.bitmap.width) / done.scale, f64::from(done.bitmap.height) / done.scale);
                        let covered = kurbo::Rect::new(origin.x, origin.y, origin.x + w, origin.y + h);
                        self.detail = Some(Detail { region: covered, scale: done.scale, picture });
                        self.pending = None;
                    }
                    Kind::Detail => {}
                }
            }
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
        if self.worker.is_none() {
            return;
        }
        let (a, b) = (self.view.to_figure(canvas.min, canvas.min), self.view.to_figure(canvas.min, canvas.max));
        let visible = kurbo::Rect::new(a.x, a.y, b.x, b.y);
        let page = kurbo::Rect::new(0.0, 0.0, session.figure.width, session.figure.height);
        let screen_scale = f64::from(self.view.zoom) * f64::from(ctx.pixels_per_point());
        let wanted = if std::env::var_os("PLOTEXTRACT_NO_DETAIL").is_some() {
            None
        } else {
            detail::plan(visible, page, screen_scale, self.base_scale)
        };
        match wanted {
            None => {
                self.detail = None;
                self.pending = None;
            }
            Some((region, scale)) => {
                let need = visible.intersect(page);
                let served = self.detail.as_ref().is_some_and(|d| detail::covers(d.region, d.scale, need, scale));
                let asked = self.pending.is_some_and(|p| detail::covers(p.region, p.scale, need, scale));
                if let (false, false, Some(worker)) = (served, asked, self.worker.as_mut()) {
                    let id = worker.request(Kind::Detail, region, scale);
                    self.pending = Some(Pending { id, region, scale });
                }
            }
        }
    }

    fn paint(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        select_tool: bool,
        session: &Session,
        wizard: Option<&Wizard>,
    ) {
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
            if let Some(base) = &self.base {
                base.draw(&painter, view, origin);
            }
            if let Some(detail) = &self.detail {
                detail.picture.draw(&painter, view, origin);
            }
        }

        let members: &[usize] = session.active_series().map_or(&[], |a| &session.series()[a].members);
        let selected = session.selection().map(|p| p.primitive);
        let hovered = self.hover.map(|h| h.primitive);

        let clip = rect.expand(3.0);
        let draw = |i: usize, stroke_width: f32, color: Option<Color32>| {
            if !clip.intersects(view.screen_rect(origin, session.bounds(i))) {
                return;
            }
            let prim = &session.figure.primitives[i];
            let color = color.unwrap_or(if prim.kind == PrimitiveKind::Stroke { STROKE_COLOR } else { FILL_COLOR });
            let stroke = Stroke::new(stroke_width, color);
            let mut shapes = Vec::new();
            for piece in prim.pieces() {
                for run in screen_runs(piece, view, origin, clip) {
                    if prim.kind == PrimitiveKind::Fill && color == FILL_COLOR {
                        shapes.extend(Shape::dashed_line(&run, stroke, 6.0, 4.0));
                    } else {
                        shapes.push(Shape::line(run, stroke));
                    }
                }
            }
            painter.extend(shapes);
        };

        // the curves worth outlining: in view and big enough to see
        let outlined: Vec<usize> = (0..session.figure.primitives.len())
            .filter(|&i| {
                let on_screen = view.screen_rect(origin, session.bounds(i));
                session.is_visible(i) && clip.intersects(on_screen) && on_screen.size().max_elem() >= MIN_OUTLINE_PX
            })
            .take(MAX_OUTLINED + 1)
            .collect();
        let too_many = outlined.len() > MAX_OUTLINED;
        if !too_many {
            for i in outlined {
                draw(i, 1.5, None);
            }
        }
        for &i in members {
            if session.is_visible(i) {
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
        let mut text = if let Some(w) = wizard {
            match (w.tool, w.pen, w.step()) {
                (Some(Tool::Crop), ..) => "Crop \u{b7} drag around the plot",
                (Some(Tool::Exclude), Pen::Brush, _) => "Exclude \u{b7} paint \u{b7} Alt erases",
                (Some(Tool::Exclude), Pen::Rectangle, _) => "Exclude \u{b7} drag a box \u{b7} Alt erases",
                (Some(Tool::Exclude), Pen::Polygon, _) => {
                    "Exclude \u{b7} click corners, double-click to close \u{b7} Alt erases"
                }
                (None, _, Step::Prepare) => "Drag to pan \u{b7} wheel to zoom",
                (None, _, Step::Colours) => "Hover to find a colour \u{b7} right-click to name it",
                (None, _, Step::PlotArea) => "Click a line for the selected tick",
                (None, _, Step::Shapes) => {
                    "Click: group \u{b7} Shift: add \u{b7} Ctrl: piece \u{b7} Drag: draw around \u{b7} H: image"
                }
                (None, _, Step::Links) => "Click pieces in order \u{b7} double-click or Esc to end",
            }
            .to_owned()
        } else if select_tool {
            "Click a curve \u{b7} Shift+click adds it to the series".to_owned()
        } else {
            "Drag to pan \u{b7} wheel to zoom \u{b7} Ctrl to select".to_owned()
        };
        if too_many {
            text.push_str(" \u{b7} zoom in to see outlines");
        }
        let galley =
            painter.layout(text, FontId::proportional(TEXT_SIZE), Color32::WHITE, (rect.width() - 44.0).max(100.0));
        let pill = egui::Rect::from_min_size(rect.min + vec2(12.0, 12.0), galley.size() + vec2(20.0, 10.0));
        painter.rect_filled(pill, CornerRadius::same(12), Color32::from_black_alpha(150));
        painter.galley(pill.min + vec2(10.0, 5.0), galley, Color32::WHITE);
    }
}

#[cfg(test)]
mod frame_cost {
    //! How long one frame of the canvas takes to record and tessellate for figures far larger than a plot usually is.
    //! Slow, so only run on request: `cargo test --release -p plotextract frame_cost -- --ignored --nocapture`.

    use std::time::Instant;

    use egui::pos2;
    use plotextract_domain::{Primitive, PrimitiveKind, Unit};

    use super::*;

    fn figure(primitives: Vec<Primitive>) -> Figure {
        Figure { width: 576.0, height: 432.0, unit: Unit::Point, primitives, page: 0, page_count: 1, truncated: false }
    }

    fn prim(kind: PrimitiveKind, index: usize, points: Vec<Point>) -> Primitive {
        Primitive::new(kind, index, points, vec![])
    }

    /// One curve of two million vertices.
    fn long_curve() -> Figure {
        let n = 2_000_000;
        let points = (0..n)
            .map(|i| Point::new(40.0 + 500.0 * i as f64 / n as f64, 216.0 + 150.0 * (i as f64 * 0.001).sin()))
            .collect();
        figure(vec![prim(PrimitiveKind::Stroke, 0, points)])
    }

    /// A hundred thousand little circles.
    fn markers() -> Figure {
        let primitives = (0..100_000)
            .map(|i| {
                let (cx, cy) = (
                    40.0 + 500.0 * ((i * 7919) % 1000) as f64 / 1000.0,
                    40.0 + 350.0 * ((i * 104729) % 997) as f64 / 997.0,
                );
                let points = (0..24).map(|k| {
                    let a = k as f64 * std::f64::consts::TAU / 23.0;
                    Point::new(cx + 1.5 * a.cos(), cy + 1.5 * a.sin())
                });
                prim(PrimitiveKind::Stroke, i, points.collect())
            })
            .collect();
        figure(primitives)
    }

    /// Five thousand polylines of twenty vertices.
    fn many_curves() -> Figure {
        let primitives = (0..5000)
            .map(|i| {
                let points = (0..20)
                    .map(|k| {
                        Point::new(
                            40.0 + 500.0 * ((i * 31 + k * 17) % 100) as f64 / 100.0,
                            40.0 + 350.0 * ((i * 53 + k * 29) % 100) as f64 / 100.0,
                        )
                    })
                    .collect();
                prim(PrimitiveKind::Stroke, i, points)
            })
            .collect();
        figure(primitives)
    }

    fn frame(ctx: &Context, session: &Session, state: &mut CanvasState) -> f64 {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let t = Instant::now();
        let output = ctx.run_ui(input, |ui| {
            show(ui, session, state, None);
        });
        let _ = ctx.tessellate(output.shapes, output.pixels_per_point);
        t.elapsed().as_secs_f64() * 1e3
    }

    #[test]
    #[ignore = "a measurement, not a check"]
    fn how_long_a_frame_takes() {
        for (name, figure) in
            [("one curve of 2M points", long_curve()), ("100k markers", markers()), ("5000 curves", many_curves())]
        {
            let session = Session::new(figure, None);
            let ctx = Context::default();
            let mut state = CanvasState::default();
            frame(&ctx, &session, &mut state); // fits the page
            let fitted = frame(&ctx, &session, &mut state);
            state.view.zoom_about(Pos2::ZERO, pos2(600.0, 400.0), 30.0, (576.0, 432.0));
            let zoomed = frame(&ctx, &session, &mut state);
            println!("{name:24} fitted {fitted:8.1} ms   zoomed in {zoomed:8.1} ms");
        }
    }
}

#[cfg(test)]
mod screen_run_tests {
    use egui::{Rect, pos2};

    use super::*;

    fn runs(points: &[(f64, f64)], clip: Rect) -> Vec<Vec<Pos2>> {
        let piece: Vec<Point> = points.iter().map(|&(x, y)| Point::new(x, y)).collect();
        let view = View::default(); // one pixel per unit, no offset
        screen_runs(&piece, &view, Pos2::ZERO, clip)
    }

    const SCREEN: Rect = Rect { min: pos2(0.0, 0.0), max: pos2(100.0, 100.0) };

    #[test]
    fn a_dense_curve_keeps_a_point_per_pixel_and_both_ends() {
        let points: Vec<(f64, f64)> = (0..=1000).map(|i| (f64::from(i) * 0.05, 10.0)).collect(); // 20 points per pixel
        let r = runs(&points, SCREEN);
        assert_eq!(r.len(), 1);
        let run = &r[0];
        assert!(run.len() < 70, "{} points kept of 1001", run.len());
        assert_eq!(run[0], pos2(0.0, 10.0));
        assert_eq!(*run.last().unwrap(), pos2(50.0, 10.0), "the end of the curve is never dropped");
    }

    #[test]
    fn the_stretch_off_screen_is_cut_and_the_curve_resumes_where_it_comes_back() {
        let r = runs(&[(10.0, 10.0), (50.0, 10.0), (500.0, 10.0), (900.0, 500.0), (60.0, 90.0), (30.0, 90.0)], SCREEN);
        // the segment 50 -> 500 still crosses the screen, so it is kept; 500 -> 900 is entirely outside; the way back
        // in starts with the point it comes from
        assert_eq!(r.len(), 2, "{r:?}");
        assert_eq!(r[0], [pos2(10.0, 10.0), pos2(50.0, 10.0), pos2(500.0, 10.0)]);
        assert_eq!(r[1].first(), Some(&pos2(900.0, 500.0)));
        assert_eq!(r[1].last(), Some(&pos2(30.0, 90.0)));
    }

    #[test]
    fn a_curve_wholly_outside_or_a_lone_point_draws_nothing() {
        assert!(runs(&[(500.0, 500.0), (600.0, 600.0)], SCREEN).is_empty());
        assert!(runs(&[(10.0, 10.0)], SCREEN).is_empty());
        assert!(runs(&[], SCREEN).is_empty());
    }
}
