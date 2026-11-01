//! Step 4, shapes: the curve colours broken into pieces, and the shapes the pieces are. Pieces that look alike are a shape
//! type; a type that recurs along a path is the dots or dashes of a curve, one that recurs far apart is markers, rows of
//! different shapes close together are text. Each type is a mask on the left; here it is listed with a picture of its
//! typical piece, said to be another kind, merged into another, and looked through from its oddest piece, where the
//! mistakes of the grouping collect. Pieces are moved between types and paths cut or joined on the image.
//!
//! Keys, while no text box has the focus: Up and Down select the type before or after, 1 to 6 say its kind, S shows it
//! alone, Z zooms to it, N and Shift+N go to its next and previous odd piece.

use egui::{Key, Modifiers, RichText, Ui};
use plotextract_raster::shapes::{Kind, Shapes};

use super::thumbs::shared_scale;
use super::{Output, WizardPanel};
use crate::model::Intent;
use crate::panel::{FIELD_HEIGHT, GRID_GAP, cell};
use crate::text::{kind_label, kind_tip, mask_name, shape_click_label, shape_click_tip};
use crate::theme::{self, Btn};
use crate::wizard::{MaskId, ShapeClick, Wizard, WizardIntent};
use crate::wizard_canvas::shape_tint;

/// The widths of the columns of a type's row, in points.
const THUMB: f32 = 24.0;
const NAME: f32 = 76.0;
const KIND: f32 = 80.0;
const INFO: f32 = 40.0;
/// The side of the pictures of the selected type's pieces.
const BIG_THUMB: f32 = 44.0;
/// A type with this many pieces or fewer (but for lines) is a lone piece; three or more of them are folded into one row.
const LONE: usize = 2;
const FOLD: usize = 3;

impl WizardPanel {
    pub(super) fn shapes_step(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        keys(ui, wizard, &mut out.intents);
        pieces_card(ui, wizard, &mut out.intents);
        offer_card(ui, wizard, &mut out.intents);
        self.shapes_card(ui, wizard, out);
        self.selected_card(ui, wizard, &mut out.intents);
    }

