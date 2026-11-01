//! The left panel: the masks that exist, one row each, the columns lined up: an eye to show or hide the mask on the image,
//! a picture of it, its name, a word about it, a bin that deletes it, and an arrow that opens what is known about it (its
//! ID, its tags and its properties). Clicking a row's picture or name selects the mask; the pointer on a row outlines the
//! mask on the image.

use std::collections::BTreeSet;

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, Ui, vec2};
use plotextract_raster::MaskMeta;
use plotextract_raster::shapes::Kind;

use super::{Output, WizardPanel};
use crate::model::Intent;
use crate::panel::{FIELD_HEIGHT, cell};
use crate::text::{tag_label, value_text};
use crate::theme::{self, ACCENT};
use crate::wizard::masks::{MaskView, Section, Swatch};
use crate::wizard::{MaskId, Wizard, WizardIntent, on_x_axis};
use crate::wizard_canvas::{series_tint, shape_tint, tick_tint, tint_of};

/// The widths of the columns, in points.
const EYE: f32 = 22.0;
const SWATCH: f32 = 22.0;
const NAME: f32 = 64.0;
const INFO: f32 = 58.0;
const DELETE: f32 = 18.0;
const OPEN: f32 = 16.0;
const GAP: f32 = 6.0;

impl WizardPanel {
    pub(super) fn masks_panel(&mut self, ui: &mut Ui, wizard: &Wizard, lit: Option<usize>) -> Output {
        let mut out = Output::new();
        // a section opened or closed by hand stays so until the step changes
        if self.sections_step != Some(wizard.step()) {
            self.sections_step = Some(wizard.step());
            self.sections.clear();
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let masks = wizard.masks();
            if masks.is_empty() {
                theme::card(ui, "Masks", |ui| theme::hint(ui, "The masks you make appear here, step by step."));
                return;
            }
            let rows: Vec<(MaskId, MaskMeta, MaskView)> =
                masks.into_iter().map(|(mask, meta)| (mask, meta, wizard.view(mask))).collect();
            for section in Section::ALL {
                let rows: Vec<&(MaskId, MaskMeta, MaskView)> =
                    rows.iter().filter(|(.., v)| v.section == section).collect();
                if rows.is_empty() {
                    continue;
                }
                let open = self.sections.get(&section).copied().unwrap_or(section.step() == wizard.step());
                let ids: Vec<MaskId> = rows.iter().map(|(m, ..)| *m).collect();
                section_card(ui, wizard, section, &ids, open, |ui, toggled| {
                    if toggled {
                        self.sections.insert(section, !open);
                    }
                    if !open {
                        return;
                    }
                    ui.spacing_mut().item_spacing = vec2(GAP, 2.0);
                    for entry in &rows {
                        row(ui, wizard, entry, lit, &mut self.expanded, &mut out);
                    }
                })
                .into_iter()
                .for_each(|intent| out.intents.push(intent));
            }
        });
        out
    }
}

/// A section of masks: a header (an arrow that opens or closes it, its title and how many masks it has, and an eye that
/// shows or hides them all) and, when open, a row for each. Returns what the eye asked for.
fn section_card(
    ui: &mut Ui,
    wizard: &Wizard,
    section: Section,
    masks: &[MaskId],
    open: bool,
    body: impl FnOnce(&mut Ui, bool),
) -> Vec<Intent> {
    let mut intents = Vec::new();
    let current = section.step() == wizard.step();
    let visuals = ui.visuals();
    let stroke = if current {
        egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.6))
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut toggled = false;
            ui.horizontal(|ui| {
                let any_shown = masks.iter().any(|&m| wizard.is_shown(m));
                cell(ui, EYE, |ui| {
                    let tip =
                        if any_shown { "Hide all of these on the image" } else { "Show all of these on the image" };
                    if eye(ui, any_shown).on_hover_text(tip).clicked() {
                        intents.extend(masks.iter().map(|&m| Intent::Wizard(WizardIntent::Show(m, !any_shown))));
                    }
                });
                let tip = if open { "Fold these away" } else { "Show these" };
                let arrow = RichText::new(if open { "\u{25be}" } else { "\u{25b8}" }).size(15.0);
                toggled |= ui.add(egui::Button::new(arrow).frame(false)).on_hover_text(tip).clicked();
                let title = RichText::new(section.title()).strong();
                let title = if current { title.color(ui.visuals().strong_text_color()) } else { title };
                toggled |= ui
                    .add(egui::Label::new(title).sense(Sense::click()).selectable(false))
                    .on_hover_text(tip)
                    .clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(masks.len().to_string()).monospace().weak());
                });
            });
            if open {
                ui.add_space(4.0);
            }
            body(ui, toggled);
        });
    ui.add_space(6.0);
    intents
}

