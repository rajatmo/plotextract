//! Step 4, shapes: the pieces of the curve colours grouped by how alike they look, and the groups fixed by hand. That is
//! all the step does. A measure can see one drawing as two groups, or put in a group pieces that do not belong to it: the
//! user selects pieces (on the image, or a group's row here) and makes them one group, or takes them out of their groups.
//!
//! The groups are listed as the repeated shapes (many small pieces alike: dots, dashes, markers), the lines (long strokes)
//! and the pieces left over, in no group. Keys, while no text box has the focus: G makes the selection one group, Delete
//! takes it out of its groups, S adds the pieces like it, Z zooms to it, Esc lets go of it.

use std::collections::BTreeMap;

use egui::{Key, Modifiers, RichText, Ui};

use super::thumbs::shared_scale;
use super::{Output, WizardPanel};
use crate::panel::{FIELD_HEIGHT, GRID_GAP, cell};
use crate::theme::eye;
use crate::theme::{self, Btn};
use crate::wizard_canvas::{LEFTOVER, group_tint};
use plotextract_model::Intent;
use plotextract_usecases::wizard::{MaskId, Pick, Wizard, WizardIntent};

/// The widths of the columns of a group's row, in points.
const EYE: f32 = 22.0;
const THUMB: f32 = 24.0;
const NAME: f32 = 112.0;
const INFO: f32 = 64.0;

impl WizardPanel {
    pub(super) fn shapes_step(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        keys(ui, wizard, &mut out.intents);
        if !wizard.has_curves() {
            theme::card(ui, "Groups", |ui| {
                theme::hint(ui, "No colour is marked Curve (step 2).");
            });
            return;
        }
        selection_card(ui, wizard, &mut out.intents);
        self.groups_card(ui, wizard, out);
    }

    /// The groups found, each with a picture of its typical piece, in the colour it is drawn in; then the pieces left
    /// over; then how the pieces are found.
    fn groups_card(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        theme::card(ui, "Groups", |ui| {
            if wizard.pieces_busy() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Finding the pieces and their groups\u{2026}").weak());
                });
            }
            let Some(shapes) = wizard.shapes() else {
                if !wizard.pieces_busy() {
                    theme::hint(ui, "No groups yet.");
                }
                return;
            };
            theme::hint(ui, "Grey: in no group. Hold H to see the image.");
            ui.add_space(4.0);
            let order = wizard.shape_order();
            let scale = shared_scale(wizard, THUMB);
            let repeated: Vec<usize> = order.iter().copied().filter(|&k| !shapes.types[k].solid()).collect();
            let lines: Vec<usize> = order.iter().copied().filter(|&k| shapes.types[k].solid()).collect();
            let leftovers = shapes.leftovers();
            if order.is_empty() && leftovers.is_empty() {
                theme::hint(ui, "There are no pieces.");
            }
            let shift = ui.input(|i| i.modifiers.shift);
            let pick = if shift { Pick::Toggle } else { Pick::Replace };
            for (title, tip, rows) in [
                ("Repeated shapes", "Many pieces alike: dots, dashes, markers", repeated),
                ("Lines", "Long strokes", lines),
            ] {
                if rows.is_empty() {
                    continue;
                }
                ui.label(RichText::new(title).strong()).on_hover_text(tip);
                egui::Grid::new(title).num_columns(4).min_col_width(0.0).spacing([GRID_GAP - 2.0, 4.0]).show(
                    ui,
                    |ui| {
                        for &k in &rows {
                            let t = &shapes.types[k];
                            let mask = MaskId::Shape(k);
                            let shown = wizard.is_shown(mask);
                            cell(ui, EYE, |ui| {
                                let tip = if shown { "Hide this group on the image" } else { "Show it" };
                                if eye(ui, shown).on_hover_text(tip).clicked() {
                                    out.intents.push(Intent::Wizard(WizardIntent::Show(mask, !shown)));
                                }
                            });
                            let chosen = t.members.iter().filter(|p| wizard.selected().contains(p)).count();
                            let thumb = self.thumb(ui, wizard, Some(t.typical), scale, group_tint(k));
                            let name = ui.add_sized(
                                [NAME, FIELD_HEIGHT],
                                egui::Button::selectable(chosen == t.members.len(), wizard.group_name(k)),
                            );
                            let tip = "Click to select its pieces; Shift+click to add them to the selection or take \
                                       them out of it";
                            if name.hovered() || thumb.hovered() {
                                out.row_hover = Some(mask);
                            }
                            if name.on_hover_text(tip).clicked() || thumb.clicked() {
                                out.intents.push(Intent::Wizard(WizardIntent::SelectPieces(t.members.clone(), pick)));
                            }
                            cell(ui, INFO, |ui| count(ui, chosen, t.members.len(), t.spacing, t.look.length));
                            ui.end_row();
                        }
                    },
                );
                ui.add_space(4.0);
            }
            if !leftovers.is_empty() {
                ui.label(RichText::new("Left over").strong()).on_hover_text("Pieces in no group");
                egui::Grid::new("left over").num_columns(4).min_col_width(0.0).spacing([GRID_GAP - 2.0, 4.0]).show(
                    ui,
                    |ui| {
                        cell(ui, EYE, |_| {});
                        let thumb = self.thumb(ui, wizard, leftovers.first().copied(), scale, LEFTOVER);
                        let chosen = leftovers.iter().filter(|p| wizard.selected().contains(p)).count();
                        let name = ui
                            .add_sized(
                                [NAME, FIELD_HEIGHT],
                                egui::Button::selectable(chosen == leftovers.len(), "In no group"),
                            )
                            .on_hover_text(
                                "The pieces like too few others to be a group, and those taken out of their groups. \
                                 Click to select them all",
                            );
                        if name.clicked() || thumb.clicked() {
                            out.intents.push(Intent::Wizard(WizardIntent::SelectPieces(leftovers.clone(), pick)));
                        }
                        cell(ui, INFO, |ui| count(ui, chosen, leftovers.len(), None, 0.0));
                        ui.end_row();
                    },
                );
            }
            ui.add_space(4.0);
            egui::CollapsingHeader::new("Detection settings")
                .id_salt("shape detection")
                .show(ui, |ui| detection(ui, wizard, &mut out.intents));
        });
    }

    /// A picture of piece `piece` framed in `colour`.
    fn thumb(
        &mut self,
        ui: &mut Ui,
        wizard: &Wizard,
        piece: Option<usize>,
        scale: f32,
        colour: egui::Color32,
    ) -> egui::Response {
        let thumb = self.thumbs.show(ui, wizard, piece, THUMB, scale);
        ui.painter().rect_stroke(
            thumb.rect,
            egui::CornerRadius::same(3),
            egui::Stroke::new(2.0, colour),
            egui::StrokeKind::Inside,
        );
        thumb
    }
}

