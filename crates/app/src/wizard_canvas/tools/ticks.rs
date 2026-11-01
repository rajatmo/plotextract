//! Step 3: the lines found and the ticks chosen; the pointer lights a line, a click gives it to the selected tick.

use crate::wizard_canvas::*;

/// The tick that a click on `line` gives it to: the selected one if it is of the line's direction, else the first of that
/// direction still without a line, else the first.
pub(in crate::wizard_canvas) fn target_tick(wizard: &Wizard, line: &Line) -> Option<Slot> {
    let of_direction = || Slot::ALL.into_iter().filter(|&t| orientation_of(t) == line.orientation);
    match wizard.active() {
        MaskId::Tick(t) if orientation_of(t) == line.orientation => Some(t),
        _ => of_direction().find(|&t| wizard.tick(t).is_none()).or_else(|| of_direction().next()),
    }
}

/// The part of the image a line covers.
pub(in crate::wizard_canvas) fn line_rect(line: &Line) -> Rect {
    let half = f64::from(line.width.max(1.0)) / 2.0;
    let across = f64::from(line.across);
    let (from, to) = (f64::from(line.from), f64::from(line.to));
    match line.orientation {
        Orientation::Horizontal => Rect::new(from, across - half, to, across + half),
        Orientation::Vertical => Rect::new(across - half, from, across + half, to),
    }
}

/// The menu of a right click on the plot area step: which of the four ticks the line under the pointer is.
pub(in crate::wizard_canvas) fn line_menu(ui: &mut Ui, wizard: &Wizard, line: Option<Line>, intents: &mut Vec<Intent>) {
    let Some(line) = line else {
        ui.add_enabled(false, egui::Label::new("Right-click on a line of the image"));
        return;
    };
    let direction = if line.orientation == Orientation::Vertical { "vertical" } else { "horizontal" };
    ui.label(egui::RichText::new(format!("This {direction} line, {:.0} px long, is:", line.length())).strong());
    ui.separator();
    for tick in Slot::ALL.into_iter().filter(|&t| orientation_of(t) == line.orientation) {
        let current = wizard.tick(tick);
        let mut label = tick_name(tick).to_string();
        match current {
            Some(old) if old == line => {}
            Some(old) => label.push_str(&format!("   (replaces {} {:.1})", axis_letter(tick), old.across)),
            None => {}
        }
        if ui.selectable_label(current == Some(line), label).clicked() {
            intents.push(Intent::Wizard(WizardIntent::SetTick(tick, Some(line))));
            ui.close();
        }
    }
    if let Some(tick) = Slot::ALL.into_iter().find(|&t| wizard.tick(t) == Some(line)) {
        ui.separator();
        if ui.button(format!("Not {}", tick_name(tick))).clicked() {
            intents.push(Intent::Wizard(WizardIntent::SetTick(tick, None)));
            ui.close();
        }
    }
}

impl WizardCanvas {
    /// The plot area step: the pointer lights the straight lines; a click gives one to a tick mask.
    pub(in crate::wizard_canvas) fn interact_ticks(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        self.gesture = Gesture::Idle;
        // the nearest line to the pointer within reach; a short line wins over a long one it crosses
        let line_at = |pos: Pos2| -> Option<Line> {
            let p = view.to_figure(origin, pos);
            let reach = f64::from(GRAB_PX) / f64::from(view.zoom);
            wizard
                .lines()
                .iter()
                .map(|line| (f64::from(line.distance(p.x as f32, p.y as f32)), line))
                .filter(|(d, _)| *d <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.length().total_cmp(&b.1.length())))
                .map(|(_, line)| *line)
        };
        let moving = ui.input(|i| i.pointer.any_down());
        self.tick_hover = if response.hovered() && !moving { response.hover_pos().and_then(line_at) } else { None };
        if self.tick_hover.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if response.secondary_clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            self.menu_line = line_at(pos);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && response.clicked_by(PointerButton::Primary)
            && let Some(line) = line_at(pos)
            && let Some(tick) = target_tick(wizard, &line)
        {
            intents.push(Intent::Wizard(WizardIntent::SetTick(tick, Some(line))));
        }
        intents
    }

    /// The straight lines found, the ticks chosen, and the line under the pointer.
    pub(in crate::wizard_canvas) fn paint_lines(
        &self,
        painter: &Painter,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
        scene: &Scene,
    ) {
        // a line is a pixel or two thick: drawn at least a few screen pixels wide so that it can be seen
        let screen = |line: &Line| view.screen_rect(origin, line_rect(line)).expand(1.5);
        let halo = |r: egui::Rect, width: f32| {
            painter.rect_stroke(
                r.expand(1.0),
                CornerRadius::ZERO,
                Stroke::new(width + 1.5, HALO),
                egui::StrokeKind::Outside,
            );
        };
        if scene.lines {
            // the lines to choose from, lightly: there can be hundreds, and the image must stay readable under them;
            // one shorter on screen than a few pixels is too small to click and is left out
            let candidate = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 150, 0, 170));
            for line in wizard.lines() {
                let r = screen(line);
                if r.width().max(r.height()) >= 6.0 {
                    painter.rect_stroke(r, CornerRadius::ZERO, candidate, egui::StrokeKind::Outside);
                }
            }
        }
        for &tick in &scene.ticks {
            let Some(line) = wizard.tick(tick) else { continue };
            let colour = tick_tint(tick);
            let r = screen(&line);
            painter.rect_filled(r, CornerRadius::ZERO, colour.gamma_multiply(0.7));
            halo(r, 2.0);
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(2.0, colour), egui::StrokeKind::Outside);
            let galley = painter.layout_no_wrap(
                tick_name(tick).to_string(),
                egui::FontId::proportional(crate::theme::TEXT_SIZE),
                Color32::WHITE,
            );
            let size = galley.size();
            let at = if on_x_axis(tick) {
                r.center_bottom() + vec2(-size.x / 2.0, 6.0)
            } else {
                r.left_center() - vec2(size.x + 14.0, size.y / 2.0)
            };
            let pill = egui::Rect::from_min_size(at, size + vec2(10.0, 4.0));
            painter.rect_filled(pill, CornerRadius::same(8), Color32::from_black_alpha(190));
            painter.galley(pill.min + vec2(5.0, 2.0), galley, colour);
        }
        // outlined black and white, as the other masks are: the line under the pointer, the line of the selected tick, and
        // the line of the tick whose row the pointer is on in a panel
        let emphasise = |line: &Line| {
            let r = screen(line).expand(2.0);
            painter.rect_stroke(
                r.expand(1.5),
                CornerRadius::ZERO,
                Stroke::new(4.0, Color32::BLACK),
                egui::StrokeKind::Outside,
            );
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
        };
        if let Some(line) = self.tick_hover.as_ref().or(self.menu_line.as_ref()) {
            emphasise(line);
        }
        for &tick in &scene.ticks {
            let mask = MaskId::Tick(tick);
            if let Some(line) = wizard.tick(tick).filter(|_| wizard.active() == mask || self.panel_hover == Some(mask))
            {
                emphasise(&line);
            }
        }
    }
}