/// One mask, and what is known about it if its row is open.
fn row(
    ui: &mut Ui,
    wizard: &Wizard,
    (mask, meta, view): &(MaskId, MaskMeta, MaskView),
    lit: Option<usize>,
    expanded: &mut BTreeSet<String>,
    out: &mut Output,
) {
    let mask = *mask;
    let active = wizard.active() == mask;
    let fill = if active {
        ACCENT.gamma_multiply(0.25)
    } else if matches!(mask, MaskId::Ink(i) if Some(i) == lit) {
        ACCENT.gamma_multiply(0.12)
    } else {
        Color32::TRANSPARENT
    };
    let open = expanded.contains(&meta.id);
    let frame = egui::Frame::new().fill(fill).corner_radius(CornerRadius::same(6)).inner_margin(3);
    let response = frame.show(ui, |ui| {
        ui.horizontal(|ui| {
            let shown = wizard.is_shown(mask);
            cell(ui, EYE, |ui| {
                let tip = if shown { "Hide this mask on the image" } else { "Show this mask on the image" };
                if eye(ui, shown).on_hover_text(tip).clicked() {
                    out.intents.push(Intent::Wizard(WizardIntent::Show(mask, !shown)));
                }
            });
            let mut chosen = false;
            cell(ui, SWATCH, |ui| {
                let (rect, picture) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                draw_swatch(ui, rect, wizard, view.swatch, shown);
                chosen |= picture.clicked();
            });
            cell(ui, NAME, |ui| {
                // what is hidden, and shapes that are no data, are named faintly
                let text = RichText::new(&view.name);
                let text = if active {
                    text.strong()
                } else if !shown || view.faint {
                    text.weak()
                } else {
                    text
                };
                let name = ui.add(egui::Label::new(text).sense(Sense::click()).selectable(false));
                chosen |= name.clicked();
            });
            cell(ui, INFO, |ui| {
                ui.label(RichText::new(&view.info).monospace().weak());
            });
            cell(ui, DELETE, |ui| {
                if bin(ui).on_hover_text(view.delete).clicked() {
                    out.intents.push(Intent::Wizard(WizardIntent::Delete(mask)));
                }
            });
            cell(ui, OPEN, |ui| {
                let arrow = egui::Button::new(RichText::new(if open { "\u{25be}" } else { "\u{25b8}" }).size(15.0))
                    .frame(false);
                let tip = if open { "Hide what is known about this mask" } else { "Show its ID, tags and properties" };
                if ui.add(arrow).on_hover_text(tip).clicked() {
                    if open {
                        expanded.remove(&meta.id);
                    } else {
                        expanded.insert(meta.id.clone());
                    }
                }
            });
            if chosen {
                out.intents.push(Intent::Wizard(WizardIntent::SetActive(mask)));
            }
        });
        if open {
            details(ui, meta);
        }
    });
    if ui.rect_contains_pointer(response.response.rect) {
        out.row_hover = Some(mask);
    }
}

/// The bin that deletes a mask: a lid over a can with three ribs.
fn bin(ui: &mut Ui) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(DELETE, FIELD_HEIGHT.min(DELETE)), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let colour = if response.hovered() { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
    let stroke = Stroke::new(1.2, colour);
    let c = rect.center();
    painter.line_segment([c + vec2(-5.0, -4.0), c + vec2(5.0, -4.0)], stroke);
    painter.line_segment([c + vec2(-1.5, -5.5), c + vec2(1.5, -5.5)], stroke);
    let can = [c + vec2(-4.0, -4.0), c + vec2(-3.2, 5.0), c + vec2(3.2, 5.0), c + vec2(4.0, -4.0)];
    painter.add(egui::Shape::line(can.to_vec(), stroke));
    for dx in [-1.6, 0.0, 1.6] {
        painter.line_segment([c + vec2(dx, -2.0), c + vec2(dx, 3.0)], stroke);
    }
    response
}

/// A mask's ID, its tags and its properties, under its name.
fn details(ui: &mut Ui, meta: &MaskMeta) {
    let indent = EYE + SWATCH + 2.0 * GAP;
    ui.horizontal(|ui| {
        ui.add_space(indent);
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("ID").weak());
                ui.label(RichText::new(&meta.id).monospace());
            });
            ui.horizontal_wrapped(|ui| {
                for tag in &meta.tags {
                    chip(ui, tag_label(*tag));
                }
            });
            if !meta.properties.is_empty() {
                egui::Grid::new(("mask properties", &meta.id)).num_columns(2).spacing([GAP * 2.0, 2.0]).show(
                    ui,
                    |ui| {
                        for (key, value) in meta.properties.iter() {
                            ui.label(RichText::new(key).weak());
                            ui.label(RichText::new(value_text(value)).monospace());
                            ui.end_row();
                        }
                    },
                );
            }
            ui.add_space(2.0);
        });
    });
}

