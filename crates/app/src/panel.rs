//! The side panel: layers, calibration, series, the curve table and the selection.
//!
//! The panel is a fixed width, so everything here is laid out against it: rows of buttons share the width equally,
//! input boxes fill what is left of their row, and table columns line up with their headers.

use egui::{Align, Layout, RichText, Stroke, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use plotextract_core::{Axis, Command, Layer, Pick, PrimitiveKind, Slot, format_g, parse_value};

use crate::app::{Action, App, curve_label, kind_name};
use crate::theme::{self, Badge, Btn, FILL_COLOR, MEMBER_COLOR, OK, STROKE_COLOR};

const TABLE_ROW: f32 = 22.0;
/// The first column of the grids (row names), in points.
const NAME_COL: f32 = 54.0;
/// The position column of the calibration grid.
const POSITION_COL: f32 = 70.0;
const GRID_GAP: f32 = 8.0;
/// The first column of the selection grid.
const SELECTION_NAME_COL: f32 = 56.0;
const FIELD_HEIGHT: f32 = 24.0;

impl App {
    pub(crate) fn side_panel(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        if self.session.is_none() {
            ui.add_space(12.0);
            ui.label(RichText::new("No plot open").weak());
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            self.layers_row(ui);
            ui.add_space(4.0);
            let cal = self.session.as_ref().expect("a figure is open").calibration();
            let badge = Badge { on: cal.is_calibrated(), tip: format!("x: {}, y: {}", cal.x.status(), cal.y.status()) };
            theme::card_with_badge(ui, "Calibration", Some(badge), |ui| self.calibration_section(ui, actions));
            theme::card(ui, "Series", |ui| self.series_section(ui, actions));
            theme::card(ui, "Curves", |ui| self.curve_table(ui));
            theme::card(ui, "Selection", |ui| self.selection_section(ui));
        });
    }

    fn layers_row(&mut self, ui: &mut Ui) {
        let session = self.session.as_ref().expect("a figure is open");
        let layers = session.layers();
        let mut want = layers;
        ui.columns(3, |columns| {
            if theme::toggle(&mut columns[0], layers.strokes, "Strokes", "Show or hide every stroked path") {
                want.strokes = !want.strokes;
            }
            if theme::toggle(&mut columns[1], layers.fills, "Fills", "Show or hide every filled path") {
                want.fills = !want.fills;
            }
            if theme::toggle(&mut columns[2], layers.page, "Picture", "Show or hide the picture behind the curves") {
                want.page = !want.page;
            }
        });
        for (layer, old, new) in [
            (Layer::Strokes, layers.strokes, want.strokes),
            (Layer::Fills, layers.fills, want.fills),
            (Layer::Page, layers.page, want.page),
        ] {
            if old != new {
                self.apply(Command::SetLayer(layer, new));
            }
        }
    }

    fn calibration_section(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        theme::hint(ui, "Ctrl+click a tick's end point, record it in a row, then type its value.");
        ui.add_space(2.0);
        let mut commands = Vec::new();
        let session = self.session.as_ref().expect("a figure is open");
        // the value boxes take whatever the other two columns leave
        let value_width = (ui.available_width() - NAME_COL - POSITION_COL - 2.0 * GRID_GAP).max(80.0);
        // a calibrated axis outlines its two value boxes in green
        let axis_ok = [session.calibration().x.map().is_ok(), session.calibration().y.map().is_ok()];
        egui::Grid::new("calibration").num_columns(3).spacing([GRID_GAP, 6.0]).show(ui, |ui| {
            cell(ui, NAME_COL, |ui| {
                ui.label("");
            });
            cell(ui, POSITION_COL, |ui| {
                ui.label(RichText::new("Position").weak());
            });
            ui.label(RichText::new("Value").weak());
            ui.end_row();
            for (i, slot) in Slot::ALL.into_iter().enumerate() {
                let row = egui::Button::selectable(self.slot == slot, slot.label());
                if ui.add_sized([NAME_COL, FIELD_HEIGHT], row).clicked() {
                    self.slot = slot;
                }
                cell(ui, POSITION_COL, |ui| match session.calibration().position(slot) {
                    Some(p) => {
                        ui.label(RichText::new(format!("{p:.3}")).monospace());
                    }
                    None => {
                        ui.label(RichText::new("\u{2013}").weak());
                    }
                });
                let field =
                    egui::TextEdit::singleline(&mut self.values[i]).hint_text("value").align(egui::Align2::LEFT_CENTER);
                let changed = ui
                    .scope(|ui| {
                        if axis_ok[i / 2] {
                            let widgets = &mut ui.visuals_mut().widgets;
                            for state in [&mut widgets.inactive, &mut widgets.hovered] {
                                state.bg_stroke = Stroke::new(1.5, OK);
                            }
                        }
                        ui.add_sized([value_width, FIELD_HEIGHT], field).changed()
                    })
                    .inner;
                if changed {
                    commands.push(Command::SetValue(slot, parse_value(&self.values[i])));
                }
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        let cal = session.calibration();
        let (mut xlog, mut ylog) = (cal.x.log, cal.y.log);
        ui.columns(2, |columns| {
            columns[0].checkbox(&mut xlog, "Log x scale");
            columns[1].checkbox(&mut ylog, "Log y scale");
        });
        let record = Btn::new("Record").primary().tip("Record the selected point in the highlighted row (Ctrl+R)");
        let clear = Btn::new("Clear").tip("Forget all four ticks");
        match theme::buttons(ui, &[record, clear]) {
            Some(0) => actions.push(Action::Record),
            Some(1) => commands.push(Command::ClearCalibration),
            _ => {}
        }
        if xlog != cal.x.log {
            commands.push(Command::SetLog(Axis::X, xlog));
        }
        if ylog != cal.y.log {
            commands.push(Command::SetLog(Axis::Y, ylog));
        }
        for c in commands {
            let clear = matches!(c, Command::ClearCalibration);
            self.apply(c);
            if clear {
                self.sync_values();
            }
        }
    }

    fn series_section(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let session = self.session.as_ref().expect("a figure is open");
        let active = session.active_series();
        let selected = session.selection().map(|p| p.primitive);
        let mut commands = Vec::new();
        let mut names: Vec<String> = session.series().iter().map(|s| s.name.clone()).collect();
        if session.series().is_empty() {
            theme::hint(ui, "A series is the data you export: one or more curves.");
            ui.add_space(2.0);
        }
        for (i, s) in session.series().iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.radio(active == Some(i), "").on_hover_text("Make this the active series").clicked() {
                    commands.push(Command::SetActiveSeries(i));
                }
                let delete_width = 52.0;
                // the gap before the name box and the gap before the button both count
                let name_width = (ui.available_width() - delete_width - 2.0 * ui.spacing().item_spacing.x).max(80.0);
                let name = egui::TextEdit::singleline(&mut names[i]).align(egui::Align2::LEFT_CENTER);
                if ui.add_sized([name_width, FIELD_HEIGHT], name).changed() {
                    commands.push(Command::RenameSeries(i, names[i].clone()));
                }
                let delete = egui::Button::new("Delete").min_size(vec2(delete_width, FIELD_HEIGHT));
                if ui.add(delete).on_hover_text("Delete this series").clicked() {
                    commands.push(Command::DeleteSeries(i));
                }
            });
            ui.horizontal(|ui| {
                ui.add_space(26.0);
                let n = s.members.len();
                ui.label(RichText::new(format!("{n} {}", if n == 1 { "curve" } else { "curves" })).weak());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Empty").on_hover_text("Remove all curves from this series").clicked() {
                        commands.push(Command::ClearSeries(i));
                    }
                    let mut sort = s.sort_by_x;
                    if ui.checkbox(&mut sort, "Sort by x").on_hover_text("Order the exported rows by x").changed() {
                        commands.push(Command::SetSortByX(i, sort));
                    }
                });
            });
            ui.add_space(2.0);
        }
        let member = active.zip(selected).is_some_and(|(a, p)| session.series()[a].members.contains(&p));
        let toggle = Btn::new(if member { "Remove curve" } else { "Add curve" })
            .enabled(selected.is_some())
            .tip("Add or remove the selected curve in the active series (Ctrl+Shift+click does the same)")
            .why_not("Select a curve first: hold Ctrl and click it");
        match theme::buttons(ui, &[Btn::new("New series"), toggle]) {
            Some(0) => commands.push(Command::NewSeries),
            Some(1) => commands.push(Command::ToggleMember(selected.expect("enabled only with a selection"))),
            _ => {}
        }
        let can_export = session.can_export();
        let why_not = if !session.calibration().is_calibrated() {
            "Calibrate both axes first"
        } else {
            "Add curves to the active series first"
        };
        let copy = Btn::new("Copy numbers").enabled(can_export).why_not(why_not);
        let export = Btn::new("Export CSV\u{2026}").primary().enabled(can_export).why_not(why_not);
        match theme::buttons(ui, &[copy, export]) {
            Some(0) => actions.push(Action::CopyNumbers),
            Some(1) => actions.push(Action::ExportCsv),
            _ => {}
        }
        for c in commands {
            self.apply(c);
        }
    }

    fn curve_table(&mut self, ui: &mut Ui) {
        let session = self.session.as_ref().expect("a figure is open");
        let selected = session.selection().map(|p| p.primitive);
        let n = session.figure.primitives.len();
        let mut member = vec![false; n];
        if let Some(a) = session.active_series() {
            for &m in &session.series()[a].members {
                member[m] = true;
            }
        }
        let mut commands = Vec::new();
        let mut clicked = None;
        let right = Layout::right_to_left(Align::Center);

        let mut table = TableBuilder::new(ui)
            .id_salt("curves")
            .striped(true)
            .resizable(false)
            .sense(egui::Sense::click())
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(48.0)) // ID
            .column(Column::remainder()) // Type
            .column(Column::exact(56.0)) // Points
            .column(Column::exact(50.0)) // Hide
            .max_scroll_height(240.0);
        if let Some(row) = self.table_scroll_to.take() {
            table = table.scroll_to_row(row, Some(Align::Center));
        }
        table
            .header(TABLE_ROW, |mut header| {
                header.col(|ui| {
                    ui.label(RichText::new("ID").weak());
                });
                header.col(|ui| {
                    ui.label(RichText::new("Type").weak());
                });
                header.col(|ui| {
                    ui.with_layout(right, |ui| ui.label(RichText::new("Points").weak()));
                });
                header.col(|_| {});
            })
            .body(|body| {
                body.rows(TABLE_ROW, n, |mut row| {
                    let i = row.index();
                    let prim = &session.figure.primitives[i];
                    let hidden = session.is_hidden(i);
                    let visible = session.is_visible(i);
                    let is_selected = selected == Some(i);
                    row.set_selected(is_selected);
                    let text = |value: String| {
                        let t = RichText::new(value);
                        if hidden { t.weak() } else { t }
                    };
                    row.col(|ui| {
                        let dot =
                            if member[i] { RichText::new("\u{25cf}").color(MEMBER_COLOR) } else { RichText::new(" ") };
                        ui.label(dot);
                        ui.label(text((i + 1).to_string()).monospace());
                    });
                    row.col(|ui| {
                        let color = if prim.kind == PrimitiveKind::Stroke { STROKE_COLOR } else { FILL_COLOR };
                        ui.label(RichText::new("\u{25a0}").color(color.to_opaque()));
                        ui.label(text(kind_name(prim.kind).to_string()));
                    });
                    row.col(|ui| {
                        ui.with_layout(right, |ui| ui.label(text(prim.points.len().to_string()).monospace()));
                    });
                    row.col(|ui| {
                        ui.with_layout(right, |ui| {
                            if ui.small_button(if hidden { "Show" } else { "Hide" }).clicked() {
                                commands.push(Command::SetHidden(i, !hidden));
                            }
                        });
                    });
                    if row.response().clicked() && visible {
                        clicked = Some(i);
                    }
                });
            });
        if let Some(i) = clicked {
            commands.push(Command::Select(Some(Pick { primitive: i, vertex: None })));
            self.clicked_in_table = Some(i);
        }
        for c in commands {
            self.apply(c);
        }
        ui.add_space(2.0);
        theme::hint(ui, "\u{25cf} marks curves in the active series. Click a row to select it.");
    }

    fn selection_section(&self, ui: &mut Ui) {
        let session = self.session.as_ref().expect("a figure is open");
        let Some(prim) = session.selected_primitive() else {
            theme::hint(ui, "Nothing selected. Hold Ctrl and click a curve, or click a row in the table.");
            return;
        };
        let pick = session.selection().expect("a primitive is selected");
        let e = prim.extent().unwrap_or(kurbo::Rect::ZERO);
        let range = |a: f64, b: f64| format!("{}\u{2013}{}", format_g(a.min(b), 6), format_g(a.max(b), 6));
        egui::Grid::new("selection").num_columns(2).spacing([GRID_GAP, 4.0]).show(ui, |ui| {
            let mut row = |name: &str, value: String| {
                cell(ui, SELECTION_NAME_COL, |ui| {
                    ui.label(RichText::new(name).weak());
                });
                ui.label(RichText::new(value).monospace());
                ui.end_row();
            };
            row("Curve", curve_label(pick.primitive, prim.kind));
            row("Points", prim.points.len().to_string());
            row("Page x", format!("{:.1}\u{2013}{:.1} pt", e.x0, e.x1));
            row("Page y", format!("{:.1}\u{2013}{:.1} pt", e.y0, e.y1));
            if let (Some((x0, y0)), Some((x1, y1))) =
                (session.plot_coords(kurbo::Point::new(e.x0, e.y0)), session.plot_coords(kurbo::Point::new(e.x1, e.y1)))
            {
                row("Plot x", range(x0, x1));
                row("Plot y", range(y0, y1));
            }
            if let (Some(vertex), Some(p)) = (pick.vertex, session.selected_point()) {
                row("Point", format!("#{}", vertex + 1));
                row("At", format!("{:.3}, {:.3} pt", p.x, p.y));
                if let Some((x, y)) = session.plot_coords(p) {
                    row("x", format_g(x, 8));
                    row("y", format_g(y, 8));
                }
            }
        });
    }
}

/// A grid cell of a fixed width, its content vertically centred.
fn cell(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, FIELD_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        add_contents(ui);
    });
}
