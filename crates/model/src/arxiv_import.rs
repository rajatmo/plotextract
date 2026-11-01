//! The Import from arXiv window's state and its jobs: the download of a paper (one at a time, the latest counting) and
//! the pictures of its figures.

use super::*;

/// How large the pictures of a paper's figures are drawn, in pixels on the longer side.
pub(super) const THUMBNAIL_SIDE: u32 = 320;

/// The download of an arXiv paper and the unpacking of its figures. Run it anywhere with [`ArxivJob::run`].
pub struct ArxivJob {
    pub(super) id: u64,
    pub(super) paper: ArxivId,
    pub(super) papers: Arc<dyn PaperSource>,
    pub(super) workspace: Arc<Workspace>,
    pub(super) progress: Arc<Progress>,
}

impl std::fmt::Debug for ArxivJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArxivJob").field("id", &self.id).field("paper", &self.paper).finish_non_exhaustive()
    }
}

impl ArxivJob {
    /// The slow part: the download. Safe to call on any thread.
    pub fn run(self) -> ArxivFetched {
        let registry = self.workspace.registry();
        let is_figure = |name: &str| registry.opens_name(name);
        let result = self.papers.fetch(&self.paper, &is_figure, &self.progress);
        ArxivFetched { id: self.id, result }
    }
}

/// The answer to an [`ArxivJob`].
pub struct ArxivFetched {
    pub(super) id: u64,
    pub(super) result: Result<Paper, ArxivError>,
}

/// A small picture of one of a paper's figures, to draw. Run it anywhere with [`ThumbnailJob::run`].
pub struct ThumbnailJob {
    /// The fetch whose figure this is.
    pub id: u64,
    pub(super) workspace: Arc<Workspace>,
    pub(super) path: PathBuf,
}

impl std::fmt::Debug for ThumbnailJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThumbnailJob").field("id", &self.id).field("path", &self.path).finish()
    }
}

impl ThumbnailJob {
    /// The slow part: open the figure and draw it small. Safe to call on any thread.
    pub fn run(self) -> Thumbnail {
        let picture = self.workspace.thumbnail(&self.path, THUMBNAIL_SIDE).map_err(|e| text::workspace_error(&e));
        Thumbnail { id: self.id, path: self.path, picture }
    }
}

/// A small picture of a figure, or why there is none.
pub struct Thumbnail {
    pub id: u64,
    pub path: PathBuf,
    pub picture: Result<Bitmap, String>,
}

/// The Import from arXiv window. What it fetched stays when it is closed, so a second figure of the same paper is a
/// click away.
#[derive(Default)]
pub struct ArxivImport {
    pub(super) open: bool,
    /// The fetch running: its job, the paper, how far the download is.
    pub(super) fetching: Option<(u64, ArxivId, Arc<Progress>)>,
    /// The last paper fetched, and the job that fetched it.
    pub(super) paper: Option<(u64, Paper)>,
    pub(super) error: Option<String>,
}

impl ArxivImport {
    /// The paper being downloaded, and how far it is.
    pub fn fetching(&self) -> Option<(&ArxivId, &Progress)> {
        self.fetching.as_ref().map(|(_, id, progress)| (id, progress.as_ref()))
    }

    pub fn paper(&self) -> Option<&Paper> {
        self.paper.as_ref().map(|(_, p)| p)
    }

    /// The job that fetched the paper shown, which its pictures come from.
    #[cfg(test)]
    pub fn paper_job(&self) -> Option<u64> {
        self.paper.as_ref().map(|(id, _)| *id)
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl Model {
    /// Take the answer of an [`ArxivJob`]: the paper's figures are listed and their pictures drawn. An answer to a fetch
    /// that is no longer the latest is dropped.
    pub fn finish_arxiv(&mut self, fetched: ArxivFetched) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.arxiv.fetching.as_ref().map(|(id, ..)| *id) != Some(fetched.id) {
            return fx;
        }
        let Some((_, asked, _)) = self.arxiv.fetching.take() else { return fx };
        match fetched.result {
            Ok(paper) => {
                let pictures = paper
                    .figures
                    .iter()
                    .map(|f| {
                        let workspace = self.workspace.clone();
                        Job::Thumbnail(Box::new(ThumbnailJob { id: fetched.id, workspace, path: f.path.clone() }))
                    })
                    .collect();
                fx.push(Effect::RunBatch(pictures));
                self.arxiv.paper = Some((fetched.id, paper));
                self.arxiv.error = None;
            }
            Err(e) => self.arxiv.error = Some(text::arxiv_error(&asked, &e)),
        }
        fx
    }

    /// Start downloading the paper `text` names. A fetch still running is made stale.
    pub(super) fn fetch_arxiv(&mut self, text: &str, fx: &mut Vec<Effect>) {
        let Some(paper) = ArxivId::parse(text) else {
            self.arxiv.error = Some(text::not_an_arxiv_number(text));
            return;
        };
        self.next_job += 1;
        let id = self.next_job;
        let progress = Arc::new(Progress::default());
        self.arxiv.fetching = Some((id, paper.clone(), progress.clone()));
        self.arxiv.error = None;
        fx.push(Effect::Run(Job::Arxiv(Box::new(ArxivJob {
            id,
            paper,
            papers: self.papers.clone(),
            workspace: self.workspace.clone(),
            progress,
        }))));
    }
}
