//! The Import from PDF window: what any document opened holds to work on (its pages and the raster images they draw),
//! listed by the load job, a small picture of each, and the one chosen opened.

use plotextract_ports::{Contents, Embedded};

use super::opening::What;
use super::*;
use crate::arxiv_import::THUMBNAIL_SIDE;

/// Something in a document to work on: a page (its paths, to pick) or an image one of its pages draws (its pixels, to
/// trace).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// A page, from 0.
    Page(usize),
    Image(Embedded),
}

/// A small picture of a part of a document. Run it anywhere with [`PartPictureJob::run`].
pub struct PartPictureJob {
    /// The load that listed the document.
    pub id: u64,
    pub(super) workspace: Arc<Workspace>,
    pub(super) source: Source,
    pub(super) part: Part,
}

impl std::fmt::Debug for PartPictureJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartPictureJob").field("id", &self.id).field("part", &self.part).finish_non_exhaustive()
    }
}

impl PartPictureJob {
    /// The slow part: draw the page small, or take the image out and draw it small. Safe to call on any thread.
    pub fn run(self) -> PartPicture {
        let source = match self.part {
            Part::Page(page) => Ok(self.source.with_page(page)),
            Part::Image(image) => self.workspace.image_of(&self.source, &image),
        };
        let picture =
            source.and_then(|s| self.workspace.thumbnail_of(s, THUMBNAIL_SIDE)).map_err(|e| text::workspace_error(&e));
        PartPicture { id: self.id, part: self.part, picture }
    }
}

/// A small picture of a part of a document, or why there is none.
pub struct PartPicture {
    pub id: u64,
    pub part: Part,
    pub picture: Result<Bitmap, String>,
}

/// A document listed: where it is, what it holds, and the file as it was read (with its password, if it has one).
pub struct Document {
    pub path: PathBuf,
    pub name: String,
    pub contents: Contents,
    source: Source,
}

/// The Import from PDF window: open while a document's part is to be chosen. The document stays when it is closed.
#[derive(Default)]
pub struct PdfImport {
    pub(super) open: bool,
    /// The document listed, and the load that listed it (which its pictures come from).
    pub(super) document: Option<(u64, Document)>,
}

impl PdfImport {
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref().map(|(_, d)| d)
    }

    /// The load that listed the document shown.
    #[cfg(test)]
    pub fn document_job(&self) -> Option<u64> {
        self.document.as_ref().map(|(id, _)| *id)
    }
}

impl Model {
    /// The Import from PDF window, while it is open.
    pub fn pdf(&self) -> Option<&PdfImport> {
        self.pdf.open.then_some(&self.pdf)
    }

    /// Show what a document holds (listed by load `id`) to choose from, and ask for the pictures of its parts.
    pub(super) fn list_document(
        &mut self,
        id: u64,
        path: PathBuf,
        source: Source,
        contents: Contents,
        fx: &mut Vec<Effect>,
    ) {
        // the images first, as the window shows them, so their pictures come first
        let parts = contents.images.iter().copied().map(Part::Image).chain((0..contents.pages).map(Part::Page));
        let pictures = parts
            .map(|part| {
                let (workspace, source) = (self.workspace.clone(), source.clone());
                Job::PartPicture(Box::new(PartPictureJob { id, workspace, source, part }))
            })
            .collect();
        fx.push(Effect::RunBatch(pictures));
        let name = opening::file_name(&path);
        self.info(format!("Choose a page or an image of {name} to open."));
        self.pdf.document = Some((id, Document { path, name, contents, source }));
        self.arxiv.open = false;
        self.pdf.open = true;
    }

    /// Open a part of the document listed, and close the window.
    pub(super) fn import_part(&mut self, part: Part, fx: &mut Vec<Effect>) {
        let Some(d) = self.pdf.document() else { return };
        let (source, path) = (d.source.clone(), d.path.clone());
        self.pdf.open = false;
        let what = match part {
            Part::Page(page) => What::Page { source: source.with_page(page), path },
            Part::Image(image) => What::Image { source, path, image },
        };
        self.start(what, None, fx);
    }
}
