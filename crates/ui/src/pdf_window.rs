//! The Import from PDF window, which every PDF opened goes through: the pages of the document and the raster images they
//! draw, shown as pictures, one of them opened. A page's curves are paths, picked; an image is taken out of the document
//! and traced. The model lists the document; the window keeps the part chosen and the pictures' textures.

use std::collections::HashMap;

use egui::{Align, Context, Key, Layout, RichText, Ui, vec2};
use plotextract_model::{Document, Intent, Part, PartPicture, PdfImport};

use crate::arxiv_window::WIDTH;
use crate::cards::{self, CARD, COLUMNS, Picture, plural};
use crate::theme;

#[derive(Default)]
pub struct PdfWindow {
    chosen: Option<Part>,
    /// The reading whose pictures are kept, and the pictures.
    job: u64,
    pictures: HashMap<Part, Picture>,
}

impl PdfWindow {
    /// Keep the picture of a part, if it is of the latest document.
    pub fn receive(&mut self, ctx: &Context, picture: PartPicture) {
        if picture.id > self.job {
            self.job = picture.id;
            self.pictures.clear();
            self.chosen = None;
        }
        if picture.id < self.job {
            return;
        }
        let name = format!("pdf part {} {:?}", picture.id, picture.part);
        self.pictures.insert(picture.part, Picture::new(ctx, name, picture.picture));
    }

    pub fn show(&mut self, ctx: &Context, import: Option<&PdfImport>, intents: &mut Vec<Intent>) {
        let Some(document) = import.and_then(PdfImport::document) else { return };
        let mut open = true;
        egui::Window::new("Import from PDF")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.set_width(WIDTH);
                about(ui, document);
                ui.add_space(4.0);
                self.parts(ui, document, intents);
                ui.separator();
                self.footer(ui, intents);
            });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape)) {
            intents.push(Intent::ClosePdf);
        }
    }

    /// The images, then the pages, each a section that folds away, four to a row. The images come first: an image is the
    /// plot itself, a page is the plot with whatever surrounds it.
    fn parts(&mut self, ui: &mut Ui, document: &Document, intents: &mut Vec<Intent>) {
        let images: Vec<Part> = document.contents.images.iter().copied().map(Part::Image).collect();
        let pages: Vec<Part> = (0..document.contents.pages).map(Part::Page).collect();
        egui::ScrollArea::vertical().max_height(CARD.y * 2.6 + 80.0).auto_shrink([false, true]).show(ui, |ui| {
            for (title, parts) in [("Images", &images), ("Pages", &pages)] {
                if parts.is_empty() {
                    continue;
                }
                let header = RichText::new(format!("{title} ({})", parts.len())).strong();
                // the sections fold per document: a new document shows both open
                let id = ui.make_persistent_id((title, &document.path));
                egui::CollapsingHeader::new(header).id_salt(id).default_open(true).show_unindented(ui, |ui| {
                    for row in parts.chunks(COLUMNS) {
                        ui.horizontal(|ui| {
                            for &part in row {
                                self.card(ui, part, intents);
                            }
                        });
                    }
                });
                ui.add_space(6.0);
            }
        });
    }

    /// A part's picture and what it is. A click chooses it, a double click opens it.
    fn card(&mut self, ui: &mut Ui, part: Part, intents: &mut Vec<Intent>) {
        let (name, about, tip) = match part {
            Part::Page(page) => (
                format!("Page {}", page + 1),
                "Paths, to pick".to_string(),
                "Open the page: its curves are paths, picked as they are drawn.",
            ),
            Part::Image(image) => (
                format!("Image {} \u{b7} page {}", image.index + 1, image.page + 1),
                format!("{} \u{d7} {} px, to trace", image.width, image.height),
                "Take the image out of the document and trace it.",
            ),
        };
        let picture = self.pictures.get(&part);
        let response = cards::card(ui, picture, self.chosen == Some(part), &name, &about);
        let tip = match picture {
            Some(Picture::Failed(e)) => format!("{tip}\n{e}"),
            _ => format!("{tip}\nDouble-click to open it."),
        };
        let response = response.on_hover_text(tip);
        if response.clicked() {
            self.chosen = Some(part);
        }
        if response.double_clicked() {
            intents.push(Intent::ImportPart(part));
        }
    }

    /// The buttons, on one row: a layout from the right would take all the height left, and the window could not shrink
    /// when a section is folded.
    fn footer(&mut self, ui: &mut Ui, intents: &mut Vec<Intent>) {
        let row = vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
            let open = ui
                .add_enabled(self.chosen.is_some(), theme::primary("Open").min_size(vec2(84.0, 0.0)))
                .on_hover_text("Open the chosen page or image")
                .on_disabled_hover_text("Choose a page or an image first");
            if open.clicked()
                && let Some(part) = self.chosen
            {
                intents.push(Intent::ImportPart(part));
            }
            if ui.button("Cancel").clicked() {
                intents.push(Intent::ClosePdf);
            }
        });
    }
}

/// The document's name and what it holds.
fn about(ui: &mut Ui, document: &Document) {
    let (pages, images) = (document.contents.pages, document.contents.images.len());
    let pages_found = format!("{pages} page{}", plural(pages));
    let text = match images {
        0 => format!("No images, {pages_found}. Open a page to pick its paths."),
        n => {
            format!("{n} image{} and {pages_found}. Open an image to trace it, or a page to pick its paths.", plural(n))
        }
    };
    ui.label(RichText::new(&document.name).strong()).on_hover_text(document.path.display().to_string());
    theme::hint(ui, &text);
}