/// A tag, as a small rounded label.
fn chip(ui: &mut Ui, text: &str) {
    egui::Frame::new()
        .fill(ui.visuals().widgets.inactive.bg_fill)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| ui.label(RichText::new(text).small()));
}

/// The eye that shows or hides a mask: open with a pupil when shown, struck through when hidden.
fn eye(ui: &mut Ui, shown: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(EYE, FIELD_HEIGHT.min(EYE)), Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let colour = if shown { ui.visuals().strong_text_color() } else { ui.visuals().weak_text_color() };
    let stroke = Stroke::new(1.4, colour);
    let c = rect.center();
    let (half_width, half_height) = (8.0, 4.5);
    // the outline of the eye: two arcs that meet at the corners
    let steps = 12;
    let arc = |sign: f32| -> Vec<egui::Pos2> {
        (0..=steps)
            .map(|k| {
                let t = k as f32 / steps as f32;
                let x = c.x - half_width + 2.0 * half_width * t;
                let y = c.y + sign * half_height * (std::f32::consts::PI * t).sin();
                egui::pos2(x, y)
            })
            .collect()
    };
    painter.add(egui::Shape::line(arc(-1.0), stroke));
    painter.add(egui::Shape::line(arc(1.0), stroke));
    if shown {
        painter.circle_filled(c, 2.6, colour);
    } else {
        painter.circle_stroke(c, 2.2, stroke);
        painter.line_segment([c + vec2(-7.0, -6.0), c + vec2(7.0, 6.0)], stroke);
    }
    response
}

/// A picture of a mask, in the colours it is drawn with on the image.
fn draw_swatch(ui: &Ui, rect: egui::Rect, wizard: &Wizard, swatch: Swatch, shown: bool) {
    let painter = ui.painter();
    match swatch {
        Swatch::Crop => {
            painter.rect_stroke(
                rect.shrink(3.0),
                CornerRadius::same(2),
                Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        Swatch::Exclude => {
            painter.rect_filled(rect.shrink(2.0), CornerRadius::same(4), Color32::from_rgb(200, 60, 60));
        }
        Swatch::Tick(t) => {
            // a short line across a longer one
            let c = rect.center();
            let (along, across) =
                if on_x_axis(t) { (vec2(0.0, 7.0), vec2(8.0, 0.0)) } else { (vec2(7.0, 0.0), vec2(0.0, 8.0)) };
            painter.line_segment([c - across, c + across], Stroke::new(1.5, Color32::from_gray(128)));
            painter.line_segment([c - along, c + along], Stroke::new(3.0, tick_tint(t)));
        }
        Swatch::Ink { ink: i, rgb } => {
            painter.rect_filled(rect.shrink(2.0), CornerRadius::same(4), Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
            // the colour the mask is tinted with on the image, once it is shown
            let outline = if shown { Stroke::new(2.0, tint_of(i)) } else { Stroke::new(1.0, Color32::from_gray(128)) };
            painter.rect_stroke(rect.shrink(1.0), CornerRadius::same(5), outline, egui::StrokeKind::Inside);
        }
        Swatch::Series(number) => {
            // a curve that goes up and down
            let c = rect.center();
            let points = [(-8.0, 4.0), (-4.0, -2.0), (0.0, 1.0), (4.0, -5.0), (8.0, -1.0)];
            let line: Vec<egui::Pos2> = points.iter().map(|&(x, y)| c + vec2(x, y)).collect();
            painter.add(egui::Shape::line(line, Stroke::new(2.5, series_tint(number))));
        }
        Swatch::Shape { k, kind } => {
            draw_kind(painter, rect, kind, shape_tint(wizard, k));
        }
    }
}

/// A small picture of a kind of shape, in `colour`.
fn draw_kind(painter: &egui::Painter, rect: egui::Rect, kind: Kind, colour: Color32) {
    let c = rect.center();
    match kind {
        Kind::Dot => {
            for dx in [-6.0, 0.0, 6.0] {
                painter.rect_filled(egui::Rect::from_center_size(c + vec2(dx, 0.0), vec2(3.5, 3.5)), 0.0, colour);
            }
        }
        Kind::Dash => {
            for dx in [-5.5, 5.5] {
                painter.line_segment([c + vec2(dx - 3.5, 0.0), c + vec2(dx + 3.5, 0.0)], Stroke::new(3.0, colour));
            }
        }
        Kind::Marker => {
            painter.circle_stroke(c, 5.0, Stroke::new(2.0, colour));
        }
        Kind::Line => {
            painter.line_segment([c + vec2(-8.0, 3.0), c + vec2(8.0, -3.0)], Stroke::new(2.5, colour));
        }
        Kind::Text | Kind::Other => {
            let text = if kind == Kind::Text { "Aa" } else { "?" };
            painter.text(c, egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(12.0), colour);
        }
    }
}
