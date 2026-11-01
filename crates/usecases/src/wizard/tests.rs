//! The wizard's tests, driven through its intents.
use kurbo::Point;
use plotextract_algo::lines::{Algorithm, Orientation};
use plotextract_domain::{PixelRect, Shape, Value};

use super::fixtures::{analysed, plot_image, plot_wizard, settle};
use super::*;
use kurbo::Rect;
use plotextract_domain::MaskOp;

fn paint(x0: f64, y0: f64, x1: f64, y1: f64) -> WizardIntent {
    WizardIntent::Exclude(MaskOp { shape: Shape::Rect(Rect::new(x0, y0, x1, y1)), erase: false })
}

#[test]
fn it_starts_with_the_whole_image_and_no_tool() {
    let w = Wizard::new(400, 300);
    assert_eq!((w.tool, w.pen, w.crop_pixels()), (None, Pen::Brush, PixelRect { x: 0, y: 0, width: 400, height: 300 }));
    assert!(w.prepare().mask.is_empty() && !w.can_undo());
    assert!((3.0..=60.0).contains(&w.brush));
}

#[test]
fn the_crop_is_made_valid_and_is_not_undone() {
    let mut w = Wizard::new(400, 300);
    w.apply(WizardIntent::SetCrop(Some(Rect::new(350.0, 250.0, 50.0, -20.0))));
    assert_eq!(w.prepare().crop, Some(Rect::new(50.0, 0.0, 350.0, 250.0)));
    assert_eq!(w.crop_pixels(), PixelRect { x: 50, y: 0, width: 300, height: 250 });
    assert!(!w.can_undo(), "a crop is chosen again, not undone");
    let good = Rect::new(10.0, 10.0, 200.0, 200.0);
    w.apply(WizardIntent::SetCrop(Some(good)));
    w.apply(WizardIntent::SetCrop(Some(Rect::new(500.0, 500.0, 600.0, 600.0))));
    assert_eq!(w.prepare().crop, Some(good), "outside the image: the crop stays as it was");
    w.apply(WizardIntent::SetCrop(Some(Rect::new(50.0, 50.0, 52.0, 52.0))));
    assert_eq!(w.prepare().crop, Some(good), "a stray click-sized drag is no crop");
    w.apply(WizardIntent::SetCrop(Some(Rect::new(-50.0, -50.0, 900.0, 900.0))));
    assert_eq!(w.prepare().crop, None, "covering the image is the same as not cropping");
    w.apply(WizardIntent::SetCrop(Some(good)));
    w.apply(WizardIntent::Delete(MaskId::Crop));
    assert_eq!(w.prepare().crop, None, "deleting the crop takes it away");
}

#[test]
fn drawing_the_exclude_mask_is_undone_one_shape_at_a_time_on_the_first_step_only() {
    let mut w = Wizard::new(400, 300);
    w.apply(paint(0.0, 0.0, 10.0, 10.0));
    w.apply(WizardIntent::Exclude(MaskOp {
        shape: Shape::Stroke { points: vec![Point::new(5.0, 5.0), Point::new(50.0, 50.0)], width: 8.0 },
        erase: true,
    }));
    assert_eq!(w.prepare().mask.len(), 2);
    assert!(w.prepare().mask[1].erase);
    assert!(w.undo());
    assert_eq!(w.prepare().mask.len(), 1);
    w.apply(paint(20.0, 20.0, 30.0, 30.0));
    assert!(!w.redo(), "a new shape ends what could be redone");
    w.apply(WizardIntent::Delete(MaskId::Exclude));
    assert!(w.prepare().mask.is_empty());
    assert!(w.undo());
    assert_eq!(w.prepare().mask.len(), 2, "deleting the exclude mask is undone too");
    // on the colours there is nothing to undo: Ctrl+Z there must not quietly change what the colours were found in
    w.apply(WizardIntent::GoTo(Step::Colours));
    assert!(!w.can_undo() && !w.undo());
    assert_eq!(w.prepare().mask.len(), 2);
    w.apply(WizardIntent::GoTo(Step::Prepare));
    assert!(w.can_undo(), "back on the first step the history is there");
}

#[test]
fn choices_are_not_undone() {
    let mut w = Wizard::new(400, 300);
    w.apply(WizardIntent::SetTool(Some(Tool::Exclude)));
    w.apply(WizardIntent::SetPen(Pen::Polygon));
    w.apply(WizardIntent::SetBrush(1000.0));
    assert_eq!((w.tool, w.pen, w.brush), (Some(Tool::Exclude), Pen::Polygon, 500.0));
    assert!(!w.can_undo());
    w.apply(WizardIntent::Delete(MaskId::Exclude));
    assert!(!w.can_undo(), "deleting an empty mask changes nothing, so nothing is recorded");
}

#[test]
fn the_mask_epoch_moves_only_when_the_mask_stops_being_an_extension_of_what_it_was() {
    let mut w = Wizard::new(400, 300);
    let e = w.mask_epoch();
    w.apply(paint(0.0, 0.0, 10.0, 10.0));
    w.apply(paint(20.0, 20.0, 30.0, 30.0));
    w.apply(WizardIntent::SetCrop(Some(Rect::new(10.0, 10.0, 200.0, 200.0))));
    assert_eq!(w.mask_epoch(), e, "adding shapes and cropping leave the picture of the mask valid");
    w.undo();
    assert_eq!(w.mask_epoch(), e + 1);
    w.redo();
    assert_eq!(w.mask_epoch(), e + 2);
    w.apply(WizardIntent::Delete(MaskId::Exclude));
    assert_eq!(w.mask_epoch(), e + 3);
    w.apply(WizardIntent::Delete(MaskId::Exclude));
    assert_eq!(w.mask_epoch(), e + 3, "deleting nothing changes nothing");
}

