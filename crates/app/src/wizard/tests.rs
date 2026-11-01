//! The wizard's tests, driven through its intents.
use kurbo::Point;
use plotextract_raster::lines::{Algorithm, Orientation};
use plotextract_raster::{PixelRect, Shape, Value};

use super::*;
use kurbo::Rect;
use plotextract_raster::MaskOp;

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
    // on the later steps there is nothing to undo: Ctrl+Z there must not quietly change what the colours were found in
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

/// Run the searches for lines and for pieces that the wizard asks for, here and now, until it asks for no more.
pub(crate) fn settle(w: &mut Wizard) {
    let mut id = 0;
    loop {
        if let Some(search) = w.take_lines_request() {
            id += 1;
            w.begin_lines(id);
            w.finish_lines(id, search.run());
        } else if let Some(search) = w.take_pieces_request() {
            id += 1;
            w.begin_pieces(id);
            w.finish_pieces(id, search.run());
        } else {
            break;
        }
    }
}

/// A wizard that has looked at the colours of a black frame with ticks on white: 200 x 150, the frame's left side
/// at x = 40 and its bottom at y = 110, both 2 pixels thick; x ticks 8 long below it at x = 70 and 130, y ticks 8 long
/// left of it at y = 40 and 80.
pub(crate) fn plot_wizard() -> Wizard {
    analysed(&plot_image())
}

/// The image of [`plot_wizard`].
fn plot_image() -> image::RgbaImage {
    use image::{Rgba, RgbaImage};
    let mut image = RgbaImage::from_pixel(200, 150, Rgba([255, 255, 255, 255]));
    let black = Rgba([0, 0, 0, 255]);
    for x in 40..160 {
        image.put_pixel(x, 110, black);
        image.put_pixel(x, 111, black);
        image.put_pixel(x, 20, black);
        image.put_pixel(x, 21, black);
    }
    for y in 20..112 {
        image.put_pixel(40, y, black);
        image.put_pixel(41, y, black);
        image.put_pixel(158, y, black);
        image.put_pixel(159, y, black);
    }
    for x in [70, 130] {
        for y in 112..120 {
            image.put_pixel(x, y, black);
            image.put_pixel(x + 1, y, black);
        }
    }
    for y in [40, 80] {
        for x in 32..40 {
            image.put_pixel(x, y, black);
            image.put_pixel(x, y + 1, black);
        }
    }
    image
}

/// A wizard that has looked at the colours of `image`.
fn analysed(image: &image::RgbaImage) -> Wizard {
    let mut w = Wizard::new(image.width(), image.height());
    w.apply(WizardIntent::GoTo(Step::Colours));
    let params = w.take_analysis_request().expect("the colours are to be looked at");
    w.begin_analysis(1);
    w.finish_analysis(1, Ok(Arc::new(plotextract_raster::analyse_colours(image, w.prepare(), &params))));
    w
}

const BLUE: [u8; 3] = [31, 119, 180];

