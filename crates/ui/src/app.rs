//! The egui shell: it owns the model and the widgets' state, gathers what the user did into intents each frame, hands
//! them to the model and performs the effects that need a dialog, the clipboard, the window or a thread.

use std::collections::VecDeque;
use std::path::PathBuf;

use egui::{Align, Color32, Context, Key, Layout, Modifiers, RichText, Ui, vec2};
use plotextract_domain::format_g;

use crate::arxiv_window::ArxivWindow;
use crate::canvas::{self, CanvasState};
use crate::panel::{self, PanelState};
use crate::pdf_window::PdfWindow;
use crate::wizard_panel::WizardPanel;
use crate::{about, theme};
use plotextract_model::jobs::{Answer, Runner};
use plotextract_model::text::{curve_label, measure};
use plotextract_model::{Effect, Intent, Level, Model};

/// The side panel's width, in points. It does not resize: the cards inside are laid out for it.
const SIDE_PANEL_WIDTH: f32 = 340.0;
/// The widths of the wizard's panels: the masks on the left, the step on the right.
const MASKS_PANEL_WIDTH: f32 = 290.0;
const STEP_PANEL_WIDTH: f32 = 320.0;

pub struct App {
    model: Model,
    canvas: CanvasState,
    panel: PanelState,
    wizard_panel: WizardPanel,
    icon: egui::TextureHandle,
    show_about: bool,
    /// Intents raised by widgets drawn after the frame's intents were run (the welcome screen), run next frame.
    deferred: Vec<Intent>,
    /// The slow work the model asked for, running on other threads.
    jobs: Runner,
    arxiv_window: ArxivWindow,
    pdf_window: PdfWindow,
    /// What is typed in the password prompt.
    password: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, model: Model, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
            .expect("the bundled icon is a valid PNG");
        let image = egui::ColorImage::from_rgba_unmultiplied([icon.width as usize, icon.height as usize], &icon.rgba);
        let icon = cc.egui_ctx.load_texture("app icon", image, egui::TextureOptions::LINEAR);

