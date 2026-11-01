//! Step 4: every piece drawn in its group's colour over the darkened image, outlined dark, and ringed when it is too small
//! on screen to see, so that a piece of one pixel is found at any zoom; the pieces in no group grey. The pointer lights a
//! piece's group. A click selects a piece's group (Shift adds or takes away a group, Ctrl one piece), a drag around
//! pieces selects them (Shift adds, Alt takes away); the panel makes the selection one group or takes it out.

use rayon::prelude::*;

use crate::wizard_canvas::*;

/// A piece smaller than this on screen, in screen pixels, gets a ring around it.
pub(in crate::wizard_canvas) const RING_BELOW: f32 = 12.0;
/// The radius of the ring around a piece of no size, in screen pixels.
pub(in crate::wizard_canvas) const RING: f32 = 6.0;
/// The most rings drawn of pieces not selected: more than that in view are dense enough to be seen.
const MAX_RINGS: usize = 30_000;
/// How dark the image is made under the pieces (0 to 255).
pub(in crate::wizard_canvas) const DARKEN: u8 = 150;
/// How strong a piece not selected is drawn while some are (0 to 255).
const UNSELECTED: u8 = 80;
/// The rim drawn around every piece, so that it shows on any colour.
pub(in crate::wizard_canvas) const RIM: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 230);

/// The colour of the pieces in no group.
pub(crate) const LEFTOVER: Color32 = Color32::from_rgb(205, 205, 205);

/// The colour group `k` is drawn with.
pub(crate) fn group_tint(k: usize) -> Color32 {
    tint_of(k + 2)
}

/// The colour piece `p` is drawn with: its group's, or grey if it is in none; `None` if its group is hidden.
pub(crate) fn piece_tint(wizard: &Wizard, p: usize) -> Option<Color32> {
    match wizard.shapes()?.type_of.get(p).copied()? {
        Some(k) if !wizard.is_shown(MaskId::Shape(k)) => None,
        Some(k) => Some(group_tint(k)),
        None => Some(LEFTOVER),
    }
}

/// The pieces of the group piece `p` is in, or `p` alone if it is in none.
fn group_of(wizard: &Wizard, p: usize) -> Vec<usize> {
    let Some(shapes) = wizard.shapes() else { return vec![p] };
    match shapes.type_of.get(p).copied().flatten() {
        Some(k) => shapes.types[k].members.clone(),
        None => vec![p],
    }
}

/// The name of the group piece `p` is in, with how many pieces it has; or that it is in none.
fn group_label(wizard: &Wizard, p: usize) -> String {
    let Some(shapes) = wizard.shapes() else { return String::new() };
    match shapes.type_of.get(p).copied().flatten() {
        Some(k) => format!("{} \u{b7} {} pieces", wizard.group_name(k), shapes.types[k].members.len()),
        None => "Left over: in no group".into(),
    }
}

