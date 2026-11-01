//! Step 5: pieces put in a series or taken out, points taken out or put in, and the pieces and points drawn.

use crate::wizard_canvas::*;

/// A piece of a curve colour that is in no series, on the curves step: pale, so that it can be seen and clicked.
pub(in crate::wizard_canvas) const LOOSE_PIECE: Color32 = Color32::from_rgba_premultiplied(150, 150, 150, 150);

/// A piece of a shape type not shown alone, on the shapes step while another is: faint.
pub(in crate::wizard_canvas) const FAINT_PIECE: Color32 = Color32::from_rgba_premultiplied(60, 60, 60, 60);

/// How near the pointer must be to a point to pick it, in screen pixels.
pub(in crate::wizard_canvas) const POINT_REACH: f32 = 8.0;

/// Whether series `number` has piece `piece`.
pub(in crate::wizard_canvas) fn has_piece(wizard: &Wizard, number: usize, piece: usize) -> bool {
    wizard.series().iter().any(|s| s.number() == number && s.pieces().contains(&piece))
}

/// The menu of a right click on the curves step: which series the piece under the pointer is in.
pub(in crate::wizard_canvas) fn piece_menu(
    ui: &mut Ui,
    wizard: &Wizard,
    piece: Option<usize>,
    intents: &mut Vec<Intent>,
) {
    let Some((piece, info)) = piece.and_then(|p| Some((p, *wizard.pieces()?.pieces().get(p)?))) else {
        ui.add_enabled(false, egui::Label::new("Right-click on a piece of a curve"));
        return;
    };
    let colour = mask_name(MaskId::Ink(info.ink));
    ui.label(egui::RichText::new(format!("This piece of {colour}, {} px, is in:", info.pixels)).strong());
    ui.separator();
    for series in wizard.series() {
        let number = series.number();
        if ui.selectable_label(series.pieces().contains(&piece), series_name(number)).clicked() {
            intents.push(Intent::Wizard(WizardIntent::SetPiece(piece, Some(number))));
            ui.close();
        }
    }
    if ui.selectable_label(wizard.series_of(piece).is_none(), "No series").clicked() {
        intents.push(Intent::Wizard(WizardIntent::SetPiece(piece, None)));
        ui.close();
    }
    ui.separator();
    if ui.button("A new series").clicked() {
        intents.push(Intent::Wizard(WizardIntent::NewSeries(Some(piece))));
        ui.close();
    }
}

