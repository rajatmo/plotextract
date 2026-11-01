//! Slow work: what the model asks the shell to do on another thread (open a file, look at the colours, search for lines or
//! pieces, download a paper, draw a figure small), and what comes back. The model hands out [`Job`]s as effects and takes
//! the [`Answer`]s back with `Model::finish`; it never sees a thread. The shell runs them with one [`Runner`]: a thread
//! for a job, a few workers for a batch, and one channel for every answer.
//!
//! The kinds of job are a closed set, so they are an enum: a new one is a variant here and an arm in `Model::finish`.

use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::{
    Analysed, AnalysisJob, ArxivFetched, ArxivJob, LineJob, LinesFound, LoadJob, Loaded, PartPicture, PartPictureJob,
    PieceJob, PiecesFound, Thumbnail, ThumbnailJob,
};

/// Work to run anywhere, once.
#[derive(Debug)]
pub enum Job {
    Load(Box<LoadJob>),
    Analyse(Box<AnalysisJob>),
    Lines(Box<LineJob>),
    Pieces(Box<PieceJob>),
    Arxiv(Box<ArxivJob>),
    Thumbnail(Box<ThumbnailJob>),
    PartPicture(Box<PartPictureJob>),
}

/// What a job found.
pub enum Answer {
    Loaded(Box<Loaded>),
    Analysed(Analysed),
    Lines(LinesFound),
    Pieces(Box<PiecesFound>),
    Arxiv(Box<ArxivFetched>),
    Thumbnail(Thumbnail),
    PartPicture(PartPicture),
}

impl Job {
    /// Do the work. Safe to call on any thread.
    pub fn run(self) -> Answer {
        match self {
            Job::Load(job) => Answer::Loaded(Box::new(job.run())),
            Job::Analyse(job) => Answer::Analysed(job.run()),
            Job::Lines(job) => Answer::Lines(job.run()),
            Job::Pieces(job) => Answer::Pieces(Box::new(job.run())),
            Job::Arxiv(job) => Answer::Arxiv(Box::new(job.run())),
            Job::Thumbnail(job) => Answer::Thumbnail(job.run()),
            Job::PartPicture(job) => Answer::PartPicture(job.run()),
        }
    }
}

/// How many threads take the jobs of a batch in turn.
const BATCH_WORKERS: usize = 4;

/// Runs jobs on other threads and keeps their answers until they are asked for. `wake` is called after each answer, so
/// the window draws again and takes it.
pub struct Runner {
    answers: (Sender<Answer>, Receiver<Answer>),
}

impl Default for Runner {
    fn default() -> Self {
        Self { answers: channel() }
    }
}

impl Runner {
    /// Run `job` on a thread of its own.
    pub fn run(&self, job: Job, wake: impl Fn() + Send + 'static) {
        let done = self.answers.0.clone();
        std::thread::spawn(move || {
            let _ = done.send(job.run());
            wake();
        });
    }

    /// Run `jobs` on a few threads that take them in turn, so the first answers come at once.
    pub fn run_batch(&self, jobs: Vec<Job>, wake: impl Fn() + Clone + Send + 'static) {
        let workers = jobs.len().min(BATCH_WORKERS);
        let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));
        for _ in 0..workers {
            let (queue, done, wake) = (queue.clone(), self.answers.0.clone(), wake.clone());
            std::thread::spawn(move || {
                loop {
                    let Some(job) = queue.lock().ok().and_then(|mut q| q.pop_front()) else { break };
                    if done.send(job.run()).is_err() {
                        break;
                    }
                    wake();
                }
            });
        }
    }

    /// The answers that came since the last call.
    pub fn answers(&self) -> Vec<Answer> {
        self.answers.1.try_iter().collect()
    }
}