/// The menu of a right click on the shapes step: select the piece's group, or work on the selection.
pub(in crate::wizard_canvas) fn shape_menu(
    ui: &mut Ui,
    wizard: &Wizard,
    piece: Option<usize>,
    intents: &mut Vec<Intent>,
) {
    let mut act = |intent: WizardIntent| intents.push(Intent::Wizard(intent));
    if let Some(p) = piece {
        ui.label(egui::RichText::new(group_label(wizard, p)).strong());
        ui.separator();
        let grouped = wizard.shapes().is_some_and(|s| s.type_of.get(p).copied().flatten().is_some());
        // with something selected, the group is added to it or taken out of it, as Shift+click does
        let group = group_of(wizard, p);
        let selected = wizard.selected();
        let all_in = group.iter().all(|q| selected.contains(q));
        let what = if grouped { "its group" } else { "this piece" };
        if !selected.is_empty() && !all_in && ui.button(format!("Add {what} to the selection (Shift+click)")).clicked()
        {
            act(WizardIntent::SelectPieces(group.clone(), Pick::Add));
            ui.close();
        }
        if all_in
            && selected.len() > group.len()
            && ui.button(format!("Take {what} out of the selection (Shift+click)")).clicked()
        {
            act(WizardIntent::SelectPieces(group.clone(), Pick::Subtract));
            ui.close();
        }
        let only = if selected.is_empty() { format!("Select {what}") } else { format!("Select only {what}") };
        if ui.button(only).clicked() {
            act(WizardIntent::SelectPieces(group, Pick::Replace));
            ui.close();
        }
        if grouped && ui.button("Take this piece out of its group").clicked() {
            act(WizardIntent::SelectPieces(vec![p], Pick::Replace));
            act(WizardIntent::RemoveSelected);
            ui.close();
        }
    }
    let n = wizard.selected().len();
    if n == 0 {
        if piece.is_none() {
            ui.add_enabled(false, egui::Label::new("Nothing is selected"));
        }
        return;
    }
    if piece.is_some() {
        ui.separator();
    }
    ui.label(egui::RichText::new(format!("{n} piece{} selected", if n == 1 { "" } else { "s" })).weak());
    if ui.button("Make them one group (G)").clicked() {
        act(WizardIntent::JoinSelected);
        ui.close();
    }
    if ui.button("Take them out of their groups (Delete)").clicked() {
        act(WizardIntent::RemoveSelected);
        ui.close();
    }
    if ui.button("Select the pieces like them (S)").clicked() {
        act(WizardIntent::SelectSimilar);
        ui.close();
    }
}

/// What piece `p` measures and which group it is in, for the tooltip of the piece under the pointer.
fn describe(wizard: &Wizard, p: usize) -> String {
    let (Some(shapes), Some(pieces)) = (wizard.shapes(), wizard.pieces()) else { return String::new() };
    let (Some(look), Some(info)) = (shapes.looks.get(p), pieces.pieces().get(p)) else { return String::new() };
    let mut text = format!(
        "{}\nThis piece: {} \u{d7} {} px, {} pixels, strokes {:.1} px",
        group_label(wizard, p),
        info.bounds.width,
        info.bounds.height,
        info.pixels,
        look.stroke
    );
    if look.holes > 0 {
        text += &format!(", {} hole{}", look.holes, if look.holes == 1 { "" } else { "s" });
    }
    text
}