#[test]
fn a_tool_is_put_down_by_choosing_none_and_the_later_steps_have_none() {
    let mut w = Wizard::new(400, 300);
    w.apply(WizardIntent::SetTool(Some(Tool::Crop)));
    assert_eq!(w.tool, Some(Tool::Crop));
    w.apply(WizardIntent::SetTool(None));
    assert_eq!(w.tool, None, "with no tool the pointer moves the image");
    w.apply(WizardIntent::SetTool(Some(Tool::Exclude)));
    w.apply(WizardIntent::GoTo(Step::Colours));
    assert_eq!(w.tool, None, "the colours have no tools");
    w.apply(WizardIntent::SetTool(Some(Tool::Crop)));
    assert_eq!(w.tool, None, "and none can be chosen there");
    assert!(Step::Prepare.has_tools() && !Step::Colours.has_tools() && !Step::PlotArea.has_tools());
}

#[test]
fn masks_are_shown_hidden_and_selected_and_shapes_always_go_to_the_exclude_mask() {
    let mut w = Wizard::new(400, 300);
    w.apply(WizardIntent::Show(MaskId::Exclude, false));
    w.apply(WizardIntent::Show(MaskId::Crop, false));
    w.apply(WizardIntent::Show(MaskId::Ink(2), true));
    assert!(!w.is_shown(MaskId::Exclude) && !w.is_shown(MaskId::Crop) && w.is_shown(MaskId::Ink(2)));
    assert!(!w.can_undo(), "showing is not an edit");
    w.apply(WizardIntent::SetActive(MaskId::Crop));
    assert_eq!((w.active(), w.tool), (MaskId::Crop, None), "selecting a mask does not choose a tool");
    w.apply(paint(0.0, 0.0, 10.0, 10.0));
    assert_eq!(w.prepare().mask.len(), 1, "whatever is selected, what is drawn is excluded");
}

/// The ink of colour `rgb`.
fn ink_of(w: &Wizard, rgb: [u8; 3]) -> usize {
    w.colours().map().unwrap().inks.iter().position(|i| i.rgb == rgb).expect("an ink of that colour")
}

const BLUE: [u8; 3] = [31, 119, 180];

