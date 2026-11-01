//! Step 5: the pieces of a curve linked in order by hand, as with a pen drawing a polyline. A click on a piece starts a
//! link or adds the piece to the one being made, a drag through pieces adds them as it passes, a double-click ends the
//! link; a click on an end of a finished link goes on with it. Each link is drawn as a line through the centres of its
//! pieces, with a square on each and its number at its start; the link being made has a line from its last piece to
//! the pointer. The pieces not linked yet are ringed, so none is missed.

use crate::wizard_canvas::*;

/// How strong the pieces of the groups not being linked are drawn (0 to 255).
const OTHERS: u8 = 60;

/// The colour link `i` is drawn with.
pub(crate) fn link_tint(i: usize) -> Color32 {
    tint_of(i + 4)
}

/// Where piece `p` is in the links: the link and its place in it.
fn place_of(links: &[Vec<usize>], p: usize) -> Option<(usize, usize)> {
    links.iter().enumerate().find_map(|(i, l)| l.iter().position(|&q| q == p).map(|k| (i, k)))
}

/// The menu of a right click on the links step: what can be done with the piece's link.
pub(in crate::wizard_canvas) fn link_menu(
    ui: &mut Ui,
    wizard: &Wizard,
    piece: Option<usize>,
    intents: &mut Vec<Intent>,
) {
    let mut act = |intent: WizardIntent| intents.push(Intent::Wizard(intent));
    let links = wizard.links();
    if let Some((i, k)) = piece.and_then(|p| place_of(&links, p)) {
        ui.label(egui::RichText::new(format!("Link {}, piece {} of {}", i + 1, k + 1, links[i].len())).strong());
        ui.separator();
        if ui.button("Take this piece out of its link").clicked() {
            act(WizardIntent::UnlinkPiece(links[i][k]));
            ui.close();
        }
        if ui.button(format!("Turn link {} round", i + 1)).clicked() {
            act(WizardIntent::ReverseLink(i));
            ui.close();
        }
        if ui.button(format!("Delete link {}", i + 1)).clicked() {
            act(WizardIntent::DeleteLink(i));
            ui.close();
        }
    } else if piece.is_some() {
        ui.label(egui::RichText::new("Not linked: click it to link it").weak());
    }
    if wizard.open_link().is_some() {
        if piece.is_some() {
            ui.separator();
        }
        if ui.button("End the link (Esc or double-click)").clicked() {
            act(WizardIntent::EndLink);
            ui.close();
        }
        if ui.button("Take its last piece off (Backspace)").clicked() {
            act(WizardIntent::LinkBack);
            ui.close();
        }
    } else if piece.is_none() {
        ui.add_enabled(false, egui::Label::new("Right-click on a piece"));
    }
}

