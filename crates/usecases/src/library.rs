//! The library of figures: the operations on files (open a figure, save and load projects, export numbers), through the
//! [`Files`] port and the [`Registry`] of formats. It knows no window, disk or format of its own, so a front end (the egui
//! app, a command line, a test) shares one implementation of what "open" and "export" mean.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use plotextract_domain::{Bitmap, ImportError, OpenOptions, Project};
use plotextract_ports::{Contents, Embedded, Files, Importer, PictureSource};

use crate::registry::Registry;
use crate::session::{Session, SessionError};

/// Why an operation on a file failed. The messages state the facts; wording for a user is the front end's job.
#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("cannot read {}: {source}", path.display())]
    Read { path: PathBuf, source: std::io::Error },
    #[error("cannot write {}: {source}", path.display())]
    Write { path: PathBuf, source: std::io::Error },
    #[error("{} is not a file format plotextract reads", path.display())]
    Unsupported { path: PathBuf },
    #[error("{} is not a valid project: {source}", path.display())]
    Project { path: PathBuf, source: serde_json::Error },
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error(transparent)]
    Session(#[from] SessionError),
}

/// A file as it was read: its bytes, the format that reads it, and what was asked of it. Cheap to clone and free to move
/// between threads, so a worker can create its own picture of it.
#[derive(Clone)]
pub struct Source {
    importer: Arc<dyn Importer>,
    bytes: Arc<Vec<u8>>,
    options: OpenOptions,
}

impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("format", &self.importer.name())
            .field("bytes", &self.bytes.len())
            .field("page", &self.options.page)
            .finish_non_exhaustive()
    }
}

impl Source {
    pub fn format_name(&self) -> &'static str {
        self.importer.name()
    }

    /// The page this source is read at, from 0.
    pub fn page(&self) -> usize {
        self.options.page
    }

    /// Whether the file holds pixels, to be traced into paths before there is anything to pick.
    pub fn needs_tracing(&self) -> bool {
        self.importer.needs_tracing()
    }

    /// The file's bytes, for steps that read the pixels of an image themselves.
    pub fn bytes(&self) -> &Arc<Vec<u8>> {
        &self.bytes
    }

    /// The same file at another page.
    pub fn with_page(&self, page: usize) -> Self {
        Self { options: OpenOptions { page, ..self.options.clone() }, ..self.clone() }
    }

    /// The same file with a password to open it.
    pub fn with_password(&self, password: String) -> Self {
        Self { options: OpenOptions { password: Some(password), ..self.options.clone() }, ..self.clone() }
    }

    /// Create a [`PictureSource`] of the page. Call it on the thread that will use it; `None` when the format draws no
    /// picture.
    pub fn picture(&self) -> Option<Box<dyn PictureSource>> {
        self.importer.picture(&self.bytes, &self.options)
    }

    /// Whether the file is a document: a page of it, or an image a page draws, is chosen before anything is opened.
    pub fn is_document(&self) -> bool {
        self.importer.is_document()
    }

    /// The pages of the file and the raster images they draw.
    pub fn contents(&self) -> Result<Contents, ImportError> {
        self.importer.contents(&self.bytes, &self.options)
    }
}

/// A figure that was opened: the session, and the source to draw pictures of it from.
pub struct Opened {
    pub session: Session,
    source: Source,
}

impl Opened {
    /// The source of the figure's pictures, unless the file is so large that drawing it whole would be unreasonable.
    pub fn picture_source(&self) -> Option<Source> {
        (!self.session.figure.truncated).then(|| self.source.clone())
    }

    /// Whether the figure is empty because the file holds pixels, to be traced.
    pub fn needs_tracing(&self) -> bool {
        self.source.needs_tracing()
    }

    /// The source, for reopening the file at another page.
    pub fn source(&self) -> &Source {
        &self.source
    }
}

pub struct Workspace {
    registry: Registry,
    files: Arc<dyn Files>,
    /// Where the files the workspace makes are kept (the images taken out of a document), so a project made from one
    /// finds it again.
    cache: PathBuf,
}