/// The plot of [`plot_wizard`] with two curves in it, which do not cross: a blue line 2 pixels thick from (50, 100) up
/// to (150, 40), and a black one under it that starts on the frame's left side at (42, 105); and a blue dash outside
/// the frame, as in a legend. Black is said to be the axes and a curve, blue a curve. On the shapes step, found.
fn lines_wizard() -> Wizard {
    let mut image = plot_image();
    let blue = image::Rgba([BLUE[0], BLUE[1], BLUE[2], 255]);
    for x in 50..150u32 {
        let y = (100.0 - f64::from(x - 50) * 0.6).round() as u32;
        image.put_pixel(x, y, blue);
        image.put_pixel(x, y + 1, blue);
    }
    for x in 42..140u32 {
        let y = 105 - (x - 42) / 10;
        image.put_pixel(x, y, image::Rgba([0, 0, 0, 255]));
        image.put_pixel(x, y + 1, image::Rgba([0, 0, 0, 255]));
    }
    for x in 165..190 {
        image.put_pixel(x, 10, blue);
        image.put_pixel(x, 11, blue);
    }
    let mut w = analysed(&image);
    let (black, blue) = (ink_of(&w, [0, 0, 0]), ink_of(&w, BLUE));
    w.apply(WizardIntent::SetRole(black, Role::Axes, true));
    w.apply(WizardIntent::SetRole(black, Role::Curve, true));
    w.apply(WizardIntent::SetRole(blue, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    w
}

#[test]
fn each_line_is_a_group_of_its_own_without_the_frame_and_a_short_dash_is_left_over() {
    let w = lines_wizard();
    assert!(w.has_curves() && !w.pieces_busy());
    let shapes = w.shapes().unwrap();
    let group = |x, y| shapes.type_of[w.pieces().unwrap().at(x, y).expect("a piece there")];
    let (blue, black) = (group(100, 70).expect("the blue line"), group(92, 100).expect("the black curve"));
    assert_ne!(blue, black);
    assert!(shapes.types[blue].solid() && shapes.types[black].solid());
    // the black curve loses the frame it starts on
    assert_eq!(shapes.types[black].members.len(), 1);
    let curve = shapes.types[black].members[0];
    assert!(w.pieces().unwrap().pieces()[curve].bounds.x >= 42, "a flat curve is not taken for a side of the frame");
    assert_eq!(group(170, 10), None, "the dash of the legend is like no other: in no group");
    assert_eq!(shapes.leftovers().len(), 1);
    // lines are listed after the repeated shapes (the four ticks, black as the curve is), named by how they look
    let names: Vec<String> = w.shape_order().into_iter().map(|k| w.group_name(k)).collect();
    assert_eq!(names[0], "Dashes");
    let mut lines = names[1..].to_vec();
    lines.sort();
    assert_eq!(lines, ["Line", "Line 2"]);
    let meta = w.meta(MaskId::Shape(blue));
    assert!(meta.has(Tag::Shape) && meta.has(Tag::Line));
    assert_eq!(meta.get("colour"), Some(&Value::Colour(BLUE)));
}

#[test]
fn without_a_colour_called_curve_there_are_no_pieces() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    assert!(!w.has_curves() && w.pieces().is_none() && w.shape_order().is_empty() && !w.pieces_busy());
}

/// The plot of [`plot_wizard`] with a dotted black curve: 12 dots 4 pixels square, 10 pixels apart, from (50, 100) up
/// to the right; black is said to be the axes and a curve. On the shapes step, found.
fn dotted_wizard() -> Wizard {
    dotted_wizard_with_tall(0)
}

/// The image of [`dotted_wizard_with_tall`].
fn dotted_image(tall: u32) -> image::RgbaImage {
    let mut image = plot_image();
    for k in 0..12 {
        let (cx, cy) = (50.0 + 10.0 * f64::from(k) * 0.894, 100.0 - 10.0 * f64::from(k) * 0.447);
        for dy in 0..if k < tall { 5 } else { 4 } {
            for dx in 0..4 {
                image.put_pixel(cx as u32 + dx, cy as u32 + dy, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    image
}

/// [`dotted_wizard`] with its first `tall` dots a pixel taller than the rest (still copies of one shape).
fn dotted_wizard_with_tall(tall: u32) -> Wizard {
    let mut w = analysed(&dotted_image(tall));
    w.apply(WizardIntent::SetRole(0, Role::Axes, true));
    w.apply(WizardIntent::SetRole(0, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    w
}

/// The group of the dots of [`dotted_wizard`]: that of its last dot.
fn dots_of(w: &Wizard) -> usize {
    let p = w.pieces().unwrap().at(150, 52).expect("the last dot");
    w.shapes().unwrap().type_of[p].expect("in a group")
}

/// The pieces of group `k`.
fn members(w: &Wizard, k: usize) -> Vec<usize> {
    w.shapes().unwrap().types[k].members.clone()
}

/// Select `pieces` (in place of what was).
fn select(w: &mut Wizard, pieces: &[usize]) {
    w.apply(WizardIntent::SelectPieces(pieces.to_vec(), Pick::Replace));
}

#[test]
fn the_dots_of_a_dotted_curve_are_one_group_listed_first_and_nothing_is_selected() {
    let w = dotted_wizard();
    let dots = dots_of(&w);
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.types[dots].members.len(), 12, "{:?}", shapes.types);
    assert!(!shapes.types[dots].solid(), "a series of shapes, not a solid line");
    assert_eq!(w.shape_order()[0], dots);
    assert_eq!(w.group_name(dots), "Dots");
    assert!(w.selected().is_empty());
    let meta = w.meta(MaskId::Shape(dots));
    assert_eq!(meta.id, format!("shape-{}", dots + 1));
    assert!(meta.has(Tag::Shape) && !meta.has(Tag::Line) && !meta.has(Tag::ByHand));
    assert_eq!(meta.get("pieces"), Some(&Value::Count(12)));
}

#[test]
fn pieces_are_selected_replaced_added_toggled_and_taken_away() {
    let mut w = dotted_wizard();
    let dots = members(&w, dots_of(&w));
    select(&mut w, &dots[..3]);
    assert_eq!(w.selected().len(), 3);
    w.apply(WizardIntent::SelectPieces(dots[2..5].to_vec(), Pick::Add));
    assert_eq!(w.selected().len(), 5);
    w.apply(WizardIntent::SelectPieces(dots[..2].to_vec(), Pick::Subtract));
    assert_eq!(w.selected().iter().copied().collect::<Vec<_>>(), {
        let mut v = dots[2..5].to_vec();
        v.sort();
        v
    });
    // toggling takes out what is all in, and adds what is not
    w.apply(WizardIntent::SelectPieces(dots[2..5].to_vec(), Pick::Toggle));
    assert!(w.selected().is_empty());
    w.apply(WizardIntent::SelectPieces(dots[4..6].to_vec(), Pick::Toggle));
    assert_eq!(w.selected().len(), 2);
    // a group's row selects its pieces
    w.apply(WizardIntent::SetActive(MaskId::Shape(dots_of(&w))));
    assert_eq!(w.selected().len(), 12);
    assert!(!matches!(w.active(), MaskId::Shape(_)), "a group is not a mask to select");
    select(&mut w, &[]);
    assert!(w.selected().is_empty());
}

/// Search for the pieces again, as a new setting does, and wait for them.
fn search_again(w: &mut Wizard) {
    let mut params = w.curve_params().clone();
    params.min_pixels = if params.min_pixels == 4 { 3 } else { 4 };
    w.apply(WizardIntent::SetCurveParams(params));
    settle(w);
}

#[test]
fn pieces_removed_are_left_over_stay_so_when_found_again_and_it_is_undone_and_redone() {
    let mut w = dotted_wizard();
    let dots = dots_of(&w);
    let piece = members(&w, dots)[5];
    select(&mut w, &[piece]);
    w.apply(WizardIntent::RemoveSelected);
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.type_of[piece], None, "taken out, it is left over");
    assert_eq!(shapes.leftovers(), [piece]);
    assert_eq!(shapes.types[dots].members.len(), 11);
    assert!(w.meta(MaskId::Shape(dots)).has(Tag::ByHand));
    assert!(w.selected().contains(&piece), "what was removed stays selected, to be seen");

    search_again(&mut w);
    assert_eq!(w.shapes().unwrap().type_of[piece], None, "the change is made again on the pieces found again");
    assert!(w.selected().contains(&piece), "and the selection follows its pieces");

    assert!(w.can_undo() && !w.can_redo());
    assert!(w.undo());
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 12);
    assert!(w.can_redo());
    assert!(w.redo());
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 11);
    // undoing is for the step it is on
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    assert!(!w.can_undo() && !w.undo());
    w.apply(WizardIntent::GoTo(Step::Shapes));
    assert!(w.can_undo());
}

#[test]
fn some_pieces_made_one_group_are_a_new_group_and_two_groups_made_one_keep_the_larger() {
    let mut w = dotted_wizard();
    let dots = dots_of(&w);
    let groups = w.shape_order().len();
    let some = members(&w, dots)[..3].to_vec();
    select(&mut w, &some);
    w.apply(WizardIntent::JoinSelected);
    let shapes = w.shapes().unwrap();
    let made = shapes.type_of[some[0]].expect("in a group");
    assert_ne!(made, dots);
    assert_eq!((shapes.types[made].members.len(), shapes.types[dots].members.len()), (3, 9));
    assert_eq!(w.shape_order().len(), groups + 1);
    assert_eq!(w.group_name(made), "Dots 2", "listed after the larger group of dots");

    // both groups, all their pieces: one group again, the larger's
    w.apply(WizardIntent::SetActive(MaskId::Shape(dots)));
    w.apply(WizardIntent::SelectPieces(members(&w, made), Pick::Add));
    assert_eq!(w.selected().len(), 12);
    w.apply(WizardIntent::JoinSelected);
    assert_eq!(members(&w, dots).len(), 12);
    assert!(members(&w, made).is_empty());
    assert!(!w.visible_masks().contains(&MaskId::Shape(made)), "an emptied group is not listed");
    search_again(&mut w);
    assert_eq!(members(&w, dots_of(&w)).len(), 12, "made again in order");
    assert_eq!(w.shape_order().len(), groups);
    // and nothing changes when the selection already is one group
    let before = w.shapes_epoch();
    w.apply(WizardIntent::JoinSelected);
    assert_eq!(w.shapes_epoch(), before);
}

#[test]
fn the_pieces_like_those_selected_are_added_to_the_selection() {
    let mut w = dotted_wizard_with_tall(3);
    let dots = dots_of(&w);
    assert_eq!(members(&w, dots).len(), 12, "a pixel taller, still copies");
    // the largest first: a tall one
    let tall = members(&w, dots)[0];
    select(&mut w, &[tall]);
    w.apply(WizardIntent::SelectSimilar);
    assert_eq!(w.selected().len(), 12, "alike as the groups are found");
    let mut shape = w.shape_params().clone();
    shape.tolerance = 0.3;
    w.apply(WizardIntent::SetShapeParams(shape));
    settle(&mut w);
    select(&mut w, &[tall]);
    w.apply(WizardIntent::SelectSimilar);
    assert_eq!(w.selected().len(), 3, "to a strict eye, only the tall ones");
}

#[test]
fn a_group_deleted_is_emptied_into_the_leftovers_and_no_longer_listed() {
    let mut w = dotted_wizard();
    let dots = dots_of(&w);
    w.apply(WizardIntent::Delete(MaskId::Shape(dots)));
    let shapes = w.shapes().unwrap();
    assert!(shapes.types[dots].members.is_empty());
    assert_eq!(shapes.leftovers().len(), 12);
    assert!(!w.shape_order().contains(&dots));
    assert!(w.undo(), "and it is undone like any change to the groups");
    assert_eq!(members(&w, dots).len(), 12);
}

#[test]
fn going_back_and_forth_does_not_search_again_unless_something_changed() {
    let mut w = dotted_wizard();
    w.apply(WizardIntent::GoTo(Step::Shapes));
    assert!(w.take_pieces_request().is_none(), "the same colours, frame and settings: the pieces stay");
    assert!(!w.pieces_busy());
    let mut params = w.curve_params().clone();
    params.min_pixels += 1;
    w.apply(WizardIntent::SetCurveParams(params));
    assert!(w.take_pieces_request().is_some(), "a new setting: searched again");
}

#[test]
fn the_pieces_picture_is_made_again_when_what_it_shows_changes() {
    let mut w = dotted_wizard();
    let dots = dots_of(&w);
    let before = w.pieces_epoch();
    w.apply(WizardIntent::Show(MaskId::Shape(dots), false));
    assert_ne!(w.pieces_epoch(), before, "a group hidden");
    let (before, two) = (w.pieces_epoch(), members(&w, dots)[..2].to_vec());
    select(&mut w, &two);
    assert_ne!(w.pieces_epoch(), before, "pieces selected");
    let before = w.pieces_epoch();
    select(&mut w, &two);
    assert_eq!(w.pieces_epoch(), before, "the same selection");
    w.apply(WizardIntent::RemoveSelected);
    assert_ne!(w.pieces_epoch(), before, "the groups changed");
}

#[test]
fn the_plot_area_step_finds_the_lines_of_the_axes_colour_and_ticks_are_chosen_by_clicking_them() {
    let mut w = plot_wizard();
    assert!(w.has_axes(), "black is said to be the axes");
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    assert_eq!(w.active(), MaskId::Tick(Slot::XMin), "the first tick is the one to choose");
    let at = |lines: &[Line], o: Orientation, across: f32| {
        lines.iter().copied().find(|l| l.orientation == o && (l.across - across).abs() < 0.6)
    };
    let lines = w.lines().to_vec();
    let x70 = at(&lines, Orientation::Vertical, 71.0).unwrap_or_else(|| panic!("{lines:?}"));
    let x130 = at(&lines, Orientation::Vertical, 131.0).expect("the second x tick");
    let y40 = at(&lines, Orientation::Horizontal, 41.0).expect("the first y tick");
    w.apply(WizardIntent::SetTick(Slot::XMin, Some(x70)));
    assert_eq!(w.active(), MaskId::Tick(Slot::XMax), "on to the next tick without a line");
    w.apply(WizardIntent::SetTick(Slot::XMax, Some(x130)));
    w.apply(WizardIntent::SetTick(Slot::XMax, Some(y40)));
    assert_eq!(w.tick(Slot::XMax), Some(x130), "a horizontal line cannot be an x tick");
    // the position is the coverage-weighted centre of the line: two black pixels at 70 and 71 are centred on 71.0
    assert!((w.tick_position(Slot::XMin).unwrap() - 71.0).abs() < 0.1);
    assert!((w.tick_position(Slot::XMax).unwrap() - 131.0).abs() < 0.1);
    // choosing a line is not undone; a tick is deleted from its row
    assert!(!w.can_undo());
    w.apply(WizardIntent::Delete(MaskId::Tick(Slot::XMin)));
    assert_eq!((w.tick(Slot::XMin), w.tick(Slot::XMax)), (None, Some(x130)));
}

#[test]
fn the_algorithm_can_be_changed_and_the_lines_are_looked_for_again() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let first = w.lines().len();
    assert!(first > 0);
    w.apply(WizardIntent::SetAlgorithm(Algorithm::Lsd));
    settle(&mut w);
    assert_eq!(w.algorithm(), Algorithm::Lsd);
    assert!(!w.lines().is_empty());
    assert!(!w.can_undo(), "choosing how to look is not an edit");
}

#[test]
fn without_a_colour_called_axes_there_are_no_lines() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::IgnoreColour(0));
    assert!(!w.has_axes());
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    assert!(w.lines().is_empty());
}

