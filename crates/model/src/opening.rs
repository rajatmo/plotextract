//! Opening a figure: the load job (run on a thread), its answer, a password asked for, and a project brought back with
//! its figure. A document (a PDF) is not opened at once: the load job lists its pages and the images they draw, and one
//! of them is chosen in the Import from PDF window ([`super::pdf_import`]).

use plotextract_ports::{Contents, Embedded};

use super::*;

/// What a load reads.
#[derive(Clone)]
pub(super) enum What {
    /// A file on disk. A document is listed, to choose a page or an image of it, unless `choose` is false (a project
    /// names its page).
    File { path: PathBuf, options: OpenOptions, choose: bool },
    /// A page of a document read already (from `path`), at its page.
    Page { source: Source, path: PathBuf },
    /// An image a document read already (from `path`) draws, taken out of it.
    Image { source: Source, path: PathBuf, image: Embedded },
}

impl What {
    pub(super) fn name(&self) -> String {
        match self {
            What::File { path, .. } => file_name(path),
            What::Page { source, path } => format!("{}, page {}", file_name(path), source.page() + 1),
            What::Image { path, image, .. } => {
                format!("{}, image {} of page {}", file_name(path), image.index + 1, image.page + 1)
            }
        }
    }

    pub(super) fn with_password(&self, password: String) -> What {
        match self {
            What::File { path, options, choose } => What::File {
                path: path.clone(),
                options: OpenOptions { password: Some(password), ..options.clone() },
                choose: *choose,
            },
            // a document read already was read with its password
            What::Page { .. } | What::Image { .. } => self.clone(),
        }
    }
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.display().to_string())
}

/// A file to open, and what to do once it is open. Run it anywhere with [`LoadJob::run`].
pub struct LoadJob {
    pub(super) id: u64,
    pub(super) workspace: Arc<Workspace>,
    pub(super) what: What,
    /// A project to apply to the figure once it is open.
    pub(super) project: Option<Project>,
}

impl std::fmt::Debug for LoadJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadJob").field("id", &self.id).field("file", &self.what.name()).finish_non_exhaustive()
    }
}

/// What a load gives: a figure opened, or a document's pages and images to choose from.
pub(super) enum Outcome {
    Opened(Box<Opened>),
    Listed { source: Source, contents: Contents },
}

impl LoadJob {
    /// The slow part: read and interpret the file, or go through the pages of a document. Safe to call on any thread.
    pub fn run(self) -> Loaded {
        let named = |path: &Path| Some(path.display().to_string());
        let opened = |o: Opened| Outcome::Opened(Box::new(o));
        let result = match &self.what {
            What::File { path, options, choose } => self.workspace.read(path, options.clone()).and_then(|source| {
                if *choose && source.is_document() {
                    let contents = source.contents()?;
                    Ok(Outcome::Listed { source, contents })
                } else {
                    self.workspace.load(source, named(path)).map(opened)
                }
            }),
            What::Page { source, path } => self.workspace.load(source.clone(), named(path)).map(opened),
            What::Image { source, path, image } => self.workspace.open_image_of(source, path, image).map(opened),
        };
        Loaded { id: self.id, what: self.what, project: self.project, result }
    }
}

/// The answer to a [`LoadJob`].
pub struct Loaded {
    pub(super) id: u64,
    pub(super) what: What,
    pub(super) project: Option<Project>,
    pub(super) result: Result<Outcome, WorkspaceError>,
}

/// A file that needs a password before it can be opened.
pub struct PasswordPrompt {
    /// The file's name, for the prompt.
    pub file: String,
    /// The last password was wrong.
    pub wrong: bool,
    pub(super) retry: What,
    pub(super) project: Option<Project>,
}