/// The plot of [`plot_wizard`] with two curves in it, which do not cross: a blue line 2 pixels thick from (50, 100) up
/// to (150, 40), and a black one under it that starts on the frame's left side at (42, 105); and a blue dash outside
/// the frame, as in a legend. Its
/// ticks are chosen (x 71 and 131 for 0 and 10, y 81 and 41 for 0 and 1) and black is a curve as well as the axes.
fn curves_wizard() -> Wizard {
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
    let black = w.colours().map().unwrap().inks.iter().position(|i| i.rgb == [0, 0, 0]).expect("black");
    w.apply(WizardIntent::SetRole(black, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    let line = |o: Orientation, across: f32| {
        w.lines().iter().copied().find(|l| l.orientation == o && (l.across - across).abs() < 0.6).expect("a tick")
    };
    let ticks = [
        (Slot::XMin, line(Orientation::Vertical, 71.0), 0.0),
        (Slot::XMax, line(Orientation::Vertical, 131.0), 10.0),
        (Slot::YMin, line(Orientation::Horizontal, 81.0), 0.0),
        (Slot::YMax, line(Orientation::Horizontal, 41.0), 1.0),
    ];
    for (slot, line, value) in ticks {
        w.apply(WizardIntent::SetTick(slot, Some(line)));
        w.apply(WizardIntent::SetTickValue(slot, Some(value)));
    }
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    w
}

/// The series drawn in `rgb`.
fn series_in(w: &Wizard, rgb: [u8; 3]) -> usize {
    w.series()
        .iter()
        .map(|s| s.number())
        .find(|&n| w.meta(MaskId::Series(n)).get("colour") == Some(&Value::Colour(rgb)))
        .expect("a series of that colour")
}

#[test]
fn the_curves_step_gives_each_curve_colour_a_series_of_its_pieces_inside_the_frame() {
    let w = curves_wizard();
    assert!(w.has_curves() && !w.pieces_busy());
    assert_eq!(w.series().len(), 2, "black and blue are curves");
    let blue = series_in(&w, BLUE);
    let series = w.series().iter().find(|s| s.number() == blue).unwrap();
    assert_eq!(series.pieces().len(), 1, "the dash of the legend is outside the frame");
    assert_eq!(series.points().len(), 100, "a point for every column");
    assert_eq!(w.active(), MaskId::Series(w.series()[0].number()), "the first series is selected");

    // the black curve loses the frame it starts on, and the frame and its ticks are in no series
    let black = series_in(&w, [0, 0, 0]);
    let curve = w.series().iter().find(|s| s.number() == black).unwrap();
    assert_eq!(curve.pieces().len(), 1, "a flat curve is not taken for a side of the frame");
    assert!(curve.points()[0].x >= 43.0, "{:?}", curve.points()[0]);

    // in plot values: at x = 100 the blue line covers rows 70 and 71, so its centre is at 71.0
    let rows = w.series_rows(blue).expect("calibrated");
    let row = rows.iter().find(|r| (r[0] - (100.5 - 71.0) / 6.0).abs() < 1e-9).expect("the column at x = 100");
    assert!((row[1] - 0.25).abs() < 1e-9, "{row:?}");
    assert!(rows.windows(2).all(|r| r[0][0] <= r[1][0]), "from left to right");
    assert!(w.series_csv(blue).unwrap().starts_with("x,y\n"));

    // a series is a mask like the others: tagged, with its properties, listed once it has pieces
    let meta = w.meta(MaskId::Series(blue));
    assert_eq!(meta.id, format!("series-{blue}"));
    assert!([Tag::Curve, Tag::Series, Tag::Line, Tag::Calibrated].iter().all(|&t| meta.has(t)));
    assert_eq!((meta.get("pieces"), meta.get("points")), (Some(&Value::Count(1)), Some(&Value::Count(100))));
    assert_eq!(w.find_masks(&[Tag::Series, Tag::Calibrated]).len(), 2);
}

#[test]
fn pieces_are_put_in_series_by_hand_and_series_follow_their_pieces_when_found_again() {
    let mut w = curves_wizard();
    let blue = series_in(&w, BLUE);
    let pieces_of = |w: &Wizard, n: usize| w.series().iter().find(|s| s.number() == n).map(|s| s.pieces().len());
    let legend = w.pieces().unwrap().at(170, 10).expect("the dash of the legend");
    assert_eq!(w.series_of(legend), None);

    w.apply(WizardIntent::SetActive(MaskId::Series(blue)));
    w.apply(WizardIntent::SetPiece(legend, Some(blue)));
    assert_eq!((pieces_of(&w, blue), w.series_of(legend)), (Some(2), Some(blue)));
    w.apply(WizardIntent::SetPiece(legend, None));
    assert_eq!(pieces_of(&w, blue), Some(1));

    // a new series of its own, selected
    w.apply(WizardIntent::NewSeries(Some(legend)));
    let legend_series = w.series_of(legend).expect("in the new series");
    assert_eq!(w.active(), MaskId::Series(legend_series));
    assert!(w.visible_masks().contains(&MaskId::Series(legend_series)));

    // the dash is 50 pixels: with a larger smallest piece it is a speck, and its series empty and unlisted; it comes
    // back with the setting
    let mut params = w.curve_params().clone();
    params.min_pixels = 60;
    w.apply(WizardIntent::SetCurveParams(params.clone()));
    assert!(w.pieces_busy());
    settle(&mut w);
    assert_eq!(pieces_of(&w, legend_series), Some(0));
    assert!(!w.visible_masks().contains(&MaskId::Series(legend_series)));
    params.min_pixels = 4;
    w.apply(WizardIntent::SetCurveParams(params));
    settle(&mut w);
    assert_eq!(pieces_of(&w, legend_series), Some(1));

    // traced as markers, a piece is a point
    w.apply(WizardIntent::SetTrace(legend_series, Trace::Markers));
    let points = w.series().iter().find(|s| s.number() == legend_series).unwrap().points().to_vec();
    assert_eq!(points, [Point::new(177.5, 11.0)]);
    assert!(w.meta(MaskId::Series(legend_series)).has(Tag::Markers));

    // deleting a series leaves its pieces in none, and selects another
    w.apply(WizardIntent::Delete(MaskId::Series(legend_series)));
    assert_eq!((w.series().len(), w.series_of(legend)), (2, None));
    assert!(matches!(w.active(), MaskId::Series(n) if n != legend_series));
    assert!(matches!(w.series_csv(legend_series), Err(ExportError::NoSuchSeries)));

    // the series stay on the other steps, but are not what is selected there
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    assert!(matches!(w.active(), MaskId::Tick(_)));
    assert_eq!(w.find_masks(&[Tag::Series]).len(), 2);
}

#[test]
fn without_a_colour_called_curve_there_are_no_pieces_and_no_series() {
    let mut w = plot_wizard();
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert!(!w.has_curves() && w.pieces().is_none() && w.series().is_empty() && !w.pieces_busy());
    // a series made by hand has nothing to be written
    w.apply(WizardIntent::NewSeries(None));
    let n = w.active_series().expect("the new series is selected");
    assert!(matches!(w.series_rows(n), Err(ExportError::NotCalibrated)));
}

/// The plot of [`plot_wizard`] with a dotted black curve: 12 dots 4 pixels square, 10 pixels apart, from (50, 100) up
/// to the right; black is a curve as well as the axes. On the shapes step, found.
fn dotted_wizard() -> Wizard {
    dotted_wizard_with_tall(0)
}

/// [`dotted_wizard`] with its first `tall` dots a pixel taller than the rest (still copies of one shape).
fn dotted_wizard_with_tall(tall: u32) -> Wizard {
    let mut image = plot_image();
    for k in 0..12 {
        let (cx, cy) = (50.0 + 10.0 * f64::from(k) * 0.894, 100.0 - 10.0 * f64::from(k) * 0.447);
        for dy in 0..if k < tall { 5 } else { 4 } {
            for dx in 0..4 {
                image.put_pixel(cx as u32 + dx, cy as u32 + dy, image::Rgba([0, 0, 0, 255]));
            }
        }
    }
    let mut w = analysed(&image);
    w.apply(WizardIntent::SetRole(0, Role::Curve, true));
    w.apply(WizardIntent::GoTo(Step::PlotArea));
    settle(&mut w);
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    w
}

/// The shape type of kind `kind`.
fn type_of_kind(w: &Wizard, kind: Kind) -> usize {
    w.shapes().unwrap().types.iter().position(|t| t.kind == kind).unwrap_or_else(|| panic!("no {kind:?}"))
}

#[test]
fn the_shapes_step_finds_the_dots_of_a_dotted_curve_and_the_curves_step_makes_one_series_of_them() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 12, "{:?}", w.shapes().unwrap().types);
    assert_eq!(w.active(), MaskId::Shape(dots), "the first shape listed is selected: dots come first");
    assert_eq!(w.shape_order()[0], dots);
    assert!(w.visible_masks().contains(&MaskId::Shape(dots)));
    let meta = w.meta(MaskId::Shape(dots));
    assert_eq!(meta.id, format!("shape-{}", dots + 1));
    assert!(meta.has(Tag::Shape) && meta.has(Tag::Dot));
    assert_eq!(meta.get("pieces"), Some(&Value::Count(12)));

    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert_eq!(w.series().len(), 1, "one curve of dots, and the ticks of the frame are in none: {:?}", w.series());
    let series = &w.series()[0];
    assert_eq!((series.trace(), series.points().len()), (Trace::Pattern, 12));
    assert!(series.points().windows(2).all(|p| p[1].x > p[0].x), "in order along the curve");
    assert!(w.meta(MaskId::Series(series.number())).has(Tag::Pattern));
}