impl WizardCanvas {
    /// The curves step: the pointer lights the pieces of the curves; a click adds one to the selected series, or takes it
    /// out (with no series selected, it starts one).
    pub(in crate::wizard_canvas) fn interact_pieces(
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
            return intents;
        };
        // a pointer beside a thin line is not exactly on it: look a few screen pixels around
        let piece_at = |pos: Pos2| {
            let p = view.to_figure(origin, pos);
            pieces.nearest(p.x, p.y, (6.0 / f64::from(view.zoom)).clamp(1.0, 24.0))
        };
        let moving = ui.input(|i| i.pointer.any_down());
        self.piece_hover = if response.hovered() && !moving { response.hover_pos().and_then(piece_at) } else { None };
        if self.piece_hover.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_piece = piece_at(pos);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && response.clicked_by(PointerButton::Primary)
            && let Some(piece) = piece_at(pos)
        {
            let change = match wizard.active_series() {
                Some(number) if has_piece(wizard, number, piece) => WizardIntent::SetPiece(piece, None),
                Some(number) => WizardIntent::SetPiece(piece, Some(number)),
                None => WizardIntent::NewSeries(Some(piece)),
            };
            intents.push(Intent::Wizard(change));
        }
        intents
    }

    /// The curves step, editing points: the pointer lights the point of the selected series it is on; a click takes it
    /// out, puts back a point taken out (a cross), or puts a point in where there is none, at the middle of the ink there.
    pub(in crate::wizard_canvas) fn interact_points(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        self.gesture = Gesture::Idle;
        self.piece_hover = None;
        let Some(series) = wizard.active_series().and_then(|n| wizard.series().iter().find(|s| s.number() == n)) else {
            self.point_hover = None;
            self.removed_hover = None;
            return intents;
        };
        let nearest = |points: &[Point], pos: Pos2| {
            points
                .iter()
                .map(|&p| ((view.to_screen(origin, p) - pos).length(), p))
                .filter(|(d, _)| *d <= POINT_REACH)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, p)| p)
        };
        let moving = ui.input(|i| i.pointer.any_down());
        let pos = if response.hovered() && !moving { response.hover_pos() } else { None };
        self.point_hover = pos.and_then(|pos| nearest(series.points(), pos));
        self.removed_hover =
            if self.point_hover.is_none() { pos.and_then(|pos| nearest(series.removed(), pos)) } else { None };
        if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && response.clicked_by(PointerButton::Primary)
        {
            let number = series.number();
            let change = if let Some(p) = nearest(series.points(), pos) {
                WizardIntent::RemovePoint(number, p)
            } else if let Some(p) = nearest(series.removed(), pos) {
                WizardIntent::RestorePoint(number, p)
            } else {
                // to the middle of the ink under the pointer, if there is any
                let p = view.to_figure(origin, pos);
                let reach = (f64::from(POINT_REACH) / f64::from(view.zoom)).clamp(2.0, 16.0);
                let at = wizard.pieces().and_then(|pieces| pieces.centre_near(p.x, p.y, reach)).unwrap_or(p);
                WizardIntent::AddPoint(number, at)
            };
            intents.push(Intent::Wizard(change));
        }
        intents
    }

    /// The pieces of the curves (each series' in its colour, on the curves step the others pale), the outline of the piece
    /// under the pointer or of a series whose row is, and the points each series is traced into.
    pub(in crate::wizard_canvas) fn paint_curves(
        &mut self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        let shown: Vec<usize> =
            wizard.series().iter().map(|s| s.number()).filter(|&n| wizard.is_shown(MaskId::Series(n))).collect();
        let step = wizard.step();
        let loose = scene.pieces == PieceLayer::BySeries;
        let by_shape = scene.pieces == PieceLayer::ByShape;
        let (Some(pieces), Some((visible, covers)), false) =
            (wizard.pieces(), picture_region(view, origin, canvas, wizard.size()), scene.pieces == PieceLayer::None)
        else {
            self.pieces_picture = None;
            self.outline = None;
            return;
        };
        {
            let (epoch, zoom) = (wizard.curves_epoch(), view.zoom.to_bits());
            let solo = wizard.solo_shape().filter(|_| by_shape);
            let current = self.pieces_picture.as_ref().is_some_and(|p| {
                p.epoch == epoch
                    && p.step == step
                    && p.loose == loose
                    && p.solo == solo
                    && p.zoom == zoom
                    && contains(p.covers, visible)
            });
            if !current {
                let tint = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), TINT);
                let mut colour = vec![if loose { LOOSE_PIECE } else { Color32::TRANSPARENT }; pieces.pieces().len()];
                match wizard.shapes().filter(|_| by_shape) {
                    // the colour of each piece: its shape type's, or faint while another is shown alone
                    Some(shapes) => {
                        for (k, t) in
                            shapes.types.iter().enumerate().filter(|(k, _)| wizard.is_shown(MaskId::Shape(*k)))
                        {
                            let c =
                                if solo.is_some_and(|s| s != k) { FAINT_PIECE } else { tint(shape_tint(wizard, k)) };
                            for &p in &t.members {
                                colour[p] = c;
                            }
                        }
                    }
                    // its series', or pale if it is in none
                    None => {
                        for series in wizard.series().iter().filter(|s| shown.contains(&s.number())) {
                            let c = tint(series_tint(series.number()));
                            for &p in series.pieces() {
                                colour[p] = c;
                            }
                        }
                    }
                }
                let scale = f64::from(view.zoom).min(1.0);
                let (texture, covers) = make_picture(painter.ctx(), "curve pieces", covers, scale, |x, y, _| {
                    pieces.at(x, y).and_then(|p| colour.get(p).copied()).unwrap_or(Color32::TRANSPARENT)
                });
                self.pieces_picture = Some(PiecesPicture { epoch, step, loose, solo, zoom, covers, texture });
            }
        }
        if let Some(p) = &self.pieces_picture {
            let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(p.texture.id(), view.screen_rect(origin, p.covers), uv, Color32::WHITE);
        }

        // outlined black and white, as the other masks are: on the shapes step a piece with every copy of its shape
        let type_of = |piece: usize| wizard.shapes().and_then(|s| s.type_of.get(piece).copied());
        let members =
            |k: usize| wizard.shapes().and_then(|s| s.types.get(k)).map(|t| t.members.clone()).unwrap_or_default();
        // moving pieces or their links, a piece is lit alone
        let whole_type = by_shape && wizard.shape_click() == ShapeClick::Select;
        let lit: Vec<usize> = match (self.piece_hover.or(self.menu_piece), self.panel_hover) {
            (Some(piece), _) if whole_type => type_of(piece).map(members).unwrap_or_default(),
            (Some(piece), _) => vec![piece],
            (None, Some(MaskId::Shape(k))) => members(k),
            (None, Some(MaskId::Series(n))) => {
                wizard.series().iter().find(|s| s.number() == n).map(|s| s.pieces().to_vec()).unwrap_or_default()
            }
            _ => Vec::new(),
        };
        if lit.is_empty() {
            self.outline = None;
        } else {
            let (epoch, zoom) = (wizard.curves_epoch(), view.zoom.to_bits());
            let current = self.outline.as_ref().is_some_and(|o| {
                o.pieces == lit && o.epoch == epoch && o.zoom == zoom && contains(o.covers, visible.intersect(o.covers))
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
                let inside =
                    |x: i64, y: i64| x >= 0 && y >= 0 && pieces.at(x as u32, y as u32).is_some_and(|p| member[p]);
                self.outline = bounds.map(|b| {
                    let region = b.inflate(2.0, 2.0).intersect(covers);
                    let scale = f64::from(view.zoom).min(1.0);
                    let (texture, covers) =
                        make_picture(painter.ctx(), "piece outline", region, scale, |x, y, step| {
                            let (x, y) = (i64::from(x), i64::from(y));
                            edge(inside(x, y), |dx, dy| inside(x + dx * step, y + dy * step))
                                .unwrap_or(Color32::TRANSPARENT)
                        });
                    Outline { pieces: lit, epoch, zoom, covers, texture }
                });
            }
            if let Some(o) = &self.outline {
                let uv = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                painter.image(o.texture.id(), view.screen_rect(origin, o.covers), uv, Color32::WHITE);
            }
        }

        // the points: a dot for every point of a line or a pattern, a ring for every marker; a point put in by hand is a
        // square, one taken out a cross. The selected series is drawn last and full size; the others small and faint,
        // and thinned to one point every few screen pixels, so that the one being worked on stands out
        if !scene.points {
            return;
        }
        let editing = wizard.edit_points();
        let selected = wizard.active_series();
        let mut order: Vec<&CurveSeries> = wizard.series().iter().filter(|s| shown.contains(&s.number())).collect();
        order.sort_by_key(|s| Some(s.number()) == selected);
        for series in order {
            let active = selected == Some(series.number());
            // while points are clicked every one is drawn, so that the one a click takes out is the one seen
            let (colour, edge, scale, gap) = if active {
                (series_tint(series.number()), Color32::BLACK, 1.0, if editing { 1.5 } else { 5.0 })
            } else {
                (series_tint(series.number()).gamma_multiply(0.6), Color32::from_black_alpha(110), 0.7, 4.0)
            };
            let mut last: Option<Pos2> = None;
            for p in series.points() {
                let at = view.to_screen(origin, *p);
                if !canvas.contains(at) || last.is_some_and(|l| (l - at).length() < gap) {
                    continue;
                }
                last = Some(at);
                if series.added().contains(p) {
                    let r = egui::Rect::from_center_size(at, vec2(8.0, 8.0) * scale);
                    painter.rect_filled(r.expand(1.5), CornerRadius::ZERO, edge);
                    painter.rect_filled(r, CornerRadius::ZERO, colour);
                    continue;
                }
                match series.trace() {
                    Trace::Line => {
                        painter.circle_filled(at, 2.2 * scale, edge);
                        painter.circle_filled(at, 1.4 * scale, colour);
                    }
                    Trace::Pattern => {
                        painter.circle_filled(at, 3.0 * scale, edge);
                        painter.circle_filled(at, 2.0 * scale, colour);
                    }
                    Trace::Markers => {
                        painter.circle_stroke(at, 4.5 * scale, Stroke::new(3.0 * scale, edge));
                        painter.circle_stroke(at, 4.5 * scale, Stroke::new(1.6 * scale, colour));
                    }
                }
            }
            if editing && active {
                for p in series.removed() {
                    let at = view.to_screen(origin, *p);
                    if canvas.contains(at) {
                        let (a, b) = (vec2(4.0, 4.0), vec2(4.0, -4.0));
                        for (from, to) in [(at - a, at + a), (at - b, at + b)] {
                            painter.line_segment([from, to], Stroke::new(3.5, Color32::WHITE));
                            painter.line_segment([from, to], Stroke::new(1.8, Color32::from_rgb(220, 30, 30)));
                        }
                    }
                }
            }
        }
        // the point a click would take out or put back
        if let Some(p) = self.point_hover.or(self.removed_hover) {
            let at = view.to_screen(origin, p);
            painter.circle_stroke(at, 8.0, Stroke::new(4.0, Color32::WHITE));
            painter.circle_stroke(at, 8.0, Stroke::new(2.0, Color32::BLACK));
        }
    }
}
