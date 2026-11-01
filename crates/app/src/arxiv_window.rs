//! The Import from arXiv window: the number of a paper in, the figures of its source shown as pictures, one of them
//! opened. The model does the fetching; the window keeps what is typed, the figure chosen and the pictures' textures.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Align, Color32, Context, CornerRadius, Key, Layout, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use plotextract_arxiv::{Figure, Kind, Paper};

use crate::model::{ArxivImport, Intent, Thumbnail};
use crate::theme::{self, ACCENT};

/// The width of the window's contents, and of a figure's card, in points.
const WIDTH: f32 = 712.0;
const CARD: egui::Vec2 = vec2(168.0, 178.0);
const COLUMNS: usize = 4;
/// The height of the picture on a card.
const PICTURE: f32 = 122.0;

#[derive(Default)]
pub struct ArxivWindow {
    /// What is typed in the number field.
    input: String,
    selected: Option<PathBuf>,
    /// The fetch whose pictures are kept, and the pictures.
    job: u64,
    pictures: HashMap<PathBuf, Picture>,
    /// The window was closed: put the cursor in the field when it opens.
    focus: bool,
}

enum Picture {
    Drawn(egui::TextureHandle),
    Failed(String),
}

impl ArxivWindow {
    /// Keep the picture of a figure, if it is of the latest paper.
    pub fn receive(&mut self, ctx: &Context, thumbnail: Thumbnail) {
        if thumbnail.id > self.job {
            self.job = thumbnail.id;
            self.pictures.clear();
        }
        if thumbnail.id < self.job {
            return;
        }
        let picture = match thumbnail.picture {
            Ok(b) => {
                let image = egui::ColorImage::from_rgba_unmultiplied([b.width as usize, b.height as usize], &b.rgba);
                let name = format!("arxiv figure {}", thumbnail.path.display());
                Picture::Drawn(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
            }
            Err(e) => Picture::Failed(e),
        };
        self.pictures.insert(thumbnail.path, picture);
    }

    pub fn show(&mut self, ctx: &Context, import: Option<&ArxivImport>, intents: &mut Vec<Intent>) {
        let Some(import) = import else {
            self.focus = true;
            return;
        };
        let mut open = true;
        egui::Window::new("Import from arXiv")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                self.number(ui, import, intents);
                ui.add_space(4.0);
                status(ui, import);
                if let Some(paper) = import.paper() {
                    // a figure chosen from another paper is not this paper's
                    if self.selected.as_ref().is_some_and(|s| !paper.figures.iter().any(|f| f.path == *s)) {
                        self.selected = None;
                    }
                    if !paper.figures.is_empty() {
                        ui.add_space(4.0);
                        self.figures(ui, paper, intents);
                    }
                    ui.separator();
                    self.footer(ui, paper, intents);
                }
            });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            intents.push(Intent::CloseArxiv);
        }
    }

    /// The field for the paper's number, and the button that fetches it.
    fn number(&mut self, ui: &mut Ui, import: &ArxivImport, intents: &mut Vec<Intent>) {
        ui.label("The number of an arXiv paper, or a link to it");
        let busy = import.fetching().is_some();
        ui.horizontal(|ui| {
            let field = egui::TextEdit::singleline(&mut self.input).hint_text("2301.01234").desired_width(WIDTH - 96.0);
            let response = ui.add(field);
            if std::mem::take(&mut self.focus) {
                response.request_focus();
            }
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            let fetch = ui
                .add_enabled(!busy, theme::primary("Fetch").min_size(vec2(84.0, 0.0)))
                .on_hover_text("Download the paper's source from arXiv and show the figures in it");
            if !busy && (enter || fetch.clicked()) {
                intents.push(Intent::FetchArxiv(self.input.clone()));
            }
        });
    }

    /// The figures, four to a row.
    fn figures(&mut self, ui: &mut Ui, paper: &Paper, intents: &mut Vec<Intent>) {
        egui::ScrollArea::vertical().max_height(CARD.y * 2.6 + 16.0).auto_shrink([false, true]).show(ui, |ui| {
            for row in paper.figures.chunks(COLUMNS) {
                ui.horizontal(|ui| {
                    for figure in row {
                        self.card(ui, figure, intents);
                    }
                });
            }
        });
    }

    /// A figure's picture, name, format and size. A click chooses it, a double click imports it.
    fn card(&mut self, ui: &mut Ui, figure: &Figure, intents: &mut Vec<Intent>) {
        let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
        let visuals = ui.visuals();
        let chosen = self.selected.as_ref() == Some(&figure.path);
        let stroke = if chosen {
            Stroke::new(2.0, ACCENT)
        } else if response.hovered() {
            visuals.widgets.hovered.bg_stroke
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        let painter = ui.painter_at(rect.expand(2.0));
        painter.rect(rect, CornerRadius::same(6), visuals.faint_bg_color, stroke, StrokeKind::Inside);

        let area = egui::Rect::from_min_size(rect.min + vec2(6.0, 6.0), vec2(CARD.x - 12.0, PICTURE));
        match self.pictures.get(&figure.path) {
            Some(Picture::Drawn(texture)) => {
                let size = texture.size_vec2();
                let k = (area.width() / size.x).min(area.height() / size.y);
                let shown = egui::Rect::from_center_size(area.center(), size * k);
                painter.rect_filled(shown, CornerRadius::ZERO, Color32::WHITE);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                painter.image(texture.id(), shown, uv, Color32::WHITE);
            }
            Some(Picture::Failed(_)) => {
                let font = egui::TextStyle::Small.resolve(ui.style());
                let color = visuals.weak_text_color();
                painter.text(area.center(), egui::Align2::CENTER_CENTER, "No picture", font, color);
            }
            None => {
                let spinner = egui::Rect::from_center_size(area.center(), vec2(18.0, 18.0));
                ui.put(spinner, egui::Spinner::new());
            }
        }

        let text =
            egui::Rect::from_min_max(egui::pos2(rect.left() + 8.0, area.bottom() + 6.0), rect.max - vec2(8.0, 4.0));
        let file = figure.name.rsplit('/').next().unwrap_or(&figure.name);
        let ext = file.rsplit_once('.').map_or("", |(_, e)| e).to_ascii_uppercase();
        let mut about = format!("{ext} \u{b7} {}", size(figure.bytes));
        if !figure.cited {
            about += " \u{b7} not drawn";
        }
        // the name and the format under it, inside the card whatever the row's layout
        ui.scope_builder(egui::UiBuilder::new().max_rect(text).layout(Layout::top_down(Align::Min)), |ui| {
            ui.set_width(text.width());
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.add(egui::Label::new(RichText::new(file).strong()).truncate().selectable(false));
            ui.add(egui::Label::new(RichText::new(about).small().weak()).truncate().selectable(false));
        });

        let drawn = if figure.cited { "The paper draws it." } else { "The paper's TeX files do not draw it." };
        let tip = match self.pictures.get(&figure.path) {
            Some(Picture::Failed(e)) => format!("{}\n{drawn}\n{e}", figure.name),
            _ => format!("{}\n{drawn}\nDouble-click to import it.", figure.name),
        };
        let response = response.on_hover_text(tip);
        if response.clicked() {
            self.selected = Some(figure.path.clone());
        }
        if response.double_clicked() {
            intents.push(Intent::ImportFigure(figure.path.clone()));
        }
    }

    /// Where the figures are kept, and the buttons.
    fn footer(&mut self, ui: &mut Ui, paper: &Paper, intents: &mut Vec<Intent>) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let chosen = self.selected.clone();
            let import = ui
                .add_enabled(chosen.is_some(), theme::primary("Import").min_size(vec2(84.0, 0.0)))
                .on_hover_text("Open the chosen figure")
                .on_disabled_hover_text("Choose a figure first");
            if import.clicked()
                && let Some(path) = chosen
            {
                intents.push(Intent::ImportFigure(path));
            }
            if ui.button("Cancel").clicked() {
                intents.push(Intent::CloseArxiv);
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let folder = paper.folder.display().to_string();
                ui.add(egui::Label::new(RichText::new(format!("Kept in {folder}")).small().weak()).truncate())
                    .on_hover_text(folder);
            });
        });
    }
}

