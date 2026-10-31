//! The application model: what the program does in answer to what the user wants, with no window in sight.
//!
//! The panels and the canvas turn mouse and keyboard input into [`Intent`]s. [`Model::dispatch`] carries one out and
//! returns the [`Effect`]s that only the shell can perform (a file dialog, the clipboard, the window title, a
//! background thread), so the whole workflow can be driven and tested without a display.
//!
//! Opening a file can take a while (a big or hostile file may take seconds), so it never runs inside `dispatch`: the
//! model hands the shell a [`LoadJob`] to run on a thread and takes the answer back with [`Model::finish_load`]. The
//! window stays responsive, and an answer that arrives after the user asked for something else is ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use plotextract_core::{Edit, ImportError, Layer, OpenOptions, Pick, Project, Session, SessionError, Slot};
use plotextract_workspace::{Opened, Source, Workspace, WorkspaceError};

use crate::text;

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
    /// Show another page of the open file, counting from 0.
    GoToPage(usize),
    /// The password the user typed for the file that asked for one.
    SubmitPassword(String),
    CancelPassword,
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
    Select(Option<Pick>),
    SetLayer(Layer, bool),
    FitView,
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
    /// Run a job on a background thread and give its [`Loaded`] to [`Model::finish_load`].
    Load(Box<LoadJob>),
    /// A different figure (or page) is now shown: reset what is tied to the old one. `source` draws its pictures,
    /// unless the file is too large to draw.
    Opened {
        title: String,
        source: Option<Source>,
    },
    /// The calibration changed in a way the typed values do not reflect (undo, a loaded project, a cleared calibration).
    ValuesChanged,
    FitView,
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

/// What a load reads.
#[derive(Clone)]
enum What {
    /// A file on disk.
    File { path: PathBuf, options: OpenOptions },
    /// A file already read, at another page or with a password.
    Source { source: Source, name: Option<String> },
}

impl What {
    fn name(&self) -> String {
        match self {
            What::File { path, .. } => file_name(path),
            What::Source { name, .. } => name.as_deref().map(|n| file_name(Path::new(n))).unwrap_or_default(),
        }
    }

    fn with_password(&self, password: String) -> What {
        match self {
            What::File { path, options } => {
                What::File { path: path.clone(), options: OpenOptions { password: Some(password), ..options.clone() } }
            }
            What::Source { source, name } => {
                What::Source { source: source.with_password(password), name: name.clone() }
            }
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// A file to open, and what to do once it is open. Run it anywhere with [`LoadJob::run`].
pub struct LoadJob {
    id: u64,
    workspace: Arc<Workspace>,
    what: What,
    /// A project to apply to the figure once it is open.
    project: Option<Project>,
}

impl std::fmt::Debug for LoadJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadJob").field("id", &self.id).field("file", &self.what.name()).finish_non_exhaustive()
    }
}

impl LoadJob {
    /// The slow part: read and interpret the file. Safe to call on any thread.
    pub fn run(self) -> Loaded {
        let result = match &self.what {
            What::File { path, options } => self.workspace.open(path, options.clone()),
            What::Source { source, name } => self.workspace.load(source.clone(), name.clone()),
        };
        Loaded { id: self.id, what: self.what, project: self.project, result }
    }
}

/// The answer to a [`LoadJob`].
pub struct Loaded {
    id: u64,
    what: What,
    project: Option<Project>,
    result: Result<Opened, WorkspaceError>,
}

/// A file that needs a password before it can be opened.
pub struct PasswordPrompt {
    /// The file's name, for the prompt.
    pub file: String,
    /// The last password was wrong.
    pub wrong: bool,
    retry: What,
    project: Option<Project>,
}

pub struct Model {
    workspace: Arc<Workspace>,
    session: Option<Session>,
    /// What the open figure was read from, for other pages of the same file.
    source: Option<Source>,
    /// The sessions of the other pages of the open file that the user worked on, so coming back finds the work.
    parked: BTreeMap<usize, (Session, Source)>,
    slot: Slot,
    message: Option<Message>,
    next_job: u64,
    /// The job whose answer the model waits for, and the name to show while it runs.
    loading: Option<(u64, String)>,
    password: Option<PasswordPrompt>,
}

impl Default for Model {
    fn default() -> Self {
        Self::new(Workspace::default())
    }
}

impl Model {
    pub fn new(workspace: Workspace) -> Self {
        Self {
            workspace: Arc::new(workspace),
            session: None,
            source: None,
            parked: BTreeMap::new(),
            slot: Slot::XMin,
            message: None,
            next_job: 0,
            loading: None,
            password: None,
        }
    }