#[test]
fn a_kind_said_by_hand_changes_the_series_proposed_until_a_series_is_changed_by_hand() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    w.apply(WizardIntent::SetKind(dots, Kind::Text));
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert!(w.series().is_empty(), "text is in no series");

    // it is kept when the shapes are found again
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    assert_eq!(w.shapes().unwrap().types[dots].kind, Kind::Text);
    w.apply(WizardIntent::SetKind(dots, Kind::Marker));
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert_eq!(w.series().len(), 1);
    assert_eq!(w.series()[0].trace(), Trace::Markers, "proposed afresh for what the shapes are now");

    // once a series is changed by hand, the colour's series are left as they are
    let number = w.series()[0].number();
    w.apply(WizardIntent::SetTrace(number, Trace::Pattern));
    w.apply(WizardIntent::GoTo(Step::Shapes));
    settle(&mut w);
    w.apply(WizardIntent::SetKind(dots, Kind::Other));
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert_eq!(w.series().iter().map(|s| s.number()).collect::<Vec<_>>(), [number]);
}

/// Search for the pieces again, as a new setting does, and wait for them.
fn search_again(w: &mut Wizard) {
    let mut params = w.curve_params().clone();
    params.min_pixels = if params.min_pixels == 4 { 3 } else { 4 };
    w.apply(WizardIntent::SetCurveParams(params));
    settle(w);
}

