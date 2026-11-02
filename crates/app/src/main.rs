//! The composition root: the one place that knows every concrete piece. It builds the adapters (the file formats,
//! arXiv, the local disk, the tick networks), hands them to the model through their ports, and runs the window.

// A release build on Windows is a GUI program: no console window behind it.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use plotextract_arxiv::{Arxiv, ArxivPapers, cache_dir};
use plotextract_model::Model;
use plotextract_usecases::{Registry, Workspace};

/// Pick curves from plots and export them as data in plot units.
#[derive(Parser)]
#[command(version, author)]
struct Args {
    /// A plot to open (PDF, SVG, EPS, PostScript or an image).
    file: Option<PathBuf>,
}

/// The file formats the program reads, in the order they are asked.
fn registry() -> Registry {
    Registry::new(vec![
        Arc::new(plotextract_pdf::PdfImporter),
        Arc::new(plotextract_svg::SvgImporter),
        Arc::new(plotextract_ps::PsImporter),
        Arc::new(plotextract_image::RasterImporter),
    ])
}

fn main() -> plotextract_ui::Result {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let args = Args::parse();
    // the images taken out of documents are kept beside the papers' figures, in the user's cache folder
    let cache = cache_dir();
    let workspace = Workspace::new(registry(), Arc::new(plotextract_fs::LocalFiles))
        .with_cache(cache.parent().map_or_else(|| cache.clone(), PathBuf::from));
    let papers = Arc::new(ArxivPapers::new(Arc::new(Arxiv), cache));
    let model = Model::new(workspace, papers).with_tick_models(Arc::new(plotextract_ticks::OnnxTicks::new()));
    plotextract_ui::run(model, args.file)
}