/// Whether `p` is inside the polygon `around`.
pub(in crate::wizard_canvas) fn inside(around: &[Point], p: Point) -> bool {
    let mut odd = false;
    for (i, a) in around.iter().enumerate() {
        let b = around[(i + 1) % around.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

/// The scale the pictures of the pieces are made at, in picture pixels per image pixel: the image's own resolution
/// zoomed in, else the power of two at or above the zoom, so that zooming out makes them afresh only now and then.
pub(in crate::wizard_canvas) fn picture_scale(zoom: f32) -> f64 {
    let zoom = f64::from(zoom);
    if zoom >= 1.0 { 1.0 } else { 2f64.powf(zoom.log2().ceil()).min(1.0) }
}

impl WizardCanvas {
    /// The shapes step: the pointer lights a piece's group; a click or a drag around pieces changes the selection.
    pub(in crate::wizard_canvas) fn interact_shapes(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        self.gesture = Gesture::Idle;
        let (Some(pieces), Some(shapes)) = (wizard.pieces(), wizard.shapes()) else {
            self.piece_hover = None;
            self.lasso.clear();
            return intents;
        };
        let shown = |p: usize| piece_tint(wizard, p).is_some();
        let piece_at = |pos: Pos2| {
            let p = view.to_figure(origin, pos);
            pieces.nearest(p.x, p.y, (6.0 / f64::from(view.zoom)).clamp(1.0, 24.0)).filter(|&p| shown(p))
        };
        let moving = ui.input(|i| i.pointer.any_down());
        let pointer = if response.hovered() && !moving { response.hover_pos() } else { None };
        self.piece_hover = pointer.and_then(piece_at);
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if let Some(p) = self.piece_hover {
            response.clone().on_hover_text_at_pointer(describe(wizard, p));
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_piece = piece_at(pos);
        }
        let mods = ui.input(|i| i.modifiers);
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.clicked_by(PointerButton::Primary)) {
            let change = match piece_at(pos) {
                Some(p) if mods.command => Some(WizardIntent::SelectPieces(vec![p], Pick::Toggle)),
                Some(p) if mods.shift => Some(WizardIntent::SelectPieces(group_of(wizard, p), Pick::Toggle)),
                Some(p) => Some(WizardIntent::SelectPieces(group_of(wizard, p), Pick::Replace)),
                // a click beside every piece lets go of the selection
                None if !mods.shift && !mods.command => Some(WizardIntent::SelectPieces(Vec::new(), Pick::Replace)),
                None => None,
            };
            intents.extend(change.map(Intent::Wizard));
        }
        // a drag goes around pieces
        if response.drag_started_by(PointerButton::Primary) {
            self.lasso.clear();
        }
        if response.dragged_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let last = self.lasso.last().map(|&l| view.to_screen(origin, l));
            if last.is_none_or(|l| (l - pos).length() >= 3.0) {
                self.lasso.push(view.to_figure(origin, pos));
            }
        }
        if response.drag_stopped() && !self.lasso.is_empty() {
            let around = std::mem::take(&mut self.lasso);
            if around.len() >= 3 {
                let caught: Vec<usize> =
                    (0..shapes.looks.len()).filter(|&p| shown(p) && inside(&around, shapes.looks[p].centre)).collect();
                let pick = if mods.alt {
                    Pick::Subtract
                } else if mods.shift {
                    Pick::Add
                } else {
                    Pick::Replace
                };
                intents.push(Intent::Wizard(WizardIntent::SelectPieces(caught, pick)));
            }
        }
        intents
    }

    /// The shapes step: the image darkened, every piece in its group's colour with a dark rim (those not selected faint
    /// while some are), a ring around each piece too small to see, the outline of the selection and of the group under
    /// the pointer (or whose row is), and the line being drawn around pieces. Holding H shows the image alone.
    pub(in crate::wizard_canvas) fn paint_pieces(
        &mut self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        let (Some(pieces), Some(shapes), Some((visible, covers)), PieceLayer::Groups) =
            (wizard.pieces(), wizard.shapes(), picture_region(view, origin, canvas, wizard.size()), scene.pieces)
        else {
            self.pieces_picture = None;
            self.outline = None;
            self.selection_outline = None;
            return;
        };
        let ctx = painter.ctx();
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::H)) {
            return;
        }
        let (w, h) = wizard.size();
        let image = view.screen_rect(origin, Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        painter.rect_filled(image.intersect(canvas), CornerRadius::ZERO, Color32::from_black_alpha(DARKEN));

        let selected = wizard.selected();
        let any_selected = !selected.is_empty();
        let colour_of = |p: usize| {
            piece_tint(wizard, p).map(|c| {
                if any_selected && !selected.contains(&p) {
                    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), UNSELECTED)
                } else {
                    c
                }
            })
        };
        let scale = picture_scale(view.zoom);
        let rim = view.zoom <= 3.0;
        let epoch = wizard.pieces_epoch();
        let current = self.pieces_picture.as_ref().is_some_and(|p| {
            p.epoch == epoch && p.scale == scale.to_bits() && p.rim == rim && contains(p.covers, visible)
        });
        if !current {
            let colours: Vec<Option<Color32>> = (0..pieces.pieces().len()).map(colour_of).collect();
            let (texture, covers) = piece_picture(ctx, pieces, &colours, covers, scale, rim);
            self.pieces_picture = Some(PiecesPicture { epoch, scale: scale.to_bits(), rim, covers, texture });
        }
        if let Some(p) = &self.pieces_picture {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(p.texture.id(), view.screen_rect(origin, p.covers), uv, Color32::WHITE);
        }

        // the group under the pointer (or whose row is)
        let lit = match (self.piece_hover.or(self.menu_piece), self.panel_hover) {
            (Some(p), _) => group_of(wizard, p),
            (None, Some(MaskId::Shape(k))) => shapes.types.get(k).map(|t| t.members.clone()).unwrap_or_default(),
            _ => Vec::new(),
        };
        let mut is_lit = vec![false; pieces.pieces().len()];
        for &p in &lit {
            is_lit[p] = true;
        }

        // a ring around each piece too small to see, and around every piece of a group whose typical piece is, so that a
        // group looks one way: those not selected first, the selected over them; the group lit has a white edge
        let mut rings: Vec<(Pos2, f32, Color32, bool, bool)> = Vec::new();
        let reach = canvas.expand(RING * 3.0);
        let typical = |p: usize| {
            let t = shapes.type_of[p].map(|k| shapes.types[k].typical);
            t.and_then(|t| pieces.pieces().get(t)).map_or(u32::MAX, |t| t.extent())
        };
        for (p, info) in pieces.pieces().iter().enumerate() {
            let extent = info.extent() as f32 * view.zoom;
            if extent.min(typical(p) as f32 * view.zoom) >= RING_BELOW {
                continue;
            }
            let Some(colour) = colour_of(p) else { continue };
            let at = view.to_screen(origin, shapes.looks[p].centre);
            if reach.contains(at) {
                rings.push((at, RING.max(extent / 2.0 + 3.0), colour, selected.contains(&p), is_lit[p]));
            }
        }
        let others = rings.iter().filter(|r| !r.3).count();
        for &(at, r, colour, chosen, lit) in &rings {
            if chosen || (others > MAX_RINGS && !lit) {
                continue;
            }
            let dark = Color32::from_black_alpha(if any_selected && !lit { 70 } else { 170 });
            if lit {
                painter.circle_stroke(at, r + 2.0, Stroke::new(1.5, Color32::WHITE));
            }
            painter.circle_stroke(at, r, Stroke::new(3.5, dark));
            painter.circle_stroke(at, r, Stroke::new(1.8, colour));
        }
        for &(at, r, _, chosen, _) in &rings {
            if chosen {
                painter.circle_stroke(at, r + 1.5, Stroke::new(5.0, Color32::BLACK));
                painter.circle_stroke(at, r + 1.5, Stroke::new(2.5, Color32::WHITE));
            }
        }

        // the outlines, black with white just outside, of the selection and of the group lit
        let chosen: Vec<usize> = selected.iter().copied().filter(|&p| piece_tint(wizard, p).is_some()).collect();
        Self::paint_outline(&mut self.selection_outline, painter, view, origin, covers, visible, wizard, chosen);
        Self::paint_outline(&mut self.outline, painter, view, origin, covers, visible, wizard, lit);

        if self.lasso.len() >= 2 {
            let mut line: Vec<Pos2> = self.lasso.iter().map(|&p| view.to_screen(origin, p)).collect();
            line.push(line[0]);
            painter.add(egui::Shape::line(line.clone(), Stroke::new(3.0, Color32::WHITE)));
            painter.add(egui::Shape::line(line, Stroke::new(1.5, Color32::BLACK)));
        }
    }

    /// The outline of the pieces `lit`, kept in `slot` between frames.
    #[allow(clippy::too_many_arguments)]
    fn paint_outline(
        slot: &mut Option<Outline>,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        covers: Rect,
        visible: Rect,
        wizard: &Wizard,
        lit: Vec<usize>,
    ) {
        let Some(pieces) = wizard.pieces().filter(|_| !lit.is_empty()) else {
            *slot = None;
            return;
        };
        let scale = picture_scale(view.zoom);
        let epoch = wizard.shapes_epoch();
        let current = slot.as_ref().is_some_and(|o| {
            o.pieces == lit
                && o.epoch == epoch
                && o.zoom == scale.to_bits()
                && contains(o.covers, visible.intersect(o.covers))
        });
        if !current {
            let bounds = lit
                .iter()
                .filter_map(|&p| pieces.pieces().get(p))
                .map(|p| {
                    let b = p.bounds;
                    Rect::new(f64::from(b.x), f64::from(b.y), f64::from(b.x + b.width), f64::from(b.y + b.height))
                })
                .reduce(|a, b| a.union(b));
            let mut member = vec![false; pieces.pieces().len()];
            for &p in &lit {
                member[p] = true;
            }
            let inside = |x: i64, y: i64| x >= 0 && y >= 0 && pieces.at(x as u32, y as u32).is_some_and(|p| member[p]);
            *slot = bounds.map(|b| {
                let region = b.inflate(2.0, 2.0).intersect(covers);
                let (texture, covers) = make_picture(painter.ctx(), "piece outline", region, scale, |x, y, step| {
                    let (x, y) = (i64::from(x), i64::from(y));
                    edge(inside(x, y), |dx, dy| inside(x + dx * step, y + dy * step)).unwrap_or(Color32::TRANSPARENT)
                });
                Outline { pieces: lit, epoch, zoom: scale.to_bits(), covers, texture }
            });
        }
        if let Some(o) = slot {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(o.texture.id(), view.screen_rect(origin, o.covers), uv, Color32::WHITE);
        }
    }
}