    pub fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// The calibration row that Record fills next.
    pub fn slot(&self) -> Slot {
        self.slot
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

    /// Take the answer of a [`LoadJob`]. An answer to a job that is no longer the latest is dropped.
    pub fn finish_load(&mut self, loaded: Loaded) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.loading.as_ref().map(|(id, _)| *id) != Some(loaded.id) {
            return fx;
        }
        self.loading = None;
        let Loaded { what, project, result, .. } = loaded;
        match result {
            Ok(opened) => {
                self.password = None;
                let wanted = project.as_ref().map(|p| p.page);
                self.install(opened, &mut fx);
                if let (Some(project), Some(s)) = (project, &mut self.session) {
                    s.load_project(project);
                    fx.push(Effect::ValuesChanged);
                    debug_assert!(wanted.is_none_or(|p| p == s.figure.page));
                }
                self.announce();
            }
            Err(WorkspaceError::Import(e @ (ImportError::PasswordRequired | ImportError::WrongPassword))) => {
                let wrong = e == ImportError::WrongPassword;
                self.password = Some(PasswordPrompt { file: what.name(), wrong, retry: what, project });
                self.message = None;
            }
            Err(e) => self.error(text::workspace_error(&e)),
        }
        fx
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
                    Effect::Load(job) => next.extend(self.finish_load(job.run())),
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
            Intent::Open(path) => {
                self.start(What::File { path, options: OpenOptions::default() }, None, fx);
            }
            Intent::LoadProject(path) => self.load_project(&path, fx),
            Intent::SaveProject(path) => {
                if let Some(s) = &self.session {
                    match self.workspace.save_project(s, &path) {
                        Ok(()) => self.info(format!("Saved {}", path.display())),
                        Err(e) => self.error(text::workspace_error(&e)),
                    }
                }
            }
            Intent::WriteCsv(path) => {
                let Some(active) = self.session.as_ref().and_then(Session::active_series) else { return };
                if let Some(s) = &self.session {
                    match self.workspace.export_csv(s, active, &path) {
                        Ok(()) => self.info(format!("Wrote {}", path.display())),
                        Err(e) => self.error(text::workspace_error(&e)),
                    }
                }
            }
            Intent::CopyNumbers => {
                if let Some(csv) = self.active_csv() {
                    fx.push(Effect::CopyText(csv));
                    self.info("Copied the series as x,y text.");
                }
            }
            Intent::GoToPage(page) => self.go_to_page(page, fx),
            Intent::SubmitPassword(password) => {
                if let Some(prompt) = self.password.take() {
                    self.start(prompt.retry.with_password(password), prompt.project, fx);
                }
            }
            Intent::CancelPassword => self.password = None,
            Intent::Edit(edit) => {
                let clears = matches!(edit, Edit::ClearCalibration);
                if self.with_session(|s| s.edit(edit)) && clears {
                    fx.push(Effect::ValuesChanged);
                }
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
        self.session
            .as_ref()
            .and_then(|s| s.active_series().map(|a| s.series()[a].name.clone()))
            .unwrap_or_else(|| "series".into())
    }

    /// The active series as CSV; why not becomes the status message.
    fn active_csv(&mut self) -> Option<String> {
        let session = self.session.as_ref()?;
        let result = match session.active_series() {
            Some(active) => self.workspace.csv(session, active),
            None => Err(SessionError::Export(plotextract_core::ExportError::NoSuchSeries).into()),
        };
        match result {
            Ok(csv) => Some(csv),
            Err(e) => {
                self.error(text::workspace_error(&e));
                None
            }
        }
    }

    // --- opening ---------------------------------------------------------------------------------------------

    /// Ask the shell to open something. A job still running is made stale: only the newest one counts.
    fn start(&mut self, what: What, project: Option<Project>, fx: &mut Vec<Effect>) {
        self.next_job += 1;
        let id = self.next_job;
        self.loading = Some((id, what.name()));
        self.password = None;
        fx.push(Effect::Load(Box::new(LoadJob { id, workspace: self.workspace.clone(), what, project })));
    }

    /// Make an opened figure the one shown. What was open before is kept only if it is another page of the same file
    /// and the user worked on it.
    fn install(&mut self, opened: Opened, fx: &mut Vec<Effect>) {
        let same_file =
            self.source.is_some() && self.session.as_ref().map(|s| &s.source) == Some(&opened.session.source);
        if let (true, Some(old), Some(source)) = (same_file, self.session.take(), self.source.take()) {
            if old.has_work() {
                self.parked.insert(old.figure.page, (old, source));
            }
        } else {
            self.parked.clear();
        }
        self.source = Some(opened.source().clone());
        let pictures = opened.picture_source();
        self.show(opened.session, pictures, fx);
    }

    fn show(&mut self, session: Session, pictures: Option<Source>, fx: &mut Vec<Effect>) {
        let name = session.source.as_deref().map(|s| file_name(Path::new(s))).unwrap_or_default();
        self.session = Some(session);
        self.slot = Slot::XMin;
        fx.push(Effect::Opened { title: format!("{name} \u{2013} plotextract"), source: pictures });
        fx.push(Effect::ValuesChanged);
    }

    fn announce(&mut self) {
        let Some(s) = &self.session else { return };
        let name = s.source.as_deref().map(|p| file_name(Path::new(p))).unwrap_or_default();
        let f = &s.figure;
        let place = if f.page_count > 1 { format!("{name}, page {} of {}", f.page + 1, f.page_count) } else { name };
        let hint = "Hold Ctrl and click a curve to select it.";
        if f.truncated {
            let n = f.primitives.len();
            self.message = Some(Message {
                text: format!(
                    "Opened {place}: it holds more paths than plotextract keeps, so only the first {n} are shown. {hint}"
                ),
                level: Level::Warning,
            });
        } else {
            self.info(format!("Opened {place}: {} curves. {hint}", f.primitives.len()));
        }
    }

    fn go_to_page(&mut self, page: usize, fx: &mut Vec<Effect>) {
        let (Some(session), Some(source)) = (&self.session, &self.source) else { return };
        let count = session.figure.page_count;
        if page >= count {
            return self.error(text::workspace_error(&ImportError::NoSuchPage { page, count }.into()));
        }
        if page == session.figure.page {
            return;
        }
        if let Some((parked, parked_source)) = self.parked.remove(&page) {
            // back to a page that has work on it
            let pictures = (!parked.figure.truncated).then(|| parked_source.clone());
            if let (Some(old), Some(old_source)) = (self.session.take(), self.source.take())
                && old.has_work()
            {
                self.parked.insert(old.figure.page, (old, old_source));
            }
            self.source = Some(parked_source);
            self.show(parked, pictures, fx);
            self.announce();
            return;
        }
        let name = session.source.clone();
        let what = What::Source { source: source.with_page(page), name };
        self.start(what, None, fx);
    }

    fn load_project(&mut self, path: &Path, fx: &mut Vec<Effect>) {
        let project = match self.workspace.read_project(path) {
            Ok(p) => p,
            Err(e) => return self.error(text::workspace_error(&e)),
        };
        let open_source = self.session.as_ref().and_then(|s| s.source.clone());
        let open_page = self.session.as_ref().map(|s| s.figure.page);
        match project.source.clone() {
            Some(source) if open_source.as_deref() != Some(source.as_str()) || open_page != Some(project.page) => {
                let options = OpenOptions { page: project.page, ..Default::default() };
                self.start(What::File { path: PathBuf::from(source), options }, Some(project), fx);
                self.info(format!("Loading {}", path.display()));
            }
            _ => match &mut self.session {
                Some(s) => {
                    s.load_project(project);
                    fx.push(Effect::ValuesChanged);
                    self.info(format!("Loaded {}", path.display()));
                }
                None => self.error("The project's figure could not be opened."),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use plotextract_core::Axis;

    use super::*;

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
        <polyline points="10,90 50,50 90,10" fill="none" stroke="#000" stroke-width="1"/></svg>"##;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("plotextract-app-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn svg(&self) -> PathBuf {
            let path = self.0.join("fig.svg");
            std::fs::write(&path, SVG).unwrap();
            path
        }

        /// A PDF with `count` pages of 100 x 100 points; page k draws a line of length 10 * (k + 1).
        fn pdf(&self, count: usize) -> PathBuf {
            let kids = (0..count).map(|k| format!("{} 0 R", 3 + 2 * k)).collect::<Vec<_>>().join(" ");
            let mut objects = vec![
                "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
                format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>"),
            ];
            for k in 0..count {
                objects.push(format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents {} 0 R >>",
                    4 + 2 * k
                ));
                let content = format!("10 10 m {} 10 l S", 10 * (k + 1) + 10);
                objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
            }
            let mut out = String::from("%PDF-1.7\n");
            let mut offsets = Vec::new();
            for (n, body) in objects.iter().enumerate() {
                offsets.push(out.len());
                out += &format!("{} 0 obj\n{body}\nendobj\n", n + 1);
            }
            let xref = out.len();
            out += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
            for o in offsets {
                out += &format!("{o:010} 00000 n \n");
            }
            out += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
            let path = self.0.join("book.pdf");
            std::fs::write(&path, out).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    /// Dispatch an intent and carry out the jobs it starts.
    fn run(m: &mut Model, intent: Intent) -> Vec<Effect> {
        let fx = m.dispatch(intent);
        m.run_jobs(fx)
    }

    fn opened(dir: &Scratch) -> Model {
        let mut m = Model::default();
        run(&mut m, Intent::Open(dir.svg()));
        m
    }

    fn pick(vertex: usize) -> Intent {
        Intent::Select(Some(Pick { primitive: 0, vertex: Some(vertex) }))
    }

    fn message(m: &Model) -> &str {
        &m.message().expect("a message").text
    }

    #[test]
    fn opening_runs_as_a_job_and_the_answer_installs_the_figure() {
        let dir = Scratch::new("open");
        let mut m = Model::default();
        let fx = m.dispatch(Intent::Open(dir.svg()));
        assert!(matches!(fx.as_slice(), [Effect::Load(_)]), "dispatch only asks for the work");
        assert!(m.session().is_none());
        assert_eq!(m.loading(), Some("fig.svg"));

        let fx = m.run_jobs(fx);
        assert!(m.session().is_some() && m.loading().is_none());
        assert!(matches!(&fx[0], Effect::Opened { title, source: Some(_) } if title == "fig.svg \u{2013} plotextract"));
        assert!(fx.iter().any(|e| matches!(e, Effect::ValuesChanged)));
        assert_eq!(message(&m), "Opened fig.svg: 1 curves. Hold Ctrl and click a curve to select it.");
        assert_eq!(m.message().unwrap().level, Level::Info);
    }

    #[test]
    fn only_the_newest_open_counts() {
        let dir = Scratch::new("stale");
        let mut m = Model::default();
        let first = m.dispatch(Intent::Open(dir.svg()));
        let second = m.dispatch(Intent::Open(dir.pdf(1)));
        let fx = m.run_jobs(first);
        assert!(fx.is_empty() && m.session().is_none(), "the older answer is ignored");
        m.run_jobs(second);
        assert_eq!(m.session().unwrap().figure.unit, plotextract_core::Unit::Point);
    }

    #[test]
    fn opening_what_cannot_be_opened_keeps_the_current_figure() {
        let dir = Scratch::new("badopen");
        let mut m = opened(&dir);
        let notes = dir.0.join("notes.txt");
        std::fs::write(&notes, "hello").unwrap();
        let fx = run(&mut m, Intent::Open(notes));
        assert!(fx.is_empty() && m.session().is_some() && m.loading().is_none());
        assert_eq!(m.message().unwrap().level, Level::Error);
    }

    #[test]
    fn recording_moves_through_the_rows_and_needs_a_selected_point() {
        let dir = Scratch::new("record");
        let mut m = opened(&dir);
        m.dispatch(Intent::Record);
        assert_eq!(m.message().unwrap().level, Level::Error, "no point is selected yet");
        assert_eq!(m.slot(), Slot::XMin);

        m.dispatch(pick(0));
        m.dispatch(Intent::Record);
        assert_eq!(m.slot(), Slot::XMax);
        assert_eq!(message(&m), "x min recorded. Type its plot value in the table.");
        assert!(m.session().unwrap().calibration().position(Slot::XMin).is_some());

        m.dispatch(pick(2));
        m.dispatch(Intent::RecordAt(Slot::YMax));
        assert_eq!(m.slot(), Slot::YMax);
        assert!(m.session().unwrap().calibration().position(Slot::YMax).is_some());
    }

    #[test]
    fn undo_and_clearing_ask_for_the_value_cells_to_be_refilled() {
        let dir = Scratch::new("undo");
        let mut m = opened(&dir);
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::XMin, Some(2.0))));
        let fx = m.dispatch(Intent::Edit(Edit::SetLog(Axis::X, true)));
        assert!(fx.is_empty(), "typing and toggling need no refill");
        assert!(matches!(m.dispatch(Intent::Undo).as_slice(), [Effect::ValuesChanged]));
        assert!(matches!(m.dispatch(Intent::Redo).as_slice(), [Effect::ValuesChanged]));
        assert!(m.dispatch(Intent::Redo).is_empty(), "nothing left to redo");
        assert!(matches!(m.dispatch(Intent::Edit(Edit::ClearCalibration)).as_slice(), [Effect::ValuesChanged]));
    }

    #[test]
    fn exporting_explains_what_is_missing_before_asking_for_a_path() {
        let dir = Scratch::new("export");
        let mut m = opened(&dir);
        assert!(m.dispatch(Intent::RequestExportCsv).is_empty());
        assert!(message(&m).starts_with("Make a series first"));

        m.dispatch(Intent::Edit(Edit::ToggleMember(0)));
        assert!(m.dispatch(Intent::CopyNumbers).is_empty());
        assert!(message(&m).starts_with("Calibrate both axes"));

        for (slot, vertex, value) in
            [(Slot::XMin, 0, 0.0), (Slot::XMax, 2, 8.0), (Slot::YMin, 0, 0.0), (Slot::YMax, 2, 8.0)]
        {
            m.dispatch(pick(vertex));
            m.dispatch(Intent::RecordAt(slot));
            m.dispatch(Intent::Edit(Edit::SetValue(slot, Some(value))));
        }
        let fx = m.dispatch(Intent::RequestExportCsv);
        assert!(matches!(fx.as_slice(), [Effect::AskCsvPath { suggested }] if suggested == "Series 1.csv"));
        let fx = m.dispatch(Intent::CopyNumbers);
        assert!(matches!(fx.as_slice(), [Effect::CopyText(t)] if t == "x,y\n0,0\n4,4\n8,8\n"));

        let out = dir.0.join("out.csv");
        m.dispatch(Intent::WriteCsv(out.clone()));
        assert_eq!(std::fs::read_to_string(out).unwrap(), "x,y\n0,0\n4,4\n8,8\n");
    }

    #[test]
    fn a_saved_project_comes_back_with_its_figure() {
        let dir = Scratch::new("project");
        let mut m = opened(&dir);
        m.dispatch(Intent::Edit(Edit::ToggleMember(0)));
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::YMax, Some(7.0))));
        let fx = m.dispatch(Intent::RequestSaveProject);
        assert!(
            matches!(fx.as_slice(), [Effect::AskProjectSavePath { suggested }] if suggested == "fig.plotextract.json")
        );
        let project = dir.0.join("fig.plotextract.json");
        m.dispatch(Intent::SaveProject(project.clone()));