impl Model {
    /// Take the answer of a [`LoadJob`]. An answer to a job that is no longer the latest is dropped.
    pub fn finish_load(&mut self, loaded: Loaded) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.loading.as_ref().map(|(id, _)| *id) != Some(loaded.id) {
            return fx;
        }
        self.loading = None;
        let Loaded { id, what, project, result } = loaded;
        match result {
            Ok(Outcome::Opened(opened)) => {
                self.password = None;
                self.install(*opened, &mut fx);
                if let (Some(mut project), Some(s)) = (project, &mut self.session) {
                    let tracing = project.tracing.take();
                    s.load_project(project);
                    fx.push(Effect::ValuesChanged);
                    self.announce();
                    self.restore_tracing(tracing, &mut fx);
                    return fx;
                }
                self.announce();
            }
            Ok(Outcome::Listed { source, contents }) => {
                self.password = None;
                if let What::File { path, .. } = what {
                    self.list_document(id, path, source, contents, &mut fx);
                }
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

    /// Ask the shell to open something. A job still running is made stale: only the newest one counts.
    pub(super) fn start(&mut self, what: What, project: Option<Project>, fx: &mut Vec<Effect>) {
        self.next_job += 1;
        let id = self.next_job;
        self.loading = Some((id, what.name()));
        self.password = None;
        fx.push(Effect::Run(Job::Load(Box::new(LoadJob { id, workspace: self.workspace.clone(), what, project }))));
    }

    /// Open the file at `path`: a figure at once, a document to choose from.
    pub(super) fn open(&mut self, path: PathBuf, fx: &mut Vec<Effect>) {
        self.start(What::File { path, options: OpenOptions::default(), choose: true }, None, fx);
    }

    /// Make an opened figure the one shown, in place of what was open.
    pub(super) fn install(&mut self, opened: Opened, fx: &mut Vec<Effect>) {
        self.source = Some(opened.source().clone());
        let pictures = opened.picture_source();
        self.wizard = opened.needs_tracing().then(|| {
            let f = &opened.session.figure;
            Wizard::new(f.width as u32, f.height as u32)
        });
        let session = opened.session;
        let name = session.source.as_deref().map(|s| file_name(Path::new(s))).unwrap_or_default();
        self.session = Some(session);
        self.slot = Slot::XMin;
        fx.push(Effect::Opened { title: format!("{name} \u{2013} plotextract"), source: pictures });
        fx.push(Effect::ValuesChanged);
    }

    pub(super) fn announce(&mut self) {
        let Some(s) = &self.session else { return };
        let name = s.source.as_deref().map(|p| file_name(Path::new(p))).unwrap_or_default();
        let f = &s.figure;
        if self.wizard.is_some() {
            let text = format!("Opened {name}: {} x {} pixels.", f.width, f.height);
            self.message = Some(Message { text, level: Level::Info });
            return;
        }
        let place = if self.source.as_ref().is_some_and(Source::is_document) {
            format!("{name}, page {}", f.page + 1)
        } else {
            name
        };
        let hint = "Ctrl+click a curve to select it.";
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

    pub(super) fn load_project(&mut self, path: &Path, fx: &mut Vec<Effect>) {
        let project = match self.workspace.read_project(path) {
            Ok(p) => p,
            Err(e) => return self.error(text::workspace_error(&e)),
        };
        let open_source = self.session.as_ref().and_then(|s| s.source.clone());
        let open_page = self.session.as_ref().map(|s| s.figure.page);
        // work in the tracing wizard is brought back on the image opened afresh, as it was when it was saved
        let reopen = project.tracing.is_some() && self.wizard.is_some();
        match project.source.clone() {
            Some(source)
                if reopen || open_source.as_deref() != Some(source.as_str()) || open_page != Some(project.page) =>
            {
                // the project names its page: nothing is chosen
                let options = OpenOptions { page: project.page, ..Default::default() };
                self.start(What::File { path: PathBuf::from(source), options, choose: false }, Some(project), fx);
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

    /// Bring back the work of a project in the wizard of the image just opened, and start what it needs found.
    fn restore_tracing(&mut self, tracing: Option<serde_json::Value>, fx: &mut Vec<Effect>) {
        let (Some(tracing), Some(w)) = (tracing, &mut self.wizard) else { return };
        match Work::from_value(tracing) {
            Ok(work) => {
                w.restore(work);
                self.start_wizard_jobs(fx);
            }
            Err(e) => {
                let text = format!("The tracing saved in the project could not be read ({e}): starting afresh.");
                self.message = Some(Message { text, level: Level::Warning });
            }
        }
    }
}