    /// The shape types, each with a picture of its typical piece and the kind it is said to be; the lone pieces folded.
    fn shapes_card(&mut self, ui: &mut Ui, wizard: &Wizard, out: &mut Output) {
        theme::card(ui, "Shapes", |ui| {
            let Some(shapes) = wizard.shapes() else {
                theme::hint(ui, "The shapes appear here once the pieces are found.");
                return;
            };
            click_toggles(ui, wizard, &mut out.intents);
            ui.add_space(4.0);
            let order = wizard.shape_order();
            if order.is_empty() {
                theme::hint(ui, "There are no pieces.");
            }
            let scale = shared_scale(wizard, THUMB);
            let lone = |k: usize| shapes.types[k].kind != Kind::Line && shapes.types[k].members.len() <= LONE;
            let folded: Vec<usize> = order.iter().copied().filter(|&k| lone(k)).collect();
            let folded = if folded.len() >= FOLD { folded } else { Vec::new() };
            let listed: Vec<usize> = order.iter().copied().filter(|k| !folded.contains(k)).collect();
            self.shape_rows(ui, wizard, shapes, &listed, scale, "shape rows", out);
            if !folded.is_empty() {
                let pieces: usize = folded.iter().map(|&k| shapes.types[k].members.len()).sum();
                let holds_selected = wizard.active_shape().is_some_and(|k| folded.contains(&k));
                egui::CollapsingHeader::new(format!("{pieces} lone pieces in {} shapes", folded.len()))
                    .id_salt("lone shapes")
                    .open(holds_selected.then_some(true))
                    .show(ui, |ui| self.shape_rows(ui, wizard, shapes, &folded, scale, "lone rows", out));
            }
            ui.add_space(2.0);
            let new =
                Btn::new("New shape").tip("Make an empty shape and select it; then click pieces to put them in it");
            if theme::buttons(ui, &[new]) == Some(0) {
                out.intents.push(Intent::Wizard(WizardIntent::NewShape));
            }
            theme::hint(
                ui,
                "Dots and dashes linked along a path are a curve; markers are a series each; lines are one series; text \
                 and other pieces are in none. Up and Down select, 1 to 6 say the kind.",
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn shape_rows(
        &mut self,
        ui: &mut Ui,
        wizard: &Wizard,
        shapes: &Shapes,
        rows: &[usize],
        scale: f32,
        id: &str,
        out: &mut Output,
    ) {
        egui::Grid::new(id).num_columns(4).min_col_width(0.0).spacing([GRID_GAP - 2.0, 4.0]).show(ui, |ui| {
            for &k in rows {
                let t = &shapes.types[k];
                let mask = MaskId::Shape(k);
                let thumb = self.thumbs.show(ui, wizard, (!t.members.is_empty()).then_some(t.typical), THUMB, scale);
                // framed in the colour the shape is drawn in on the image
                ui.painter().rect_stroke(
                    thumb.rect,
                    egui::CornerRadius::same(3),
                    egui::Stroke::new(2.0, shape_tint(wizard, k)),
                    egui::StrokeKind::Inside,
                );
                let name = ui
                    .add_sized([NAME, FIELD_HEIGHT], egui::Button::selectable(wizard.active() == mask, mask_name(mask)))
                    .on_hover_text(kind_tip(t.kind));
                if name.clicked() || thumb.clicked() {
                    out.intents.push(Intent::Wizard(WizardIntent::SetActive(mask)));
                }
                if name.hovered() || thumb.hovered() {
                    out.row_hover = Some(mask);
                }
                cell(ui, KIND, |ui| {
                    if let Some(kind) = kind_box(ui, ("shape kind", k), t.kind) {
                        out.intents.push(Intent::Wizard(WizardIntent::SetKind(k, kind)));
                    }
                });
                cell(ui, INFO, |ui| {
                    let n = t.members.len();
                    let mut tip = match (t.recurs, t.spacing) {
                        (true, Some(s)) => {
                            format!(
                                "{n} pieces of {:.0} \u{d7} {:.0} px, about {s:.0} px apart",
                                t.look.length, t.look.breadth
                            )
                        }
                        _ => format!("{n} pieces"),
                    };
                    if t.by_hand {
                        tip += "; changed by hand";
                    }
                    let text = if t.by_hand { format!("\u{d7}{n}*") } else { format!("\u{d7}{n}") };
                    ui.label(RichText::new(text).monospace().weak()).on_hover_text(tip);
                });
                ui.end_row();
            }
        });
    }

    /// The selected type: pictures of its typical piece and its two oddest, its odd pieces one by one, and the way to
    /// zoom to it, show it alone or merge it into another.
    fn selected_card(&mut self, ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
        let (Some(k), Some(shapes)) = (wizard.active_shape(), wizard.shapes()) else {
            return;
        };
        let t = &shapes.types[k];
        theme::card(ui, &format!("{} \u{b7} {}", mask_name(MaskId::Shape(k)), kind_label(t.kind)), |ui| {
            if t.members.is_empty() {
                theme::hint(ui, "No pieces yet. With Move on, click pieces on the image to put them in this shape.");
                return;
            }
            let scale = shared_scale(wizard, BIG_THUMB);
            ui.horizontal(|ui| {
                self.thumbs.show(ui, wizard, Some(t.typical), BIG_THUMB, scale).on_hover_text("Its most typical piece");
                ui.add_space(6.0);
                for &p in t.odd.iter().filter(|&&p| p != t.typical).take(2) {
                    let thumb = self.thumbs.show(ui, wizard, Some(p), BIG_THUMB, scale);
                    if thumb.on_hover_text("One of its oddest pieces: click to look at it on the image").clicked() {
                        intents.extend(focus(wizard, p));
                    }
                }
            });
            let l = &t.look;
            let mut facts = format!(
                "{} pieces of {:.0} \u{d7} {:.0} px, strokes {:.1} px",
                t.members.len(),
                l.length,
                l.breadth,
                l.stroke
            );
            if l.holes > 0 {
                facts += &format!(", {} hole{}", l.holes, if l.holes == 1 { "" } else { "s" });
            }
            if let Some(s) = t.spacing {
                facts += &format!(", about {s:.0} px apart");
            }
            theme::hint(ui, &facts);
            ui.add_space(4.0);

            // the odd pieces, one by one
            let rank = wizard.focus_piece().and_then(|f| t.odd.iter().position(|&p| p == f));
            ui.horizontal(|ui| {
                let back = ui.add_enabled(rank.is_some_and(|r| r > 0), egui::Button::new("<"));
                let at = match rank {
                    Some(r) => format!("Odd piece {} of {}", r + 1, t.odd.len()),
                    None => format!("Oddest first: {} pieces", t.odd.len()),
                };
                ui.label(at);
                let next = ui.add_enabled(rank.is_none_or(|r| r + 1 < t.odd.len()), egui::Button::new(">"));
                let step = if back.on_hover_text("The odd piece before (Shift+N)").clicked() {
                    rank.map(|r| r - 1)
                } else if next.on_hover_text("The next odd piece, the oddest first (N)").clicked() {
                    Some(rank.map_or(0, |r| r + 1))
                } else {
                    None
                };
                if let Some(&p) = step.and_then(|r| t.odd.get(r)) {
                    intents.extend(focus(wizard, p));
                }
            });

            let zoom = Btn::new("Zoom to it").tip("Fit the view to its pieces (Z)");
            let solo = Btn::new(if wizard.solo() { "Show all" } else { "Show alone" })
                .tip("Show this shape alone, the others faint (S)");
            match theme::buttons(ui, &[zoom, solo]) {
                Some(0) => intents.extend(group_bounds(wizard, k).map(Intent::ShowArea)),
                Some(1) => intents.push(Intent::Wizard(WizardIntent::SetSolo(!wizard.solo()))),
                _ => {}
            }

            // merge into another of its colour
            let others: Vec<usize> = wizard
                .shape_order()
                .into_iter()
                .filter(|&o| o != k && shapes.types[o].ink == t.ink && !shapes.types[o].members.is_empty())
                .collect();
            if !others.is_empty() {
                ui.horizontal(|ui| {
                    ui.label("Merge into");
                    egui::ComboBox::from_id_salt(("merge", k)).selected_text("\u{2026}").width(120.0).show_ui(
                        ui,
                        |ui| {
                            for o in others {
                                let text =
                                    format!("{} ({})", mask_name(MaskId::Shape(o)), kind_label(shapes.types[o].kind));
                                if ui.selectable_label(false, text).clicked() {
                                    intents.push(Intent::Wizard(WizardIntent::MergeShape(k, o)));
                                }
                            }
                        },
                    );
                })
                .response
                .on_hover_text("Put all of its pieces in another shape of the same colour");
            }
        });
    }
}

/// How the curve colours are broken into pieces, how alike copies must be, and what was found.
fn pieces_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    theme::card(ui, "Pieces", |ui| {
        if !wizard.has_curves() {
            theme::hint(ui, "No colour is marked Curve. Go back to step 2 and mark the colours of the curves.");
            return;
        }
        let mut params = wizard.curve_params().clone();
        ui.label("Smallest piece");
        // the slider and its value together fill the card's width
        ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
        ui.add(egui::Slider::new(&mut params.min_pixels, 1..=200).logarithmic(true).suffix(" px"))
            .on_hover_text("Pieces with fewer pixels than this are specks, and are dropped");
        if params != *wizard.curve_params() {
            intents.push(Intent::Wizard(WizardIntent::SetCurveParams(params)));
        }
        egui::CollapsingHeader::new("Advanced").id_salt("shape advanced").show(ui, |ui| {
            let mut shape = wizard.shape_params().clone();
            ui.label("How alike copies of a shape must be");
            // indented here: the slider is narrower than the one above
            ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(60.0);
            ui.add(egui::Slider::new(&mut shape.tolerance, 0.3..=2.0).fixed_decimals(2))
                .on_hover_text("Smaller is stricter: pieces must be more alike to be one shape. 1 is the default");
            theme::hint(ui, "Left is stricter, right looser; 1 is the default.");
            if shape != *wizard.shape_params() {
                intents.push(Intent::Wizard(WizardIntent::SetShapeParams(shape)));
            }
        });
        ui.add_space(2.0);
        if wizard.pieces_busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Finding the pieces and their shapes\u{2026}").weak());
            });
        } else if let Some(shapes) = wizard.shapes() {
            let n = wizard.pieces().map_or(0, |p| p.pieces().len());
            ui.label(format!("{n} piece{} found", if n == 1 { "" } else { "s" }));
            for line in summary(wizard, shapes) {
                ui.label(RichText::new(format!("\u{2022} {line}")).weak());
            }
        }
    });
}

