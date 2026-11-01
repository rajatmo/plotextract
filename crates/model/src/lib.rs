//! The application model: what the program does in answer to what the user wants, with no window in sight.
//!
//! The panels and the canvas turn mouse and keyboard input into [`Intent`]s. [`Model::dispatch`] carries one out and
//! returns the [`Effect`]s that only the shell can perform (a file dialog, the clipboard, the window title, a
//! background thread), so the whole workflow can be driven and tested without a display.
//!
//! Opening a file can take a while (a big or hostile file may take seconds), so it never runs inside `dispatch`: the
//! model hands the shell a [`LoadJob`] to run on a thread and takes the answer back with [`Model::finish_load`]. The
//! window stays responsive, and an answer that arrives after the user asked for something else is ignored. A document (a
//! PDF) is not opened at once: the job lists its pages and the images they draw, and the one chosen is opened.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use plotextract_algo::lines::Line;
use plotextract_algo::{ColourMap, ColourParams, analyse_colours, decode};
use plotextract_domain::{ArxivId, Bitmap, Edit, ImportError, Layer, OpenOptions, Pick, Prepare, Project, Slot};
use plotextract_ports::papers::{ArxivError, Paper, PaperSource, Progress};
use plotextract_usecases::wizard::{Found, LineSearch, PieceSearch, Wizard, WizardIntent, Work};
use plotextract_usecases::{Opened, Session, SessionError, Source, Workspace, WorkspaceError};

mod arxiv_import;
pub mod jobs;
mod opening;
mod pdf_import;
#[cfg(test)]
mod tests;
pub mod text;
mod tracing;

pub use self::arxiv_import::{ArxivFetched, ArxivImport, ArxivJob, Thumbnail, ThumbnailJob};
pub use self::opening::{LoadJob, Loaded, PasswordPrompt};
pub use self::pdf_import::{Document, Part, PartPicture, PartPictureJob, PdfImport};
pub use self::tracing::{Analysed, AnalysisJob, LineJob, LinesFound, PieceJob, PiecesFound};
use crate::jobs::{Answer, Job};

/// Something the user wants done.
#[derive(Clone, Debug)]
pub enum Intent {
    // files
    RequestOpen,
    RequestLoadProject,
    RequestSaveProject,
    RequestExportCsv,
    Open(PathBuf),
    LoadProject(PathBuf),
    SaveProject(PathBuf),
    WriteCsv(PathBuf),
    CopyNumbers,
    /// The password the user typed for the file that asked for one.
    SubmitPassword(String),
    CancelPassword,
    /// Show the window that imports a figure of an arXiv paper.
    RequestArxiv,
    /// Download the paper the text names (its number or a link) and unpack its figures.
    FetchArxiv(String),
    /// Open one of the paper's figures, and close the window.
    ImportFigure(PathBuf),
    CloseArxiv,
    /// Open a page or an image of the document listed, and close the window.
    ImportPart(Part),
    ClosePdf,
    // the document
    Edit(Edit),
    Undo,
    Redo,
    /// Store the selected point in the current calibration row.
    Record,
    /// Make a calibration row the current one and store the selected point in it.
    RecordAt(Slot),
    SelectSlot(Slot),
    // the view
    /// A change in the tracing wizard.
    Wizard(WizardIntent),
    Select(Option<Pick>),
    SetLayer(Layer, bool),
    FitView,
    /// Zoom and pan the view to show a part of the image, in image pixels.
    ShowArea(kurbo::Rect),
    ToggleAbout,
    Quit,
}

/// What the shell has to do after an intent: the parts that need a dialog, the clipboard, the window or a thread.
#[derive(Debug)]
pub enum Effect {
    AskOpenPath,
    AskProjectToLoad,
    AskProjectSavePath {
        suggested: String,
    },
    AskCsvPath {
        suggested: String,
    },
    CopyText(String),
    /// Run a job on another thread and give its answer to [`Model::finish`].
    Run(Job),
    /// Run jobs on a few threads that take them in turn (the pictures of what an import window lists).
    RunBatch(Vec<Job>),
    /// A different figure is now shown: reset what is tied to the old one. `source` draws its pictures,
    /// unless the file is too large to draw.
    Opened {
        title: String,
        source: Option<Source>,
    },
    /// The calibration changed in a way the typed values do not reflect (undo, a loaded project, a cleared calibration).
    ValuesChanged,
    FitView,
    ShowArea(kurbo::Rect),
    ToggleAbout,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

/// A line for the status bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub text: String,
    pub level: Level,
}