impl WizardCanvas {
    /// The links step: a click links the piece under the pointer, a drag links the pieces it passes through, a
    /// double-click ends the link.
    pub(in crate::wizard_canvas) fn interact_links(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        self.gesture = Gesture::Idle;
        let Some(pieces) = wizard.pieces() else {
            self.piece_hover = None;
            self.link_pointer = None;
            return intents;
        };
        let piece_at = |pos: Pos2| {
            let p = view.to_figure(origin, pos);
            pieces.nearest(p.x, p.y, (8.0 / f64::from(view.zoom)).clamp(1.0, 32.0)).filter(|&p| wizard.linkable(p))
        };
        let pointer = response.hover_pos().filter(|_| response.hovered());
        self.link_pointer = pointer.map(|pos| view.to_figure(origin, pos));
        self.piece_hover = pointer.and_then(piece_at);
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_piece = piece_at(pos);
        }
        if let Some(pos) = response.interact_pointer_pos().filter(|_| response.clicked_by(PointerButton::Primary))
            && let Some(p) = piece_at(pos)
        {
            intents.push(Intent::Wizard(WizardIntent::LinkPiece(p)));
        }
        if response.double_clicked_by(PointerButton::Primary) {
            intents.push(Intent::Wizard(WizardIntent::EndLink));
        }
        // a drag links the pieces it passes through, in the order it meets them
        if response.drag_started_by(PointerButton::Primary) {
            self.dragged_piece = None;
        }
        if response.dragged_by(PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            // the pointer may jump past small pieces in a frame: look along the way it came
            let last = pos - response.drag_delta();
            let steps = ((pos - last).length() / 3.0).ceil().max(1.0) as usize;
            for s in 1..=steps {
                let at = last + (pos - last) * (s as f32 / steps as f32);
                if let Some(p) = piece_at(at).filter(|&p| Some(p) != self.dragged_piece) {
                    self.dragged_piece = Some(p);
                    intents.push(Intent::Wizard(WizardIntent::LinkPiece(p)));
                }
            }
        }
        intents
    }

    /// The links step: the image darkened, the pieces that can be linked in their group's colour (the others faint),
    /// a ring around each piece not linked yet that is small on screen, and each link as a line through its pieces.
    pub(in crate::wizard_canvas) fn paint_links(
        &mut self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        let (Some(pieces), Some(shapes), Some((visible, covers)), PieceLayer::Links) =
            (wizard.pieces(), wizard.shapes(), picture_region(view, origin, canvas, wizard.size()), scene.pieces)
        else {
            self.links_picture = None;
            return;
        };
        let ctx = painter.ctx();
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_down(Key::H)) {
            return;
        }
        let (w, h) = wizard.size();
        let image = view.screen_rect(origin, Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        painter.rect_filled(image.intersect(canvas), CornerRadius::ZERO, Color32::from_black_alpha(DARKEN));

        let colour_of = |p: usize| {
            let c = match shapes.type_of.get(p).copied().flatten() {
                Some(k) => group_tint(k),
                None => LEFTOVER,
            };
            if wizard.linkable(p) { c } else { Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), OTHERS) }
        };
        let scale = picture_scale(view.zoom);
        let rim = view.zoom <= 3.0;
        // what the picture shows changes with the groups and the group chosen
        let epoch = wizard.shapes_epoch() * 1000 + wizard.link_group().map_or(999, |k| k as u64 % 999);
        let current = self.links_picture.as_ref().is_some_and(|p| {
            p.epoch == epoch && p.scale == scale.to_bits() && p.rim == rim && contains(p.covers, visible)
        });
        if !current {
            let colours: Vec<Option<Color32>> = (0..pieces.pieces().len()).map(|p| Some(colour_of(p))).collect();
            let (texture, covers) = piece_picture(ctx, pieces, &colours, covers, scale, rim);
            self.links_picture = Some(PiecesPicture { epoch, scale: scale.to_bits(), rim, covers, texture });
        }
        if let Some(p) = &self.links_picture {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(p.texture.id(), view.screen_rect(origin, p.covers), uv, Color32::WHITE);
        }

        let links = wizard.links();
        let mut linked = vec![false; pieces.pieces().len()];
        for &p in links.iter().flatten() {
            linked[p] = true;
        }
        let at = |p: usize| view.to_screen(origin, shapes.looks[p].centre);
        let reach = canvas.expand(RING * 3.0);

        // the pieces still to link, ringed when small on screen
        for (p, info) in pieces.pieces().iter().enumerate() {
            if linked[p] || !wizard.linkable(p) || info.extent() as f32 * view.zoom >= RING_BELOW {
                continue;
            }
            let c = at(p);
            if reach.contains(c) {
                let r = RING.max(info.extent() as f32 * view.zoom / 2.0 + 3.0);
                painter.circle_stroke(c, r, Stroke::new(3.5, Color32::from_black_alpha(170)));
                painter.circle_stroke(c, r, Stroke::new(1.8, colour_of(p)));
            }
        }

        // each link: a line through its points (along the strokes of its long pieces), a square on each spot, its number
        // at its start and an arrow at its end, which way it runs
        let open = wizard.open_link();
        let strokes = wizard.link_strokes();
        for (i, link) in links.iter().enumerate() {
            let colour = link_tint(i);
            let mut line: Vec<Pos2> =
                strokes.get(i).map(|s| s.iter().map(|&q| view.to_screen(origin, q)).collect()).unwrap_or_default();
            // a point every screen pixel or more is enough to draw
            line.dedup_by(|a, b| (*a - *b).length() < 1.5);
            if line.len() >= 2 {
                painter.add(egui::Shape::line(line.clone(), Stroke::new(4.5, Color32::from_black_alpha(200))));
                painter.add(egui::Shape::line(line.clone(), Stroke::new(2.2, colour)));
            }
            if open != Some(i)
                && let [.., before, end] = line.as_slice()
            {
                let dir = (*end - *before).normalized();
                let side = vec2(-dir.y, dir.x);
                let tip = [*end + dir * 6.0, *end - dir * 6.0 + side * 6.0, *end - dir * 6.0 - side * 6.0];
                painter.add(egui::Shape::convex_polygon(tip.to_vec(), colour, Stroke::new(1.5, Color32::BLACK)));
            }
            let spots: Vec<Pos2> = link.iter().filter(|&&p| !wizard.is_stroke(p)).map(|&p| at(p)).collect();
            for &c in spots.iter().filter(|c| reach.contains(**c)) {
                let square = egui::Rect::from_center_size(c, vec2(7.0, 7.0));
                painter.rect_filled(square.expand(1.5), CornerRadius::ZERO, Color32::BLACK);
                painter.rect_filled(square, CornerRadius::ZERO, Color32::WHITE);
            }
            if let Some(&start) = line.first() {
                let label = start + vec2(-14.0, -14.0);
                painter.circle_filled(label, 10.0, Color32::BLACK);
                painter.circle_filled(label, 8.5, colour);
                painter.text(
                    label,
                    egui::Align2::CENTER_CENTER,
                    (i + 1).to_string(),
                    egui::FontId::proportional(12.0),
                    Color32::BLACK,
                );
            }
            // the link being made: its last piece marked, and a line from it to the pointer
            if open == Some(i)
                && let Some(&last) = line.last()
            {
                painter.circle_stroke(last, 10.0, Stroke::new(4.0, Color32::BLACK));
                painter.circle_stroke(last, 10.0, Stroke::new(2.0, Color32::WHITE));
                if let Some(to) = self.link_pointer.map(|p| view.to_screen(origin, p)) {
                    let to = self.piece_hover.map_or(to, at);
                    painter.extend(egui::Shape::dashed_line(&[last, to], Stroke::new(2.0, colour), 6.0, 4.0));
                }
            }
        }

        // the piece under the pointer
        if let Some(p) = self.piece_hover.or(self.menu_piece) {
            let r = (pieces.pieces()[p].extent() as f32 * view.zoom / 2.0 + 6.0).max(RING + 4.0);
            painter.circle_stroke(at(p), r, Stroke::new(4.0, Color32::BLACK));
            painter.circle_stroke(at(p), r, Stroke::new(2.0, Color32::WHITE));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_is_found_in_the_links_with_its_place() {
        let links = vec![vec![4, 2, 9], vec![7]];
        assert_eq!(place_of(&links, 2), Some((0, 1)));
        assert_eq!(place_of(&links, 7), Some((1, 0)));
        assert_eq!(place_of(&links, 5), None);
    }
}
