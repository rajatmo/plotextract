//! Step 1: the crop and exclude tools (dragging the crop and its handles, painting with the brush, a rectangle or a
//! polygon) and what they draw.

use crate::wizard_canvas::*;

/// The mask's colour over the image.
pub(in crate::wizard_canvas) const MASK_FILL: Color32 = Color32::from_rgba_premultiplied(150, 28, 28, 110);

pub(in crate::wizard_canvas) const MASK_OUTLINE: Color32 = Color32::from_rgb(220, 50, 50);

pub(in crate::wizard_canvas) const ERASE_OUTLINE: Color32 = Color32::from_rgb(40, 170, 90);

pub(in crate::wizard_canvas) const CROP_OUTLINE: Color32 = Color32::from_rgb(47, 128, 237);

/// Which part of the crop the pointer holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::wizard_canvas) enum Handle {
    /// Outside the crop: drag out a new one.
    New,
    /// Inside it: move it.
    Move,
    /// An edge or a corner: -1 is the left or top side, 1 the right or bottom, 0 not that axis.
    Edge(i8, i8),
}

pub(in crate::wizard_canvas) enum Gesture {
    Idle,
    Crop { handle: Handle, grab: Point, start: Rect, current: Rect },
    Rectangle { from: Point, to: Point, erase: bool },
    Stroke { points: Vec<Point>, erase: bool },
    Polygon { points: Vec<Point>, erase: bool },
}

/// Which part of `crop` the pointer at screen position `pos` is on.
pub(in crate::wizard_canvas) fn hit(view: &View, origin: Pos2, crop: Rect, pos: Pos2) -> Handle {
    let s = view.screen_rect(origin, crop);
    let near = |a: f32, b: f32| (a - b).abs() <= GRAB_PX;
    let in_y = pos.y >= s.min.y - GRAB_PX && pos.y <= s.max.y + GRAB_PX;
    let in_x = pos.x >= s.min.x - GRAB_PX && pos.x <= s.max.x + GRAB_PX;
    let x = if in_y && near(pos.x, s.min.x) {
        -1
    } else if in_y && near(pos.x, s.max.x) {
        1
    } else {
        0
    };
    let y = if in_x && near(pos.y, s.min.y) {
        -1
    } else if in_x && near(pos.y, s.max.y) {
        1
    } else {
        0
    };
    match (x, y) {
        (0, 0) if s.contains(pos) => Handle::Move,
        (0, 0) => Handle::New,
        (x, y) => Handle::Edge(x, y),
    }
}

/// The crop after the pointer has moved from `grab` to `to`, held by `handle`, kept inside the image.
pub(in crate::wizard_canvas) fn dragged_crop(handle: Handle, start: Rect, grab: Point, to: Point, image: Rect) -> Rect {
    let clamp = |p: Point| Point::new(p.x.clamp(image.x0, image.x1), p.y.clamp(image.y0, image.y1));
    let to = clamp(to);
    match handle {
        Handle::New => Rect::from_points(clamp(grab), to),
        Handle::Move => {
            let mut d = to - grab;
            d.x = d.x.clamp(image.x0 - start.x0, image.x1 - start.x1);
            d.y = d.y.clamp(image.y0 - start.y0, image.y1 - start.y1);
            start + d
        }
        Handle::Edge(x, y) => {
            let mut r = start;
            match x {
                -1 => r.x0 = to.x,
                1 => r.x1 = to.x,
                _ => {}
            }
            match y {
                -1 => r.y0 = to.y,
                1 => r.y1 = to.y,
                _ => {}
            }
            r.abs()
        }
    }
}

pub(in crate::wizard_canvas) fn cursor(handle: Handle) -> CursorIcon {
    match handle {
        Handle::New => CursorIcon::Crosshair,
        Handle::Move => CursorIcon::Move,
        Handle::Edge(0, _) => CursorIcon::ResizeVertical,
        Handle::Edge(_, 0) => CursorIcon::ResizeHorizontal,
        Handle::Edge(x, y) if x == y => CursorIcon::ResizeNwSe,
        Handle::Edge(..) => CursorIcon::ResizeNeSw,
    }
}