#[test]
fn a_piece_moved_by_hand_stays_moved_when_the_pieces_are_found_again() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    let piece = w.shapes().unwrap().types[dots].members[5];
    w.apply(WizardIntent::MovePieces(vec![piece], None));
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.kind(piece), Some(Kind::Other), "taken out, it is among the other pieces");
    assert_eq!(shapes.types[dots].members.len(), 11);
    assert!(shapes.types[dots].by_hand && w.meta(MaskId::Shape(dots)).has(Tag::ByHand));
    assert!(w.moved_by_hand(piece));
    assert!(w.shape_offer().is_none(), "the other dots are as like the rest as like it: none is offered");

    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert_eq!(w.series()[0].points().len(), 11, "the piece taken out is in no series");

    w.apply(WizardIntent::GoTo(Step::Shapes));
    search_again(&mut w);
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.kind(piece), Some(Kind::Other), "the move is made again on the pieces found again");
    w.apply(WizardIntent::MovePieces(vec![piece], Some(dots)));
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 12);
}

#[test]
fn the_pieces_that_look_like_one_moved_more_than_like_the_rest_are_offered_and_moved_when_asked() {
    let mut w = dotted_wizard_with_tall(3);
    let dots = type_of_kind(&w, Kind::Dot);
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.types[dots].members.len(), 12, "a pixel taller, still copies");
    // the largest first: the tall ones
    let tall = shapes.types[dots].members[0];
    assert_eq!(shapes.types[dots].odd.len(), 12);
    w.apply(WizardIntent::MovePieces(vec![tall], None));
    let offer = w.shape_offer().expect("the other tall dots look like it");
    assert_eq!((offer.pieces.len(), offer.to), (2, None));
    w.apply(WizardIntent::LookAlikes(true));
    assert!(w.shape_offer().is_none());
    let shapes = w.shapes().unwrap();
    assert_eq!(shapes.types[dots].members.len(), 9, "the three tall dots are out");
    assert!(shapes.types[dots].members.iter().all(|&p| shapes.looks[p].area < shapes.looks[tall].area));

    // or left where they are
    let mut w = dotted_wizard_with_tall(3);
    w.apply(WizardIntent::MovePieces(vec![tall], None));
    w.apply(WizardIntent::LookAlikes(false));
    assert!(w.shape_offer().is_none());
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 11);
}

#[test]
fn a_shape_emptied_is_not_listed_and_makes_no_series() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    let all = w.shapes().unwrap().types[dots].members.clone();
    w.apply(WizardIntent::MovePieces(all, None));
    assert!(w.shapes().unwrap().types[dots].members.is_empty());
    w.apply(WizardIntent::SetActive(MaskId::Exclude));
    assert!(!w.shape_order().contains(&dots), "an empty shape is not listed");
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert!(w.series().is_empty(), "no dots, no series");
}

