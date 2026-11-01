//! The figures of an arXiv paper: given its number, its source is downloaded from arXiv (the files the authors
//! submitted, a gzipped tar), the figure files in it are unpacked into a folder, and they are listed in the order the
//! paper's TeX files draw them.
//!
//! Which files are figures is the caller's to say (the formats the program opens), so this crate knows nothing of them.
//! The download is behind [`Downloader`], so the rest is tested without a network.

mod id;
mod tex;
mod unpack;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub use id::ArxivId;
pub use unpack::{MAX_UNPACKED, unpack};

/// The largest source downloaded. arXiv's own limit on a submission is far below it.
pub const MAX_DOWNLOAD: u64 = 1 << 30;

const USER_AGENT: &str =
    concat!("plotextract/", env!("CARGO_PKG_VERSION"), " (+https://github.com/rajatmo/plotextract)");

/// Why a paper's figures could not be had. The messages state the facts; wording for a user is the front end's job.
#[derive(Debug, thiserror::Error)]
pub enum ArxivError {
    #[error("arXiv has no paper {0}")]
    NotFound(String),
    #[error("arXiv answered {status} for {id}")]
    Status { id: String, status: u16 },
    #[error("cannot reach arXiv: {0}")]
    Network(String),
    #[error("the source is larger than {} MB", .0 >> 20)]
    TooLarge(u64),
    #[error("the source is not a file plotextract can unpack")]
    NotAnArchive,
    #[error("the source is damaged: {0}")]
    Damaged(String),
    #[error("cannot write {}: {source}", path.display())]
    Write { path: PathBuf, source: std::io::Error },
}

/// What a paper's source is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An archive of TeX files and figures.
    Archive,
    /// A single TeX file: there are no figure files.
    Tex,
    /// The PDF alone, given as the one figure: its figures are on its pages.
    Pdf,
}

/// A figure file of a paper, unpacked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Figure {
    /// Where it was unpacked to.
    pub path: PathBuf,
    /// Its name in the source, with `/` between names.
    pub name: String,
    /// Its size.
    pub bytes: u64,
    /// Whether the paper's TeX files draw it.
    pub cited: bool,
}

/// A paper whose figures were unpacked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Paper {
    /// Its number, with the version arXiv sent.
    pub id: String,
    pub kind: Kind,
    /// The folder the figures are in.
    pub folder: PathBuf,
    /// The figures the paper draws, in the order it draws them, then the others by name.
    pub figures: Vec<Figure>,
}

/// How far a download is, for a progress bar on another thread.
#[derive(Debug, Default)]
pub struct Progress {
    done: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    /// The bytes received so far.
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// The bytes there are to receive, once the server has said.
    pub fn total(&self) -> Option<u64> {
        Some(self.total.load(Ordering::Relaxed)).filter(|&t| t > 0)
    }
}

/// A source as it was downloaded.
pub struct Download {
    /// The number with the version, if the server named it.
    pub version: Option<ArxivId>,
    pub bytes: Vec<u8>,
}

/// Where sources come from.
pub trait Downloader: Send + Sync {
    fn download(&self, id: &ArxivId, progress: &Progress) -> Result<Download, ArxivError>;
}

/// arXiv itself.
pub struct Arxiv;

impl Downloader for Arxiv {
    fn download(&self, id: &ArxivId, progress: &Progress) -> Result<Download, ArxivError> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .build()
            .into();
        let mut response = agent.get(&id.source_url()).call().map_err(|e| match e {
            ureq::Error::StatusCode(404) => ArxivError::NotFound(id.to_string()),
            ureq::Error::StatusCode(status) => ArxivError::Status { id: id.to_string(), status },
            e => ArxivError::Network(e.to_string()),
        })?;
        let header = |name: &str| response.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let version = header("content-disposition").and_then(|d| version_of(&d));
        let total = header("content-length").and_then(|l| l.parse::<u64>().ok()).unwrap_or(0);
        if total > MAX_DOWNLOAD {
            return Err(ArxivError::TooLarge(MAX_DOWNLOAD));
        }
        progress.total.store(total, Ordering::Relaxed);
        let mut reader = response.body_mut().with_config().limit(MAX_DOWNLOAD + 1).reader();
        let mut bytes = Vec::with_capacity(total as usize);
        let mut buf = vec![0u8; 1 << 16];
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ArxivError::Network(e.to_string())),
            };
            bytes.extend_from_slice(&buf[..n]);
            progress.done.fetch_add(n as u64, Ordering::Relaxed);
            if bytes.len() as u64 > MAX_DOWNLOAD {
                return Err(ArxivError::TooLarge(MAX_DOWNLOAD));
            }
        }
        Ok(Download { version, bytes })
    }
}