/// How many pieces a row has, and how many of them are selected if some are.
fn count(ui: &mut Ui, chosen: usize, n: usize, spacing: Option<f64>, length: f64) {
    let text = if chosen > 0 && chosen < n { format!("{chosen}/{n}") } else { format!("\u{d7}{n}") };
    let tip = match spacing {
        Some(s) => format!("{n} pieces about {length:.0} px long, about {s:.0} px apart"),
        None => format!("{n} piece{}", if n == 1 { "" } else { "s" }),
    };
    let tip = if chosen > 0 { format!("{tip}; {chosen} selected") } else { tip };
    ui.label(RichText::new(text).monospace().weak()).on_hover_text(tip);
}

/// What is selected, and what can be done with it.
fn selection_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Selection", |ui| {
        let selected = wizard.selected();
        let n = selected.len();
        if n == 0 {
            theme::hint(ui, "Click a piece: its group. Shift: add a group. Ctrl: one piece. Drag: draw around pieces.");
        } else {
            ui.label(RichText::new(format!("{n} piece{} selected", if n == 1 { "" } else { "s" })).strong());
            theme::hint(ui, &format!("from {}", sources(wizard)));
        }
        ui.add_space(4.0);
        let some = n > 0;
        let join = Btn::new("Make one group")
            .primary()
            .enabled(some)
            .tip("The selected pieces become one group, and nothing else is in it (G)")
            .why_not("Select pieces first");
        let remove = Btn::new("Remove")
            .enabled(some)
            .tip("Take the selected pieces out of their groups: they are left over (Delete)")
            .why_not("Select pieces first");
        match theme::buttons(ui, &[join, remove]) {
            Some(0) => intents.push(Intent::Wizard(WizardIntent::JoinSelected)),
            Some(1) => intents.push(Intent::Wizard(WizardIntent::RemoveSelected)),
            _ => {}
        }
        let similar = Btn::new("Select similar")
            .enabled(some)
            .tip("Add the pieces that look like one of those selected (S)")
            .why_not("Select pieces first");
        let zoom = Btn::new("Zoom to them")
            .enabled(some)
            .tip("Fit the view to the selection (Z)")
            .why_not("Select pieces first");
        match theme::buttons(ui, &[similar, zoom]) {
            Some(0) => intents.push(Intent::Wizard(WizardIntent::SelectSimilar)),
            Some(1) => intents.extend(selection_bounds(wizard).map(Intent::ShowArea)),
            _ => {}
        }
        let undo = Btn::new("Undo")
            .enabled(wizard.can_undo())
            .tip("Undo the last change to the groups (Ctrl+Z)")
            .why_not("Nothing to undo");
        let redo = Btn::new("Redo")
            .enabled(wizard.can_redo())
            .tip("Make the change undone again (Ctrl+Shift+Z)")
            .why_not("Nothing to redo");
        match theme::buttons(ui, &[undo, redo]) {
            Some(0) => intents.push(Intent::Undo),
            Some(1) => intents.push(Intent::Redo),
            _ => {}
        }
    });
}

