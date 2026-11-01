//! Step 3, plot area: the search for straight lines, and the four ticks. Choosing happens here, on the right: select a tick,
//! click its line in the image, type its plot value. Each tick's mask appears on the left once it has a line.

use egui::{RichText, Ui};
use plotextract_algo::lines::Algorithm;
use plotextract_domain::{Axis, Slot, format_g, parse_value};

use super::{Output, WizardPanel};
use crate::panel::{FIELD_HEIGHT, GRID_GAP, POSITION_COL, cell, value_field};
use crate::theme::{self, Badge};
use crate::wizard_canvas::tick_tint;
use plotextract_model::Intent;
use plotextract_model::text::{axis_status, slot_label};
use plotextract_usecases::wizard::{MaskId, Wizard, WizardIntent};

/// The width of the column of tick names: a dot and "x min".
const TICK_NAME: f32 = 72.0;

impl WizardPanel {
    pub(super) fn plot_area_step(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        self.ticks_card(ui, wizard, out);
        lines_card(ui, wizard, &mut out.intents);
    }

    /// The four ticks: which one is being chosen, where it is, and its plot value; and whether the axes are logarithmic.
    fn ticks_card(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        let cal = wizard.calibration();
        let badge =
            Badge { on: cal.is_calibrated(), tip: format!("x: {}, y: {}", axis_status(&cal.x), axis_status(&cal.y)) };
        theme::card_with_badge(ui, "Ticks", Some(badge), |ui| {
            theme::hint(ui, "Select a tick, click its line, type its value.");
            ui.add_space(2.0);
            let value_width = (ui.available_width() - TICK_NAME - POSITION_COL - 2.0 * GRID_GAP).max(60.0);
            let axis_ok = [cal.x.map().is_ok(), cal.y.map().is_ok()];
            egui::Grid::new("tick values").num_columns(3).spacing([GRID_GAP, 6.0]).show(ui, |ui| {
                for (i, slot) in Slot::ALL.into_iter().enumerate() {
                    // the text stays as typed while it means what the wizard has; after an undo or a new image it shows that
                    if parse_value(&self.values[i]) != wizard.tick_value(slot) {
                        self.values[i] = wizard.tick_value(slot).map(|v| format_g(v, 10)).unwrap_or_default();
                    }
                    let mask = MaskId::Tick(slot);
                    let name = ui.add_sized(
                        [TICK_NAME, FIELD_HEIGHT],
                        egui::Button::selectable(wizard.active() == mask, tick_label(ui, slot)),
                    );
                    if name.clicked() {
                        out.intents.push(Intent::Wizard(WizardIntent::SetActive(mask)));
                    }
                    if name.hovered() {
                        out.row_hover = Some(mask);
                    }
                    cell(ui, POSITION_COL, |ui| match wizard.tick_position(slot) {
                        Some(p) => {
                            ui.label(RichText::new(format!("{p:.1}")).monospace());
                        }
                        None => {
                            ui.label(RichText::new("\u{2013}").weak())
                                .on_hover_text("Click this tick's line in the image");
                        }
                    });
                    if value_field(ui, &mut self.values[i], value_width, axis_ok[usize::from(slot.axis() == Axis::Y)]) {
                        out.intents
                            .push(Intent::Wizard(WizardIntent::SetTickValue(slot, parse_value(&self.values[i]))));
                    }
                    ui.end_row();
                }
            });
            ui.add_space(4.0);
            let (mut xlog, mut ylog) = (wizard.is_log(Axis::X), wizard.is_log(Axis::Y));
            ui.columns(2, |columns| {
                columns[0].checkbox(&mut xlog, "Log x scale");
                columns[1].checkbox(&mut ylog, "Log y scale");
            });
            for (axis, was, now) in [(Axis::X, wizard.is_log(Axis::X), xlog), (Axis::Y, wizard.is_log(Axis::Y), ylog)] {
                if was != now {
                    out.intents.push(Intent::Wizard(WizardIntent::SetLog(axis, now)));
                }
            }
            ui.add_space(2.0);
            if !cal.is_calibrated() {
                theme::hint(ui, &format!("x: {}. y: {}.", axis_status(&cal.x), axis_status(&cal.y)));
            }
        });
    }
}

/// A tick's name with a dot in the colour its line is drawn in.
fn tick_label(ui: &Ui, slot: Slot) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    let dot = egui::TextFormat { font_id: font_id.clone(), color: tick_tint(slot), ..Default::default() };
    job.append("\u{25cf} ", 0.0, dot);
    let text = egui::TextFormat { font_id, color: ui.visuals().text_color(), ..Default::default() };
    job.append(slot_label(slot), 0.0, text);
    job
}

/// How the straight lines are looked for, and what was found.
fn lines_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Line detection", |ui| {
        if !wizard.has_axes() {
            theme::hint(ui, "No colour is marked Axes (step 2).");
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Look with");
            let mut algorithm = wizard.algorithm();
            egui::ComboBox::from_id_salt("line algorithm")
                .selected_text(algorithm.label())
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for a in Algorithm::ALL {
                        ui.selectable_value(&mut algorithm, a, a.label()).on_hover_text(a.tip());
                    }
                });
            if algorithm != wizard.algorithm() {
                intents.push(Intent::Wizard(WizardIntent::SetAlgorithm(algorithm)));
            }
        });
        ui.add_space(2.0);
        let mut params = wizard.line_params().clone();
        let slider = |ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, tip: &str| {
            ui.label(label);
            // the slider and its value together fill the card's width
            ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
            ui.add(egui::Slider::new(value, range).step_by(1.0).max_decimals(0).suffix(" px")).on_hover_text(tip);
        };
        slider(
            ui,
            "Shortest line",
            &mut params.min_length,
            2.0..=40.0,
            "Lines shorter than this are dropped. Lower it to find short ticks (and more strokes of letters)",
        );
        slider(
            ui,
            "Widest line",
            &mut params.max_width,
            1.0..=20.0,
            "Anything wider than this is a block, not a line, and is taken out first",
        );
        if wizard.algorithm() == Algorithm::Lsd {
            let mut enlarge = params.enlarge as f32;
            ui.label("Enlarge");
            ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
            let tip = "Make the mask this many times larger before looking: a short line then has enough pixels to be told from chance";
            ui.add(egui::Slider::new(&mut enlarge, 1.0..=4.0).step_by(1.0).max_decimals(0).suffix(" \u{d7}"))
                .on_hover_text(tip);
            params.enlarge = enlarge as u32;
        }
        if params != *wizard.line_params() {
            intents.push(Intent::Wizard(WizardIntent::SetLineParams(params)));
        }
        ui.add_space(2.0);
        if wizard.lines_busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Looking for lines\u{2026}").weak());
            });
        } else {
            ui.label(format!("{} lines found", wizard.lines().len()));
        }
    });
}
