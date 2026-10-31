//! The egui shell: application state, menus, file handling, and the frame loop that turns input into `Session` commands.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use egui::{Align, Color32, Context, Key, Layout, Modifiers, RichText, Ui};
use kurbo::Point;
use plotextract_core::{Command, Figure, Hit, Importer, PrimitiveKind, Session, Slot, format_g};
use plotextract_pdf::PdfImporter;
use plotextract_ps::PsImporter;
use plotextract_svg::SvgImporter;

use crate::canvas::{Detail, Picture};
use crate::detail::Worker;
use crate::theme;
use crate::view::View;

/// The side panel's width, in points. It does not resize: the cards inside are laid out for it.
const SIDE_PANEL_WIDTH: f32 = 340.0;

/// Things the UI asks for that need a dialog, the clipboard or the whole app.
pub(crate) enum Action {
    OpenDialog,
    Open(PathBuf),
    SaveProject,
    LoadProject,
    ExportCsv,
    CopyNumbers,
    Undo,
    Redo,
    Fit,
    Record,
    About,
    Quit,
}

pub struct App {
    pub(crate) session: Option<Session>,
    pub(crate) backdrop: Option<Picture>,
    pub(crate) detail: Option<Detail>,
    /// The detail render asked for and not yet delivered: its id, region and scale.
    pub(crate) pending: Option<(u64, kurbo::Rect, f64)>,
    pub(crate) worker: Option<Worker>,
    /// View state seen last frame, and when it last differed.
    pub(crate) last_view: (f32, egui::Vec2, egui::Vec2),
    pub(crate) changed_at: Instant,
    pub(crate) view: View,
    pub(crate) hover: Option<Hit>,
    /// The calibration row the Record button fills next.
    pub(crate) slot: Slot,
    /// Text of the plot value cells, so half-typed numbers survive.
    pub(crate) values: [String; 4],
    pub(crate) message: Option<(String, bool)>,
    pub(crate) icon: egui::TextureHandle,
    pub(crate) show_about: bool,
    /// Actions requested by widgets drawn outside the panels (the welcome screen), run next frame.
    pub(crate) pending_actions: Vec<Action>,
    /// The curve table scrolls to this row (a curve selected on the canvas).
    pub(crate) table_scroll_to: Option<usize>,
    last_selection: Option<usize>,
    /// The curve selected by clicking its table row: no need to scroll to it.
    pub(crate) clicked_in_table: Option<usize>,
}

/// "Stroke" or "Fill".
pub(crate) fn kind_name(kind: PrimitiveKind) -> &'static str {
    match kind {
        PrimitiveKind::Stroke => "Stroke",
        PrimitiveKind::Fill => "Fill",
    }
}

/// How a curve is named in messages: its type and its 1-based ID, as the curve table shows them.
pub(crate) fn curve_label(index: usize, kind: PrimitiveKind) -> String {
    format!("{} #{}", kind_name(kind), index + 1)
}

fn importers() -> Vec<Arc<dyn Importer>> {
    vec![Arc::new(PdfImporter), Arc::new(SvgImporter), Arc::new(PsImporter)]
}

/// A loaded figure with what is needed to re-render parts of it later: the importer and the file's bytes.
type Loaded = (Figure, Arc<dyn Importer>, Arc<Vec<u8>>);