#[test]
fn coming_back_to_the_colours_keeps_them_unless_the_crop_changed_meanwhile() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::SetRole(0, Role::Axes, false));
    w.apply(WizardIntent::SetRole(0, Role::Curve, false));
    w.apply(WizardIntent::SetRole(0, Role::Grid, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Colours));
    assert!(w.take_analysis_request().is_none(), "nothing changed: what was found and said stays");
    assert_eq!(w.colours().roles(0), crate::colours::Roles::from([Role::Grid]));
    w.apply(WizardIntent::GoTo(Step::Prepare));
    w.apply(WizardIntent::SetCrop(Some(Rect::new(10.0, 10.0, 190.0, 140.0))));
    w.apply(WizardIntent::GoTo(Step::Colours));
    assert!(w.take_analysis_request().is_some(), "the crop changed, so the colours are looked at again");
}

#[test]
fn the_settings_of_the_search_change_the_lines_found_and_are_not_edits() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let before = w.lines().len();
    let mut params = w.line_params().clone();
    params.min_length = 20.0;
    w.apply(WizardIntent::SetLineParams(params.clone()));
    settle(&mut w);
    assert!(w.lines().len() < before, "the ticks are shorter than 20 pixels: {} then {}", before, w.lines().len());
    assert_eq!(w.line_params(), &params);
    assert!(!w.can_undo());
}