impl Workspace {
    /// A workspace that keeps the files it makes in the system's temporary folder; see [`Workspace::with_cache`].
    pub fn new(registry: Registry, files: Arc<dyn Files>) -> Self {
        Self { registry, files, cache: std::env::temp_dir().join("plotextract") }
    }

    /// Keep the files the workspace makes in `dir`.
    pub fn with_cache(self, dir: PathBuf) -> Self {
        Self { cache: dir, ..self }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Read a file and find the format that reads it, without interpreting it yet.
    pub fn read(&self, path: &Path, options: OpenOptions) -> Result<Source, WorkspaceError> {
        let bytes = self.files.read(path).map_err(|source| WorkspaceError::Read { path: path.into(), source })?;
        let importer = self
            .registry
            .detect(&bytes, path.extension().and_then(|e| e.to_str()))
            .ok_or_else(|| WorkspaceError::Unsupported { path: path.into() })?;
        Ok(Source { importer, bytes: Arc::new(bytes), options })
    }

    /// Interpret a source into a fresh session. `name` is where it came from, as the user gave it.
    pub fn load(&self, source: Source, name: Option<String>) -> Result<Opened, WorkspaceError> {
        let figure = source.importer.import(&source.bytes, &source.options)?;
        let sane = |v: f64| v.is_finite() && v > 0.0;
        if !(sane(figure.width) && sane(figure.height)) {
            return Err(ImportError::message("the page has no size").into());
        }
        Ok(Opened { session: Session::new(figure, name), source })
    }

    /// Open a figure file in a fresh session.
    pub fn open(&self, path: &Path, options: OpenOptions) -> Result<Opened, WorkspaceError> {
        self.load(self.read(path, options)?, Some(path.display().to_string()))
    }

    /// A small picture of the first page of a figure file, `side` pixels on its longer side.
    pub fn thumbnail(&self, path: &Path, side: u32) -> Result<Bitmap, WorkspaceError> {
        self.thumbnail_of(self.read(path, OpenOptions::default())?, side)
    }

    /// A small picture of the page a source is read at, `side` pixels on its longer side.
    pub fn thumbnail_of(&self, source: Source, side: u32) -> Result<Bitmap, WorkspaceError> {
        let opened = self.load(source, None)?;
        let f = &opened.session.figure;
        let mut picture = opened.source.picture().ok_or_else(|| ImportError::message("the file draws no picture"))?;
        let scale = f64::from(side) / f.width.max(f.height);
        Ok(picture.render(kurbo::Rect::new(0.0, 0.0, f.width, f.height), scale)?)
    }

    /// One of the images a document draws, as a source of its own (a PNG file in memory).
    pub fn image_of(&self, document: &Source, image: &Embedded) -> Result<Source, WorkspaceError> {
        let png = document.importer.image(&document.bytes, &document.options, image)?;
        let importer = self
            .registry
            .detect(&png, Some("png"))
            .ok_or_else(|| ImportError::message("no format reads PNG images"))?;
        Ok(Source { importer, bytes: Arc::new(png), options: OpenOptions::default() })
    }

    /// Take one of the images a document (read from `path`) draws out of it, into the cache folder, and open it. The file
    /// is named after the document (and what is in it, so two documents of one name do not share it), the page and the
    /// image.
    pub fn open_image_of(&self, document: &Source, path: &Path, image: &Embedded) -> Result<Opened, WorkspaceError> {
        let source = self.image_of(document, image)?;
        let stem = path.file_stem().map_or("document".into(), |s| s.to_string_lossy());
        let mut hash = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(document.bytes.as_slice(), &mut hash);
        let name = format!(
            "{stem}-{:08x}-page{}-image{}.png",
            std::hash::Hasher::finish(&hash) as u32,
            image.page + 1,
            image.index + 1
        );
        let out = self.cache.join("pdf").join(name);
        self.files.write(&out, &source.bytes).map_err(|source| WorkspaceError::Write { path: out.clone(), source })?;
        self.load(source, Some(out.display().to_string()))
    }

    /// A good file name for the project of a session.
    pub fn project_file_name(session: &Session) -> String {
        let stem = session.source.as_deref().and_then(|s| Path::new(s).file_stem()).map(|s| s.to_string_lossy());
        format!("{}.plotextract.json", stem.as_deref().unwrap_or("project"))
    }

    /// Save the project of a session, with what was done in its tracing wizard if it has one.
    pub fn save_project(
        &self,
        session: &Session,
        wizard: Option<&crate::wizard::Wizard>,
        path: &Path,
    ) -> Result<(), WorkspaceError> {
        let mut project = session.project();
        project.tracing = wizard.map(|w| w.work().to_value());
        let text = project.to_json().map_err(|source| WorkspaceError::Project { path: path.into(), source })?;
        self.write(path, text)
    }

    pub fn read_project(&self, path: &Path) -> Result<Project, WorkspaceError> {
        let bytes = self.files.read(path).map_err(|source| WorkspaceError::Read { path: path.into(), source })?;
        let text = String::from_utf8_lossy(&bytes);
        Project::from_json(&text).map_err(|source| WorkspaceError::Project { path: path.into(), source })
    }

    /// The numbers of a series, as CSV text.
    pub fn csv(&self, session: &Session, series: usize) -> Result<String, WorkspaceError> {
        Ok(session.csv(series)?)
    }

    pub fn export_csv(&self, session: &Session, series: usize, path: &Path) -> Result<(), WorkspaceError> {
        self.write(path, self.csv(session, series)?)
    }

    /// Write CSV text made elsewhere (the points of an image traced) to `path`.
    pub fn write_csv(&self, path: &Path, csv: String) -> Result<(), WorkspaceError> {
        self.write(path, csv)
    }

    fn write(&self, path: &Path, text: String) -> Result<(), WorkspaceError> {
        self.files.write(path, text.as_bytes()).map_err(|source| WorkspaceError::Write { path: path.into(), source })
    }
}

#[cfg(test)]
mod tests {
    use plotextract_domain::{Edit, Pick, Slot};

