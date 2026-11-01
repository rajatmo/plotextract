//! The window: what the user sees and does, drawn with egui from the model. Each frame the shell ([`app`]) gathers what
//! the user did into intents, hands them to the model and performs the effects it returns; the panels and the canvas
//! read the model and the wizard and never change them but through intents.
//!
//! The vector figure has the side [`panel`] and the [`canvas`]; the tracing wizard has the masks and step panels
//! ([`wizard_panel`]) and a tool for each step on the canvas ([`wizard_canvas`]).

mod about;
mod app;
mod arxiv_window;
#[cfg(test)]
mod bench;
mod canvas;
mod cards;
mod cursor;
mod detail;
mod panel;
mod pdf_window;
mod theme;
mod view;
mod wizard_canvas;
mod wizard_panel;

use std::path::PathBuf;

use plotextract_model::Model;

/// What running the window gives back.
pub type Result = eframe::Result;

/// The smallest window, in points: the fixed-width side panel plus room for the page, and a menu bar and status bar
/// above and below.
const MIN_WINDOW: [f32; 2] = [800.0, 560.0];

/// The window and taskbar icon.
fn icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")).expect("the bundled icon is a valid PNG")
}

/// Run the window on `model`, opening `file` first if given, until it is closed.
pub fn run(model: Model, file: Option<PathBuf>) -> Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 760.0])
            .with_min_inner_size(MIN_WINDOW)
            .with_title("plotextract")
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native("plotextract", options, Box::new(|cc| Ok(Box::new(app::App::new(cc, model, file)))))
}