/// A picture of the pieces over the image region `covers` at `scale`, each in its colour (`None`: not drawn), and the
/// region it covers exactly. Zoomed out, a pixel of the picture shows a piece if any image pixel it stands for is one, so
/// that no piece is lost however thin; with `rim`, every piece has a dark rim a pixel of the picture wide.
pub(in crate::wizard_canvas) fn piece_picture(
    ctx: &Context,
    pieces: &Pieces,
    colours: &[Option<Color32>],
    covers: Rect,
    scale: f64,
    rim: bool,
) -> (TextureHandle, Rect) {
    let (w, h) =
        ((covers.width() * scale).ceil().max(1.0) as usize, (covers.height() * scale).ceil().max(1.0) as usize);
    let step = (1.0 / scale).round().max(1.0) as u32;
    let drawn = |p: usize| colours.get(p).copied().flatten();
    // the piece each pixel of the picture shows, if any: the first drawn one among the image pixels it stands for. The
    // rows are made on every core: a picture of the whole image is millions of lookups
    let shows: Vec<Option<Color32>> = (0..h)
        .into_par_iter()
        .flat_map_iter(|j| {
            let y0 = (covers.y0 + j as f64 / scale).floor().max(0.0) as u32;
            (0..w).map(move |i| {
                let x0 = (covers.x0 + i as f64 / scale).floor().max(0.0) as u32;
                (0..step)
                    .flat_map(|dy| (0..step).map(move |dx| (x0 + dx, y0 + dy)))
                    .find_map(|(x, y)| pieces.at(x, y).and_then(drawn))
            })
        })
        .collect();
    let solid = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && shows[y as usize * w + x as usize].is_some_and(|c| c.a() > UNSELECTED)
    };
    let (shows, solid) = (&shows, &solid);
    let pixels: Vec<Color32> = (0..h)
        .into_par_iter()
        .flat_map_iter(|j| {
            (0..w).map(move |i| match shows[j * w + i] {
                Some(c) => c,
                None if rim && (-1i64..=1).any(|dy| (-1i64..=1).any(|dx| solid(i as i64 + dx, j as i64 + dy))) => RIM,
                None => Color32::TRANSPARENT,
            })
        })
        .collect();
    let picture = egui::ColorImage { size: [w, h], source_size: vec2(w as f32, h as f32), pixels };
    let texture = ctx.load_texture("curve pieces", picture, egui::TextureOptions::NEAREST);
    (texture, Rect::new(covers.x0, covers.y0, covers.x0 + w as f64 / scale, covers.y0 + h as f64 / scale))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_point_is_inside_the_line_drawn_around_it() {
        let square = [Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(10.0, 10.0), Point::new(0.0, 10.0)];
        assert!(inside(&square, Point::new(5.0, 5.0)));
        assert!(!inside(&square, Point::new(15.0, 5.0)));
        assert!(!inside(&square, Point::new(5.0, -1.0)));
    }

    #[test]
    fn the_pictures_are_made_at_the_image_resolution_or_a_power_of_two_below_it() {
        assert_eq!(picture_scale(3.0), 1.0);
        assert_eq!(picture_scale(1.0), 1.0);
        assert_eq!(picture_scale(0.6), 1.0);
        assert_eq!(picture_scale(0.5), 0.5);
        assert_eq!(picture_scale(0.3), 0.5);
        assert_eq!(picture_scale(0.1), 0.125);
    }
}
