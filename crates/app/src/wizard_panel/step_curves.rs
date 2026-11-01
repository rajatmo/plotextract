//! Step 5, curves: the series made of the shapes. A series is proposed for each curve of dots or dashes, the solid lines and
//! each kind of marker of a curve colour. Choosing happens here and on the image: select a series, then click pieces to add
//! them to it or take them out, or, with Points, click a stray point to take it out and click where one was missed to put
//! it in. A series appears on the left once it has pieces, and is copied or saved here.

use egui::{RichText, Ui};
use plotextract_raster::curves::Trace;

use super::{Output, WizardPanel};
use crate::model::Intent;
use crate::panel::{FIELD_HEIGHT, GRID_GAP, cell};
use crate::text::{axis_status, series_error, series_name, trace_label, trace_tip};
use crate::theme::{self, Badge, Btn};
use crate::wizard::{MaskId, Wizard, WizardIntent};
use crate::wizard_canvas::series_tint;

/// The widths of the columns of a series' row, in points.
const NAME: f32 = 92.0;
const TRACE: f32 = 84.0;
const INFO: f32 = 60.0;

impl WizardPanel {
    pub(super) fn curves_step(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        edit_card(ui, wizard, &mut out.intents);
        series_card(ui, wizard, out);
    }
}

/// What a click on the image does: put pieces in a series or take them out, or the same with single points.
fn edit_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Edit", |ui| {
        if !wizard.has_curves() {
            theme::hint(ui, "No colour is marked Curve. Go back to step 2 and mark the colours of the curves.");
            return;
        }
        if wizard.pieces_busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Finding the pieces and their shapes\u{2026}").weak());
            });
        }
        ui.columns(2, |columns| {
            let tip = "A click puts a piece in the selected series, or takes it out";
            if theme::toggle(&mut columns[0], !wizard.edit_points(), "Pieces", tip) {
                intents.push(Intent::Wizard(WizardIntent::SetEditPoints(false)));
            }
            let tip = "A click on a point takes it out; a click where one is missing puts one in";
            if theme::toggle(&mut columns[1], wizard.edit_points(), "Points", tip) {
                intents.push(Intent::Wizard(WizardIntent::SetEditPoints(true)));
            }
        });
        theme::hint(
            ui,
            if wizard.edit_points() {
                "Click a stray point of the selected series to take it out, and where a point is missing to put one \
                 in (it goes to the middle of the ink there). Click a cross to put a point back."
            } else {
                "Move over the image to light a piece; click it to add it to the selected series, or take it out."
            },
        );
    });
}

/// The series: which one is selected, what it is made of, how it is traced; and the way to copy or save it.
fn series_card(ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
    let cal = wizard.calibration();
    let badge =
        Badge { on: cal.is_calibrated(), tip: format!("x: {}, y: {}", axis_status(&cal.x), axis_status(&cal.y)) };
    theme::card_with_badge(ui, "Series", Some(badge), |ui| {
        if wizard.series().is_empty() {
            theme::hint(ui, "No series yet: click a piece of a curve in the image, or make one.");
        }
        egui::Grid::new("series rows").num_columns(3).spacing([GRID_GAP, 6.0]).show(ui, |ui| {
            for series in wizard.series() {
                let mask = MaskId::Series(series.number());
                let name = ui.add_sized(
                    [NAME, FIELD_HEIGHT],
                    egui::Button::selectable(wizard.active() == mask, series_label(ui, series.number())),
                );
                if name.clicked() {
                    out.intents.push(Intent::Wizard(WizardIntent::SetActive(mask)));
                }
                if name.hovered() {
                    out.row_hover = Some(mask);
                }
                cell(ui, TRACE, |ui| {
                    let mut trace = series.trace();
                    egui::ComboBox::from_id_salt(("series trace", series.number()))
                        .selected_text(trace_label(trace))
                        .width(TRACE)
                        .show_ui(ui, |ui| {
                            for t in Trace::ALL {
                                ui.selectable_value(&mut trace, t, trace_label(t)).on_hover_text(trace_tip(t));
                            }
                        });
                    if trace != series.trace() {
                        out.intents.push(Intent::Wizard(WizardIntent::SetTrace(series.number(), trace)));
                    }
                });
                let (pieces, points) = (series.pieces().len(), series.points().len());
                let (added, removed) = (series.added().len(), series.removed().len());
                cell(ui, INFO, |ui| {
                    let mut tip = format!("{pieces} pieces, traced into {points} points");
                    if added + removed > 0 {
                        tip += &format!("; {added} put in and {removed} taken out by hand");
                    }
                    ui.label(RichText::new(format!("{points} pt")).monospace().weak()).on_hover_text(tip);
                });
                ui.end_row();
            }
        });
        ui.add_space(2.0);
        let new = Btn::new("New series").tip("Make an empty series and select it; then click its pieces in the image");
        if theme::buttons(ui, &[new]) == Some(0) {
            out.intents.push(Intent::Wizard(WizardIntent::NewSeries(None)));
        }

        ui.separator();
        let active = wizard.active_series();
        let why_not = match active.map(|n| wizard.series_rows(n)) {
            Some(Ok(_)) => String::new(),
            Some(Err(e)) => series_error(&e),
            None => "Select a series first.".into(),
        };
        let ready = why_not.is_empty();
        let label = active.map_or_else(|| "the selected series".to_string(), series_name);
        ui.label(RichText::new(format!("Write {label} in plot values")).strong());
        let copy = Btn::new("Copy").tip("Copy it as x,y text").enabled(ready).why_not(&why_not);
        let save = Btn::new("Save CSV\u{2026}").primary().tip("Save it as a CSV file").enabled(ready).why_not(&why_not);
        match theme::buttons(ui, &[copy, save]) {
            Some(0) => out.intents.push(Intent::CopyNumbers),
            Some(1) => out.intents.push(Intent::RequestExportCsv),
            _ => {}
        }
        if !ready {
            theme::hint(ui, &why_not);
        }
    });
}

/// A series' name with a dot in the colour it is drawn in.
fn series_label(ui: &Ui, number: usize) -> egui::text::LayoutJob {
    let font_id = egui::TextStyle::Button.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    let dot = egui::TextFormat { font_id: font_id.clone(), color: series_tint(number), ..Default::default() };
    job.append("\u{25cf} ", 0.0, dot);
    let text = egui::TextFormat { font_id, color: ui.visuals().text_color(), ..Default::default() };
    job.append(&series_name(number), 0.0, text);
    job
}
