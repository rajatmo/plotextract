//! Step 4: the pointer lights a piece (and, selecting, every copy of its shape) and says what it is and why. What a click
//! does is chosen on the panel: select the shape, move pieces into the selected shape or out of it (a drag goes around
//! many), or cut and join the paths of the dots and dashes, whose links are drawn between them.

use crate::wizard_canvas::*;

/// How near the pointer must be to a link to pick it, in screen pixels.
const LINK_REACH: f32 = 6.0;

/// How long a link must be on the screen, in points, to be drawn but while links are edited.
const LINK_SEEN: f32 = 12.0;

/// The menu of a right click on the shapes step: what the shape of the piece under the pointer is.
pub(in crate::wizard_canvas) fn shape_menu(
    ui: &mut Ui,
    wizard: &Wizard,
    piece: Option<usize>,
    intents: &mut Vec<Intent>,
) {
    let Some((k, kind)) = piece.and_then(|p| {
        let shapes = wizard.shapes()?;
        let k = *shapes.type_of.get(p)?;
        Some((k, shapes.types.get(k)?.kind))
    }) else {
        ui.add_enabled(false, egui::Label::new("Right-click on a piece of a curve"));
        return;
    };
    let count = wizard.shapes().map_or(0, |s| s.types[k].members.len());
    ui.label(egui::RichText::new(format!("{} ({count} pieces) is:", mask_name(MaskId::Shape(k)))).strong());
    ui.separator();
    for choice in Kind::ALL {
        if ui.selectable_label(kind == choice, kind_label(choice)).on_hover_text(kind_tip(choice)).clicked() {
            intents.push(Intent::Wizard(WizardIntent::SetKind(k, choice)));
            ui.close();
        }
    }
    if let (Some(active), Some(p)) = (wizard.active_shape().filter(|&a| a != k), piece) {
        ui.separator();
        if ui.button(format!("Put this piece in {}", mask_name(MaskId::Shape(active)))).clicked() {
            intents.push(Intent::Wizard(WizardIntent::MovePieces(vec![p], Some(active))));
            ui.close();
        }
    }
}

/// What piece `p` is and why, for the tooltip of the piece under the pointer.
fn describe(wizard: &Wizard, p: usize) -> String {
    let (Some(shapes), Some(pieces)) = (wizard.shapes(), wizard.pieces()) else { return String::new() };
    let (Some(&k), Some(look), Some(info)) = (shapes.type_of.get(p), shapes.looks.get(p), pieces.pieces().get(p))
    else {
        return String::new();
    };
    let t = &shapes.types[k];
    let mut text = format!("{} \u{b7} {}\n", mask_name(MaskId::Shape(k)), kind_label(t.kind));
    text += &format!(
        "{} \u{d7} {} px, {} pixels, strokes {:.1} px",
        info.bounds.width, info.bounds.height, info.pixels, look.stroke
    );
    if look.holes > 0 {
        text += &format!(", {} hole{}", look.holes, if look.holes == 1 { "" } else { "s" });
    }
    let path = shapes.chain_of(p).map_or(0, |c| shapes.chains[c].len());
    let why = if wizard.moved_by_hand(p) {
        "Put in this shape by hand".to_string()
    } else {
        match t.kind {
            Kind::Line => format!(
                "A solid line: {:.0} px long, {:.0} times as long as it is thick",
                look.length,
                look.length / look.stroke.max(1.0)
            ),
            Kind::Dot | Kind::Dash => match shapes.links_at(p) {
                2 => format!("Linked to its neighbours on both sides, on a path of {path} pieces"),
                1 => format!("At an end of a path of {path} pieces"),
                _ => "On no path, but a copy of the dots or dashes of one".into(),
            },
            Kind::Marker => match t.spacing {
                Some(s) => format!("One of {} copies of a shape on no path, about {s:.0} px apart", t.members.len()),
                None => format!("One of {} copies of a shape on no path", t.members.len()),
            },
            Kind::Text => "Close to pieces of other shapes, as the letters of a word are".into(),
            Kind::Other => "Like no other piece of its colour near it, and not a line".into(),
        }
    };
    text += "\n";
    text += &why;
    if t.by_hand && !wizard.moved_by_hand(p) {
        text += "\nIts shape was changed by hand";
    }
    text
}