/// What the shapes make, as the next step will propose them (before the pieces outside the frame are left out).
fn summary(wizard: &Wizard, shapes: &Shapes) -> Vec<String> {
    let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let list = |counts: &[usize]| {
        let words: Vec<String> = counts.iter().map(usize::to_string).collect();
        match words.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => words.concat(),
        }
    };
    let mut lines = Vec::new();
    let curves: Vec<usize> = shapes
        .curves()
        .iter()
        .map(|c| c.iter().filter(|&&p| matches!(shapes.kind(p), Some(Kind::Dot | Kind::Dash))).count())
        .filter(|&n| n > 0)
        .collect();
    if !curves.is_empty() {
        lines.push(format!("{} of dots or dashes ({} pieces)", plural(curves.len(), "curve", "curves"), list(&curves)));
    }
    let of_kind = |kind: Kind| -> Vec<usize> {
        wizard
            .shape_order()
            .into_iter()
            .map(|k| &shapes.types[k])
            .filter(|t| t.kind == kind && !t.members.is_empty())
            .map(|t| t.members.len())
            .collect()
    };
    let markers = of_kind(Kind::Marker);
    if !markers.is_empty() {
        lines.push(format!("{} ({})", plural(markers.len(), "series of markers", "series of markers"), list(&markers)));
    }
    let lines_found: usize = of_kind(Kind::Line).iter().sum();
    if lines_found > 0 {
        lines.push(plural(lines_found, "solid line", "solid lines"));
    }
    let text: usize = of_kind(Kind::Text).iter().sum();
    if text > 0 {
        lines.push(format!("{} of text, in no series", plural(text, "piece", "pieces")));
    }
    let other: usize = of_kind(Kind::Other).iter().sum();
    if other > 0 {
        lines.push(format!(
            "{} that {} nothing, in no series",
            plural(other, "piece", "pieces"),
            if other == 1 { "is" } else { "are" }
        ));
    }
    lines
}