/// The groups the selected pieces are in, by name, the most first: "Dots (12), Dashes (3), no group (1)".
fn sources(wizard: &Wizard) -> String {
    let Some(shapes) = wizard.shapes() else { return String::new() };
    let mut count: BTreeMap<Option<usize>, usize> = BTreeMap::new();
    for &p in wizard.selected() {
        *count.entry(shapes.type_of.get(p).copied().flatten()).or_default() += 1;
    }
    let mut by_size: Vec<(Option<usize>, usize)> = count.into_iter().collect();
    by_size.sort_by_key(|&(k, n)| (std::cmp::Reverse(n), k));
    let name = |k: Option<usize>| k.map_or_else(|| "no group".to_string(), |k| wizard.group_name(k));
    let mut parts: Vec<String> = by_size.iter().take(3).map(|&(k, n)| format!("{} ({n})", name(k))).collect();
    if by_size.len() > 3 {
        parts.push(format!("{} more", by_size.len() - 3));
    }
    parts.join(", ")
}

/// How the curve colours are broken into pieces, how alike pieces must be to be one group, and how many were found.
fn detection(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    let mut params = wizard.curve_params().clone();
    ui.label("Smallest piece");
    // the slider and its value together fill the card's width
    ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
    ui.add(egui::Slider::new(&mut params.min_pixels, 1..=200).logarithmic(true).suffix(" px"))
        .on_hover_text("Pieces with fewer pixels than this are specks, and are dropped");
    if params != *wizard.curve_params() {
        intents.push(Intent::Wizard(WizardIntent::SetCurveParams(params)));
    }
    let mut shape = wizard.shape_params().clone();
    ui.label("How alike pieces of one group must be");
    ui.add(egui::Slider::new(&mut shape.tolerance, 0.3..=2.0).fixed_decimals(2))
        .on_hover_text("Smaller is stricter: pieces must be more alike to be one group. 1 is the default");
    if shape != *wizard.shape_params() {
        intents.push(Intent::Wizard(WizardIntent::SetShapeParams(shape)));
    }
    if let (Some(pieces), Some(shapes)) = (wizard.pieces(), wizard.shapes()) {
        let (n, k) = (pieces.pieces().len(), wizard.shape_order().len());
        let left = shapes.leftovers().len();
        ui.label(format!(
            "{n} piece{}: {k} group{}, {left} in none",
            if n == 1 { "" } else { "s" },
            if k == 1 { "" } else { "s" }
        ));
    }
}

/// Where the selected pieces are on the image, together.
fn selection_bounds(wizard: &Wizard) -> Option<kurbo::Rect> {
    let pieces = wizard.pieces()?;
    wizard
        .selected()
        .iter()
        .filter_map(|&p| pieces.pieces().get(p))
        .map(|p| {
            let b = p.bounds;
            kurbo::Rect::new(f64::from(b.x), f64::from(b.y), f64::from(b.x + b.width), f64::from(b.y + b.height))
        })
        .reduce(|a, b| a.union(b))
        .map(|r| r.inflate(10.0, 10.0))
}

/// The keys of the shapes step, while no text box has the focus.
fn keys(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    if ui.ctx().egui_wants_keyboard_input() || wizard.selected().is_empty() {
        return;
    }
    let take = |key: Key| ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
    if take(Key::G) {
        intents.push(Intent::Wizard(WizardIntent::JoinSelected));
    }
    if take(Key::Delete) || take(Key::Backspace) {
        intents.push(Intent::Wizard(WizardIntent::RemoveSelected));
    }
    if take(Key::S) {
        intents.push(Intent::Wizard(WizardIntent::SelectSimilar));
    }
    if take(Key::Z) {
        intents.extend(selection_bounds(wizard).map(Intent::ShowArea));
    }
    if take(Key::Escape) {
        intents.push(Intent::Wizard(WizardIntent::SelectPieces(Vec::new(), Pick::Replace)));
    }
}
