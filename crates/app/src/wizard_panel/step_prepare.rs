//! Step 1, crop and mask: the two tools, Crop and Exclude. With neither chosen, the pointer moves the image. The exclude
//! tool draws with a brush, a rectangle or a polygon, and erases while Alt is held; what it draws can be undone with Ctrl+Z.

use egui::Ui;

use crate::model::Intent;
use crate::theme;
use crate::wizard::{Pen, Tool, Wizard, WizardIntent};

pub(super) fn show(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Tools", |ui| {
        let tools = [
            (
                Tool::Crop,
                "Crop",
                "Drag out the plot; drag its edges and corners to adjust. Click again to move the image",
            ),
            (
                Tool::Exclude,
                "Exclude",
                "Cover what is not part of the plot; hold Alt to erase, Ctrl+Z to undo. Click again to move the image",
            ),
        ];
        ui.columns(2, |columns| {
            for (column, (tool, label, tip)) in columns.iter_mut().zip(tools) {
                let on = wizard.tool == Some(tool);
                if theme::toggle(column, on, label, tip) {
                    // a chosen tool is put down by choosing it again
                    intents.push(Intent::Wizard(WizardIntent::SetTool(if on { None } else { Some(tool) })));
                }
            }
        });
        if wizard.tool == Some(Tool::Exclude) {
            ui.add_space(4.0);
            let pens = [
                (Pen::Brush, "Brush", "Paint a stroke"),
                (Pen::Rectangle, "Rectangle", "Drag out a block"),
                (Pen::Polygon, "Polygon", "Click the corners; double-click or Enter closes it"),
            ];
            ui.columns(3, |columns| {
                for (column, (pen, label, tip)) in columns.iter_mut().zip(pens) {
                    if theme::toggle(column, wizard.pen == pen, label, tip) {
                        intents.push(Intent::Wizard(WizardIntent::SetPen(pen)));
                    }
                }
            });
            if wizard.pen == Pen::Brush {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label("Brush size");
                    let mut width = wizard.brush;
                    let slider = egui::Slider::new(&mut width, 1.0..=300.0).logarithmic(true).suffix(" px");
                    if ui.add_sized([ui.available_width(), 22.0], slider).changed() {
                        intents.push(Intent::Wizard(WizardIntent::SetBrush(width)));
                    }
                });
            }
        }
        ui.add_space(2.0);
        theme::hint(ui, "Excluded pixels are unknown, not paper: what is under them is not guessed.");
    });
}
