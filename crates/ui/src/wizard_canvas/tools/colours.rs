//! Step 2: the pointer is a lens that shows where a colour is used; a click selects it, a right click says what it is.

use crate::wizard_canvas::*;

/// The menu of a right click on the colour step: what the selected colour is.
pub(in crate::wizard_canvas) fn colour_menu(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    let colours = wizard.colours();
    let (Some(map), Some(ink)) = (colours.map(), colours.selected()) else {
        ui.add_enabled(false, egui::Label::new("Right-click on a colour of the image"));
        return;
    };
    let rgb = map.inks[ink].rgb;
    ui.label(egui::RichText::new(format!("This colour (rgb {}, {}, {}) is:", rgb[0], rgb[1], rgb[2])).strong());
    ui.separator();
    for role in Role::ALL {
        let mut on = colours.has_role(ink, role);
        if ui.checkbox(&mut on, role.label()).on_hover_text(role.tip()).changed() {
            intents.push(Intent::Wizard(WizardIntent::SetRole(ink, role, on)));
        }
    }
    ui.separator();
    if ui
        .add_enabled(!colours.is_ignored(ink), egui::Button::new("Ignore"))
        .on_hover_text("Not part of the plot")
        .clicked()
    {
        intents.push(Intent::Wizard(WizardIntent::IgnoreColour(ink)));
        ui.close();
    }
}

impl WizardCanvas {
    /// The colour step: the pointer is a lens that shows where a colour is used; a click selects it.
    pub(in crate::wizard_canvas) fn interact_colours(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let mut intents = Vec::new();
        self.gesture = Gesture::Idle;
        let Some(map) = wizard.colours().map() else {
            self.hovered = None;
            return intents;
        };
        // a pointer beside a thin line is not exactly on it: look a few screen pixels around
        let ink_at = |pos: Pos2| {
            let p = view.to_figure(origin, pos);
            map.nearest_ink(p.x, p.y, (6.0 / f64::from(view.zoom)).clamp(1.0, 24.0))
        };
        let moving = ui.input(|i| i.pointer.any_down());
        self.hovered = if response.hovered() && !moving { response.hover_pos().and_then(ink_at) } else { None };
        if self.hovered.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }
        if let Some(pos) = response.interact_pointer_pos()
            && (response.clicked_by(PointerButton::Primary) || response.secondary_clicked())
        {
            intents.push(Intent::Wizard(WizardIntent::Select(ink_at(pos))));
        }
        intents
    }
}