impl WizardCanvas {
    /// The crop and exclude tools: the gesture in progress, and the edit it makes when it ends.
    pub(in crate::wizard_canvas) fn interact_prepare(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        let tool = (wizard.tool, wizard.pen);
        // the exclude tool erases while Alt is held
        let erase = ui.input(|i| i.modifiers.alt);
        let to_image = |p: Pos2| view.to_figure(origin, p);
        // A gesture starts the moment the button goes down, where it went down. egui only calls a press a drag once the
        // pointer has travelled a few pixels, and starting there would lose the first stretch of every stroke and delay
        // the first mark.
        let down = ui.input(|i| i.pointer.button_down(PointerButton::Primary));
        let pressed = ui.input(|i| i.pointer.button_pressed(PointerButton::Primary)) && response.contains_pointer();
        let start = if pressed { ui.input(|i| i.pointer.press_origin()) } else { None };
        let now = ui.input(|i| i.pointer.latest_pos()).map(to_image);
        // The cursor is asked for only over the canvas (or while a button held on it is down): the canvas is drawn last,
        // so a cursor it asks for every frame would replace the menu's, the panel's and the buttons' everywhere.
        let over = response.hovered() || down;
        let size = wizard.size();
        let image = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let mut emit = |op: WizardIntent| intents.push(Intent::Wizard(op));
        let add = |shape: Shape, erase: bool| WizardIntent::Exclude(MaskOp { shape, erase });

        match tool {
            (None, _) => {}
            (Some(Tool::Crop), _) => {
                let current = wizard.prepare().crop;
                if let Some(pos) = start {
                    let handle = current.map_or(Handle::New, |c| hit(view, origin, c, pos));
                    let p = to_image(pos);
                    let from = current.unwrap_or(Rect::from_points(p, p));
                    self.gesture = Gesture::Crop { handle, grab: p, start: from, current: from };
                } else if matches!(self.gesture, Gesture::Idle)
                    && response.hovered()
                    && let Some(pos) = response.hover_pos()
                {
                    let handle = current.map_or(Handle::New, |c| hit(view, origin, c, pos));
                    ui.ctx().set_cursor_icon(cursor(handle));
                }
                if let Gesture::Crop { handle, grab, start, current } = &mut self.gesture {
                    if down {
                        if let Some(p) = now {
                            *current = dragged_crop(*handle, *start, *grab, p, image);
                        }
                        ui.ctx().set_cursor_icon(cursor(*handle));
                    } else if let Gesture::Crop { current, .. } = std::mem::replace(&mut self.gesture, Gesture::Idle) {
                        emit(WizardIntent::SetCrop(Some(current)));
                    }
                }
            }
            (Some(Tool::Exclude), Pen::Rectangle) => {
                if over {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                if let Some(pos) = start {
                    let p = to_image(pos);
                    self.gesture = Gesture::Rectangle { from: p, to: p, erase };
                }
                if let Gesture::Rectangle { to, erase: e, .. } = &mut self.gesture {
                    if down {
                        if let Some(p) = now {
                            *to = p;
                        }
                        *e = erase;
                    } else if let Gesture::Rectangle { from, to, erase } =
                        std::mem::replace(&mut self.gesture, Gesture::Idle)
                    {
                        let r = Rect::from_points(from, to);
                        if r.width() * f64::from(view.zoom) >= 2.0 && r.height() * f64::from(view.zoom) >= 2.0 {
                            emit(add(Shape::Rect(r), erase));
                        }
                    }
                }
            }
            (Some(Tool::Exclude), Pen::Brush) => {
                if over {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                let step = 1.0 / f64::from(view.zoom);
                if let Some(pos) = start {
                    self.gesture = Gesture::Stroke { points: vec![to_image(pos)], erase };
                }
                if let Gesture::Stroke { points, erase: e } = &mut self.gesture {
                    if down {
                        *e = erase;
                        if let Some(p) = now
                            && points.last().is_none_or(|q| (*q - p).hypot() >= step)
                        {
                            points.push(p);
                        }
                    } else if let Gesture::Stroke { points, erase } =
                        std::mem::replace(&mut self.gesture, Gesture::Idle)
                    {
                        // a press that never moved is a dot
                        emit(add(Shape::Stroke { points, width: wizard.brush }, erase));
                    }
                }
            }
            (Some(Tool::Exclude), Pen::Polygon) => {
                if over {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                let (enter, escape, back) = ui
                    .input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape), i.key_pressed(Key::Backspace)));
                if escape {
                    self.gesture = Gesture::Idle;
                }
                if let Gesture::Polygon { points, .. } = &mut self.gesture
                    && back
                {
                    points.pop();
                }
                let mut close = enter || response.double_clicked_by(PointerButton::Primary);
                if let Some(pos) = start {
                    let p = to_image(pos);
                    match &mut self.gesture {
                        Gesture::Polygon { points, erase: e } => {
                            let first = points.first().map(|q| view.to_screen(origin, *q));
                            if points.len() >= 3 && first.is_some_and(|f| (f - pos).length() <= GRAB_PX) {
                                close = true;
                            } else {
                                *e = erase;
                                points.push(p);
                            }
                        }
                        _ => self.gesture = Gesture::Polygon { points: vec![p], erase },
                    }
                }
                if close
                    && let Gesture::Polygon { mut points, erase } = std::mem::replace(&mut self.gesture, Gesture::Idle)
                {
                    // a double click adds its corner twice
                    points.dedup_by(|a, b| (*a - *b).hypot() * f64::from(view.zoom) < 2.0);
                    if points.len() >= 3 {
                        emit(add(Shape::Polygon(points), erase));
                    }
                }
            }
        }
        intents
    }