/// The number and version a `Content-Disposition` header names: arXiv sends `filename="arXiv-1706.03762v7.tar.gz"`.
fn version_of(disposition: &str) -> Option<ArxivId> {
    let (_, name) = disposition.split_once("filename=")?;
    let name = name.trim().trim_matches('"');
    let name = name.split('"').next()?;
    let name = name.strip_prefix("arXiv-").unwrap_or(name);
    let stem = [".tar.gz", ".gz", ".tar", ".pdf"].iter().find_map(|ext| name.strip_suffix(ext)).unwrap_or(name);
    ArxivId::parse(stem)
}

/// Download paper `id` with `downloader` and unpack its figures (the files `is_figure` accepts) into a folder of
/// `cache` named after it.
pub fn fetch(
    downloader: &dyn Downloader,
    id: &ArxivId,
    cache: &Path,
    is_figure: &dyn Fn(&str) -> bool,
    progress: &Progress,
) -> Result<Paper, ArxivError> {
    let download = downloader.download(id, progress)?;
    let id = download.version.unwrap_or_else(|| id.clone());
    let folder = cache.join(id.folder_name());
    let unpacked = unpack(&download.bytes, &folder, &id.folder_name(), is_figure)?;
    Ok(Paper { id: id.to_string(), kind: unpacked.kind, folder, figures: unpacked.figures })
}

/// Where the papers' figures are kept: the user's cache folder, so a project made from a figure finds it again.
pub fn cache_dir() -> PathBuf {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library").join("Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("plotextract").join("arxiv")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_from_the_file_name_arxiv_gives() {
        let v = |d: &str| version_of(d).map(|i| i.to_string());
        assert_eq!(v(r#"attachment; filename="arXiv-1706.03762v7.tar.gz""#).as_deref(), Some("1706.03762v7"));
        assert_eq!(v(r#"attachment; filename="arXiv-2401.00001v1.pdf""#).as_deref(), Some("2401.00001v1"));
        assert_eq!(v(r#"attachment; filename="../../evil.tar.gz""#), None);
        assert_eq!(v("inline"), None);
    }

    struct Fake(Vec<u8>);

    impl Downloader for Fake {
        fn download(&self, id: &ArxivId, progress: &Progress) -> Result<Download, ArxivError> {
            progress.done.store(self.0.len() as u64, Ordering::Relaxed);
            let version = ArxivId::parse(&format!("{id}v3"));
            Ok(Download { version, bytes: self.0.clone() })
        }
    }

    #[test]
    fn a_paper_is_unpacked_into_a_folder_named_after_its_version() {
        let cache = std::env::temp_dir().join(format!("plotextract-arxiv-{}-fetch", std::process::id()));
        let id = ArxivId::parse("hep-th/9901001").unwrap();
        let progress = Progress::default();
        let paper = fetch(&Fake(b"%PDF-1.7".to_vec()), &id, &cache, &|_| true, &progress).unwrap();
        assert_eq!(paper.id, "hep-th/9901001v3");
        assert_eq!(paper.folder, cache.join("hep-th_9901001v3"));
        assert_eq!(paper.figures[0].path, paper.folder.join("hep-th_9901001v3.pdf"));
        assert_eq!((progress.done(), progress.total()), (8, None));
        std::fs::remove_dir_all(&cache).ok();
    }

    /// Needs the network: `cargo test -p plotextract-arxiv -- --ignored`.
    #[test]
    #[ignore]
    fn a_real_paper_is_downloaded_and_its_figures_found() {
        let cache = std::env::temp_dir().join(format!("plotextract-arxiv-{}-real", std::process::id()));
        let id = ArxivId::parse("1706.03762v7").unwrap();
        let progress = Progress::default();
        let is_figure = |name: &str| name.ends_with(".png") || name.ends_with(".pdf");
        let paper = fetch(&Arxiv, &id, &cache, &is_figure, &progress).unwrap();
        assert_eq!((paper.id.as_str(), paper.kind), ("1706.03762v7", Kind::Archive));
        assert!(paper.figures.iter().any(|f| f.cited), "{:?}", paper.figures);
        assert_eq!(progress.total(), Some(progress.done()));
        std::fs::remove_dir_all(&cache).ok();
    }
}
