//! The About window.

use egui::{Align2, Context, RichText, TextureId, vec2};

use crate::theme;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const LICENCE: &str = env!("CARGO_PKG_LICENSE");

/// The libraries the app is built on.
const BUILT_WITH: [(&str, &str); 6] = [
    ("hayro", "reads and renders PDF"),
    ("stet", "interprets PostScript and EPS"),
    ("usvg and resvg", "parse and draw SVG"),
    ("kurbo", "2D paths and transforms"),
    ("egui", "the window and widgets"),
    ("rfd", "native file dialogs"),
];

/// Show the window while `show` is true; closing it (the button, the title bar, Escape) clears `show`.
pub fn window(ctx: &Context, show: &mut bool, icon: TextureId) {
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *show = false;
    }
    let mut open = *show;
    egui::Window::new("About plotextract")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.vertical_centered(|ui| {
                ui.image((icon, vec2(88.0, 88.0)));
                ui.add_space(4.0);
                ui.label(RichText::new("plotextract").heading().strong());
                ui.label(RichText::new(format!("Version {VERSION}")).weak());
                ui.add_space(6.0);
                ui.label(RichText::new("Get the numbers out of a plot.").strong());
                ui.add_space(2.0);
                ui.label("Open a vector figure, calibrate its axes from two ticks, pick the curves and export their points as x,y.");
            });
            ui.add_space(10.0);
            ui.separator();
            egui::Grid::new("about").num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("Author").weak());
                ui.label(AUTHORS);
                ui.end_row();
                ui.label(RichText::new("Licence").weak());
                ui.label(LICENCE.replace(" OR ", " or "));
                ui.end_row();
                ui.label(RichText::new("Copyright").weak());
                ui.label(format!("\u{a9} 2026 {AUTHORS}"));
                ui.end_row();
            });
            ui.add_space(6.0);
            ui.label(RichText::new("Built with").weak());
            egui::Grid::new("built_with").num_columns(2).spacing([14.0, 2.0]).show(ui, |ui| {
                for (name, what) in BUILT_WITH {
                    ui.label(name);
                    theme::hint(ui, what);
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                if ui.add(theme::primary("Close")).clicked() {
                    *show = false;
                }
            });
        });
    *show = *show && open;
}