pub struct Model {
    workspace: Arc<Workspace>,
    session: Option<Session>,
    /// What the open figure was read from: its pixels, for the wizard to look at.
    source: Option<Source>,
    slot: Slot,
    message: Option<Message>,
    /// Set while an image is being prepared for tracing.
    wizard: Option<Wizard>,
    next_job: u64,
    /// The job whose answer the model waits for, and the name to show while it runs.
    loading: Option<(u64, String)>,
    password: Option<PasswordPrompt>,
    arxiv: ArxivImport,
    pdf: PdfImport,
    /// Where papers come from.
    papers: Arc<dyn PaperSource>,
}

impl Model {
    /// A model that opens files with `workspace` and gets papers from `papers`: the ports, chosen by the composition
    /// root.
    pub fn new(workspace: Workspace, papers: Arc<dyn PaperSource>) -> Self {
        Self {
            workspace: Arc::new(workspace),
            session: None,
            source: None,
            slot: Slot::XMin,
            message: None,
            wizard: None,
            next_job: 0,
            loading: None,
            password: None,
            arxiv: ArxivImport::default(),
            pdf: PdfImport::default(),
            papers,
        }
    }

    /// The Import from arXiv window, while it is open.
    pub fn arxiv(&self) -> Option<&ArxivImport> {
        self.arxiv.open.then_some(&self.arxiv)
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// The calibration row that Record fills next.
    pub fn slot(&self) -> Slot {
        self.slot
    }

    /// The tracing wizard, while an image that needs tracing is open.
    pub fn wizard(&self) -> Option<&Wizard> {
        self.wizard.as_ref()
    }

    pub fn message(&self) -> Option<&Message> {
        self.message.as_ref()
    }

    /// The name of the file being opened, while it is.
    pub fn loading(&self) -> Option<&str> {
        self.loading.as_ref().map(|(_, name)| name.as_str())
    }

    /// The file waiting for a password, if any.
    pub fn password_prompt(&self) -> Option<&PasswordPrompt> {
        self.password.as_ref()
    }

    /// The file extensions that can be opened.
    pub fn extensions(&self) -> Vec<&'static str> {
        self.workspace.registry().extensions()
    }