        let mut app = Self {
            model,
            canvas: CanvasState::default(),
            panel: PanelState::default(),
            wizard_panel: WizardPanel::default(),
            icon,
            show_about: false,
            deferred: Vec::new(),
            jobs: Runner::default(),
            arxiv_window: ArxivWindow::default(),
            pdf_window: PdfWindow::default(),
            password: String::new(),
        };
        if let Some(path) = path {
            app.run(&cc.egui_ctx, vec![Intent::Open(path)]);
        }
        app
    }

    /// Hand intents to the model and perform what it asks of the shell. Dialogs answer with further intents.
    fn run(&mut self, ctx: &Context, intents: Vec<Intent>) {
        let mut queue = VecDeque::from(intents);
        while let Some(intent) = queue.pop_front() {
            for effect in self.model.dispatch(intent) {
                self.perform(ctx, effect, &mut queue);
            }
        }
    }

    /// Give the answers of the jobs that finished to the model (a picture to the import window that asked for it), and do
    /// what they ask for next.
    fn receive_answers(&mut self, ctx: &Context) {
        for answer in self.jobs.answers() {
            let answer = match answer {
                Answer::Thumbnail(thumbnail) => {
                    self.arxiv_window.receive(ctx, thumbnail);
                    continue;
                }
                Answer::PartPicture(picture) => {
                    self.pdf_window.receive(ctx, picture);
                    continue;
                }
                other => other,
            };
            let mut queue = VecDeque::new();
            for effect in self.model.finish(answer) {
                self.perform(ctx, effect, &mut queue);
            }
            while let Some(intent) = queue.pop_front() {
                self.run(ctx, vec![intent]);
            }
        }
    }

    fn perform(&mut self, ctx: &Context, effect: Effect, then: &mut VecDeque<Intent>) {
        match effect {
            Effect::AskOpenPath => {
                let dialog = rfd::FileDialog::new().add_filter("Plots", &self.model.extensions());
                then.extend(dialog.pick_file().map(Intent::Open));
            }
            Effect::AskProjectToLoad => {
                let dialog = rfd::FileDialog::new().add_filter("plotextract project", &["json"]);
                then.extend(dialog.pick_file().map(Intent::LoadProject));
            }
            Effect::AskProjectSavePath { suggested } => {
                let dialog =
                    rfd::FileDialog::new().set_file_name(suggested).add_filter("plotextract project", &["json"]);
                then.extend(dialog.save_file().map(Intent::SaveProject));
            }
            Effect::AskCsvPath { suggested } => {
                let dialog = rfd::FileDialog::new().set_file_name(suggested).add_filter("CSV", &["csv"]);
                then.extend(dialog.save_file().map(Intent::WriteCsv));
            }
            Effect::CopyText(text) => ctx.copy_text(text),
            Effect::Run(job) => {
                let ctx = ctx.clone();
                self.jobs.run(job, move || ctx.request_repaint());
            }
            Effect::RunBatch(jobs) => {
                let ctx = ctx.clone();
                self.jobs.run_batch(jobs, move || ctx.request_repaint());
            }
            Effect::Opened { title, source } => {
                if let Some(session) = self.model.session() {
                    self.canvas.open(ctx, &session.figure, source);
                }
                self.panel = PanelState::default();
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            }
            Effect::ValuesChanged => {
                if let Some(session) = self.model.session() {
                    self.panel.sync_values(session);
                }
            }
            Effect::FitView => self.canvas.view.request_fit(),
            Effect::ShowArea(area) => {
                if let Some(s) = self.model.session() {
                    self.canvas.view.show(area, (s.figure.width, s.figure.height));
                }
            }
            Effect::ToggleAbout => self.show_about = !self.show_about,
            Effect::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }

    fn shortcuts(&self, ctx: &Context, intents: &mut Vec<Intent>) {
        let take = |mods: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(mods, key));
        if take(Modifiers::COMMAND, Key::O) {
            intents.push(Intent::RequestOpen);
        }
        if take(Modifiers::COMMAND, Key::S) {
            intents.push(Intent::RequestSaveProject);
        }
        if take(Modifiers::COMMAND, Key::R) {
            intents.push(Intent::Record);
        }
        if take(Modifiers::COMMAND, Key::Num0) {
            intents.push(Intent::FitView);
        }
        if take(Modifiers::NONE, Key::F1) {
            intents.push(Intent::ToggleAbout);
        }
        if take(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || take(Modifiers::COMMAND, Key::Y) {
            intents.push(Intent::Redo);
        } else if take(Modifiers::COMMAND, Key::Z) {
            intents.push(Intent::Undo);
        }
    }

    fn menu(&self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let session = self.model.session();
        let has_figure = session.is_some();
        let mut item = |ui: &mut Ui, enabled: bool, text: &str, shortcut: &str, intent: Intent| {
            let button = egui::Button::new(text).shortcut_text(shortcut);
            if ui.add_enabled(enabled, button).clicked() {
                intents.push(intent);
                ui.close();
            }
        };
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                item(ui, true, "Open\u{2026}", "Ctrl+O", Intent::RequestOpen);
                item(ui, true, "Import from arXiv\u{2026}", "", Intent::RequestArxiv);
                ui.separator();
                item(ui, true, "Open project\u{2026}", "", Intent::RequestLoadProject);
                item(ui, has_figure, "Save project\u{2026}", "Ctrl+S", Intent::RequestSaveProject);
                ui.separator();
                item(ui, true, "Quit", "", Intent::Quit);
            });
            ui.menu_button("Edit", |ui| {
                let (undo, redo) = session.map_or((false, false), |s| (s.can_undo(), s.can_redo()));
                item(ui, undo, "Undo", "Ctrl+Z", Intent::Undo);
                item(ui, redo, "Redo", "Ctrl+Shift+Z", Intent::Redo);
            });
            ui.menu_button("View", |ui| {
                item(ui, has_figure, "Fit page", "Ctrl+0", Intent::FitView);
            });
            ui.menu_button("Help", |ui| {
                item(ui, true, "About plotextract", "F1", Intent::ToggleAbout);
            });
        });
    }

    fn status_bar(&self, ui: &mut Ui) {
        let session = self.model.session();
        // the size and zoom sit at the right and keep their room; the message gets the rest and is cut to fit
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(s) = session {
                let zoom = f64::from(self.canvas.view.zoom) * 100.0;
                let figure = &s.figure;
                ui.label(
                    RichText::new(format!(
                        "{:.0} \u{d7} {:.0} {}    {zoom:.0}%",
                        figure.width,
                        figure.height,
                        figure.unit.symbol()
                    ))
                    .weak(),
                );
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                if let Some(name) = self.model.loading() {
                    ui.spinner();
                    ui.label(format!("Opening {name}\u{2026}"));
                } else if let (Some(hit), Some(s)) = (self.canvas.hover(), session) {
                    let p = &s.figure.primitives[hit.primitive];
                    let at = format!("{}, point {}", curve_label(hit.primitive, p.kind), hit.vertex + 1);
                    let readout = match s.plot_coords(hit.pos) {
                        Some((x, y)) => format!("x = {}    y = {}", format_g(x, 6), format_g(y, 6)),
                        None => {
                            let unit = s.figure.unit;
                            format!(
                                "x = {}    y = {}    (not calibrated)",
                                measure(hit.pos.x, unit, 3),
                                measure(hit.pos.y, unit, 3)
                            )
                        }
                    };
                    ui.label(RichText::new(at).strong());
                    ui.label(RichText::new(readout).monospace());
                } else if let Some(message) = self.model.message() {
                    let color = match message.level {
                        Level::Error => Color32::from_rgb(214, 70, 70),
                        Level::Warning => Color32::from_rgb(214, 140, 20),
                        Level::Info => ui.visuals().text_color(),
                    };
                    ui.add(egui::Label::new(RichText::new(&message.text).color(color)).truncate())
                        .on_hover_text(&message.text);
                } else {
                    ui.label(RichText::new("Open a plot to begin").weak());
                }
            });
        });
    }

    /// The screen shown before a file is open.
    fn welcome(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.28).max(20.0));
            ui.image((self.icon.id(), vec2(104.0, 104.0)));
            ui.add_space(6.0);
            ui.label(RichText::new("plotextract").heading().strong().size(28.0));
            ui.label(RichText::new("Get the numbers out of a plot").weak());
            ui.add_space(14.0);
            if ui.add_sized([180.0, 34.0], theme::primary("Open a plot\u{2026}")).clicked() {
                self.deferred.push(Intent::RequestOpen);
                ui.ctx().request_repaint();
            }
            ui.add_space(4.0);
            if ui.add_sized([180.0, 30.0], egui::Button::new("Import from arXiv\u{2026}")).clicked() {
                self.deferred.push(Intent::RequestArxiv);
                ui.ctx().request_repaint();
            }
            ui.add_space(8.0);
            theme::hint(ui, "or drop a file here");
        });
    }

    /// Ask for the password of a protected file. Returns what the user decided.
    fn password_prompt(&mut self, ctx: &Context, intents: &mut Vec<Intent>) {
        let Some(prompt) = self.model.password_prompt() else {
            self.password.clear();
            return;
        };
        let (file, wrong) = (prompt.file.clone(), prompt.wrong);
        let mut submit = false;
        let mut cancel = ctx.input(|i| i.key_pressed(Key::Escape));
        egui::Window::new("Password required")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(320.0);
                ui.label(format!("{file} is protected. Type its password to open it."));
                ui.add_space(6.0);
                let field = egui::TextEdit::singleline(&mut self.password).password(true).desired_width(f32::INFINITY);
                let response = ui.add(field);
                response.request_focus();
                // the field keeps the focus, so Enter is read directly rather than as the field giving it up
                if response.has_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    submit = true;
                }
                if wrong {
                    ui.add_space(2.0);
                    ui.label(RichText::new("That password is wrong.").color(Color32::from_rgb(214, 70, 70)));
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!self.password.is_empty(), theme::primary("Open")).clicked() {
                        submit = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if submit && !self.password.is_empty() {
            intents.push(Intent::SubmitPassword(std::mem::take(&mut self.password)));
        } else if cancel {
            self.password.clear();
            intents.push(Intent::CancelPassword);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_answers(&ctx);
        let mut intents = std::mem::take(&mut self.deferred);

        if let Some(path) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf())) {
            intents.push(Intent::Open(path));
        }
        self.shortcuts(&ctx, &mut intents);
        self.panel.track_selection(self.model.session().and_then(|s| s.selection()).map(|p| p.primitive));

        egui::Panel::top("menu").show(ui, |ui| self.menu(ui, &mut intents));
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui));
        let panel_frame = egui::Frame::side_top_panel(ui.style()).inner_margin(egui::Margin::same(12));
        match self.model.wizard() {
            // tracing an image: the masks made so far on the left, the step on the right
            Some(wizard) => {
                let lit = self.canvas.hovered_ink();
                let mut hover = None;
                egui::Panel::left("masks").resizable(false).exact_size(MASKS_PANEL_WIDTH).frame(panel_frame).show(
                    ui,
                    |ui| {
                        let out = self.wizard_panel.masks(ui, wizard, lit);
                        hover = out.row_hover;
                        intents.extend(out.intents);
                    },
                );
                egui::Panel::right("step").resizable(false).exact_size(STEP_PANEL_WIDTH).frame(panel_frame).show(
                    ui,
                    |ui| {
                        let out = self.wizard_panel.step(ui, wizard, lit);
                        hover = hover.or(out.row_hover);
                        intents.extend(out.intents);
                    },
                );
                // the mask whose row the pointer is on is outlined on the image
                self.canvas.set_panel_hover(hover);
            }
            None => {
                egui::Panel::right("side")
                    .resizable(false)
                    .exact_size(SIDE_PANEL_WIDTH)
                    .frame(panel_frame)
                    .show(ui, |ui| intents.extend(panel::show(ui, &self.model, &mut self.panel)));
            }
        }
        egui::CentralPanel::default().show(ui, |ui| match self.model.session() {
            Some(session) => intents.extend(canvas::show(ui, session, &mut self.canvas, self.model.wizard())),
            None => self.welcome(ui),
        });
        about::window(&ctx, &mut self.show_about, self.icon.id());
        self.arxiv_window.show(&ctx, self.model.arxiv(), &mut intents);
        self.pdf_window.show(&ctx, self.model.pdf(), &mut intents);
        self.password_prompt(&ctx, &mut intents);

        if !intents.is_empty() {
            self.run(&ctx, intents);
            ctx.request_repaint();
        }
    }
}