fn load_figure(path: &Path) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let head = &bytes[..bytes.len().min(1024)];
    let importer = importers()
        .into_iter()
        .find(|i| i.sniff(head))
        .ok_or_else(|| "this file format is not supported yet".to_string())?;
    let figure = importer.import(bytes.clone()).map_err(|e| e.to_string())?;
    Ok((figure, importer, Arc::new(bytes)))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("the bundled icon is a valid PNG");
        let image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
        let icon = cc.egui_ctx.load_texture("app icon", image, egui::TextureOptions::LINEAR);

        let mut app = Self {
            session: None,
            backdrop: None,
            detail: None,
            pending: None,
            worker: None,
            last_view: (0.0, egui::Vec2::ZERO, egui::Vec2::ZERO),
            changed_at: Instant::now(),
            view: View::default(),
            hover: None,
            slot: Slot::XMin,
            values: Default::default(),
            message: None,
            icon,
            show_about: false,
            pending_actions: Vec::new(),
            table_scroll_to: None,
            last_selection: None,
            clicked_in_table: None,
        };
        if let Some(path) = path {
            app.open(&cc.egui_ctx, &path);
        }
        app
    }

    pub(crate) fn error(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), true));
    }

    pub(crate) fn info(&mut self, text: impl Into<String>) {
        self.message = Some((text.into(), false));
    }

    fn open(&mut self, ctx: &Context, path: &Path) {
        match load_figure(path) {
            Ok((figure, importer, bytes)) => {
                self.backdrop = figure.backdrop.as_ref().map(|b| Picture::new(ctx, &b.bitmap, Point::ZERO, b.scale));
                self.detail = None;
                self.pending = None;
                self.worker = Some(Worker::spawn(ctx.clone(), importer, bytes));
                let n = figure.primitives.len();
                self.view = View::default();
                self.hover = None;
                self.slot = Slot::XMin;
                self.table_scroll_to = None;
                self.last_selection = None;
                self.clicked_in_table = None;
                self.session = Some(Session::new(figure, Some(path.display().to_string())));
                self.sync_values();
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name} \u{2013} plotextract")));
                self.info(format!("Opened {name}: {n} curves. Hold Ctrl and click a curve to select it."));
            }
            Err(e) => self.error(format!("Cannot open {}: {e}", path.display())),
        }
    }

    /// Fill the value cells from the session's calibration (after an undo, a project load, a new document).
    pub(crate) fn sync_values(&mut self) {
        if let Some(s) = &self.session {
            for (i, slot) in Slot::ALL.into_iter().enumerate() {
                self.values[i] = s.calibration().value(slot).map(|v| format_g(v, 10)).unwrap_or_default();
            }
        }
    }

    pub(crate) fn apply(&mut self, command: Command) -> bool {
        let Some(s) = &mut self.session else { return false };
        match s.apply(command) {
            Ok(()) => true,
            Err(e) => {
                self.error(e.to_string());
                false
            }
        }
    }

    // --- actions that need dialogs or the clipboard ------------------------------------------------

    fn run(&mut self, ctx: &Context, action: Action) {
        match action {
            Action::OpenDialog => {
                if let Some(path) = rfd::FileDialog::new().add_filter("Plots", &["pdf", "svg", "eps", "ps"]).pick_file()
                {
                    self.open(ctx, &path);
                }
            }
            Action::Open(path) => self.open(ctx, &path),
            Action::SaveProject => self.save_project(),
            Action::LoadProject => self.load_project(ctx),
            Action::ExportCsv => {
                if let Some(text) = self.csv_or_warn() {
                    let name = self.active_name();
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("{name}.csv"))
                        .add_filter("CSV", &["csv"])
                        .save_file()
                    {
                        match std::fs::write(&path, text) {
                            Ok(()) => self.info(format!("Wrote {}", path.display())),
                            Err(e) => self.error(format!("Cannot write {}: {e}", path.display())),
                        }
                    }
                }
            }
            Action::CopyNumbers => {
                if let Some(text) = self.csv_or_warn() {
                    ctx.copy_text(text);
                    self.info("Copied the series as x,y text.");
                }
            }
            Action::Undo => {
                if self.session.as_mut().is_some_and(|s| s.undo()) {
                    self.sync_values();
                }
            }
            Action::Redo => {
                if self.session.as_mut().is_some_and(|s| s.redo()) {
                    self.sync_values();
                }
            }
            Action::Fit => self.view.request_fit(),
            Action::Record => {
                if self.apply(Command::RecordSelected(self.slot)) {
                    let slot = self.slot;
                    self.slot = slot.next();
                    self.info(format!("{} recorded. Type its plot value in the table.", slot.label()));
                }
            }
            Action::About => self.show_about = !self.show_about,
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn active_name(&self) -> String {
        self.session
            .as_ref()
            .and_then(|s| s.active_series().map(|a| s.series()[a].name.clone()))
            .unwrap_or_else(|| "series".into())
    }

    fn csv_or_warn(&mut self) -> Option<String> {
        let s = self.session.as_ref()?;
        let Some(active) = s.active_series() else {
            self.error("Make a series first: select a curve, then Add curve.");
            return None;
        };
        match s.csv(active) {
            Ok(text) => Some(text),
            Err(e) => {
                self.error(e.to_string());
                None
            }
        }
    }

    fn save_project(&mut self) {
        let Some(s) = &self.session else { return };
        let suggested = s
            .source
            .as_deref()
            .and_then(|p| Path::new(p).file_stem())
            .map(|n| format!("{}.plotextract.json", n.to_string_lossy()));
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(suggested.unwrap_or_else(|| "project.plotextract.json".into()))
            .add_filter("plotextract project", &["json"])
            .save_file()
        else {
            return;
        };
        let result = serde_json::to_string_pretty(&s.project())
            .map_err(|e| e.to_string())
            .and_then(|t| std::fs::write(&path, t).map_err(|e| e.to_string()));
        match result {
            Ok(()) => self.info(format!("Saved {}", path.display())),
            Err(e) => self.error(format!("Cannot save {}: {e}", path.display())),
        }
    }

    fn load_project(&mut self, ctx: &Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("plotextract project", &["json"]).pick_file() else {
            return;
        };
        let project: Result<plotextract_core::Project, String> = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()));
        let project = match project {
            Ok(p) => p,
            Err(e) => return self.error(format!("Cannot read {}: {e}", path.display())),
        };
        if let Some(source) = project.source.clone() {
            let source = PathBuf::from(source);
            // Reopen the figure unless it is the one already shown.
            if self.session.as_ref().and_then(|s| s.source.as_deref()) != source.to_str() {
                self.open(ctx, &source);
            }
        }
        match &mut self.session {
            Some(s) => {
                s.load_project(project);
                self.sync_values();
                self.info(format!("Loaded {}", path.display()));
            }
            None => self.error("The project's figure could not be opened."),
        }
    }

    // --- frame ---------------------------------------------------------------------------------------

    fn shortcuts(&self, ctx: &Context, actions: &mut Vec<Action>) {
        let take = |mods: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(mods, key));
        if take(Modifiers::COMMAND, Key::O) {
            actions.push(Action::OpenDialog);
        }
        if take(Modifiers::COMMAND, Key::S) {
            actions.push(Action::SaveProject);
        }
        if take(Modifiers::COMMAND, Key::R) {
            actions.push(Action::Record);
        }
        if take(Modifiers::COMMAND, Key::Num0) {
            actions.push(Action::Fit);
        }
        if take(Modifiers::NONE, Key::F1) {
            actions.push(Action::About);
        }
        if take(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || take(Modifiers::COMMAND, Key::Y) {
            actions.push(Action::Redo);
        } else if take(Modifiers::COMMAND, Key::Z) {
            actions.push(Action::Undo);
        }
    }

    fn menu(&self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let has_figure = self.session.is_some();
        let mut item = |ui: &mut Ui, enabled: bool, text: &str, shortcut: &str, action: Action| {
            let button = egui::Button::new(text).shortcut_text(shortcut);
            if ui.add_enabled(enabled, button).clicked() {
                actions.push(action);
                ui.close();
            }
        };
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item(ui, true, "Open\u{2026}", "Ctrl+O", Action::OpenDialog);
                ui.separator();
                item(ui, true, "Open project\u{2026}", "", Action::LoadProject);
                item(ui, has_figure, "Save project\u{2026}", "Ctrl+S", Action::SaveProject);
                ui.separator();
                item(ui, true, "Quit", "", Action::Quit);
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = self.session.as_ref().map_or((false, false), |s| (s.can_undo(), s.can_redo()));
                item(ui, undo, "Undo", "Ctrl+Z", Action::Undo);
                item(ui, redo, "Redo", "Ctrl+Shift+Z", Action::Redo);
            });
            ui.menu_button("View", |ui| {
                item(ui, has_figure, "Fit page", "Ctrl+0", Action::Fit);
            });
            ui.menu_button("Help", |ui| {
                item(ui, true, "About plotextract", "F1", Action::About);
            });
        });
    }

    fn status_bar(&self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let (Some(hit), Some(s)) = (&self.hover, &self.session) {
                let p = &s.figure.primitives[hit.primitive];
                let at = format!("{}, point {}", curve_label(hit.primitive, p.kind), hit.vertex + 1);
                let readout = match s.plot_coords(hit.pos) {
                    Some((x, y)) => format!("x = {}    y = {}", format_g(x, 6), format_g(y, 6)),
                    None => format!("x = {:.3} pt    y = {:.3} pt    (not calibrated)", hit.pos.x, hit.pos.y),
                };
                ui.label(RichText::new(at).strong());
                ui.label(RichText::new(readout).monospace());
            } else if let Some((text, is_error)) = &self.message {
                let color = if *is_error { Color32::from_rgb(214, 70, 70) } else { ui.visuals().text_color() };
                ui.label(RichText::new(text).color(color));
            } else {
                ui.label(RichText::new("Open a plot to begin").weak());
            }
            if let Some(s) = &self.session {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let zoom = f64::from(self.view.zoom) * 100.0;
                    ui.label(
                        RichText::new(format!("{:.0} \u{d7} {:.0} pt    {zoom:.0}%", s.figure.width, s.figure.height))
                            .weak(),
                    );
                });
            }
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut actions = std::mem::take(&mut self.pending_actions);

        if let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
            actions.push(Action::Open(path));
        }
        self.shortcuts(&ctx, &mut actions);

        // a curve selected on the canvas scrolls into view in the table; one clicked in the table does not move it
        let selection = self.session.as_ref().and_then(|s| s.selection()).map(|p| p.primitive);
        if selection != self.last_selection {
            if selection.is_some() && selection != self.clicked_in_table {
                self.table_scroll_to = selection;
            }
            self.last_selection = selection;
            self.clicked_in_table = None;
        }

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui, &mut actions));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        let panel_frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::same(12));
        egui::Panel::right("side")
            .resizable(false)
            .exact_size(SIDE_PANEL_WIDTH)
            .frame(panel_frame)
            .show(ui, |ui| self.side_panel(ui, &mut actions));
        egui::CentralPanel::default().show(ui, |ui| self.canvas(ui));
        self.about_window(&ctx);

        for action in actions {
            self.run(&ctx, action);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_are_labelled_by_type_and_one_based_id() {
        assert_eq!(curve_label(0, PrimitiveKind::Stroke), "Stroke #1");
        assert_eq!(curve_label(11, PrimitiveKind::Fill), "Fill #12");
    }
}