#[test]
fn a_new_shape_takes_pieces_and_is_merged_into_another() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    w.apply(WizardIntent::NewShape);
    let made = w.active_shape().expect("the new shape is selected");
    assert_ne!(made, dots);
    assert_eq!(w.shape_click(), ShapeClick::Move, "a click puts pieces in it");
    assert!(!crate::wizard_canvas::WizardCanvas::pans(&w), "a drag goes around pieces");
    assert!(w.visible_masks().contains(&MaskId::Shape(made)), "listed while selected, though empty");
    let some: Vec<usize> = w.shapes().unwrap().types[dots].members[..3].to_vec();
    w.apply(WizardIntent::MovePieces(some.clone(), Some(made)));
    let shapes = w.shapes().unwrap();
    assert_eq!((shapes.types[made].members.len(), shapes.types[dots].members.len()), (3, 9));
    assert_eq!(shapes.kind(some[0]), Some(Kind::Marker), "a new shape is markers until said otherwise");
    assert!(w.shape_offer().is_none(), "look-alikes are offered for one piece moved, not many");

    w.apply(WizardIntent::MergeShape(made, dots));
    assert_eq!(w.active(), MaskId::Shape(dots));
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 12);
    assert!(!w.visible_masks().contains(&MaskId::Shape(made)), "emptied and not selected: not listed");
    search_again(&mut w);
    assert_eq!(w.shapes().unwrap().types[dots].members.len(), 12, "made again in order");
}

#[test]
fn a_path_cut_and_joined_by_hand_is_kept_when_the_pieces_are_found_again() {
    let mut w = dotted_wizard();
    let chain = w.shapes().unwrap().chains[0].clone();
    assert_eq!(chain.len(), 12);
    w.apply(WizardIntent::SetShapeClick(ShapeClick::Links));
    w.apply(WizardIntent::CutLink(chain[3], chain[4]));
    assert_eq!(w.shapes().unwrap().chains.len(), 2);
    search_again(&mut w);
    assert_eq!(w.shapes().unwrap().chains.len(), 2, "the cut is made again");
    w.apply(WizardIntent::JoinPieces(chain[3], chain[4]));
    assert_eq!(w.shapes().unwrap().chains.len(), 1);
    assert_eq!(w.shapes().unwrap().links.len(), 11);
}

#[test]
fn a_shape_is_shown_alone_and_an_odd_piece_ringed() {
    let mut w = dotted_wizard();
    let dots = type_of_kind(&w, Kind::Dot);
    assert_eq!(w.solo_shape(), None);
    w.apply(WizardIntent::SetSolo(true));
    assert_eq!(w.solo_shape(), Some(dots));
    let odd = w.shapes().unwrap().types[dots].odd[0];
    w.apply(WizardIntent::FocusPiece(Some(odd)));
    assert_eq!(w.focus_piece(), Some(odd));
    let mut shape = w.shape_params().clone();
    shape.tolerance = 0.5;
    w.apply(WizardIntent::SetShapeParams(shape));
    assert!(w.take_pieces_request().is_some(), "a new tolerance: searched again");
}

#[test]
fn a_lone_side_of_the_frame_bounds_the_plot_on_its_own_side() {
    // the top and right sides of a frame, as when the line search breaks up the axes: the plot is below and left
    let mut w = plot_wizard();
    let side = |orientation, from, to, across| Line { orientation, from, to, across, width: 2.0 };
    w.plot_area.lines =
        vec![side(Orientation::Horizontal, 40.0, 160.0, 21.0), side(Orientation::Vertical, 20.0, 112.0, 159.0)];
    assert_eq!(w.frame(), Some(Rect::new(0.0, 21.0, 159.0, 150.0)));
    // the left and bottom axes: above and right
    w.plot_area.lines =
        vec![side(Orientation::Horizontal, 40.0, 160.0, 111.0), side(Orientation::Vertical, 20.0, 112.0, 41.0)];
    assert_eq!(w.frame(), Some(Rect::new(41.0, 0.0, 200.0, 111.0)));
}

#[test]
fn going_back_and_forth_does_not_search_again_unless_something_changed() {
    let mut w = dotted_wizard();
    w.apply(WizardIntent::GoTo(Step::Curves));
    assert!(w.take_pieces_request().is_none(), "the same colours, frame and settings: the pieces stay");
    assert!(!w.pieces_busy());
    w.apply(WizardIntent::GoTo(Step::Shapes));
    assert!(w.take_pieces_request().is_none());
    let mut params = w.curve_params().clone();
    params.min_pixels += 1;
    w.apply(WizardIntent::SetCurveParams(params));
    assert!(w.take_pieces_request().is_some(), "a new setting: searched again");
}