        let mut fresh = Model::default();
        let fx = run(&mut fresh, Intent::LoadProject(project));
        assert!(matches!(&fx[0], Effect::Opened { .. }), "the figure the project names is opened");
        assert!(matches!(fx.last(), Some(Effect::ValuesChanged)));
        let s = fresh.session().unwrap();
        assert_eq!(s.series()[0].members, vec![0]);
        assert_eq!(s.calibration().value(Slot::YMax), Some(7.0));
    }

    #[test]
    fn selecting_a_hidden_curve_is_refused_with_a_message() {
        let dir = Scratch::new("hidden");
        let mut m = opened(&dir);
        m.dispatch(Intent::Edit(Edit::SetHidden(0, true)));
        m.dispatch(pick(0));
        assert_eq!(m.session().unwrap().selection(), None);
        assert_eq!(message(&m), "That curve is hidden.");
        m.dispatch(Intent::SetLayer(Layer::Fills, false));
        assert!(!m.session().unwrap().layers().fills);
    }

    #[test]
    fn requests_that_need_a_dialog_become_effects() {
        let mut m = Model::default();
        assert!(matches!(m.dispatch(Intent::RequestOpen).as_slice(), [Effect::AskOpenPath]));
        assert!(matches!(m.dispatch(Intent::RequestLoadProject).as_slice(), [Effect::AskProjectToLoad]));
        assert!(m.dispatch(Intent::RequestSaveProject).is_empty(), "nothing to save without a figure");
        assert!(matches!(m.dispatch(Intent::Quit).as_slice(), [Effect::Quit]));
        assert!(m.extensions().contains(&"pdf"));
    }

    #[test]
    fn pages_are_opened_on_demand_and_work_on_them_is_kept() {
        let dir = Scratch::new("pages");
        let mut m = Model::default();
        run(&mut m, Intent::Open(dir.pdf(3)));
        assert_eq!(m.session().unwrap().figure.page_count, 3);
        assert_eq!(message(&m), "Opened book.pdf, page 1 of 3: 1 curves. Hold Ctrl and click a curve to select it.");

        // work on page 1, move on, work on page 3
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::XMin, Some(11.0))));
        let fx = run(&mut m, Intent::GoToPage(2));
        assert!(matches!(&fx[0], Effect::Opened { .. }));
        assert_eq!(m.session().unwrap().figure.page, 2);
        assert_eq!(m.session().unwrap().calibration().value(Slot::XMin), None, "a new page starts clean");
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::XMin, Some(33.0))));

        // coming back finds the work, without reading the file again
        let fx = m.dispatch(Intent::GoToPage(0));
        assert!(matches!(&fx[0], Effect::Opened { .. }), "restored at once: {fx:?}");
        assert_eq!(m.session().unwrap().calibration().value(Slot::XMin), Some(11.0));
        let fx = m.dispatch(Intent::GoToPage(2));
        assert!(matches!(&fx[0], Effect::Opened { .. }));
        assert_eq!(m.session().unwrap().calibration().value(Slot::XMin), Some(33.0));

        // a page nobody touched is not remembered
        run(&mut m, Intent::GoToPage(1));
        assert!(m.dispatch(Intent::GoToPage(1)).is_empty(), "already there");
        assert!(matches!(m.dispatch(Intent::GoToPage(3)).as_slice(), []));
        assert_eq!(m.message().unwrap().level, Level::Error);
    }

    #[test]
    fn opening_another_file_forgets_the_pages_of_the_old_one() {
        let dir = Scratch::new("forget");
        let mut m = Model::default();
        run(&mut m, Intent::Open(dir.pdf(2)));
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::XMin, Some(1.0))));
        run(&mut m, Intent::GoToPage(1));
        assert!(m.parked.contains_key(&0));
        run(&mut m, Intent::Open(dir.svg()));
        assert!(m.parked.is_empty());
    }

    #[test]
    fn a_project_names_its_page() {
        let dir = Scratch::new("projectpage");
        let mut m = Model::default();
        run(&mut m, Intent::Open(dir.pdf(3)));
        run(&mut m, Intent::GoToPage(2));
        m.dispatch(Intent::Edit(Edit::SetValue(Slot::YMin, Some(5.0))));
        let saved = dir.0.join("p.plotextract.json");
        m.dispatch(Intent::SaveProject(saved.clone()));

        let mut fresh = Model::default();
        run(&mut fresh, Intent::LoadProject(saved));
        assert_eq!(fresh.session().unwrap().figure.page, 2);
        assert_eq!(fresh.session().unwrap().calibration().value(Slot::YMin), Some(5.0));
    }

    #[test]
    fn a_figure_the_importer_cut_short_warns_and_draws_no_picture() {
        let dir = Scratch::new("truncated");
        let mut m = Model::default();
        run(&mut m, Intent::Open(dir.svg()));
        // pretend the file was too big: what matters is how the model reacts
        let mut opened = Workspace::default().open(&dir.svg(), OpenOptions::default()).unwrap();
        opened.session.figure.truncated = true;
        let mut fx = Vec::new();
        m.install(opened, &mut fx);
        m.announce();
        assert!(matches!(&fx[0], Effect::Opened { source: None, .. }));
        assert_eq!(m.message().unwrap().level, Level::Warning);
        assert!(message(&m).contains("only the first 1"));
    }
}
