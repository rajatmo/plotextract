//! The side panel: layers, calibration, series, the curve table and the selection. It reads the model, keeps only the
//! state of its own widgets and reports what the user did as [`Intent`]s.
//!
//! The panel is a fixed width, so everything here is laid out against it: rows of buttons share the width equally,
//! input boxes fill what is left of their row, and table columns line up with their headers.

use std::collections::HashSet;

use egui::{Align, Layout, RichText, Stroke, Ui, vec2};
use egui_extras::{Column, TableBuilder};
use plotextract_core::{Axis, Edit, Layer, Pick, PrimitiveKind, Session, Slot, format_g, parse_value};

use crate::model::{Intent, Model};
use crate::text::{axis_status, curve_label, kind_name, measure, slot_label};
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

/// What the panel's widgets remember between frames.
#[derive(Default)]
pub struct PanelState {
    /// Text of the plot value cells, so half-typed numbers survive.
    values: [String; 4],
    /// The curve table scrolls to this row (a curve selected on the canvas).
    table_scroll_to: Option<usize>,
    last_selection: Option<usize>,
    /// The curve selected by clicking its table row: no need to scroll to it.
    clicked_in_table: Option<usize>,
}

impl PanelState {
    /// Fill the value cells from the session's calibration (after an undo, a project load, a new document).
    pub fn sync_values(&mut self, session: &Session) {
        for (cell, slot) in self.values.iter_mut().zip(Slot::ALL) {
            *cell = session.calibration().value(slot).map(|v| format_g(v, 10)).unwrap_or_default();
        }
    }

    /// Note which curve is selected: one selected on the canvas scrolls into view in the table, one clicked in the
    /// table does not move it.
    pub fn track_selection(&mut self, selection: Option<usize>) {
        if selection != self.last_selection {
            if selection.is_some() && selection != self.clicked_in_table {
                self.table_scroll_to = selection;
            }
            self.last_selection = selection;
            self.clicked_in_table = None;
        }
    }
}

pub fn show(ui: &mut Ui, model: &Model, state: &mut PanelState) -> Vec<Intent> {
    let Some(session) = model.session() else {
        ui.add_space(12.0);
        ui.label(RichText::new("No plot open").weak());
        return Vec::new();
    };
    let mut intents = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        page_row(ui, session, &mut intents);
        layers_row(ui, session, &mut intents);
        ui.add_space(4.0);
        let cal = session.calibration();
        let badge =
            Badge { on: cal.is_calibrated(), tip: format!("x: {}, y: {}", axis_status(&cal.x), axis_status(&cal.y)) };
        theme::card_with_badge(ui, "Calibration", Some(badge), |ui| {
            calibration_section(ui, model, session, state, &mut intents)
        });
        theme::card(ui, "Series", |ui| series_section(ui, session, &mut intents));
        theme::card(ui, "Curves", |ui| curve_table(ui, session, state, &mut intents));
        theme::card(ui, "Selection", |ui| selection_section(ui, session));
    });
    intents
}

/// For a file with several pages: which one is shown, and a way to move.
fn page_row(ui: &mut Ui, session: &Session, intents: &mut Vec<Intent>) {
    let (page, count) = (session.figure.page, session.figure.page_count);
    if count < 2 {
        return;
    }
    ui.horizontal(|ui| {
        let step = |ui: &mut Ui, label: &str, enabled: bool, tip: &str| {
            ui.add_enabled(enabled, egui::Button::new(label).min_size(vec2(FIELD_HEIGHT, FIELD_HEIGHT)))
                .on_hover_text(tip)
                .clicked()
        };
        if step(ui, "\u{25c0}", page > 0, "Previous page (Page Up)") {
            intents.push(Intent::GoToPage(page - 1));
        }
        ui.label("Page");
        let mut shown = page + 1;
        let field = egui::DragValue::new(&mut shown).range(1..=count).speed(0.1);
        if ui.add(field).on_hover_text("Type or drag to go to a page").changed() && shown != page + 1 {
            intents.push(Intent::GoToPage(shown - 1));
        }
        ui.label(RichText::new(format!("of {count}")).weak());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if step(ui, "\u{25b6}", page + 1 < count, "Next page (Page Down)") {
                intents.push(Intent::GoToPage(page + 1));
            }
        });
    });
    ui.add_space(4.0);
}

fn layers_row(ui: &mut Ui, session: &Session, intents: &mut Vec<Intent>) {
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
            intents.push(Intent::SetLayer(layer, new));
        }
    }
}