    /// The crop (what is outside it dimmed, its handles while the crop tool is chosen, its size while it is chosen), what
    /// is being drawn on the exclude mask, and the brush.
    pub(in crate::wizard_canvas) fn paint_prepare(
        &self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        canvas: egui::Rect,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        let size = wizard.size();
        let image = Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let crop = match &self.gesture {
            Gesture::Crop { current, .. } => Some(*current),
            _ => wizard.prepare().crop,
        };
        if let Some(crop) = crop.filter(|_| scene.crop) {
            let page = view.screen_rect(origin, image);
            let inner = view.screen_rect(origin, crop);
            let dim = Color32::from_black_alpha(120);
            for (a, b) in [
                (page.min, Pos2::new(page.max.x, inner.min.y)),
                (Pos2::new(page.min.x, inner.max.y), page.max),
                (Pos2::new(page.min.x, inner.min.y), Pos2::new(inner.min.x, inner.max.y)),
                (Pos2::new(inner.max.x, inner.min.y), Pos2::new(page.max.x, inner.max.y)),
            ] {
                painter.rect_filled(egui::Rect::from_two_pos(a, b), CornerRadius::ZERO, dim);
            }
            painter.rect_stroke(
                inner.expand(1.0),
                CornerRadius::ZERO,
                Stroke::new(4.0, HALO),
                egui::StrokeKind::Outside,
            );
            painter.rect_stroke(inner, CornerRadius::ZERO, Stroke::new(2.0, CROP_OUTLINE), egui::StrokeKind::Outside);
            if wizard.tool == Some(Tool::Crop) {
                let h = 4.0;
                for x in [inner.min.x, inner.center().x, inner.max.x] {
                    for y in [inner.min.y, inner.center().y, inner.max.y] {
                        if (x, y) != (inner.center().x, inner.center().y) {
                            let r = egui::Rect::from_center_size(Pos2::new(x, y), vec2(2.0 * h, 2.0 * h));
                            painter.rect_filled(r, CornerRadius::ZERO, Color32::WHITE);
                            painter.rect_stroke(
                                r,
                                CornerRadius::ZERO,
                                Stroke::new(1.5, CROP_OUTLINE),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                }
            }
            // its size, while it is being chosen
            if wizard.step() == Step::Prepare {
                let label = format!("{:.0} \u{d7} {:.0} px", crop.width().round(), crop.height().round());
                let galley =
                    painter.layout_no_wrap(label, egui::FontId::proportional(crate::theme::TEXT_SIZE), Color32::WHITE);
                let at = Pos2::new(inner.min.x, (inner.min.y - galley.size().y - 8.0).max(canvas.min.y + 4.0));
                let pill = egui::Rect::from_min_size(at, galley.size() + vec2(12.0, 4.0));
                painter.rect_filled(pill, CornerRadius::same(8), Color32::from_black_alpha(170));
                painter.galley(pill.min + vec2(6.0, 2.0), galley, Color32::WHITE);
            }
        }

        // what is being drawn
        let outline = |erase: bool| Stroke::new(2.0, if erase { ERASE_OUTLINE } else { MASK_OUTLINE });
        let screen = |p: &Point| view.to_screen(origin, *p);
        match &self.gesture {
            Gesture::Rectangle { from, to, erase } => {
                let r = egui::Rect::from_two_pos(screen(from), screen(to));
                painter.rect_filled(r, CornerRadius::ZERO, MASK_FILL);
                painter.rect_stroke(r, CornerRadius::ZERO, outline(*erase), egui::StrokeKind::Inside);
            }
            Gesture::Stroke { points, erase } => {
                let width = (wizard.brush as f32 * view.zoom).max(1.0);
                let color = if *erase {
                    Color32::from_rgba_unmultiplied(40, 170, 90, 110)
                } else {
                    Color32::from_rgba_unmultiplied(220, 50, 50, 110)
                };
                for p in points {
                    painter.circle_filled(screen(p), width / 2.0, color);
                }
            }
            Gesture::Polygon { points, erase } => {
                let pts: Vec<Pos2> = points.iter().map(screen).collect();
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(4.5, HALO)));
                painter.add(egui::Shape::line(pts.clone(), outline(*erase)));
                for p in &pts {
                    painter.circle_filled(*p, 3.5, Color32::WHITE);
                    painter.circle_stroke(*p, 3.5, outline(*erase));
                }
                if let (Some(last), Some(pointer)) = (pts.last(), painter.ctx().pointer_hover_pos()) {
                    painter.line_segment([*last, pointer], Stroke::new(3.5, HALO));
                    painter.line_segment([*last, pointer], Stroke::new(1.0, MASK_OUTLINE));
                }
            }
            _ => {}
        }

        // the brush, as big as it paints
        if (wizard.tool, wizard.pen) == (Some(Tool::Exclude), Pen::Brush)
            && let Some(pointer) = painter.ctx().pointer_hover_pos().filter(|p| canvas.contains(*p))
        {
            let erase = painter.ctx().input(|i| i.modifiers.alt);
            let r = wizard.brush as f32 * view.zoom / 2.0;
            painter.circle_stroke(pointer, r, Stroke::new(4.5, HALO));
            painter.circle_stroke(pointer, r, outline(erase));
        }
    }
}