#[test]
fn the_ticks_and_their_typed_values_make_a_calibration() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let line = |o: Orientation, across: f32| {
        w.lines().iter().copied().find(|l| l.orientation == o && (l.across - across).abs() < 0.6).expect("a line")
    };
    // x ticks at x = 71 and 131 (pixels), y ticks at y = 41 and 81
    let (x1, x2) = (line(Orientation::Vertical, 71.0), line(Orientation::Vertical, 131.0));
    let (y1, y2) = (line(Orientation::Horizontal, 81.0), line(Orientation::Horizontal, 41.0));
    assert!(!w.calibration().is_calibrated());
    for (slot, line) in [(Slot::XMin, x1), (Slot::XMax, x2), (Slot::YMin, y1), (Slot::YMax, y2)] {
        w.apply(WizardIntent::SetTick(slot, Some(line)));
    }
    assert_eq!(w.calibration().x.map().unwrap_err(), plotextract_domain::CalError::Incomplete, "no values yet");

    // the values: x from 0 to 10 over 60 pixels, y from 0 to 1 upwards over 40 pixels
    for (slot, value) in [(Slot::XMin, 0.0), (Slot::XMax, 10.0), (Slot::YMin, 0.0), (Slot::YMax, 1.0)] {
        w.apply(WizardIntent::SetTickValue(slot, Some(value)));
    }
    let (x, y) = w.calibration().maps().expect("both axes are calibrated");
    assert!((x.apply(101.0) - 5.0).abs() < 0.1, "halfway along x: {}", x.apply(101.0));
    assert!((y.apply(61.0) - 0.5).abs() < 0.1, "halfway up y: {}", y.apply(61.0));

    // a log axis needs positive values; a value that is not a number is none
    w.apply(WizardIntent::SetLog(Axis::X, true));
    assert_eq!(w.calibration().x.map().unwrap_err(), plotextract_domain::CalError::NonPositiveLog);
    w.apply(WizardIntent::SetTickValue(Slot::XMin, Some(1.0)));
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(100.0)));
    assert!((w.calibration().x.map().unwrap().apply(101.0) - 10.0).abs() < 0.5, "half a decade along");
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(f64::NAN)));
    assert_eq!(w.tick_value(Slot::XMax), None);

    // taking a tick's line away leaves its axis incomplete
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(100.0)));
    w.apply(WizardIntent::SetTick(Slot::YMax, None));
    assert_eq!(w.calibration().y.map().unwrap_err(), plotextract_domain::CalError::Incomplete);
    // the typed values stay with their ticks
    assert_eq!((w.tick_value(Slot::XMax), w.tick_value(Slot::YMax)), (Some(100.0), Some(1.0)));
}