#[test]
fn the_pieces_picture_is_made_again_when_their_colours_change_and_not_for_a_point() {
    let mut w = dotted_wizard();
    let before = w.curves_epoch();
    w.apply(WizardIntent::Show(MaskId::Shape(0), false));
    assert_ne!(w.curves_epoch(), before, "a shape hidden");
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    let number = w.series()[0].number();
    let before = w.curves_epoch();
    w.apply(WizardIntent::AddPoint(number, Point::new(10.0, 10.0)));
    w.apply(WizardIntent::SetTrace(number, Trace::Markers));
    assert_eq!(w.curves_epoch(), before, "points and tracing are drawn on their own");
    w.apply(WizardIntent::Show(MaskId::Series(number), false));
    assert_ne!(w.curves_epoch(), before);
}

#[test]
fn stray_points_are_taken_out_and_missed_ones_put_in_by_hand() {
    let mut w = dotted_wizard();
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    w.apply(WizardIntent::SetEditPoints(true));
    let number = w.series()[0].number();
    let points = |w: &Wizard| w.series()[0].points().to_vec();
    let before = points(&w);

    // a point taken out is left out, and comes back when put back
    let stray = before[5];
    w.apply(WizardIntent::RemovePoint(number, stray));
    assert_eq!(points(&w).len(), 11);
    assert!(!points(&w).contains(&stray));
    assert_eq!(w.series()[0].removed(), [stray]);
    w.apply(WizardIntent::RestorePoint(number, stray));
    assert_eq!(points(&w), before);

    // a point put in goes where it makes the path shortest, and is written with the rest
    let missed = Point::new(before[2].x + 4.0, before[2].y - 2.0);
    w.apply(WizardIntent::AddPoint(number, missed));
    let now = points(&w);
    assert_eq!((now.len(), now[3]), (13, missed), "{now:?}");
    assert_eq!(w.meta(MaskId::Series(number)).get("points put in"), Some(&Value::Count(1)));
    // and taking it out removes it rather than leaving it out
    w.apply(WizardIntent::RemovePoint(number, missed));
    assert_eq!(points(&w), before);
    assert!(w.series()[0].removed().is_empty() && w.series()[0].added().is_empty());

    // the points stay when the pieces are found again
    w.apply(WizardIntent::RemovePoint(number, stray));
    w.apply(WizardIntent::GoTo(Step::Curves));
    settle(&mut w);
    assert_eq!(points(&w).len(), 11);
}

#[test]
fn the_plot_area_step_finds_the_lines_of_the_axes_colour_and_ticks_are_chosen_by_clicking_them() {
    let mut w = plot_wizard();
    assert!(w.has_axes(), "black is guessed to be the axes");
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
    assert_eq!(w.calibration().x.map().unwrap_err(), plotextract_core::CalError::Incomplete, "no values yet");

    // the values: x from 0 to 10 over 60 pixels, y from 0 to 1 upwards over 40 pixels
    for (slot, value) in [(Slot::XMin, 0.0), (Slot::XMax, 10.0), (Slot::YMin, 0.0), (Slot::YMax, 1.0)] {
        w.apply(WizardIntent::SetTickValue(slot, Some(value)));
    }
    let (x, y) = w.calibration().maps().expect("both axes are calibrated");
    assert!((x.apply(101.0) - 5.0).abs() < 0.1, "halfway along x: {}", x.apply(101.0));
    assert!((y.apply(61.0) - 0.5).abs() < 0.1, "halfway up y: {}", y.apply(61.0));

    // a log axis needs positive values; a value that is not a number is none
    w.apply(WizardIntent::SetLog(Axis::X, true));
    assert_eq!(w.calibration().x.map().unwrap_err(), plotextract_core::CalError::NonPositiveLog);
    w.apply(WizardIntent::SetTickValue(Slot::XMin, Some(1.0)));
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(100.0)));
    assert!((w.calibration().x.map().unwrap().apply(101.0) - 10.0).abs() < 0.5, "half a decade along");
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(f64::NAN)));
    assert_eq!(w.tick_value(Slot::XMax), None);

    // taking a tick's line away leaves its axis incomplete
    w.apply(WizardIntent::SetTickValue(Slot::XMax, Some(100.0)));
    w.apply(WizardIntent::SetTick(Slot::YMax, None));
    assert_eq!(w.calibration().y.map().unwrap_err(), plotextract_core::CalError::Incomplete);
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
