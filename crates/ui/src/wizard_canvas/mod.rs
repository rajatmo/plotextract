//! The canvas side of the wizard: dragging out the crop and its handles, painting a mask with a rectangle, a polygon or a
//! brush, lighting the colours, the straight lines and the pieces of the curves under the pointer, selecting pieces, the
//! right-click menus, and drawing the masks and the groups of pieces over the image. What the user does leaves as
//! [`Intent`]s; what is half done (a drag in progress, a polygon not yet closed) lives here, since it is not yet an edit.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{
    Color32, Context, CornerRadius, CursorIcon, Key, Painter, PointerButton, Pos2, Response, Stroke, TextureHandle, Ui,
    vec2,
};
use kurbo::{Point, Rect};
use plotextract_algo::ColourMap;
use plotextract_algo::curves::Pieces;
use plotextract_algo::lines::{Line, Orientation};
use plotextract_domain::Slot;
use plotextract_domain::{Grid, MaskOp, Shape};

use crate::view::View;
use plotextract_model::Intent;
use plotextract_model::text::{axis_letter, tick_name};
use plotextract_usecases::wizard::{
    MaskId, Pen, Pick, Role, Step, Tool, Wizard, WizardIntent, on_x_axis, orientation_of,
};

mod pictures;
mod scene;
#[cfg(test)]
mod tests;
mod tools;

use self::pictures::*;
pub(crate) use self::scene::{PieceLayer, Scene, scene};
use self::tools::colours::*;
pub(crate) use self::tools::links::link_tint;
use self::tools::links::*;
use self::tools::prepare::*;
use self::tools::shapes::*;
pub(crate) use self::tools::shapes::{LEFTOVER, group_tint};
use self::tools::ticks::*;

/// How close, in screen pixels, the pointer must be to grab a crop edge or corner.
const GRAB_PX: f32 = 8.0;

/// How strongly an ink's mask is tinted where it covers a pixel fully (0 to 255).
const TINT: u8 = 150;

/// A pale edge under coloured lines drawn over the image, so they show on red, dark or light alike.
const HALO: Color32 = Color32::from_rgba_premultiplied(200, 200, 200, 200);

/// A colour to tint ink `ink` with, different from its neighbours' and not the image's own.
pub(crate) fn tint_of(ink: usize) -> Color32 {
    const PALETTE: [[u8; 3]; 8] = [
        [255, 0, 200],
        [90, 255, 0],
        [0, 200, 255],
        [255, 150, 0],
        [170, 80, 255],
        [255, 235, 0],
        [0, 255, 160],
        [255, 70, 70],
    ];
    let [r, g, b] = PALETTE[ink % PALETTE.len()];
    Color32::from_rgb(r, g, b)
}

/// The colour a tick is drawn with, different from the other three and from the ink tints.
pub(crate) fn tick_tint(tick: Slot) -> Color32 {
    tint_of(match tick {
        Slot::XMin => 3,
        Slot::XMax => 5,
        Slot::YMin => 2,
        Slot::YMax => 4,
    })
}

pub struct WizardCanvas {
    /// The line under the pointer (the plot area step).
    tick_hover: Option<Line>,
    /// The line that was right-clicked, while its menu is open.
    menu_line: Option<Line>,
    /// The piece under the pointer (the shapes step).
    piece_hover: Option<usize>,
    /// The piece that was right-clicked, while its menu is open.
    menu_piece: Option<usize>,
    pieces_picture: Option<PiecesPicture>,
    /// The links step: the picture of the pieces, where the pointer is (in image pixels), and the piece a drag linked
    /// last.
    links_picture: Option<PiecesPicture>,
    link_pointer: Option<Point>,
    dragged_piece: Option<usize>,
    /// The outline of the group under the pointer, and of the pieces selected.
    outline: Option<Outline>,
    selection_outline: Option<Outline>,
    gesture: Gesture,
    /// The tool and pen the gesture in progress was made with: changing either drops it.
    last_tool: Option<(Option<Tool>, Pen)>,
    overlay: Option<Overlay>,
    /// The lit colour: the one under the pointer, or in the panel, or selected.
    spotlight: Option<InkPicture>,
    /// The inks whose masks are shown.
    layers: HashMap<usize, Option<InkPicture>>,
    /// The colour under the pointer on the image (the colour step).
    hovered: Option<usize>,
    /// The mask whose row the pointer is on in a side panel.
    panel_hover: Option<MaskId>,
    /// The shapes step: the line drawn around pieces to select them, in image pixels.
    lasso: Vec<Point>,
}

impl Default for WizardCanvas {
    fn default() -> Self {
        Self {
            tick_hover: None,
            menu_line: None,
            piece_hover: None,
            menu_piece: None,
            pieces_picture: None,
            links_picture: None,
            link_pointer: None,
            dragged_piece: None,
            outline: None,
            selection_outline: None,
            gesture: Gesture::Idle,
            last_tool: None,
            overlay: None,
            spotlight: None,
            layers: HashMap::new(),
            hovered: None,
            panel_hover: None,
            lasso: Vec::new(),
        }
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.x0 <= inner.x0 && outer.y0 <= inner.y0 && outer.x1 >= inner.x1 && outer.y1 >= inner.y1
}

impl WizardCanvas {
    /// Whether the pointer pans the image (instead of drawing) with the primary button: when no tool is chosen, and not on
    /// the shapes step (a drag goes around pieces; the middle button pans).
    pub fn pans(wizard: &Wizard) -> bool {
        wizard.tool.is_none() && !matches!(wizard.step(), Step::Shapes | Step::Links)
    }