/// What a click on the image does: select, move pieces, or cut and join paths.
fn click_toggles(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    let now = wizard.shape_click();
    ui.columns(3, |columns| {
        for (column, click) in columns.iter_mut().zip([ShapeClick::Select, ShapeClick::Move, ShapeClick::Links]) {
            if theme::toggle(column, now == click, shape_click_label(click), shape_click_tip(click)) {
                intents.push(Intent::Wizard(WizardIntent::SetShapeClick(click)));
            }
        }
    });
    let hint = match now {
        ShapeClick::Select => "Click a piece to select its shape; right-click to say what it is.".to_string(),
        ShapeClick::Move => match wizard.active_shape() {
            Some(k) => format!(
                "Click a piece to put it in {}, or to take it out; drag around pieces to put them all in (with Alt, \
                 to take them out).",
                mask_name(MaskId::Shape(k))
            ),
            None => "Select the shape to move pieces to first.".into(),
        },
        ShapeClick::Links => "Click a link to cut the path there. Click the end of a path, then another end (or a \
                              piece on no path), to join them. Esc lets go."
            .into(),
    };
    theme::hint(ui, &hint);
}

/// The pieces that look like the one just moved, offered to be moved too.
fn offer_card(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    let Some(offer) = wizard.shape_offer() else { return };
    let n = offer.pieces.len();
    let to =
        offer.to.map_or_else(|| "out of their shape".to_string(), |k| format!("to {}", mask_name(MaskId::Shape(k))));
    theme::card(ui, "Look-alikes", |ui| {
        ui.label(format!(
            "{n} more piece{} of the same shape look{} like the one moved. Move {} {to} too?",
            if n == 1 { "" } else { "s" },
            if n == 1 { "s" } else { "" },
            if n == 1 { "it" } else { "them" }
        ));
        let yes = Btn::new("Move them").primary().tip("Move them as the piece was moved (Enter)");
        let no = Btn::new("No").tip("Leave them where they are (Esc)");
        match theme::buttons(ui, &[yes, no]) {
            Some(0) => intents.push(Intent::Wizard(WizardIntent::LookAlikes(true))),
            Some(1) => intents.push(Intent::Wizard(WizardIntent::LookAlikes(false))),
            _ => {}
        }
    });
}