fn calibration_section(
    ui: &mut Ui,
    model: &Model,
    session: &Session,
    state: &mut PanelState,
    intents: &mut Vec<Intent>,
) {
    theme::hint(ui, "Ctrl+click a tick's end point, record it in a row, then type its value.");
    ui.add_space(2.0);
    let cal = session.calibration();
    // the value boxes take whatever the other two columns leave
    let value_width = (ui.available_width() - NAME_COL - POSITION_COL - 2.0 * GRID_GAP).max(80.0);
    // a calibrated axis outlines its two value boxes in green
    let axis_ok = [cal.x.map().is_ok(), cal.y.map().is_ok()];
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
            let row = egui::Button::selectable(model.slot() == slot, slot_label(slot));
            if ui.add_sized([NAME_COL, FIELD_HEIGHT], row).clicked() {
                intents.push(Intent::SelectSlot(slot));
            }
            cell(ui, POSITION_COL, |ui| match cal.position(slot) {
                Some(p) => {
                    ui.label(RichText::new(format!("{p:.3}")).monospace());
                }
                None => {
                    ui.label(RichText::new("\u{2013}").weak());
                }
            });
            let field =
                egui::TextEdit::singleline(&mut state.values[i]).hint_text("value").align(egui::Align2::LEFT_CENTER);
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
                intents.push(Intent::Edit(Edit::SetValue(slot, parse_value(&state.values[i]))));
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    let (mut xlog, mut ylog) = (cal.x.log, cal.y.log);
    ui.columns(2, |columns| {
        columns[0].checkbox(&mut xlog, "Log x scale");
        columns[1].checkbox(&mut ylog, "Log y scale");
    });
    let record = Btn::new("Record").primary().tip("Record the selected point in the highlighted row (Ctrl+R)");
    let clear = Btn::new("Clear").tip("Forget all four ticks");
    match theme::buttons(ui, &[record, clear]) {
        Some(0) => intents.push(Intent::Record),
        Some(1) => intents.push(Intent::Edit(Edit::ClearCalibration)),
        _ => {}
    }
    if xlog != cal.x.log {
        intents.push(Intent::Edit(Edit::SetLog(Axis::X, xlog)));
    }
    if ylog != cal.y.log {
        intents.push(Intent::Edit(Edit::SetLog(Axis::Y, ylog)));
    }
}

fn series_section(ui: &mut Ui, session: &Session, intents: &mut Vec<Intent>) {
    let active = session.active_series();
    let selected = session.selection().map(|p| p.primitive);
    let mut names: Vec<String> = session.series().iter().map(|s| s.name.clone()).collect();
    if session.series().is_empty() {
        theme::hint(ui, "A series is the data you export: one or more curves.");
        ui.add_space(2.0);
    }
    for (i, s) in session.series().iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.radio(active == Some(i), "").on_hover_text("Make this the active series").clicked() {
                intents.push(Intent::Edit(Edit::SetActiveSeries(i)));
            }
            let delete_width = 52.0;
            // the gap before the name box and the gap before the button both count
            let name_width = (ui.available_width() - delete_width - 2.0 * ui.spacing().item_spacing.x).max(80.0);
            let name = egui::TextEdit::singleline(&mut names[i]).align(egui::Align2::LEFT_CENTER);
            if ui.add_sized([name_width, FIELD_HEIGHT], name).changed() {
                intents.push(Intent::Edit(Edit::RenameSeries(i, names[i].clone())));
            }
            let delete = egui::Button::new("Delete").min_size(vec2(delete_width, FIELD_HEIGHT));
            if ui.add(delete).on_hover_text("Delete this series").clicked() {
                intents.push(Intent::Edit(Edit::DeleteSeries(i)));
            }
        });
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            let n = s.members.len();
            ui.label(RichText::new(format!("{n} {}", if n == 1 { "curve" } else { "curves" })).weak());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Empty").on_hover_text("Remove all curves from this series").clicked() {
                    intents.push(Intent::Edit(Edit::ClearSeries(i)));
                }
                let mut sort = s.sort_by_x;
                if ui.checkbox(&mut sort, "Sort by x").on_hover_text("Order the exported rows by x").changed() {
                    intents.push(Intent::Edit(Edit::SetSortByX(i, sort)));
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
        Some(0) => intents.push(Intent::Edit(Edit::NewSeries)),
        Some(1) => intents.push(Intent::Edit(Edit::ToggleMember(selected.expect("enabled only with a selection")))),
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
        Some(0) => intents.push(Intent::CopyNumbers),
        Some(1) => intents.push(Intent::RequestExportCsv),
        _ => {}
    }
}