#[test]
fn the_masks_appear_as_they_are_made() {
    let mut w = Wizard::new(200, 150);
    assert!(w.visible_masks().is_empty(), "nothing is made yet");
    w.apply(paint(0.0, 0.0, 10.0, 10.0));
    assert_eq!(w.visible_masks(), [MaskId::Exclude], "the exclude mask appears with its first shape");
    w.apply(WizardIntent::SetCrop(Some(Rect::new(10.0, 10.0, 190.0, 140.0))));
    assert_eq!(w.visible_masks(), [MaskId::Crop, MaskId::Exclude]);
    w.apply(WizardIntent::Delete(MaskId::Crop));
    w.apply(WizardIntent::Delete(MaskId::Exclude));
    assert!(w.visible_masks().is_empty(), "a mask that is deleted is gone");

    // the colours: one mask for each that is not ignored
    let mut w = plot_wizard();
    let inks = w.colours().map().map_or(0, |m| m.inks.len());
    assert_eq!(inks, 1, "the sample is drawn in black on white");
    assert_eq!(w.visible_masks(), [MaskId::Ink(0)]);
    w.apply(WizardIntent::Show(MaskId::Ink(0), true));
    w.apply(WizardIntent::SetActive(MaskId::Ink(0)));
    w.apply(WizardIntent::Delete(MaskId::Ink(0)));
    assert!(w.colours().is_ignored(0), "deleting a colour's mask ignores the colour");
    assert!(w.visible_masks().is_empty(), "an ignored colour has no mask");
    assert!(!w.is_shown(MaskId::Ink(0)) && w.active() == MaskId::Exclude, "and is not the mask the tools work on");
    w.apply(WizardIntent::SetRole(0, Role::Axes, true));
    assert_eq!(w.visible_masks(), [MaskId::Ink(0)], "it comes back when it is something again");

    // the ticks, as lines are chosen for them
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let line = w.lines().iter().copied().find(|l| l.orientation == Orientation::Vertical).expect("a line");
    w.apply(WizardIntent::SetTick(Slot::XMin, Some(line)));
    assert_eq!(w.visible_masks(), [MaskId::Ink(0), MaskId::Tick(Slot::XMin)]);

    // the colours found no longer match once the crop changes, so their masks wait for the next look
    w.apply(WizardIntent::GoTo(Step::Prepare));
    w.apply(WizardIntent::SetCrop(Some(Rect::new(10.0, 10.0, 190.0, 140.0))));
    assert_eq!(w.visible_masks(), [MaskId::Crop, MaskId::Tick(Slot::XMin)]);
}

#[test]
fn every_mask_has_an_id_tags_for_what_it_is_and_properties_for_the_rest() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::Prepare));
    w.apply(paint(0.0, 0.0, 10.0, 10.0));
    w.apply(WizardIntent::Exclude(MaskOp { shape: Shape::Rect(Rect::new(2.0, 2.0, 4.0, 4.0)), erase: true }));
    let exclude = w.meta(MaskId::Exclude);
    assert_eq!(exclude.id, "exclude");
    assert!(exclude.has(Tag::Exclude));
    assert_eq!((exclude.get("shapes"), exclude.get("erased shapes")), (Some(&Value::Count(1)), Some(&Value::Count(1))));

    w.apply(WizardIntent::SetCrop(Some(Rect::new(10.0, 20.0, 110.0, 70.0))));
    let crop = w.meta(MaskId::Crop);
    assert!(crop.has(Tag::Crop));
    assert_eq!((crop.get("left"), crop.get("width")), (Some(&Value::Count(10)), Some(&Value::Count(100))));

    // a colour is tagged with what it is
    let mut w = plot_wizard();
    let black = w.meta(MaskId::Ink(0));
    assert_eq!(black.id, "colour-1");
    assert!(black.has(Tag::Colour) && black.has(Tag::Axes) && !black.has(Tag::Curve));
    assert_eq!(black.get("colour"), Some(&Value::Colour([0, 0, 0])));
    assert!(matches!(black.get("share"), Some(Value::Percent(p)) if *p > 0.0));
    // one colour can be both the axes and a curve: it is tagged both, and found either way
    w.apply(WizardIntent::SetRole(0, Role::Curve, true));
    let both = w.meta(MaskId::Ink(0));
    assert!(both.has(Tag::Curve) && both.has(Tag::Axes));
    assert_eq!(w.find_masks(&[Tag::Colour, Tag::Curve]), [MaskId::Ink(0)]);
    assert_eq!(w.find_masks(&[Tag::Colour, Tag::Axes]), [MaskId::Ink(0)]);
    w.apply(WizardIntent::SetRole(0, Role::Axes, false));
    assert!(!w.has_axes(), "a curve alone is not the axes");
    w.apply(WizardIntent::SetRole(0, Role::Curve, false));
    w.apply(WizardIntent::SetRole(0, Role::Axes, true));

    // a tick: which one it is, where, and its value; calibrated once its axis is
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let line = |o: Orientation, across: f32| {
        w.lines().iter().copied().find(|l| l.orientation == o && (l.across - across).abs() < 0.6).expect("a line")
    };
    let (x1, x2) = (line(Orientation::Vertical, 71.0), line(Orientation::Vertical, 131.0));
    w.apply(WizardIntent::SetTick(Slot::XMin, Some(x1)));
    w.apply(WizardIntent::SetTick(Slot::XMax, Some(x2)));
    let tick = w.meta(MaskId::Tick(Slot::XMin));
    assert_eq!(tick.id, "tick-xmin");
    assert!([Tag::Tick, Tag::Axes, Tag::XAxis, Tag::XMin].iter().all(|&t| tick.has(t)));
    assert!(!tick.has(Tag::Calibrated) && tick.get("value").is_none());
    assert_eq!(tick.get("position"), Some(&Value::Pixels(f64::from(x1.across))));
    w.apply(WizardIntent::SetTickValue(Slot::XMin, Some(0.0)));
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(10.0)));
    let tick = w.meta(MaskId::Tick(Slot::XMax));
    assert!(tick.has(Tag::Calibrated) && tick.has(Tag::XMax));
    assert_eq!(tick.get("value"), Some(&Value::Number(10.0)));
    assert_eq!(w.find_masks(&[Tag::Tick, Tag::Calibrated]).len(), 2);
    assert_eq!(w.masks().len(), w.visible_masks().len(), "every mask that exists is described");
}

/// Choose the ticks of the plot of [`plot_wizard`] (x 71 and 131 for 0 and 10, y 81 and 41 for 0 and 1), and come back
/// to the step it was on.
fn calibrate(w: &mut Wizard) {
    let step = w.step();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(w);
    let line = |w: &Wizard, o: Orientation, across: f32| {
        w.lines().iter().copied().find(|l| l.orientation == o && (l.across - across).abs() < 0.6).expect("a tick")
    };
    let ticks = [
        (Slot::XMin, line(w, Orientation::Vertical, 71.0), 0.0),
        (Slot::XMax, line(w, Orientation::Vertical, 131.0), 10.0),
        (Slot::YMin, line(w, Orientation::Horizontal, 81.0), 0.0),
        (Slot::YMax, line(w, Orientation::Horizontal, 41.0), 1.0),
    ];
    for (slot, line, value) in ticks {
        w.apply(WizardIntent::SetTick(slot, Some(line)));
        w.apply(WizardIntent::SetTickValue(slot, Some(value)));
    }
    w.apply(WizardIntent::GoTo(step));
    settle(w);
}

