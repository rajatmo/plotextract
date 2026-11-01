//! Step 2, colours: the colours the plot is drawn in, and what each one is: any of the roles (curve, fill, axes, grid),
//! several at once if need be, or none to ignore it. Choosing happens here, on the right; the masks of the colours that are
//! not ignored appear on the left.

use egui::{Color32, CornerRadius, RichText, Sense, Ui, vec2};

use super::{Output, WizardPanel};
use crate::theme::{self, ACCENT};
use plotextract_model::Intent;
use plotextract_usecases::wizard::{MaskId, Role, Wizard, WizardIntent};

/// The widths of the columns of a colour's row, in points.
const SWATCH: f32 = 22.0;
const NAME: f32 = 62.0;
const SHARE: f32 = 44.0;

impl WizardPanel {
    pub(super) fn colours_step(&mut self, ui: &mut Ui, wizard: &Wizard, lit: Option<usize>, out: &mut Output) {
        colours_card(ui, wizard, lit, out);
        self.tolerance_card(ui, wizard, &mut out.intents);
    }

    fn tolerance_card(&mut self, ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
        theme::card(ui, "Telling colours apart", |ui| {
            let mut tolerance = self.tolerance.unwrap_or(wizard.colours().tolerance);
            ui.label("Merge closer than");
            // the slider and its value together fill the card's width
            ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
            let slider = egui::Slider::new(&mut tolerance, 2.0..=40.0).suffix(" \u{394}E");
            let response = ui.add(slider).on_hover_text(
                "Colours less different than this are one ink. Lower to separate similar colours, raise to merge them",
            );
            if response.changed() {
                self.tolerance = Some(tolerance);
            }
            if response.drag_stopped() || (response.changed() && !response.dragged()) {
                self.tolerance = None;
                intents.push(Intent::Wizard(WizardIntent::SetTolerance(tolerance)));
            }
        });
    }
}

/// Every colour found, with a way to say what it is.
fn colours_card(ui: &mut Ui, wizard: &Wizard, lit: Option<usize>, out: &mut Output) {
    theme::card(ui, "Colours", |ui| {
        let Some(map) = wizard.colours().map() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Looking at the colours\u{2026}").weak());
            });
            return;
        };
        if map.inks.is_empty() {
            theme::hint(ui, "No colours found.");
            return;
        }
        theme::hint(ui, "Unmarked colours are ignored. Hover the image to find a colour.");
        ui.add_space(2.0);
        ui.spacing_mut().item_spacing.x = 6.0;
        for ink in 0..map.inks.len() {
            colour_row(ui, wizard, ink, lit, out);
        }
    });
}

/// One colour: a swatch, its name, how much of the image it is, and what it is.
fn colour_row(ui: &mut Ui, wizard: &Wizard, ink: usize, lit: Option<usize>, out: &mut Output) {
    let colours = wizard.colours();
    let fill = if colours.selected() == Some(ink) {
        ACCENT.gamma_multiply(0.25)
    } else if lit == Some(ink) {
        ACCENT.gamma_multiply(0.12)
    } else {
        Color32::TRANSPARENT
    };
    let frame = egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(6)).inner_margin(3);
    let response = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let rgb = colours.map().map_or([0; 3], |m| m.inks[ink].rgb);
            let (rect, swatch) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
            ui.painter().rect_filled(
                rect.shrink(2.0),
                CornerRadius::same(4),
                Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
            );
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                CornerRadius::same(5),
                egui::Stroke::new(1.0, Color32::from_gray(128)),
                egui::StrokeKind::Inside,
            );
            let name =
                ui.add_sized([NAME, SWATCH], egui::Label::new(format!("Colour {}", ink + 1)).sense(Sense::click()));
            let share = colours.map().map_or(0.0, |m| {
                100.0 * m.inks[ink].pixels as f64 / (f64::from(m.crop.width) * f64::from(m.crop.height)).max(1.0)
            });
            ui.add_sized([SHARE, SWATCH], egui::Label::new(RichText::new(format!("{share:.1}%")).monospace().weak()));
            if swatch.clicked() || name.clicked() {
                out.intents.push(Intent::Wizard(WizardIntent::Select(Some(ink))));
            }
            if colours.is_ignored(ink) {
                ui.label(RichText::new("ignored").weak().italics());
            }
        });
        // what it is: as many of the roles as apply, none to ignore it
        ui.columns(Role::ALL.len(), |columns| {
            for (column, role) in columns.iter_mut().zip(Role::ALL) {
                let on = colours.has_role(ink, role);
                if theme::toggle(column, on, role.label(), role.tip()) {
                    out.intents.push(Intent::Wizard(WizardIntent::SetRole(ink, role, !on)));
                }
            }
        });
    });
    if ui.rect_contains_pointer(response.response.rect) {
        out.row_hover = Some(MaskId::Ink(ink));
    }
}