fn curve_table(ui: &mut Ui, session: &Session, state: &mut PanelState, intents: &mut Vec<Intent>) {
    let selected = session.selection().map(|p| p.primitive);
    let n = session.figure.primitives.len();
    let member: HashSet<usize> =
        session.active_series().map(|a| session.series()[a].members.iter().copied().collect()).unwrap_or_default();
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
    if let Some(row) = state.table_scroll_to.take() {
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
                row.set_selected(selected == Some(i));
                let text = |value: String| {
                    let t = RichText::new(value);
                    if hidden { t.weak() } else { t }
                };
                row.col(|ui| {
                    let dot = if member.contains(&i) {
                        RichText::new("\u{25cf}").color(MEMBER_COLOR)
                    } else {
                        RichText::new(" ")
                    };
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
                            intents.push(Intent::Edit(Edit::SetHidden(i, !hidden)));
                        }
                    });
                });
                if row.response().clicked() && visible {
                    clicked = Some(i);
                }
            });
        });
    if let Some(i) = clicked {
        intents.push(Intent::Select(Some(Pick { primitive: i, vertex: None })));
        state.clicked_in_table = Some(i);
    }
    ui.add_space(2.0);
    theme::hint(ui, "\u{25cf} marks curves in the active series. Click a row to select it.");
    if session.figure.truncated {
        let note = format!(
            "The file holds more paths than plotextract keeps: only the first {n} are listed, and no picture is drawn."
        );
        ui.label(RichText::new(note).color(egui::Color32::from_rgb(214, 140, 20)));
    }
}

fn selection_section(ui: &mut Ui, session: &Session) {
    let Some(prim) = session.selected_primitive() else {
        theme::hint(ui, "Nothing selected. Hold Ctrl and click a curve, or click a row in the table.");
        return;
    };
    let pick = session.selection().expect("a primitive is selected");
    let unit = session.figure.unit;
    let e = prim.extent().unwrap_or(kurbo::Rect::ZERO);
    let span = |a: f64, b: f64| format!("{:.1}\u{2013}{:.1} {}", a, b, unit.symbol());
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
        row("Page x", span(e.x0, e.x1));
        row("Page y", span(e.y0, e.y1));
        if let (Some((x0, y0)), Some((x1, y1))) =
            (session.plot_coords(kurbo::Point::new(e.x0, e.y0)), session.plot_coords(kurbo::Point::new(e.x1, e.y1)))
        {
            row("Plot x", range(x0, x1));
            row("Plot y", range(y0, y1));
        }
        if let (Some(vertex), Some(p)) = (pick.vertex, session.selected_point()) {
            row("Point", format!("#{}", vertex + 1));
            row("At", format!("{:.3}, {}", p.x, measure(p.y, unit, 3)));
            if let Some((x, y)) = session.plot_coords(p) {
                row("x", format_g(x, 8));
                row("y", format_g(y, 8));
            }
        }
    });
}

/// A grid cell of a fixed width, its content vertically centred.
fn cell(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, FIELD_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        add_contents(ui);
    });
}

#[cfg(test)]
mod tests {
    use kurbo::Point;
    use plotextract_core::{Figure, Primitive, PrimitiveKind, Unit};

    use super::*;

    fn session() -> Session {
        let line = Primitive::new(PrimitiveKind::Stroke, 0, vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)], vec![]);
        let figure = Figure {
            width: 10.0,
            height: 10.0,
            unit: Unit::Point,
            primitives: vec![line],
            page: 0,
            page_count: 1,
            truncated: false,
        };
        Session::new(figure, None)
    }

    #[test]
    fn the_value_cells_follow_the_calibration() {
        let mut s = session();
        let mut panel = PanelState::default();
        s.edit(Edit::SetValue(Slot::XMax, Some(1500.0))).unwrap();
        panel.sync_values(&s);
        assert_eq!(panel.values, ["", "1500", "", ""]);
        s.undo();
        panel.sync_values(&s);
        assert_eq!(panel.values, ["", "", "", ""]);
    }

    #[test]
    fn a_curve_picked_on_the_canvas_scrolls_into_view_but_one_clicked_in_the_table_does_not() {
        let mut panel = PanelState::default();
        panel.track_selection(Some(7)); // picked on the canvas
        assert_eq!(panel.table_scroll_to.take(), Some(7));
        panel.track_selection(Some(7));
        assert_eq!(panel.table_scroll_to, None, "an unchanged selection scrolls nothing");

        panel.clicked_in_table = Some(3); // the table row handler records its click, then the selection changes
        panel.track_selection(Some(3));
        assert_eq!(panel.table_scroll_to, None);
        assert_eq!(panel.clicked_in_table, None);

        panel.track_selection(None);
        assert_eq!(panel.table_scroll_to, None, "deselecting scrolls nothing");
    }
}