/// [`dotted_wizard`] on the links step, and its dots from left to right.
fn links_wizard() -> (Wizard, Vec<usize>) {
    let mut w = dotted_wizard();
    w.apply(WizardIntent::GoTo(Step::Links));
    settle(&mut w);
    let shapes = w.shapes().unwrap();
    let mut dots = members(&w, dots_of(&w));
    dots.sort_by(|&a, &b| shapes.looks[a].centre.x.total_cmp(&shapes.looks[b].centre.x));
    (w, dots)
}

fn link(w: &mut Wizard, pieces: &[usize]) {
    for &p in pieces {
        w.apply(WizardIntent::LinkPiece(p));
    }
}

#[test]
fn a_link_is_made_click_by_click_ended_and_another_started_and_the_links_are_put_in_order() {
    let (mut w, dots) = links_wizard();
    assert_eq!(w.link_group(), Some(dots_of(&w)), "the group with the most pieces, as nothing was selected");
    link(&mut w, &dots[..5]);
    assert_eq!((w.links(), w.open_link()), (vec![dots[..5].to_vec()], Some(0)));
    // a double-click: the piece again, which changes nothing, and the link ends
    w.apply(WizardIntent::LinkPiece(dots[4]));
    w.apply(WizardIntent::EndLink);
    assert_eq!((w.links().len(), w.open_link()), (1, None));
    // the next click starts another; this one is clicked from right to left
    let rest: Vec<usize> = dots[5..].iter().rev().copied().collect();
    link(&mut w, &rest);
    w.apply(WizardIntent::EndLink);
    assert_eq!(w.links(), vec![dots[..5].to_vec(), rest.clone()]);

    // turned round, and put first
    w.apply(WizardIntent::ReverseLink(1));
    assert_eq!(w.links()[1], dots[5..]);
    w.apply(WizardIntent::MoveLink(1, true));
    assert_eq!(w.links(), vec![dots[5..].to_vec(), dots[..5].to_vec()]);
    w.apply(WizardIntent::MoveLink(0, true));
    assert_eq!(w.links()[0], dots[5..], "the first link cannot go up");

    // the points are the centres of the pieces, link after link
    let pieces = w.pieces().unwrap();
    let want: Vec<Point> = dots[5..].iter().chain(&dots[..5]).filter_map(|&p| pieces.centre(p)).collect();
    assert_eq!(w.link_points(), want);

    // a piece of another group (a tick, black as the dots are) is not linked while the dots are chosen
    let tick = w.pieces().unwrap().at(70, 115).expect("a tick");
    assert!(!w.linkable(tick));
    w.apply(WizardIntent::LinkPiece(tick));
    assert_eq!(w.links().len(), 2);
    w.apply(WizardIntent::SetLinkGroup(None));
    assert!(w.linkable(tick), "with every piece shown, it is");
}

#[test]
fn a_link_goes_on_from_either_end_and_pieces_are_taken_off_it_and_out_of_it() {
    let (mut w, dots) = links_wizard();
    link(&mut w, &dots[..3]);
    w.apply(WizardIntent::EndLink);
    // a click on its last piece goes on with it, as a pen does with an open path
    link(&mut w, &[dots[2], dots[3]]);
    assert_eq!((w.links(), w.open_link()), (vec![dots[..4].to_vec()], Some(0)));
    w.apply(WizardIntent::LinkBack);
    assert_eq!(w.links()[0], dots[..3]);
    w.apply(WizardIntent::UnlinkPiece(dots[1]));
    assert_eq!(w.links()[0], [dots[0], dots[2]]);
    w.apply(WizardIntent::EndLink);
    // and a click on its first piece goes on from there: the link is turned round
    link(&mut w, &[dots[0], dots[5]]);
    assert_eq!(w.links()[0], [dots[2], dots[0], dots[5]]);
    // a piece linked again moves to the link being made
    w.apply(WizardIntent::EndLink);
    link(&mut w, &[dots[8], dots[2]]);
    assert_eq!(w.links(), vec![vec![dots[0], dots[5]], vec![dots[8], dots[2]]]);

    w.apply(WizardIntent::DeleteLink(0));
    assert_eq!(w.links(), vec![vec![dots[8], dots[2]]]);
    assert_eq!(w.open_link(), Some(0), "the link being made is still being made, now first");
    assert!(w.undo());
    assert_eq!(w.links().len(), 2);
    assert!(w.redo());
    assert_eq!(w.links().len(), 1);
    // undoing is for the step it is on
    w.apply(WizardIntent::GoTo(Step::Shapes));
    assert!(!w.can_undo());
}

#[test]
fn the_points_are_written_link_after_link_in_plot_values_and_the_links_follow_their_pieces() {
    let (mut w, dots) = links_wizard();
    assert!(matches!(w.link_rows(), Err(ExportError::NotCalibrated)));
    calibrate(&mut w);
    assert_eq!(w.step(), Step::Links);
    assert!(matches!(w.link_rows(), Err(ExportError::Empty)), "nothing linked yet");
    link(&mut w, &dots[6..]);
    w.apply(WizardIntent::EndLink);
    link(&mut w, &dots[..6]);
    w.apply(WizardIntent::EndLink);
    w.apply(WizardIntent::MoveLink(1, true));
    let rows = w.link_rows().expect("calibrated and linked");
    assert_eq!(rows.len(), 12);
    assert!(rows.windows(2).all(|r| r[1][0] > r[0][0] && r[1][1] > r[0][1]), "left to right and up: {rows:?}");
    // the first dot covers x 50 to 53: its centre is at 52.0, which is (52 - 71) / 6 in plot values
    assert!((rows[0][0] - (52.0 - 71.0) / 6.0).abs() < 1e-9, "{:?}", rows[0]);
    let csv = w.links_csv().unwrap();
    assert!(csv.starts_with("x,y\n") && csv.lines().count() == 13, "{csv}");

    search_again(&mut w);
    assert_eq!(w.links().iter().map(Vec::len).collect::<Vec<_>>(), [6, 6], "the links name their pieces by pixels");
    assert_eq!(w.link_rows().unwrap(), rows);
}