    /// Carry out one intent. Failures become the status message; the returned effects are for the shell.
    pub fn dispatch(&mut self, intent: Intent) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.handle(intent, &mut effects);
        effects
    }

    /// Take the answer of a job; what it asks for next is returned. (A figure's picture is the window's, not the model's.)
    pub fn finish(&mut self, answer: Answer) -> Vec<Effect> {
        match answer {
            Answer::Loaded(loaded) => self.finish_load(*loaded),
            Answer::Analysed(analysed) => {
                self.finish_analysis(analysed);
                // work brought back from a project goes on from the colours to the lines and the pieces
                let mut fx = Vec::new();
                self.start_wizard_jobs(&mut fx);
                fx
            }
            Answer::Lines(found) => self.finish_lines(found),
            Answer::Pieces(found) => self.finish_pieces(*found),
            Answer::Arxiv(fetched) => self.finish_arxiv(*fetched),
            Answer::Thumbnail(_) | Answer::PartPicture(_) => Vec::new(),
        }
    }

    #[cfg(test)]
    /// Run the jobs among `effects` here and now, and return the rest, together with what the answers asked for. For
    /// drivers that have no thread to put them on (the tests, a command line).
    pub fn run_jobs(&mut self, effects: Vec<Effect>) -> Vec<Effect> {
        let mut out = Vec::new();
        let mut todo = effects;
        while !todo.is_empty() {
            let mut next = Vec::new();
            for effect in todo {
                match effect {
                    Effect::Run(job) => next.extend(self.finish(job.run())),
                    other => out.push(other),
                }
            }
            todo = next;
        }
        out
    }

    fn handle(&mut self, intent: Intent, fx: &mut Vec<Effect>) {
        match intent {
            Intent::RequestOpen => fx.push(Effect::AskOpenPath),
            Intent::RequestLoadProject => fx.push(Effect::AskProjectToLoad),
            Intent::RequestSaveProject => {
                if let Some(s) = &self.session {
                    fx.push(Effect::AskProjectSavePath { suggested: Workspace::project_file_name(s) });
                }
            }
            Intent::RequestExportCsv => {
                if self.active_csv().is_some() {
                    fx.push(Effect::AskCsvPath { suggested: format!("{}.csv", self.active_name()) });
                }
            }
            Intent::Open(path) => self.open(path, fx),
            Intent::LoadProject(path) => self.load_project(&path, fx),
            Intent::SaveProject(path) => {
                if let Some(s) = &self.session {
                    match self.workspace.save_project(s, self.wizard.as_ref(), &path) {
                        Ok(()) => self.info(format!("Saved {}", path.display())),
                        Err(e) => self.error(text::workspace_error(&e)),
                    }
                }
            }
            Intent::WriteCsv(path) => {
                let Some(csv) = self.active_csv() else { return };
                match self.workspace.write_csv(&path, csv) {
                    Ok(()) => self.info(format!("Wrote {}", path.display())),
                    Err(e) => self.error(text::workspace_error(&e)),
                }
            }
            Intent::CopyNumbers => {
                if let Some(csv) = self.active_csv() {
                    fx.push(Effect::CopyText(csv));
                    self.info("Copied the series as x,y text.");
                }
            }
            Intent::SubmitPassword(password) => {
                if let Some(prompt) = self.password.take() {
                    self.start(prompt.retry.with_password(password), prompt.project, fx);
                }
            }
            Intent::CancelPassword => self.password = None,
            Intent::RequestArxiv => {
                self.pdf.open = false;
                self.arxiv.open = true;
            }
            Intent::FetchArxiv(text) => self.fetch_arxiv(&text, fx),
            Intent::ImportFigure(path) => {
                self.arxiv.open = false;
                self.open(path, fx);
            }
            Intent::CloseArxiv => self.arxiv.open = false,
            Intent::ImportPart(part) => self.import_part(part, fx),
            Intent::ClosePdf => self.pdf.open = false,
            Intent::Edit(edit) => {
                let clears = matches!(edit, Edit::ClearCalibration);
                if self.with_session(|s| s.edit(edit)) && clears {
                    fx.push(Effect::ValuesChanged);
                }
            }
            Intent::Wizard(change) => {
                if let Some(w) = &mut self.wizard {
                    w.apply(change);
                }
                self.start_wizard_jobs(fx);
            }
            Intent::Undo if self.wizard.is_some() => {
                if let Some(w) = &mut self.wizard {
                    w.undo();
                }
                self.start_wizard_jobs(fx);
            }
            Intent::Redo if self.wizard.is_some() => {
                if let Some(w) = &mut self.wizard {
                    w.redo();
                }
                self.start_wizard_jobs(fx);
            }
            Intent::Undo => {
                if self.session.as_mut().is_some_and(Session::undo) {
                    fx.push(Effect::ValuesChanged);
                }
            }
            Intent::Redo => {
                if self.session.as_mut().is_some_and(Session::redo) {
                    fx.push(Effect::ValuesChanged);
                }
            }
            Intent::Record => self.record(self.slot),
            Intent::RecordAt(slot) => self.record(slot),
            Intent::SelectSlot(slot) => self.slot = slot,
            Intent::Select(pick) => {
                self.with_session(|s| s.select(pick));
            }
            Intent::SetLayer(layer, on) => {
                if let Some(s) = &mut self.session {
                    s.set_layer(layer, on);
                }
            }
            Intent::FitView => fx.push(Effect::FitView),
            Intent::ShowArea(area) => fx.push(Effect::ShowArea(area)),
            Intent::ToggleAbout => fx.push(Effect::ToggleAbout),
            Intent::Quit => fx.push(Effect::Quit),
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), level: Level::Info });
    }

    fn error(&mut self, text: impl Into<String>) {
        self.message = Some(Message { text: text.into(), level: Level::Error });
    }

    /// Run a change on the session; a refusal becomes the status message. Whether it went through.
    fn with_session(&mut self, change: impl FnOnce(&mut Session) -> Result<(), SessionError>) -> bool {
        let Some(session) = &mut self.session else { return false };
        match change(session) {
            Ok(()) => true,
            Err(e) => {
                self.error(text::session_error(&e));
                false
            }
        }
    }

    fn record(&mut self, slot: Slot) {
        if self.with_session(|s| s.record_selected(slot)) {
            self.slot = slot.next();
            self.info(format!("{} recorded. Type its plot value in the table.", text::slot_label(slot)));
        }
    }

    fn active_name(&self) -> String {
        if self.wizard.is_some() {
            return "points".into();
        }
        self.session
            .as_ref()
            .and_then(|s| s.active_series().map(|a| s.series()[a].name.clone()))
            .unwrap_or_else(|| "series".into())
    }

    /// The active series as CSV (while an image is traced, the points of its links); why not becomes the status message.
    fn active_csv(&mut self) -> Option<String> {
        if let Some(w) = &self.wizard {
            return match w.links_csv() {
                Ok(csv) => Some(csv),
                Err(e) => {
                    self.error(text::links_error(&e));
                    None
                }
            };
        }
        let session = self.session.as_ref()?;
        let result = match session.active_series() {
            Some(active) => self.workspace.csv(session, active),
            None => Err(SessionError::Export(plotextract_domain::ExportError::NoSuchSeries).into()),
        };
        match result {
            Ok(csv) => Some(csv),
            Err(e) => {
                self.error(text::workspace_error(&e));
                None
            }
        }
    }
}
