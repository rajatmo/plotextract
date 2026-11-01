//! Opening a figure: the load job (run on a thread), its answer, a password asked for, other pages of the same file,
//! and a project brought back with its figure.

use super::*;

/// What a load reads.
#[derive(Clone)]
pub(super) enum What {
    /// A file on disk.
    File { path: PathBuf, options: OpenOptions },
    /// A file already read, at another page or with a password.
    Source { source: Source, name: Option<String> },
}

impl What {
    pub(super) fn name(&self) -> String {
        match self {
            What::File { path, .. } => file_name(path),
            What::Source { name, .. } => name.as_deref().map(|n| file_name(Path::new(n))).unwrap_or_default(),
        }
    }

    pub(super) fn with_password(&self, password: String) -> What {
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
    pub(super) id: u64,
    pub(super) what: What,
    pub(super) project: Option<Project>,
    pub(super) result: Result<Opened, WorkspaceError>,
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

    /// Ask the shell to open something. A job still running is made stale: only the newest one counts.
    pub(super) fn start(&mut self, what: What, project: Option<Project>, fx: &mut Vec<Effect>) {
        self.next_job += 1;
        let id = self.next_job;
        self.loading = Some((id, what.name()));
        self.password = None;
        fx.push(Effect::Run(Job::Load(Box::new(LoadJob { id, workspace: self.workspace.clone(), what, project }))));
    }

    /// Make an opened figure the one shown. What was open before is kept only if it is another page of the same file
    /// and the user worked on it.
    pub(super) fn install(&mut self, opened: Opened, fx: &mut Vec<Effect>) {
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
        self.wizard = opened.needs_tracing().then(|| {
            let f = &opened.session.figure;
            Wizard::new(f.width as u32, f.height as u32)
        });
        self.show(opened.session, pictures, fx);
    }

    pub(super) fn show(&mut self, session: Session, pictures: Option<Source>, fx: &mut Vec<Effect>) {
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
            let text = format!(
                "Opened {name}: {} x {} pixels. Choose Crop to select the plot, then Exclude to cover what should be ignored.",
                f.width, f.height
            );
            self.message = Some(Message { text, level: Level::Info });
            return;
        }
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

    pub(super) fn go_to_page(&mut self, page: usize, fx: &mut Vec<Effect>) {
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

    pub(super) fn load_project(&mut self, path: &Path, fx: &mut Vec<Effect>) {
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
