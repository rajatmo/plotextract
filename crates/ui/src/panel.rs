//! The panels of a vector figure, laid out as the tracing wizard's: on the left what the figure is made of (the layers
//! and every curve, one row each, as the wizard lists its masks), on the right what is done with it (calibration, series,
//! the selection), as the wizard shows its step. They read the model, keep only the state of their own widgets and
//! report what the user did as [`Intent`]s.
//!
//! The panels are a fixed width, so everything here is laid out against it: rows of buttons share the width equally,
//! input boxes fill what is left of their row, and the columns of the rows line up.

use std::collections::HashSet;

use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, vec2};
use plotextract_domain::{Axis, Edit, Layer, Pick, PrimitiveKind, Slot, format_g, parse_value};
use plotextract_usecases::Session;

use crate::theme::{self, ACCENT, Badge, Btn, EYE, FILL_COLOR, MEMBER_COLOR, OK, STROKE_COLOR, eye};
use plotextract_model::text::{axis_status, curve_label, measure, slot_label};
use plotextract_model::{Intent, Model};

/// The height of a curve's row, and the widths of its columns (an eye, a picture, the name, the points).
const ROW: f32 = 24.0;
const SWATCH: f32 = 22.0;
const CURVE_NAME: f32 = 120.0;
const COL_GAP: f32 = 6.0;
/// The first column of the grids (row names), in points.
pub(crate) const NAME_COL: f32 = 54.0;
/// The position column of the calibration grid.
pub(crate) const POSITION_COL: f32 = 70.0;
pub(crate) const GRID_GAP: f32 = 8.0;
/// The first column of the selection grid.
const SELECTION_NAME_COL: f32 = 56.0;
pub(crate) const FIELD_HEIGHT: f32 = 24.0;

/// What the panel's widgets remember between frames.
#[derive(Default)]
pub struct PanelState {
    /// Text of the plot value cells, so half-typed numbers survive.
    values: [String; 4],
    /// The list of curves scrolls to this row (a curve selected on the canvas).
    table_scroll_to: Option<usize>,
    last_selection: Option<usize>,
    /// The curve selected by clicking its row: no need to scroll to it.
    clicked_in_table: Option<usize>,
    /// How many strokes and fills the figure has, counted once (it can have a million curves). The state is made afresh
    /// for each figure opened.
    kinds: Option<(usize, usize)>,
}

/// What the curves panel returns: what the user did, and the curve whose row the pointer is on (outlined on the page).
pub struct CurvesOutput {
    pub intents: Vec<Intent>,
    pub row_hover: Option<usize>,
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

/// The right panel: what the figure is, then calibration, series and the selection.
pub fn show(ui: &mut Ui, session: &Session, model: &Model, state: &mut PanelState) -> Vec<Intent> {
    let mut intents = Vec::new();
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        theme::card(ui, "Vector figure", |ui| {
            theme::hint(ui, "Its curves are the paths it was drawn with: calibrate, then pick them.")
        });
        let cal = session.calibration();
        let badge =
            Badge { on: cal.is_calibrated(), tip: format!("x: {}, y: {}", axis_status(&cal.x), axis_status(&cal.y)) };
        theme::card_with_badge(ui, "Calibration", Some(badge), |ui| {
            calibration_section(ui, model, session, state, &mut intents)
        });
        theme::card(ui, "Series", |ui| series_section(ui, session, &mut intents));
        theme::card(ui, "Selection", |ui| selection_section(ui, session));
    });
    intents
}

/// The left panel: the layers, each with an eye, and every curve, one row each.
pub fn curves(ui: &mut Ui, session: &Session, state: &mut PanelState) -> CurvesOutput {
    let mut out = CurvesOutput { intents: Vec::new(), row_hover: None };
    let kinds = *state.kinds.get_or_insert_with(|| {
        let strokes = session.figure.primitives.iter().filter(|p| p.kind == PrimitiveKind::Stroke).count();
        (strokes, session.figure.primitives.len() - strokes)
    });
    layers_card(ui, session, kinds, &mut out.intents);
    let n = session.figure.primitives.len();
    theme::card(ui, &format!("Curves ({n})"), |ui| curve_list(ui, session, state, &mut out));
    out
}