/// Whether `p` is inside the polygon `around`.
fn inside(around: &[Point], p: Point) -> bool {
    let mut odd = false;
    for (i, a) in around.iter().enumerate() {
        let b = around[(i + 1) % around.len()];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            odd = !odd;
        }
    }
    odd
}

/// How far the screen point `p` is from the segment `a`-`b`.
fn to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

impl WizardCanvas {
    /// The shapes step: the pointer lights a piece; a click does what the panel says (select, move, links).
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
            self.link_hover = None;
            self.lasso.clear();
            return intents;
        };
        let click = wizard.shape_click();
        if click != ShapeClick::Links {
            self.join_from = None;
        }
        if click != ShapeClick::Move {
            self.lasso.clear();
        }
        let piece_at = |pos: Pos2| {
            let p = view.to_figure(origin, pos);
            pieces.nearest(p.x, p.y, (6.0 / f64::from(view.zoom)).clamp(1.0, 24.0))
        };
        let centre = |p: usize| view.to_screen(origin, shapes.looks[p].centre);
        let link_at = |pos: Pos2| {
            shapes
                .links
                .iter()
                .map(|&(a, b)| (to_segment(pos, centre(a), centre(b)), (a, b)))
                .filter(|(d, _)| *d <= LINK_REACH)
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, link)| link)
        };
        let moving = ui.input(|i| i.pointer.any_down());
        let pointer = if response.hovered() && !moving { response.hover_pos() } else { None };
        self.piece_hover = pointer.and_then(piece_at);
        self.link_hover = match (click, self.piece_hover) {
            (ShapeClick::Links, None) => pointer.and_then(link_at),
            _ => None,
        };
        if self.piece_hover.is_some() || self.link_hover.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if let Some(p) = self.piece_hover.filter(|_| self.lasso.is_empty()) {
            let text = describe(wizard, p);
            response.clone().on_hover_text_at_pointer(text);
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_piece = piece_at(pos);
        }
        let clicked = response.interact_pointer_pos().filter(|_| response.clicked_by(PointerButton::Primary));
        let type_of = |p: usize| shapes.type_of.get(p).copied();
        match click {
            ShapeClick::Select => {
                if let Some(k) = clicked.and_then(piece_at).and_then(type_of) {
                    intents.push(Intent::Wizard(WizardIntent::SetActive(MaskId::Shape(k))));
                }
            }
            ShapeClick::Move => {
                let active = wizard.active_shape();
                if let Some(p) = clicked.and_then(piece_at) {
                    intents.push(Intent::Wizard(match active {
                        Some(k) if type_of(p) == Some(k) => WizardIntent::MovePieces(vec![p], None),
                        Some(k) => WizardIntent::MovePieces(vec![p], Some(k)),
                        None => WizardIntent::SetActive(MaskId::Shape(type_of(p).unwrap_or(0))),
                    }));
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
                if response.drag_stopped() {
                    let around = std::mem::take(&mut self.lasso);
                    let out = ui.input(|i| i.modifiers.alt);
                    if around.len() >= 3 && (out || active.is_some()) {
                        let caught: Vec<usize> =
                            (0..shapes.looks.len()).filter(|&p| inside(&around, shapes.looks[p].centre)).collect();
                        let caught: Vec<usize> =
                            if out { caught } else { caught.into_iter().filter(|&p| type_of(p) != active).collect() };
                        if !caught.is_empty() {
                            let to = if out { None } else { active };
                            intents.push(Intent::Wizard(WizardIntent::MovePieces(caught, to)));
                        }
                    }
                }
            }
            ShapeClick::Links => {
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.join_from = None;
                }
                if let Some(pos) = clicked {
                    let end = |p: usize| shapes.links_at(p) < 2;
                    match (piece_at(pos), self.join_from) {
                        (Some(p), Some(from)) if p != from && end(p) => {
                            intents.push(Intent::Wizard(WizardIntent::JoinPieces(from, p)));
                            self.join_from = None;
                        }
                        (Some(p), _) if end(p) => self.join_from = Some(p),
                        (Some(_), _) => {}
                        (None, _) => {
                            self.join_from = None;
                            if let Some((a, b)) = link_at(pos) {
                                intents.push(Intent::Wizard(WizardIntent::CutLink(a, b)));
                            }
                        }
                    }
                }
            }
        }
        intents
    }

    /// On the shapes step: the links of the paths of dots and dashes (their ends ringed while links are edited), the link
    /// under the pointer, the piece a join starts from, the odd piece looked at, and the line drawn around pieces.
    pub(in crate::wizard_canvas) fn paint_shapes(
        &self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        let (Some(shapes), true) = (wizard.shapes(), scene.chains) else { return };
        let editing = wizard.shape_click() == ShapeClick::Links;
        let centre = |p: usize| view.to_screen(origin, shapes.looks[p].centre);
        let (width, colour) = if editing {
            (2.0, Color32::from_rgb(255, 190, 0))
        } else {
            (1.2, Color32::from_rgba_unmultiplied(255, 190, 0, 170))
        };
        let near = canvas.expand(20.0);
        for &(a, b) in &shapes.links {
            let (sa, sb) = (centre(a), centre(b));
            if !near.contains(sa) && !near.contains(sb) {
                continue;
            }
            // zoomed out, links between pieces a few screen pixels apart would hide the pieces: they are drawn once the
            // gaps they bridge can be seen, and while links are edited without their dark edge
            let long = (sa - sb).length();
            if !editing && long < LINK_SEEN {
                continue;
            }
            if long >= LINK_SEEN {
                painter.line_segment([sa, sb], Stroke::new(width + 2.0, Color32::from_black_alpha(120)));
            }
            painter.line_segment([sa, sb], Stroke::new(width, colour));
        }
        if editing {
            for chain in &shapes.chains {
                for &p in [chain.first(), chain.last()].into_iter().flatten() {
                    let at = centre(p);
                    if canvas.contains(at) {
                        painter.circle_stroke(at, 5.0, Stroke::new(3.0, Color32::BLACK));
                        painter.circle_stroke(at, 5.0, Stroke::new(1.5, Color32::from_rgb(255, 190, 0)));
                    }
                }
            }
        }
        if let Some((a, b)) = self.link_hover {
            painter.line_segment([centre(a), centre(b)], Stroke::new(5.0, Color32::WHITE));
            painter.line_segment([centre(a), centre(b)], Stroke::new(3.0, Color32::from_rgb(220, 30, 30)));
        }
        if let Some(from) = self.join_from.filter(|&p| p < shapes.looks.len()) {
            let at = centre(from);
            painter.circle_stroke(at, 7.0, Stroke::new(4.0, Color32::WHITE));
            painter.circle_stroke(at, 7.0, Stroke::new(2.0, Color32::from_rgb(30, 160, 60)));
            if let Some(to) = painter.ctx().pointer_hover_pos().filter(|p| canvas.contains(*p)) {
                painter.line_segment([at, to], Stroke::new(3.0, Color32::WHITE));
                painter.line_segment([at, to], Stroke::new(1.5, Color32::from_rgb(30, 160, 60)));
            }
        }
        // the odd piece being looked at, ringed so it is found at any zoom
        if let (Some(p), Some(pieces)) = (wizard.focus_piece(), wizard.pieces()) {
            let b = pieces.pieces()[p].bounds;
            let r = (b.width.max(b.height) as f32 * view.zoom / 2.0 + 8.0).max(12.0);
            let at = centre(p);
            painter.circle_stroke(at, r, Stroke::new(4.5, Color32::WHITE));
            painter.circle_stroke(at, r, Stroke::new(2.5, Color32::from_rgb(220, 30, 30)));
        }
        if self.lasso.len() >= 2 {
            let mut line: Vec<Pos2> = self.lasso.iter().map(|&p| view.to_screen(origin, p)).collect();
            line.push(line[0]);
            painter.add(egui::Shape::line(line.clone(), Stroke::new(3.0, Color32::WHITE)));
            painter.add(egui::Shape::line(line, Stroke::new(1.5, Color32::BLACK)));
        }
    }
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
    fn the_distance_to_a_link_is_to_its_nearest_point() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0));
        assert_eq!(to_segment(Pos2::new(5.0, 3.0), a, b), 3.0);
        assert_eq!(to_segment(Pos2::new(14.0, 3.0), a, b), 5.0);
    }
}
