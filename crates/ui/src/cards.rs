//! The cards the import windows show what can be imported on: a small picture with a name and a line about it under it.

use egui::{Align, Color32, CornerRadius, Layout, Response, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use plotextract_domain::Bitmap;

use crate::theme::ACCENT;

/// A card's size, in points.
pub const CARD: egui::Vec2 = vec2(168.0, 178.0);
/// The cards in a row.
pub const COLUMNS: usize = 4;
/// The height of the picture on a card.
const PICTURE: f32 = 122.0;

/// The picture on a card, once drawn.
pub enum Picture {
    Drawn(egui::TextureHandle),
    Failed(String),
}

impl Picture {
    /// A picture made from what a job drew, named `name` for egui.
    pub fn new(ctx: &egui::Context, name: String, drawn: Result<Bitmap, String>) -> Self {
        match drawn {
            Ok(b) => {
                let image = egui::ColorImage::from_rgba_unmultiplied([b.width as usize, b.height as usize], &b.rgba);
                Picture::Drawn(ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
            }
            Err(e) => Picture::Failed(e),
        }
    }
}

/// A card: its picture (a spinner until there is one), its name in bold and a line about it. Clicked to choose it.
pub fn card(ui: &mut Ui, picture: Option<&Picture>, chosen: bool, name: &str, about: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(CARD, Sense::click());
    let visuals = ui.visuals();
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
    match picture {
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

    let text = egui::Rect::from_min_max(egui::pos2(rect.left() + 8.0, area.bottom() + 6.0), rect.max - vec2(8.0, 4.0));
    // the name and the line under it, inside the card whatever the row's layout
    ui.scope_builder(egui::UiBuilder::new().max_rect(text).layout(Layout::top_down(Align::Min)), |ui| {
        ui.set_width(text.width());
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.add(egui::Label::new(RichText::new(name).strong()).truncate().selectable(false));
        ui.add(egui::Label::new(RichText::new(about).small().weak()).truncate().selectable(false));
    });
    response
}

/// A size in bytes, for a person.
pub fn size(bytes: u64) -> String {
    match bytes {
        b if b < 1 << 10 => format!("{b} B"),
        b if b < 1 << 20 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / f64::from(1 << 20)),
    }
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_text() {
        assert_eq!((size(512), size(150_000), size(8_406_248)), ("512 B".into(), "146 KB".into(), "8.0 MB".into()));
    }
}
