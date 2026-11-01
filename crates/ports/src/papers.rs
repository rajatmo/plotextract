//! The figures of a paper: where the use cases get them from ([`PaperSource`]), and what they get.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use plotextract_domain::ArxivId;

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

    /// The server said how many bytes there are.
    pub fn set_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
    }

    /// `bytes` more came.
    pub fn add(&self, bytes: u64) {
        self.done.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// Where papers come from: download paper `id`, unpack the files `is_figure` accepts, and list them.
pub trait PaperSource: Send + Sync {
    fn fetch(&self, id: &ArxivId, is_figure: &dyn Fn(&str) -> bool, progress: &Progress) -> Result<Paper, ArxivError>;
}
