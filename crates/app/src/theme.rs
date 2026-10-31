//! Look and feel: colours, spacing, and the few widgets the panels share.

use egui::{Align, Color32, Context, CornerRadius, Layout, Margin, RichText, Sense, Stroke, TextStyle, Ui, vec2};

/// The size of every piece of text in the app (the brand titles on the welcome screen and in About aside).
pub const TEXT_SIZE: f32 = 14.0;

/// The accent colour of buttons and selections.
pub const ACCENT: Color32 = Color32::from_rgb(47, 128, 237);
/// Positive status text.
pub const OK: Color32 = Color32::from_rgb(52, 168, 83);

pub const STROKE_COLOR: Color32 = Color32::from_rgba_premultiplied(21, 70, 154, 170);
pub const FILL_COLOR: Color32 = Color32::from_rgba_premultiplied(168, 98, 0, 170);
pub const SELECT_COLOR: Color32 = Color32::from_rgb(220, 20, 20);
pub const MEMBER_COLOR: Color32 = Color32::from_rgb(20, 150, 60);
pub const GUIDE_COLOR: Color32 = Color32::from_rgb(0, 150, 60);

/// Spacing, corner radii, accent colour and type sizes, for both the light and the dark theme.
pub fn apply(ctx: &Context) {
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(10.0, 4.0);
        style.spacing.interact_size.y = 24.0;
        style.spacing.window_margin = Margin::same(16);
        style.spacing.menu_margin = Margin::same(8);
        // keep the scroll bar's lane free so content neither sits under it nor shifts when it appears
        style.spacing.scroll.floating_allocated_width = 10.0;

        let widgets = &mut style.visuals.widgets;
        for w in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(6);
        }
        style.visuals.window_corner_radius = CornerRadius::same(12);
        style.visuals.menu_corner_radius = CornerRadius::same(8);
        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.55);
        style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE); // text on the accent fill
        style.visuals.hyperlink_color = ACCENT;

        for (text_style, size) in [
            (TextStyle::Heading, 20.0),
            (TextStyle::Body, TEXT_SIZE),
            (TextStyle::Button, TEXT_SIZE),
            (TextStyle::Small, TEXT_SIZE),
            (TextStyle::Monospace, TEXT_SIZE),
        ] {
            if let Some(font) = style.text_styles.get_mut(&text_style) {
                font.size = size;
            }
        }
    });
}

/// A yes/no state shown in a card's title row as a dot: filled green when on, an empty ring when off, with the reason
/// in a tooltip.
pub struct Badge {
    pub on: bool,
    pub tip: String,
}

/// A titled panel section.
pub fn card(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui)) {
    card_with_badge(ui, title, None, add_contents);
}

/// A titled panel section with a status dot at the right of its title.
pub fn card_with_badge(ui: &mut Ui, title: &str, badge: Option<Badge>, add_contents: impl FnOnce(&mut Ui)) {
    let visuals = ui.visuals();
    egui::Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(visuals.widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong());
                if let Some(badge) = badge {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| status_dot(ui, &badge));
                }
            });
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(8.0);
}

/// The main action of a row of buttons.
pub fn primary(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(ACCENT)
}

/// Explanatory text: the same size as everything else, in a quieter colour.
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).weak());
}

fn status_dot(ui: &mut Ui, badge: &Badge) {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
    if badge.on {
        ui.painter().circle_filled(rect.center(), 6.0, OK);
    } else {
        ui.painter().circle_stroke(rect.center(), 5.5, Stroke::new(1.5, Color32::GRAY));
    }
    response.on_hover_text(&badge.tip);
}

/// Height of the buttons that span a row.
const BUTTON_HEIGHT: f32 = 28.0;

/// One button of a [`buttons`] row.
pub struct Btn<'a> {
    pub label: &'a str,
    pub primary: bool,
    pub enabled: bool,
    /// Shown when hovering the enabled button.
    pub tip: &'a str,
    /// Shown when hovering the disabled button: why it is disabled.
    pub why_not: &'a str,
}

impl<'a> Btn<'a> {
    pub fn new(label: &'a str) -> Self {
        Self { label, primary: false, enabled: true, tip: "", why_not: "" }
    }

    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn tip(mut self, tip: &'a str) -> Self {
        self.tip = tip;
        self
    }

    pub fn why_not(mut self, why_not: &'a str) -> Self {
        self.why_not = why_not;
        self
    }
}

/// A row of buttons of equal width across the whole row. Returns the index of the one clicked.
pub fn buttons(ui: &mut Ui, items: &[Btn]) -> Option<usize> {
    let mut clicked = None;
    ui.columns(items.len(), |columns| {
        for (i, (ui, b)) in columns.iter_mut().zip(items).enumerate() {
            let button = if b.primary { primary(b.label) } else { egui::Button::new(b.label) };
            let size = vec2(ui.available_width(), BUTTON_HEIGHT);
            // add_sized centres the label in the button
            let mut response = ui.add_enabled_ui(b.enabled, |ui| ui.add_sized(size, button)).inner;
            if !b.tip.is_empty() {
                response = response.on_hover_text(b.tip);
            }
            if !b.why_not.is_empty() {
                response = response.on_disabled_hover_text(b.why_not);
            }
            if response.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// An on/off button that fills its column and shows the accent colour while on. Returns whether it was clicked.
pub fn toggle(ui: &mut Ui, on: bool, label: &str, tip: &str) -> bool {
    let size = vec2(ui.available_width(), BUTTON_HEIGHT - 2.0);
    ui.add_sized(size, egui::Button::new(label).selected(on)).on_hover_text(tip).clicked()
}