#[test]
fn a_group_of_one_piece_links_itself_its_points_run_along_it_and_it_turns_round() {
    let mut w = lines_wizard();
    let blue = {
        let p = w.pieces().unwrap().at(100, 70).expect("the blue line");
        w.shapes().unwrap().type_of[p].expect("in a group")
    };
    w.apply(WizardIntent::GoTo(Step::Links));
    settle(&mut w);
    w.apply(WizardIntent::SetLinkGroup(Some(blue)));
    assert_eq!(w.links().len(), 1, "linked as soon as it is chosen");
    let points = w.link_points();
    // a line about 117 pixels long: a point every other pixel along it, from left to right
    assert!(points.len() >= 45, "{}", points.len());
    assert!(points.windows(2).all(|p| p[1].x > p[0].x - 0.5), "{points:?}");
    w.apply(WizardIntent::ReverseLink(0));
    let back = w.link_points();
    assert!(back[0].x > back[back.len() - 1].x, "turned round, it runs from right to left");
    assert!(w.undo(), "turning it round is undone like any change");
    w.apply(WizardIntent::SetLinkGroup(Some(blue)));
    assert_eq!(w.links().len(), 1, "choosing it again links nothing more");
}

/// Look at the colours a wizard asks to, of `image`, and run the searches that follow.
fn analyse_and_settle(w: &mut Wizard, image: &image::RgbaImage) {
    let params = w.take_analysis_request().expect("the colours are to be looked at");
    w.begin_analysis(7);
    w.finish_analysis(7, Ok(Arc::new(plotextract_algo::analyse_colours(image, w.prepare(), &params))));
    settle(w);
}

#[test]
fn the_work_saved_is_brought_back_on_a_new_wizard_with_the_groups_and_links_found_again() {
    let image = dotted_image(0);
    let mut w = Wizard::new(image.width(), image.height());
    w.apply(paint(170.0, 0.0, 200.0, 15.0));
    w.apply(WizardIntent::GoTo(Step::Colours));
    analyse_and_settle(&mut w, &image);
    w.apply(WizardIntent::SetRole(0, Role::Axes, true));
    w.apply(WizardIntent::SetRole(0, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let tick = *w
        .lines()
        .iter()
        .find(|l| l.orientation == Orientation::Vertical && l.across > 69.0 && l.across < 73.0)
        .expect("a tick");
    w.apply(WizardIntent::SetTick(Slot::XMin, Some(tick)));
    w.apply(WizardIntent::SetTickValue(Slot::XMin, Some(0.5)));
    w.apply(WizardIntent::SetLog(Axis::Y, true));
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    let mut dots = members(&w, dots_of(&w));
    dots.sort_by(|&a, &b| w.shapes().unwrap().looks[a].centre.x.total_cmp(&w.shapes().unwrap().looks[b].centre.x));
    w.apply(WizardIntent::SelectPieces(vec![dots[0]], Pick::Replace));
    w.apply(WizardIntent::RemoveSelected);
    w.apply(WizardIntent::GoTo(Step::Links));
    settle(&mut w);
    link(&mut w, &dots[1..5]);
    w.apply(WizardIntent::EndLink);
    link(&mut w, &[dots[7], dots[6]]);
    w.apply(WizardIntent::EndLink);

    // saved as JSON text and read back, as a project file is
    let work = w.work();
    let text = serde_json::to_string(&work.to_value()).unwrap();
    let read = Work::from_value(serde_json::from_str(&text).unwrap()).unwrap();
    assert_eq!(read, work);

    let mut back = Wizard::new(image.width(), image.height());
    back.restore(read);
    assert_eq!(back.step(), Step::Links);
    analyse_and_settle(&mut back, &image);
    assert_eq!(back.prepare(), w.prepare());
    assert_eq!(back.colours().roles(0), w.colours().roles(0));
    assert_eq!((back.tick(Slot::XMin), back.tick_value(Slot::XMin)), (Some(tick), Some(0.5)));
    assert!(back.is_log(Axis::Y));
    assert_eq!(back.shapes().unwrap().type_of, w.shapes().unwrap().type_of, "the dot taken out is out again");
    assert_eq!(back.links(), w.links());
    assert_eq!(back.link_points(), w.link_points());
    assert_eq!(back.work(), work, "saved again, it is the same");
    assert!(!back.can_undo(), "what was brought back is not a change to undo");
}

#[test]
fn work_saved_before_the_colours_were_looked_at_waits_for_them() {
    let image = dotted_image(0);
    let mut w = analysed(&image);
    w.apply(WizardIntent::SetRole(0, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::Prepare));
    let work = w.work();
    assert_eq!(work.inks.len(), 1, "what the colour is, is saved from any step");

    let mut back = Wizard::new(image.width(), image.height());
    back.restore(work.clone());
    assert!(back.take_analysis_request().is_none(), "on the first step nothing is looked at");
    assert_eq!(back.work(), work, "and nothing is lost meanwhile");
    back.apply(WizardIntent::GoTo(Step::Colours));
    analyse_and_settle(&mut back, &image);
    assert_eq!(back.colours().roles(0), crate::colours::Roles::from([Role::Curve]));
}
