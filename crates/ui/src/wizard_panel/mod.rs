//! The panels while an image is being traced. Both read the wizard and return what the user chose as [`Intent`]s.
//!
//! **On the left, the masks** that exist so far ([`masks`]): none to begin with, then the crop and the exclude mask once
//! they are made, a mask for every colour that is not ignored, and each tick once it has a line. The list grows as the
//! steps are taken. Each row opens to show what is known about the mask: its ID, its tags and its properties. The groups
//! of pieces are listed on their own step, where they are fixed.
//!
//! **On the right, the step**: its name and what it is for, what is chosen in it, and the way on. Each step has a file of
//! its own: [`step_prepare`] has the tools that make the crop and the exclude mask, [`step_colours`] has the list of colours
//! to say what each is, [`step_plot_area`] has the search for lines and the four ticks with their values, and
//! [`step_shapes`] has the groups of the pieces of the curves, the selection, and the two changes made to it, and
//! [`step_links`] the group to link, the links in order, and the copying and saving of their points.

mod masks;
mod step_colours;
mod step_links;
mod step_plot_area;
mod step_prepare;
mod step_shapes;
mod thumbs;

use std::collections::BTreeSet;

use crate::theme::{self, Btn};
use plotextract_model::Intent;
use plotextract_usecases::wizard::{MaskId, Step, Wizard, WizardIntent};

/// What the panels remember between frames.
#[derive(Default)]
pub struct WizardPanel {
    /// The text of the value boxes of the four ticks, so half-typed numbers survive.
    values: [String; 4],
    /// The tolerance while its slider is being dragged: the colours are looked at again only when it is let go.
    tolerance: Option<f32>,
    /// The masks whose rows are open, by ID.
    expanded: BTreeSet<String>,
    /// The sections of the masks panel opened or closed by hand, and the step that was done on: a section is open on its
    /// own step otherwise.
    sections: std::collections::BTreeMap<plotextract_usecases::wizard::masks::Section, bool>,
    sections_step: Option<Step>,
    /// The pictures of the pieces on the shapes step.
    thumbs: thumbs::Thumbs,
}

/// What a panel returns.
pub struct Output {
    pub intents: Vec<Intent>,
    /// The mask whose row the pointer is on, to be outlined on the image.
    pub row_hover: Option<MaskId>,
}

impl Output {
    fn new() -> Self {
        Self { intents: Vec::new(), row_hover: None }
    }
}

impl WizardPanel {
    /// The left panel: the masks. `lit` is the colour the pointer is on in the image, so its row can be shown.
    pub fn masks(&mut self, ui: &mut egui::Ui, wizard: &Wizard, lit: Option<usize>) -> Output {
        self.masks_panel(ui, wizard, lit)
    }

    /// The right panel: the step. `lit` is as for [`WizardPanel::masks`].
    pub fn step(&mut self, ui: &mut egui::Ui, wizard: &Wizard, lit: Option<usize>) -> Output {
        let mut out = Output::new();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            step_card(ui, wizard);
            match wizard.step() {
                Step::Prepare => step_prepare::show(ui, wizard, &mut out.intents),
                Step::Colours => self.colours_step(ui, wizard, lit, &mut out),
                Step::PlotArea => self.plot_area_step(ui, wizard, &mut out),
                Step::Shapes => self.shapes_step(ui, wizard, &mut out),
                Step::Links => self.links_step(ui, wizard, &mut out),
            }
            navigation(ui, wizard, &mut out.intents);
        });
        out
    }
}

/// Which step this is, and what it is for.
fn step_card(ui: &mut egui::Ui, wizard: &Wizard) {
    theme::card(ui, wizard.step().title(), |ui| theme::hint(ui, wizard.step().about()));
}

fn navigation(ui: &mut egui::Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    match wizard.step() {
        Step::Prepare => {
            let next = Btn::new("Next: Colours").primary().tip("Find the colours the plot is drawn in");
            if theme::buttons(ui, &[next]) == Some(0) {
                intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
            }
        }
        Step::Colours => {
            let back = Btn::new("Back").tip("Back to the crop and the exclude mask");
            let next = Btn::new("Next: Plot area")
                .primary()
                .tip("Find the axes and choose the ticks")
                .enabled(wizard.colours().map().is_some())
                .why_not("The colours are still being looked at");
            match theme::buttons(ui, &[back, next]) {
                Some(0) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Prepare))),
                Some(1) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::PlotArea))),
                _ => {}
            }
        }
        Step::PlotArea => {
            let back = Btn::new("Back").tip("Back to the colours");
            let next = Btn::new("Next: Shapes").primary().tip("Find the shapes the curves are drawn with");
            match theme::buttons(ui, &[back, next]) {
                Some(0) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Colours))),
                Some(1) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Shapes))),
                _ => {}
            }
        }
        Step::Shapes => {
            let back = Btn::new("Back").tip("Back to the axes and the ticks");
            let next =
                Btn::new("Next: Link").primary().tip("Link the pieces of a curve in order and write their points");
            match theme::buttons(ui, &[back, next]) {
                Some(0) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::PlotArea))),
                Some(1) => intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Links))),
                _ => {}
            }
        }
        Step::Links => {
            let back = Btn::new("Back").tip("Back to the groups");
            if theme::buttons(ui, &[back]) == Some(0) {
                intents.push(Intent::Wizard(WizardIntent::GoTo(Step::Shapes)));
            }
        }
    }
}