    use super::*;

    /// A library of every format, on the local disk.
    fn library() -> Workspace {
        let registry = Registry::new(vec![
            Arc::new(plotextract_pdf::PdfImporter),
            Arc::new(plotextract_svg::SvgImporter),
            Arc::new(plotextract_ps::PsImporter),
            Arc::new(plotextract_image::RasterImporter),
        ]);
        Workspace::new(registry, Arc::new(plotextract_fs::LocalFiles))
    }

    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
        <polyline points="10,90 50,50 90,10" fill="none" stroke="#000" stroke-width="1"/></svg>"##;

    /// A scratch directory unique to the test.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("plotextract-workspace-{}-{name}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self, name: &str, content: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).ok();
        }
    }

    fn open(ws: &Workspace, path: &Path) -> Result<Opened, WorkspaceError> {
        ws.open(path, OpenOptions::default())
    }

    /// A PDF whose page k (from 0) is 100 x 100 and draws one line of length 10 * (k + 1).
    fn pdf_with_pages(count: usize) -> Vec<u8> {
        let mut objects: Vec<String> = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            format!(
                "<< /Type /Pages /Kids [{}] /Count {count} >>",
                (0..count).map(|k| format!("{} 0 R", 3 + 2 * k)).collect::<Vec<_>>().join(" ")
            ),
        ];
        for k in 0..count {
            objects
                .push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents {} 0 R >>", 4 + 2 * k));
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
        out.into_bytes()
    }

    #[test]
    fn opens_a_figure_and_reports_its_unit() {
        let dir = Scratch::new("open");
        let opened = open(&library(), &dir.file("fig.svg", SVG)).unwrap();
        assert_eq!(opened.source().format_name(), "SVG");
        assert_eq!(opened.session.figure.primitives.len(), 1);
        assert_eq!(opened.session.figure.unit.symbol(), "px");
        assert_eq!(opened.session.figure.page, 0);
        assert!(!opened.source().is_document(), "an SVG is opened at once");
    }

    #[test]
    fn a_thumbnail_is_the_page_drawn_small() {
        let dir = Scratch::new("thumb");
        let ws = library();
        let wide = SVG.replace(r#"height="100" viewBox="0 0 100 100""#, r#"height="50" viewBox="0 0 100 50""#);
        let small = ws.thumbnail(&dir.file("wide.svg", wide), 40).unwrap();
        assert_eq!((small.width, small.height), (40, 20));
        assert!(small.rgba.chunks(4).any(|p| p[0] < 240), "the line is drawn, faint at this size");
        assert!(ws.thumbnail(&dir.file("notes.txt", "x"), 40).is_err());
    }

    #[test]
    fn refuses_unknown_and_missing_files() {
        let dir = Scratch::new("refuse");
        let ws = library();
        let other = dir.file("notes.txt", "hello");
        assert!(matches!(open(&ws, &other), Err(WorkspaceError::Unsupported { .. })));
        assert!(matches!(open(&ws, &dir.0.join("gone.svg")), Err(WorkspaceError::Read { .. })));
    }

    #[test]
    fn damaged_files_are_refused_with_a_reason_and_never_panic() {
        let dir = Scratch::new("damaged");
        let ws = library();
        let good = pdf_with_pages(2);
        let mut flipped = good.clone();
        for i in (60..flipped.len()).step_by(7) {
            flipped[i] ^= 0xFF;
        }
        let cases: [(&str, Vec<u8>); 7] = [
            ("empty.pdf", Vec::new()),
            ("header.pdf", b"%PDF-1.7\n".to_vec()),
            ("half.pdf", good[..good.len() / 2].to_vec()),
            ("flipped.pdf", flipped),
            ("noise.pdf", [b"%PDF-1.7\n".as_slice(), &[0xA5; 5000]].concat()),
            ("broken.svg", b"<svg xmlns='http://www.w3.org/2000/svg'><path d='M0 0 L".to_vec()),
            ("garbage.ps", "%!PS\n(((( ] }} >> \\ \n".repeat(50).into_bytes()),
        ];
        for (name, bytes) in cases {
            // a file may open (the readers recover what they can) or be refused, but never takes the program down
            if let Err(e) = open(&ws, &dir.file(name, bytes)) {
                assert!(!e.to_string().is_empty(), "{name}");
            }
        }
    }

    #[test]
    fn a_zero_sized_page_is_refused() {
        let dir = Scratch::new("zero");
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"##;
        assert!(open(&library(), &dir.file("zero.svg", svg)).is_err());
    }

    #[test]
    fn a_pdf_is_a_document_listing_its_pages_each_of_which_opens_and_a_missing_one_is_refused() {
        let dir = Scratch::new("pages");
        let ws = library();
        let path = dir.file("book.pdf", pdf_with_pages(3));
        let source = ws.read(&path, OpenOptions::default()).unwrap();
        assert!(source.is_document());
        assert_eq!(source.contents().unwrap(), Contents { pages: 3, images: Vec::new() });
        for page in 0..3 {
            let opened = ws.open(&path, OpenOptions { page, ..Default::default() }).unwrap();
            let f = &opened.session.figure;
            assert_eq!(f.page, page);
            let line = &f.primitives[0];
            let length = line.points[1].x - line.points[0].x;
            assert!((length - 10.0 * (page as f64 + 1.0)).abs() < 1e-6, "page {page} drew a line of {length}");
        }
        let err = ws.open(&path, OpenOptions { page: 3, ..Default::default() }).err().unwrap();
        assert!(matches!(err, WorkspaceError::Import(ImportError::NoSuchPage { page: 3, count: 3 })), "{err}");
    }

    #[test]
    fn a_source_reopens_at_another_page_without_the_file() {
        let dir = Scratch::new("reopen");
        let ws = library();
        let path = dir.file("book.pdf", pdf_with_pages(2));
        let first = open(&ws, &path).unwrap();
        std::fs::remove_file(&path).unwrap();
        let second = ws.load(first.source().with_page(1), first.session.source.clone()).unwrap();
        assert_eq!(second.session.figure.page, 1);
        assert_eq!(second.source().page(), 1);
    }

    #[test]
    fn a_picture_comes_from_the_opened_figure() {
        let dir = Scratch::new("picture");
        let opened = open(&library(), &dir.file("fig.svg", SVG)).unwrap();
        let mut picture = opened.picture_source().unwrap().picture().expect("SVG draws");
        let bitmap = picture.render(kurbo::Rect::new(0.0, 0.0, 50.0, 50.0), 2.0).unwrap();
        assert_eq!((bitmap.width, bitmap.height), (100, 100));
        assert_eq!(bitmap.rgba.len(), 100 * 100 * 4);
    }

    #[test]
    fn a_truncated_figure_draws_no_picture() {
        let dir = Scratch::new("truncated");
        let mut opened = open(&library(), &dir.file("fig.svg", SVG)).unwrap();
        opened.session.figure.truncated = true;
        assert!(opened.picture_source().is_none());
    }

    #[test]
    fn exports_the_active_series_as_csv() {
        let dir = Scratch::new("csv");
        let ws = library();
        let mut s = open(&ws, &dir.file("fig.svg", SVG)).unwrap().session;
        for (slot, vertex, value) in
            [(Slot::XMin, 0, 0.0), (Slot::XMax, 2, 8.0), (Slot::YMin, 0, 0.0), (Slot::YMax, 2, 8.0)]
        {
            s.select(Some(Pick { primitive: 0, vertex: Some(vertex) })).unwrap();
            s.record_selected(slot).unwrap();
            s.edit(Edit::SetValue(slot, Some(value))).unwrap();
        }
        s.edit(Edit::ToggleMember(0)).unwrap();
        let out = dir.0.join("out.csv");
        ws.export_csv(&s, 0, &out).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "x,y\n0,0\n4,4\n8,8\n");
        assert!(matches!(ws.csv(&s, 5), Err(WorkspaceError::Session(SessionError::Export(_)))));
    }

    #[test]
    fn projects_round_trip_with_their_page() {
        let dir = Scratch::new("project");
        let ws = library();
        let path = dir.file("book.pdf", pdf_with_pages(2));
        let mut s = ws.open(&path, OpenOptions { page: 1, ..Default::default() }).unwrap().session;
        s.edit(Edit::ToggleMember(0)).unwrap();
        s.edit(Edit::SetValue(Slot::XMin, Some(2.5))).unwrap();
        assert_eq!(Workspace::project_file_name(&s), "book.plotextract.json");
        let saved = dir.0.join("book.plotextract.json");
        ws.save_project(&s, None, &saved).unwrap();
        assert!(!std::fs::read_to_string(&saved).unwrap().contains("tracing"), "a vector figure has no wizard");

        let project = ws.read_project(&saved).unwrap();
        assert_eq!((project.source.as_deref(), project.page), (s.source.as_deref(), 1));
        let source = Path::new(project.source.as_deref().unwrap());
        let mut reopened = ws.open(source, OpenOptions { page: project.page, ..Default::default() }).unwrap().session;
        reopened.load_project(project);
        assert_eq!(reopened.series(), s.series());
        assert_eq!(reopened.calibration().value(Slot::XMin), Some(2.5));

        let bad = dir.file("bad.json", "{not json");
        assert!(matches!(ws.read_project(&bad), Err(WorkspaceError::Project { .. })));
    }

    #[test]
    fn projects_from_before_pages_load_as_the_first_page() {
        let dir = Scratch::new("oldproject");
        let axis = r#"{"pos":[null,null],"value":[null,null],"log":false}"#;
        let old =
            format!(r#"{{"version":1,"source":null,"calibration":{{"x":{axis},"y":{axis}}},"series":[],"hidden":[]}}"#);
        let project = library().read_project(&dir.file("old.json", old)).unwrap();
        assert_eq!(project.page, 0);
    }
}
