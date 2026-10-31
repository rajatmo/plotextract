// A release build on Windows is a GUI program: no console window behind it.
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod about;
mod app;
mod canvas;
mod detail;
mod model;
mod panel;
mod text;
mod theme;
mod view;

use std::path::PathBuf;

use clap::Parser;

/// Pick curves from plots and export them as data in plot units.
#[derive(Parser)]
#[command(version, author)]
struct Args {
    /// A plot to open (PDF, SVG, EPS or PostScript).
    file: Option<PathBuf>,
}

/// The smallest window, in points: the fixed-width side panel plus room for the page, and a menu bar and status bar
/// above and below.
const MIN_WINDOW: [f32; 2] = [800.0, 560.0];

/// The window and taskbar icon.
fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("the bundled icon is a valid PNG")
}

fn main() -> eframe::Result {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let args = Args::parse();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size(MIN_WINDOW)
            .with_title("plotextract")
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native("plotextract", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, args.file)))))
}
