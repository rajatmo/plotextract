//! The tracing wizard's jobs: the analysis of the colours, the search for lines and the search for pieces and shapes,
//! each run on a thread, one at a time, the answer of a job no longer waited for dropped.

use super::*;

/// The colours of the prepared image, to be found. Run it anywhere with [`AnalysisJob::run`].
pub struct AnalysisJob {
    pub(super) id: u64,
    pub(super) bytes: Arc<Vec<u8>>,
    pub(super) prepare: Prepare,
    pub(super) params: ColourParams,
}

impl std::fmt::Debug for AnalysisJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnalysisJob").field("id", &self.id).finish_non_exhaustive()
    }
}

impl AnalysisJob {
    /// The slow part: read the pixels and find the colours. Safe to call on any thread.
    pub fn run(self) -> Analysed {
        let result = decode::decode(&self.bytes)
            .map(|image| Arc::new(analyse_colours(&image, &self.prepare, &self.params)))
            .map_err(|e| e.to_string());
        Analysed { id: self.id, result }
    }
}

/// The answer to an [`AnalysisJob`].
pub struct Analysed {
    pub(super) id: u64,
    pub(super) result: Result<Arc<ColourMap>, String>,
}

/// A search for the straight lines of the axes. Run it anywhere with [`LineJob::run`].
pub struct LineJob {
    pub(super) id: u64,
    pub(super) search: LineSearch,
}

impl std::fmt::Debug for LineJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineJob").field("id", &self.id).finish_non_exhaustive()
    }
}

impl LineJob {
    /// The slow part. Safe to call on any thread.
    pub fn run(self) -> LinesFound {
        LinesFound { id: self.id, lines: self.search.run() }
    }
}

/// The answer to a [`LineJob`].
pub struct LinesFound {
    pub(super) id: u64,
    pub(super) lines: Vec<Line>,
}

/// A search for the pieces of the curves and their shapes. Run it anywhere with [`PieceJob::run`].
pub struct PieceJob {
    pub(super) id: u64,
    pub(super) search: PieceSearch,
}

impl std::fmt::Debug for PieceJob {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PieceJob").field("id", &self.id).finish_non_exhaustive()
    }
}

impl PieceJob {
    /// The slow part. Safe to call on any thread.
    pub fn run(self) -> PiecesFound {
        PiecesFound { id: self.id, found: self.search.run() }
    }
}

/// The answer to a [`PieceJob`].
pub struct PiecesFound {
    pub(super) id: u64,
    pub(super) found: Found,
}

impl Model {
    /// Take the answer of an [`AnalysisJob`]. An answer to a job that is no longer the one waited for is dropped.
    pub fn finish_analysis(&mut self, analysed: Analysed) {
        let Some(w) = &mut self.wizard else { return };
        match analysed.result {
            Ok(map) => w.finish_analysis(analysed.id, Ok(map)),
            Err(e) => {
                w.finish_analysis(analysed.id, Err(()));
                self.error(format!("Cannot look at the colours: {e}"));
            }
        }
    }

    /// Take the answer of a [`LineJob`]. An answer to a job that is no longer the one waited for is dropped. If the lines were
    /// asked for again meanwhile, the next search starts now: one runs at a time, so dragging a slider does not pile up
    /// threads.
    pub fn finish_lines(&mut self, found: LinesFound) -> Vec<Effect> {
        if let Some(w) = &mut self.wizard {
            w.finish_lines(found.id, found.lines);
        }
        let mut fx = Vec::new();
        self.start_lines_if_wanted(&mut fx);
        // the pieces wait for the lines of the axes, which are cut out of them
        self.start_pieces_if_wanted(&mut fx);
        fx
    }

    /// Take the answer of a [`PieceJob`]. As with the lines, one search runs at a time: if the pieces were asked for
    /// again meanwhile, the next search starts now.
    pub fn finish_pieces(&mut self, found: PiecesFound) -> Vec<Effect> {
        if let Some(w) = &mut self.wizard {
            w.finish_pieces(found.id, found.found);
        }
        let mut fx = Vec::new();
        self.start_pieces_if_wanted(&mut fx);
        fx
    }

    /// Start a search for the pieces of the curves if the wizard asks for one and none is running.
    pub(super) fn start_pieces_if_wanted(&mut self, fx: &mut Vec<Effect>) {
        let Some(w) = &mut self.wizard else { return };
        let Some(search) = w.take_pieces_request() else { return };
        self.next_job += 1;
        let id = self.next_job;
        w.begin_pieces(id);
        fx.push(Effect::Run(Job::Pieces(Box::new(PieceJob { id, search }))));
    }

    /// Start the searches the wizard asks for after a change.
    pub(super) fn start_wizard_jobs(&mut self, fx: &mut Vec<Effect>) {
        self.start_analysis_if_wanted(fx);
        self.start_lines_if_wanted(fx);
        self.start_pieces_if_wanted(fx);
    }

    /// Start a search for lines if the wizard asks for one and none is running.
    pub(super) fn start_lines_if_wanted(&mut self, fx: &mut Vec<Effect>) {
        let Some(w) = &mut self.wizard else { return };
        let Some(search) = w.take_lines_request() else { return };
        self.next_job += 1;
        let id = self.next_job;
        w.begin_lines(id);
        fx.push(Effect::Run(Job::Lines(Box::new(LineJob { id, search }))));
    }

    /// Start the analysis of the colours if the wizard asks for one.
    pub(super) fn start_analysis_if_wanted(&mut self, fx: &mut Vec<Effect>) {
        let (Some(w), Some(source)) = (&mut self.wizard, &self.source) else { return };
        let Some(params) = w.take_analysis_request() else { return };
        self.next_job += 1;
        let id = self.next_job;
        w.begin_analysis(id);
        fx.push(Effect::Run(Job::Analyse(Box::new(AnalysisJob {
            id,
            bytes: source.bytes().clone(),
            prepare: w.prepare().clone(),
            params,
        }))));
    }
}