/// The picture, the strokes and the fills, each a row that shows or hides it, with how many curves it has.
fn layers_card(ui: &mut Ui, session: &Session, (strokes, fills): (usize, usize), intents: &mut Vec<Intent>) {
    let layers = session.layers();
    let rows = [
        (Layer::Page, layers.page, "Picture", None, "the picture behind the curves"),
        (Layer::Strokes, layers.strokes, "Strokes", Some(strokes), "every stroked path"),
        (Layer::Fills, layers.fills, "Fills", Some(fills), "every filled path"),
    ];
    theme::card(ui, "Layers", |ui| {
        ui.spacing_mut().item_spacing = vec2(COL_GAP, 2.0);
        for (layer, shown, name, count, what) in rows {
            ui.horizontal(|ui| {
                cell(ui, EYE, |ui| {
                    let tip = format!("{} {what}", if shown { "Hide" } else { "Show" });
                    if eye(ui, shown).on_hover_text(tip).clicked() {
                        intents.push(Intent::SetLayer(layer, !shown));
                    }
                });
                cell(ui, SWATCH, |ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::hover());
                    match layer {
                        Layer::Page => {
                            ui.painter().rect_filled(rect.shrink(3.0), CornerRadius::same(2), Color32::WHITE);
                        }
                        Layer::Strokes => draw_swatch(ui, rect, PrimitiveKind::Stroke, false),
                        Layer::Fills => draw_swatch(ui, rect, PrimitiveKind::Fill, false),
                    }
                });
                let text = RichText::new(name);
                ui.label(if shown { text } else { text.weak() });
                if let Some(n) = count {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(n.to_string()).monospace().weak());
                    });
                }
            });
        }
    });
}

/// Every curve, one row each, the rows drawn only where they are seen (a figure can have a million curves): an eye that
/// hides it, a picture of its kind (outlined green while it is in the active series), its name and its points. A click
/// on a row selects the curve; the pointer on a row outlines it on the page.
fn curve_list(ui: &mut Ui, session: &Session, state: &mut PanelState, out: &mut CurvesOutput) {
    let selected = session.selection().map(|p| p.primitive);
    let n = session.figure.primitives.len();
    let member: HashSet<usize> =
        session.active_series().map(|a| session.series()[a].members.iter().copied().collect()).unwrap_or_default();
    theme::hint(ui, "Click a row to select its curve; green: in the active series.");
    ui.add_space(4.0);
    let gap = 2.0;
    let mut area = egui::ScrollArea::vertical()
        .id_salt("curves")
        .max_height((ui.available_height() - 40.0).max(4.0 * ROW))
        .auto_shrink([false, true]);
    if let Some(row) = state.table_scroll_to.take() {
        // the row in the middle of what is seen
        area = area.vertical_scroll_offset((row as f32 * (ROW + gap) - 4.0 * ROW).max(0.0));
    }
    let mut clicked = None;
    area.show_rows(ui, ROW, n, |ui, rows| {
        ui.spacing_mut().item_spacing = vec2(COL_GAP, gap);
        for i in rows {
            let prim = &session.figure.primitives[i];
            let hidden = session.is_hidden(i);
            let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
            let fill = if selected == Some(i) {
                ACCENT.gamma_multiply(0.25)
            } else if response.hovered() {
                ACCENT.gamma_multiply(0.12)
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
            let mut row =
                ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
            cell(&mut row, EYE, |ui| {
                let tip =
                    if hidden { "Show this curve" } else { "Hide this curve: it is no longer drawn or snapped to" };
                if eye(ui, !hidden).on_hover_text(tip).clicked() {
                    out.intents.push(Intent::Edit(Edit::SetHidden(i, !hidden)));
                }
            });
            cell(&mut row, SWATCH, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::hover());
                draw_swatch(ui, r, prim.kind, member.contains(&i));
            });
            cell(&mut row, CURVE_NAME, |ui| {
                let text = RichText::new(curve_label(i, prim.kind));
                let text = if selected == Some(i) {
                    text.strong()
                } else if hidden {
                    text.weak()
                } else {
                    text
                };
                ui.add(egui::Label::new(text).selectable(false));
            });
            row.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                ui.label(RichText::new(format!("{} pts", prim.points.len())).monospace().weak());
            });
            if response.clicked() && session.is_visible(i) {
                clicked = Some(i);
            }
            if response.hovered() {
                out.row_hover = Some(i);
            }
        }
    });
    if let Some(i) = clicked {
        out.intents.push(Intent::Select(Some(Pick { primitive: i, vertex: None })));
        state.clicked_in_table = Some(i);
    }
    if session.figure.truncated {
        ui.add_space(4.0);
        let note = format!(
            "The file holds more paths than plotextract keeps: only the first {n} are listed, and no picture is drawn."
        );
        ui.label(RichText::new(note).color(Color32::from_rgb(214, 140, 20)));
    }
}

