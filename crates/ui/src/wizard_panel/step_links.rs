//! Step 5, links: the group whose pieces are linked, the links in the order their points are written (each can be moved
//! up or down, turned round or deleted), and the copying and saving of the points. Keys, while no text box has the
//! focus: Esc or Enter ends the link being made, Backspace takes its last piece off.

use egui::{Key, Modifiers, RichText, Ui};

use super::masks::bin;
use super::{Output, WizardPanel};
use crate::theme::{self, Btn};
use crate::wizard_canvas::{group_tint, link_tint};
use plotextract_model::Intent;
use plotextract_model::text;
use plotextract_usecases::wizard::{Wizard, WizardIntent};

impl WizardPanel {
    pub(super) fn links_step(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        keys(ui, wizard, &mut out.intents);
        if wizard.shapes().is_none() {
            theme::card(ui, "Links", |ui| {
                if wizard.pieces_busy() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Finding the pieces\u{2026}").weak());
                    });
                } else {
                    theme::hint(ui, "No pieces: mark the curve colours on step 2.");
                }
            });
            return;
        }
        group_card(ui, wizard, &mut out.intents);
        links_card(ui, wizard, &mut out.intents);
        export_card(ui, wizard, &mut out.intents);
    }
}

/// Which group's pieces are shown to be linked.
fn group_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Group", |ui| {
        let Some(shapes) = wizard.shapes() else { return };
        let now = wizard.link_group();
        let name = |k: Option<usize>| match k {
            Some(k) => format!("{} (\u{d7}{})", wizard.group_name(k), shapes.types[k].members.len()),
            None => "Every piece".into(),
        };
        ui.horizontal(|ui| {
            if let Some(k) = now {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 5.0, group_tint(k));
            }
            egui::ComboBox::from_id_salt("link group")
                .selected_text(name(now))
                .width((ui.available_width() - 8.0).min(220.0))
                .show_ui(ui, |ui| {
                    for k in wizard.shape_order().into_iter().map(Some).chain([None]) {
                        if ui.selectable_label(now == k, name(k)).clicked() {
                            intents.push(Intent::Wizard(WizardIntent::SetLinkGroup(k)));
                        }
                    }
                });
        });
        let linkable = (0..shapes.looks.len()).filter(|&p| wizard.linkable(p)).count();
        let linked = wizard.links().iter().flatten().filter(|&&p| wizard.linkable(p)).count();
        theme::hint(ui, &format!("{linked} of {linkable} linked"));
    });
}

/// The links, in the order their points are written.
fn links_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Links", |ui| {
        let links = wizard.links();
        let open = wizard.open_link();
        if links.is_empty() {
            theme::hint(ui, "Click pieces in order (or drag through them). Double-click or Esc ends a link.");
        } else {
            theme::hint(ui, "Exported in this order.");
        }
        let n = links.len();
        // two lines a link, so that the panel keeps its width: what it is, then what can be done with it
        for (i, link) in links.iter().enumerate() {
            let fill = if open == Some(i) { theme::ACCENT.gamma_multiply(0.18) } else { egui::Color32::TRANSPARENT };
            egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius::same(6)).inner_margin(4).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 8.0, link_tint(i));
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        (i + 1).to_string(),
                        egui::FontId::proportional(11.0),
                        egui::Color32::BLACK,
                    );
                    let pieces = format!("{} piece{}", link.len(), if link.len() == 1 { "" } else { "s" });
                    if open == Some(i) {
                        ui.label(RichText::new(format!("{pieces}, being made")).strong());
                    } else {
                        ui.label(pieces);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if bin(ui).on_hover_text("Delete this link").clicked() {
                            intents.push(Intent::Wizard(WizardIntent::DeleteLink(i)));
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    let up = ui.add_enabled(i > 0, egui::Button::new("Up").small()).on_hover_text("Write it earlier");
                    if up.clicked() {
                        intents.push(Intent::Wizard(WizardIntent::MoveLink(i, true)));
                    }
                    let down =
                        ui.add_enabled(i + 1 < n, egui::Button::new("Down").small()).on_hover_text("Write it later");
                    if down.clicked() {
                        intents.push(Intent::Wizard(WizardIntent::MoveLink(i, false)));
                    }
                    if ui.small_button("Turn round").on_hover_text("Its last piece first").clicked() {
                        intents.push(Intent::Wizard(WizardIntent::ReverseLink(i)));
                    }
                });
            });
        }
        ui.add_space(4.0);
        let end = Btn::new("End link")
            .enabled(open.is_some())
            .tip("End the link being made: the next click starts another (Esc, or double-click)")
            .why_not("No link is being made");
        let undo = Btn::new("Undo").enabled(wizard.can_undo()).tip("Undo the last change to the links (Ctrl+Z)");
        let redo = Btn::new("Redo").enabled(wizard.can_redo()).tip("Make the change undone again (Ctrl+Shift+Z)");
        match theme::buttons(ui, &[end, undo, redo]) {
            Some(0) => intents.push(Intent::Wizard(WizardIntent::EndLink)),
            Some(1) => intents.push(Intent::Undo),
            Some(2) => intents.push(Intent::Redo),
            _ => {}
        }
    });
}

/// How many points the links make, and the way to copy or save them.
fn export_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Points", |ui| {
        let points = wizard.link_points().len();
        let why = match wizard.link_rows() {
            Ok(_) => None,
            Err(e) => Some(text::links_error(&e)),
        };
        match &why {
            None => theme::hint(ui, &format!("{points} point{}, in plot units", if points == 1 { "" } else { "s" })),
            Some(why) => theme::hint(ui, why),
        }
        let why_not = why.as_deref().unwrap_or("");
        let copy = Btn::new("Copy").enabled(why.is_none()).tip("Copy the points as x,y text").why_not(why_not);
        let save =
            Btn::new("Save CSV").primary().enabled(why.is_none()).tip("Save the points as a CSV file").why_not(why_not);
        match theme::buttons(ui, &[copy, save]) {
            Some(0) => intents.push(Intent::CopyNumbers),
            Some(1) => intents.push(Intent::RequestExportCsv),
            _ => {}
        }
    });
}

/// The keys of the links step, while no text box has the focus.
fn keys(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    if ui.ctx().egui_wants_keyboard_input() || wizard.open_link().is_none() {
        return;
    }
    let take = |key: Key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if take(Key::Escape) || take(Key::Enter) {
        intents.push(Intent::Wizard(WizardIntent::EndLink));
    }
    if take(Key::Backspace) || take(Key::Delete) {
        intents.push(Intent::Wizard(WizardIntent::LinkBack));
    }
}