/// The choice of a kind; the kind chosen, if it changed.
fn kind_box(ui: &mut Ui, id: (&str, usize), now: Kind) -> Option<Kind> {
    let mut kind = now;
    egui::ComboBox::from_id_salt(id).selected_text(kind_label(kind)).width(KIND).show_ui(ui, |ui| {
        for (n, choice) in Kind::ALL.into_iter().enumerate() {
            ui.selectable_value(&mut kind, choice, format!("{}  {}", n + 1, kind_label(choice)))
                .on_hover_text(kind_tip(choice));
        }
    });
    (kind != now).then_some(kind)
}

/// Ring piece `p` on the image and bring it into view.
fn focus(wizard: &Wizard, p: usize) -> Vec<Intent> {
    let mut intents = vec![Intent::Wizard(WizardIntent::FocusPiece(Some(p)))];
    intents.extend(piece_bounds(wizard, &[p]).map(Intent::ShowArea));
    intents
}

/// Where the pieces `members` are on the image, together.
fn piece_bounds(wizard: &Wizard, members: &[usize]) -> Option<kurbo::Rect> {
    let pieces = wizard.pieces()?;
    members
        .iter()
        .filter_map(|&p| pieces.pieces().get(p))
        .map(|p| {
            let b = p.bounds;
            kurbo::Rect::new(f64::from(b.x), f64::from(b.y), f64::from(b.x + b.width), f64::from(b.y + b.height))
        })
        .reduce(|a, b| a.union(b))
}

/// Where the pieces of shape type `k` are.
fn group_bounds(wizard: &Wizard, k: usize) -> Option<kurbo::Rect> {
    piece_bounds(wizard, &wizard.shapes()?.types.get(k)?.members)
}

/// The keys of the shapes step, while no text box has the focus.
fn keys(ui: &mut Ui, wizard: &Wizard, intents: &mut Vec<Intent>) {
    if ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let take = |mods: Modifiers, key: Key| ui.input_mut(|i| i.consume_key(mods, key));
    let order = wizard.shape_order();
    let at = wizard.active_shape().and_then(|k| order.iter().position(|&o| o == k));
    let select = |i: usize| Intent::Wizard(WizardIntent::SetActive(MaskId::Shape(order[i])));
    if !order.is_empty() {
        if take(Modifiers::NONE, Key::ArrowDown) {
            intents.push(select(at.map_or(0, |a| (a + 1).min(order.len() - 1))));
        }
        if take(Modifiers::NONE, Key::ArrowUp) {
            intents.push(select(at.map_or(0, |a| a.saturating_sub(1))));
        }
    }
    if wizard.shape_offer().is_some() {
        if take(Modifiers::NONE, Key::Enter) {
            intents.push(Intent::Wizard(WizardIntent::LookAlikes(true)));
        }
        if take(Modifiers::NONE, Key::Escape) {
            intents.push(Intent::Wizard(WizardIntent::LookAlikes(false)));
        }
    }
    let Some(k) = wizard.active_shape() else { return };
    let digits = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6];
    for (key, kind) in digits.into_iter().zip(Kind::ALL) {
        if take(Modifiers::NONE, key) {
            intents.push(Intent::Wizard(WizardIntent::SetKind(k, kind)));
        }
    }
    if take(Modifiers::NONE, Key::S) {
        intents.push(Intent::Wizard(WizardIntent::SetSolo(!wizard.solo())));
    }
    if take(Modifiers::NONE, Key::Z) {
        intents.extend(group_bounds(wizard, k).map(Intent::ShowArea));
    }
    let odd = wizard.shapes().map(|s| s.types[k].odd.clone()).unwrap_or_default();
    let rank = wizard.focus_piece().and_then(|f| odd.iter().position(|&p| p == f));
    let step = if take(Modifiers::SHIFT, Key::N) {
        rank.and_then(|r| r.checked_sub(1))
    } else if take(Modifiers::NONE, Key::N) {
        Some(rank.map_or(0, |r| r + 1))
    } else {
        None
    };
    if let Some(&p) = step.and_then(|r| odd.get(r)) {
        intents.extend(focus(wizard, p));
    }
}