    /// The colour the pointer is on, on the image.
    pub fn hovered(&self) -> Option<usize> {
        self.hovered
    }

    /// Light a colour because its row in the side panel is under the pointer.
    pub fn set_panel_hover(&mut self, mask: Option<MaskId>) {
        self.panel_hover = mask;
    }

    /// The menu of a right click: on the colour step what the colour is, on the plot area step which tick the line is, on
    /// the shapes step the piece's group and the selection.
    pub fn context_menu(&mut self, response: &Response, wizard: &Wizard, intents: &mut Vec<Intent>) {
        match wizard.step() {
            Step::Colours => {
                response.context_menu(|ui| colour_menu(ui, wizard, intents));
            }
            Step::PlotArea => {
                let line = self.menu_line;
                response.context_menu(|ui| line_menu(ui, wizard, line, intents));
                if !response.context_menu_opened() {
                    self.menu_line = None;
                }
            }
            Step::Shapes => {
                let piece = self.menu_piece;
                response.context_menu(|ui| shape_menu(ui, wizard, piece, intents));
                if !response.context_menu_opened() {
                    self.menu_piece = None;
                }
            }
            Step::Links => {
                let piece = self.menu_piece;
                response.context_menu(|ui| link_menu(ui, wizard, piece, intents));
                if !response.context_menu_opened() {
                    self.menu_piece = None;
                }
            }
            Step::Prepare => {}
        }
    }

    /// Read the pointer and keys; return the edits that finished.
    pub fn interact(
        &mut self,
        ui: &Ui,
        response: &Response,
        view: &View,
        origin: Pos2,
        wizard: &Wizard,
    ) -> Vec<Intent> {
        let tool = (wizard.tool, wizard.pen);
        if self.last_tool != Some(tool) {
            self.gesture = Gesture::Idle;
            self.last_tool = Some(tool);
        }
        // with no tool the pointer moves the image, and on the later steps lights what it is on
        let step = if wizard.tool.is_none() { wizard.step() } else { Step::Prepare };
        if step != Step::Colours {
            self.hovered = None;
        }
        if step != Step::PlotArea {
            self.tick_hover = None;
        }
        if !matches!(step, Step::Shapes | Step::Links) {
            self.piece_hover = None;
        }
        if step != Step::Shapes {
            self.lasso.clear();
        }
        if step != Step::Links {
            self.link_pointer = None;
        }
        match step {
            Step::Colours => self.interact_colours(ui, response, view, origin, wizard),
            Step::PlotArea => self.interact_ticks(ui, response, view, origin, wizard),
            Step::Shapes => self.interact_shapes(ui, response, view, origin, wizard),
            Step::Links => self.interact_links(ui, response, view, origin, wizard),
            Step::Prepare => self.interact_prepare(ui, response, view, origin, wizard),
        }
    }

    /// Draw what [`scene`] says the image shows, and what is being drawn, over the image.
    pub fn paint(&mut self, painter: &Painter, view: &View, origin: Pos2, canvas: egui::Rect, wizard: &Wizard) {
        let scene = scene(wizard);
        if scene.exclude {
            self.paint_mask(painter.ctx(), painter, view, origin, canvas, wizard);
        } else {
            self.overlay = None;
        }
        if let Some(map) = wizard.colours().map().filter(|_| wizard.inks_current()) {
            let shown = scene.inks.clone();
            self.layers.retain(|ink, _| shown.contains(ink));
            for ink in shown {
                let slot = self.layers.entry(ink).or_default();
                let tint = tint_of(ink);
                let look =
                    if wizard.active() == MaskId::Ink(ink) { Look::OutlinedTint(tint) } else { Look::Tint(tint) };
                Self::paint_ink(slot, painter, view, origin, canvas, wizard, map, ink, look);
            }
            // the colour lit: under the pointer on the image or in a panel, or the one selected on the colour step
            let panel = if let Some(MaskId::Ink(i)) = self.panel_hover { Some(i) } else { None };
            let selected = wizard.colours().selected().filter(|_| wizard.step() == Step::Colours);
            match self.hovered.or(panel).or(selected) {
                Some(ink) => Self::paint_ink(
                    &mut self.spotlight,
                    painter,
                    view,
                    origin,
                    canvas,
                    wizard,
                    map,
                    ink,
                    Look::Spotlight,
                ),
                None => self.spotlight = None,
            }
        } else {
            self.layers.clear();
            self.spotlight = None;
        }

        self.paint_lines(painter, view, origin, wizard, &scene);
        self.paint_pieces(painter, view, origin, canvas, wizard, &scene);
        self.paint_links(painter, view, origin, canvas, wizard, &scene);
        self.paint_prepare(painter, view, origin, canvas, wizard, &scene);
    }
}