/// A picture of a kind of curve, in the colours it is outlined with on the page: a line for a stroke, a patch for a fill;
/// ringed green while it is in the active series.
fn draw_swatch(ui: &Ui, rect: egui::Rect, kind: PrimitiveKind, member: bool) {
    let painter = ui.painter();
    let c = rect.center();
    match kind {
        PrimitiveKind::Stroke => {
            painter
                .line_segment([c + vec2(-7.0, 4.0), c + vec2(7.0, -4.0)], Stroke::new(2.5, STROKE_COLOR.to_opaque()));
        }
        PrimitiveKind::Fill => {
            painter.rect_filled(rect.shrink(5.0), CornerRadius::same(2), FILL_COLOR.to_opaque());
        }
    }
    if member {
        painter.rect_stroke(
            rect.shrink(1.0),
            CornerRadius::same(5),
            Stroke::new(2.0, MEMBER_COLOR),
            egui::StrokeKind::Inside,
        );
    }
}

fn calibration_section(
    ui: &mut Ui,
    model: &Model,
    session: &Session,
    state: &mut PanelState,
    intents: &mut Vec<Intent>,
) {
    theme::hint(ui, "Ctrl+click a tick, Record, then type its value.");
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
            if value_field(ui, &mut state.values[i], value_width, axis_ok[i / 2]) {
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
        theme::hint(ui, "A series is what you export.");
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

fn selection_section(ui: &mut Ui, session: &Session) {
    let Some(prim) = session.selected_primitive() else {
        theme::hint(ui, "Ctrl+click a curve to select it.");
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

/// The box a tick's plot value is typed in, outlined in green when its axis is calibrated. Whether the text changed.
pub(crate) fn value_field(ui: &mut Ui, text: &mut String, width: f32, calibrated: bool) -> bool {
    let field = egui::TextEdit::singleline(text).hint_text("value").align(egui::Align2::LEFT_CENTER);
    ui.scope(|ui| {
        if calibrated {
            let widgets = &mut ui.visuals_mut().widgets;
            for state in [&mut widgets.inactive, &mut widgets.hovered] {
                state.bg_stroke = Stroke::new(1.5, OK);
            }
        }
        ui.add_sized([width, FIELD_HEIGHT], field).changed()
    })
    .inner
}

/// A grid cell of a fixed width, its content vertically centred.
pub(crate) fn cell(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(vec2(width, FIELD_HEIGHT), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        add_contents(ui);
    });
}

#[cfg(test)]
mod tests {
    use kurbo::Point;
    use plotextract_domain::{Figure, Primitive, PrimitiveKind, Unit};

    use super::*;

    fn session() -> Session {
        let line = Primitive::new(PrimitiveKind::Stroke, 0, vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)], vec![]);
        let figure =
            Figure { width: 10.0, height: 10.0, unit: Unit::Point, primitives: vec![line], page: 0, truncated: false };
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