/// What the window is doing, or what it found.
fn status(ui: &mut Ui, import: &ArxivImport) {
    if let Some((id, progress)) = import.fetching() {
        ui.horizontal(|ui| {
            ui.spinner();
            let text = match progress.total() {
                Some(total) if progress.done() >= total => format!("Unpacking {id}\u{2026}"),
                Some(total) => format!("Downloading {id}\u{2026} {} of {}", size(progress.done()), size(total)),
                None if progress.done() > 0 => format!("Downloading {id}\u{2026} {}", size(progress.done())),
                None => format!("Asking arXiv for {id}\u{2026}"),
            };
            ui.label(text);
        });
        // the download runs on another thread and does not wake the window
        ui.ctx().request_repaint_after(Duration::from_millis(100));
    } else if let Some(error) = import.error() {
        ui.label(RichText::new(error).color(Color32::from_rgb(214, 70, 70)));
    } else if let Some(paper) = import.paper() {
        let n = paper.figures.len();
        let drawn = paper.figures.iter().filter(|f| f.cited).count();
        let text = match paper.kind {
            Kind::Pdf => {
                format!("arXiv has only the PDF of {}: import it to work on the figures on its pages.", paper.id)
            }
            Kind::Tex => format!("{} has no figure files: its source is a single TeX file.", paper.id),
            Kind::Archive if n == 0 => format!("{} has no figures in a format plotextract opens.", paper.id),
            Kind::Archive if drawn == 0 => format!("{}: {n} figure{}.", paper.id, plural(n)),
            Kind::Archive if drawn == n => {
                format!("{}: {n} figure{}, in the order the paper draws them.", paper.id, plural(n))
            }
            Kind::Archive => format!(
                "{}: {n} figure{}, the {drawn} the paper draws first, in the order it draws them.",
                paper.id,
                plural(n)
            ),
        };
        theme::hint(ui, &text);
    } else {
        theme::hint(ui, "The paper's source is downloaded, and every figure in it is shown here to choose from.");
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// A size in bytes, for a person.
fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1 << 10 => format!("{b} B"),
        b if b < 1 << 20 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / f64::from(1 << 20)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_text() {
        assert_eq!((size(512), size(150_000), size(8_406_248)), ("512 B".into(), "146 KB".into(), "8.0 MB".into()));
    }
}
