//! The wizard canvas's tests.

use super::*;

#[cfg(test)]
mod scene_and_crop {
    use egui::pos2;

    use super::*;
    use plotextract_usecases::wizard::fixtures::{plot_wizard, settle};

    #[test]
    fn each_step_draws_its_own_masks_and_those_of_the_steps_before_it_but_not_those_after() {
        let mut w = plot_wizard();
        w.apply(WizardIntent::Show(MaskId::Ink(0), true));
        let s = scene(&w);
        assert_eq!((s.inks.clone(), s.lines, s.ticks.len(), s.pieces), (vec![0], false, 0, PieceLayer::None));

        w.apply(WizardIntent::GoTo(Step::PlotArea));
        settle(&mut w);
        let line = w.lines().iter().copied().find(|l| l.orientation == Orientation::Vertical).expect("a line");
        w.apply(WizardIntent::SetTick(Slot::XMin, Some(line)));
        let s = scene(&w);
        assert!(s.lines && s.ticks == [Slot::XMin] && s.inks == [0], "the lines to choose from, on their step");

        // later: the tick stays, the lines to choose from go
        w.apply(WizardIntent::GoTo(Step::Shapes));
        settle(&mut w);
        let s = scene(&w);
        assert!(!s.lines && s.ticks == [Slot::XMin]);
        // a hidden mask is not drawn
        w.apply(WizardIntent::Show(MaskId::Tick(Slot::XMin), false));
        assert!(scene(&w).ticks.is_empty());

        // earlier: nothing made after the step is drawn
        w.apply(WizardIntent::Show(MaskId::Tick(Slot::XMin), true));
        w.apply(WizardIntent::GoTo(Step::Colours));
        let s = scene(&w);
        assert!(s.ticks.is_empty() && !s.lines && s.pieces == PieceLayer::None);
        w.apply(WizardIntent::GoTo(Step::Prepare));
        assert!(scene(&w).inks.is_empty(), "the colours are not drawn on the step before them");
    }

    const IMAGE: Rect = Rect { x0: 0.0, y0: 0.0, x1: 400.0, y1: 300.0 };

    #[test]
    fn dragging_out_a_new_crop_from_any_corner_gives_the_same_rectangle() {
        let (a, b) = (Point::new(300.0, 200.0), Point::new(100.0, 50.0));
        let want = Rect::new(100.0, 50.0, 300.0, 200.0);
        assert_eq!(dragged_crop(Handle::New, Rect::from_points(a, a), a, b, IMAGE), want);
        assert_eq!(dragged_crop(Handle::New, Rect::from_points(b, b), b, a, IMAGE), want);
        // dragging past the image stops at its edge
        let out = dragged_crop(Handle::New, Rect::from_points(b, b), b, Point::new(900.0, -40.0), IMAGE);
        assert_eq!(out, Rect::new(100.0, 0.0, 400.0, 50.0));
    }

    #[test]
    fn an_edge_moves_one_side_a_corner_two_and_the_crop_may_be_turned_inside_out() {
        let start = Rect::new(100.0, 100.0, 200.0, 200.0);
        let g = Point::new(0.0, 0.0);
        assert_eq!(
            dragged_crop(Handle::Edge(1, 0), start, g, Point::new(250.0, 999.0), IMAGE),
            Rect::new(100.0, 100.0, 250.0, 200.0)
        );
        assert_eq!(
            dragged_crop(Handle::Edge(0, -1), start, g, Point::new(5.0, 40.0), IMAGE),
            Rect::new(100.0, 40.0, 200.0, 200.0)
        );
        assert_eq!(
            dragged_crop(Handle::Edge(-1, -1), start, g, Point::new(20.0, 30.0), IMAGE),
            Rect::new(20.0, 30.0, 200.0, 200.0)
        );
        // the left edge dragged past the right one: the sides swap
        assert_eq!(
            dragged_crop(Handle::Edge(-1, 0), start, g, Point::new(300.0, 0.0), IMAGE),
            Rect::new(200.0, 100.0, 300.0, 200.0)
        );
    }

    #[test]
    fn moving_the_crop_keeps_its_size_and_stays_inside_the_image() {
        let start = Rect::new(100.0, 100.0, 200.0, 180.0);
        let grab = Point::new(150.0, 140.0);
        assert_eq!(
            dragged_crop(Handle::Move, start, grab, Point::new(170.0, 150.0), IMAGE),
            Rect::new(120.0, 110.0, 220.0, 190.0)
        );
        assert_eq!(
            dragged_crop(Handle::Move, start, grab, Point::new(900.0, 900.0), IMAGE),
            Rect::new(300.0, 220.0, 400.0, 300.0)
        );
        assert_eq!(
            dragged_crop(Handle::Move, start, grab, Point::new(-900.0, -900.0), IMAGE),
            Rect::new(0.0, 0.0, 100.0, 80.0)
        );
    }

    #[test]
    fn the_pointer_finds_the_handle_it_is_on() {
        let view = View::default(); // one pixel per unit, no offset
        let crop = Rect::new(100.0, 100.0, 200.0, 180.0);
        let h = |x: f32, y: f32| hit(&view, Pos2::ZERO, crop, pos2(x, y));
        assert_eq!(h(150.0, 140.0), Handle::Move);
        assert_eq!(h(50.0, 50.0), Handle::New);
        assert_eq!(h(100.0, 140.0), Handle::Edge(-1, 0));
        assert_eq!(h(203.0, 140.0), Handle::Edge(1, 0), "a few pixels outside still holds the edge");
        assert_eq!(h(150.0, 98.0), Handle::Edge(0, -1));
        assert_eq!(h(100.0, 100.0), Handle::Edge(-1, -1));
        assert_eq!(h(200.0, 180.0), Handle::Edge(1, 1));
        assert_eq!(h(200.0, 100.0), Handle::Edge(1, -1));
        assert_eq!(h(300.0, 140.0), Handle::New);
    }
}

#[cfg(test)]
mod press_start {
    //! A gesture starts where the button went down, not where egui first calls the press a drag.

    use egui::{Event, Modifiers, RawInput, Sense, pos2};

    use super::*;

    /// Press at (100, 100), move to `to` in single pixel steps (staying short of egui's drag distance), release; the
    /// edits made, as intents.
    fn gesture(tool: Tool, pen: Pen, to: Pos2) -> Vec<Intent> {
        let ctx = Context::default();
        let mut wizard = Wizard::new(800, 600);
        wizard.apply(WizardIntent::SetTool(Some(tool)));
        wizard.apply(WizardIntent::SetPen(pen));
        let (view, mut canvas) = (View::default(), WizardCanvas::default());
        let mut time = 0.0;
        let mut intents = Vec::new();
        let mut frame = |events: Vec<Event>| {
            time += 1.0 / 60.0;
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let response = ui.allocate_response(egui::vec2(800.0, 600.0), Sense::click_and_drag());
                intents.extend(canvas.interact(ui, &response, &view, Pos2::ZERO, &wizard));
            });
            output.textures_delta.clear(); // nothing is drawn, so the font textures are not wanted
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let start = pos2(100.0, 100.0);
        frame(vec![Event::PointerMoved(start)]);
        frame(vec![button(start, true)]);
        let steps = (to - start).length().ceil() as usize;
        for k in 1..=steps {
            frame(vec![Event::PointerMoved(start + (to - start) * (k as f32 / steps as f32))]);
        }
        frame(vec![button(to, false)]);
        frame(vec![]);
        intents
    }

    fn mask(intents: &[Intent]) -> Vec<&MaskOp> {
        intents
            .iter()
            .filter_map(|i| match i {
                Intent::Wizard(WizardIntent::Exclude(op)) => Some(op),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_short_brush_stroke_keeps_its_start() {
        let intents = gesture(Tool::Exclude, Pen::Brush, pos2(104.0, 100.0)); // 4 pixels: less than egui's drag distance
        let ops = mask(&intents);
        assert_eq!(ops.len(), 1, "{intents:?}");
        let Shape::Stroke { points, .. } = &ops[0].shape else { panic!("{:?}", ops[0]) };
        assert_eq!(points.first(), Some(&Point::new(100.0, 100.0)), "it starts where the button went down");
        assert_eq!(points.last(), Some(&Point::new(104.0, 100.0)));
    }

    #[test]
    fn a_press_that_does_not_move_is_a_dot_and_a_short_drag_makes_a_rectangle() {
        let dot = gesture(Tool::Exclude, Pen::Brush, pos2(100.0, 100.0));
        assert!(matches!(&mask(&dot)[..], [MaskOp { shape: Shape::Stroke { points, .. }, .. }] if points.len() == 1));
        let rect = gesture(Tool::Exclude, Pen::Rectangle, pos2(104.0, 105.0));
        assert!(matches!(&mask(&rect)[..], [MaskOp { shape: Shape::Rect(r), .. }]
            if *r == Rect::new(100.0, 100.0, 104.0, 105.0)));
    }

    #[test]
    fn a_polygon_corner_is_placed_by_the_press_even_if_the_pointer_wanders() {
        // the press and the release are 5 pixels apart: egui would not call this a click
        let intents = gesture(Tool::Exclude, Pen::Polygon, pos2(105.0, 100.0));
        assert!(mask(&intents).is_empty(), "one corner is not a polygon yet");
    }

    #[test]
    fn a_crop_starts_at_the_press_too() {
        let intents = gesture(Tool::Crop, Pen::Brush, pos2(104.0, 103.0));
        let crop = intents.iter().find_map(|i| match i {
            Intent::Wizard(WizardIntent::SetCrop(Some(r))) => Some(*r),
            _ => None,
        });
        assert_eq!(crop, Some(Rect::new(100.0, 100.0, 104.0, 103.0)));
    }

    /// The cursor icon egui ends up with when the pointer is at `at` and the canvas covers the top left quarter.
    fn cursor_with_pointer_at(tool: Tool, pen: Pen, at: Pos2) -> CursorIcon {
        let ctx = Context::default();
        let mut wizard = Wizard::new(800, 600);
        wizard.apply(WizardIntent::SetTool(Some(tool)));
        wizard.apply(WizardIntent::SetPen(pen));
        let (view, mut canvas) = (View::default(), WizardCanvas::default());
        let mut icon = CursorIcon::Default;
        for _ in 0..2 {
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
                events: vec![Event::PointerMoved(at)],
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let response = ui.allocate_response(egui::vec2(400.0, 300.0), Sense::click_and_drag());
                canvas.interact(ui, &response, &view, Pos2::ZERO, &wizard);
            });
            output.textures_delta.clear();
            icon = output.platform_output.cursor_icon;
        }
        icon
    }

    #[test]
    fn the_mask_tools_ask_for_their_cursor_only_over_the_canvas() {
        for pen in [Pen::Rectangle, Pen::Polygon, Pen::Brush] {
            assert_eq!(
                cursor_with_pointer_at(Tool::Exclude, pen, pos2(100.0, 100.0)),
                CursorIcon::Crosshair,
                "{pen:?} over the canvas"
            );
            assert_eq!(
                cursor_with_pointer_at(Tool::Exclude, pen, pos2(600.0, 450.0)),
                CursorIcon::Default,
                "{pen:?} elsewhere"
            );
        }
        assert_eq!(cursor_with_pointer_at(Tool::Crop, Pen::Brush, pos2(600.0, 450.0)), CursorIcon::Default);
    }

    fn stroke_op(seed: usize, erase: bool) -> WizardIntent {
        let points = (0..400)
            .map(|i| {
                Point::new(
                    200.0 + (seed * 37 % 3000) as f64 + i as f64 * 2.0,
                    300.0 + (seed * 53 % 2000) as f64 + 40.0 * (i as f64 * 0.05).sin(),
                )
            })
            .collect();
        WizardIntent::Exclude(MaskOp { shape: Shape::Stroke { points, width: 40.0 }, erase })
    }

    /// Run a frame of the canvas and return how long it took in milliseconds.
    fn frame(ctx: &Context, canvas: &mut WizardCanvas, view: &View, wizard: &Wizard) -> f64 {
        let input = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(1500.0, 1000.0))),
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let mut output = ctx.run_ui(input, |ui| {
            let rect = ui.max_rect();
            canvas.paint(ui.painter(), view, rect.min, rect, wizard);
        });
        let took = t.elapsed().as_secs_f64() * 1e3;
        output.textures_delta.clear();
        took
    }

    #[test]
    fn a_shape_added_to_the_mask_is_drawn_into_the_picture_the_same_as_a_picture_made_afresh() {
        let ctx = Context::default();
        let mut wizard = Wizard::new(4000, 3000);
        let (view, mut canvas) = (View::default(), WizardCanvas::default());
        for seed in 0..5 {
            wizard.apply(stroke_op(seed, false));
        }
        frame(&ctx, &mut canvas, &view, &wizard);
        let epoch = canvas.overlay.as_ref().unwrap().epoch;
        // add a shape and an erasing shape: the picture is brought up to date, not made again
        wizard.apply(stroke_op(7, false));
        wizard.apply(stroke_op(2, true));
        frame(&ctx, &mut canvas, &view, &wizard);
        let o = canvas.overlay.as_ref().unwrap();
        assert_eq!((o.epoch, o.shapes), (epoch, 7), "the same picture, now holding the new shapes");
        assert!(o.mask == wizard.prepare().rasterize_mask(&o.grid), "the same pixels as painting everything again");
        // undoing makes a new picture
        wizard.undo();
        frame(&ctx, &mut canvas, &view, &wizard);
        let o = canvas.overlay.as_ref().unwrap();
        assert_eq!(o.shapes, 6);
        assert!(o.mask == wizard.prepare().rasterize_mask(&o.grid));
    }

    #[test]
    #[ignore = "a measurement, not a check"]
    fn how_long_the_frame_after_a_new_shape_takes() {
        for (strokes, zoom) in [(1usize, 1.0f32), (20, 1.0), (100, 1.0), (100, 0.3), (100, 3.0)] {
            let ctx = Context::default();
            let mut wizard = Wizard::new(4000, 3000);
            let mut view = View::default();
            view.zoom = zoom;
            let mut canvas = WizardCanvas::default();
            for seed in 0..strokes {
                wizard.apply(stroke_op(seed, false));
            }
            frame(&ctx, &mut canvas, &view, &wizard);
            wizard.apply(stroke_op(3, false)); // in view
            let added = frame(&ctx, &mut canvas, &view, &wizard);
            wizard.undo();
            let undone = frame(&ctx, &mut canvas, &view, &wizard);
            println!("{strokes:4} strokes, zoom {zoom}: a shape added {added:6.2} ms, undone {undone:6.2} ms");
        }
    }

    /// Run frames of the plot area step on `wizard`, each with the events given; what the canvas returned, and the canvas.
    fn plot_area_frames(wizard: &Wizard, frames: Vec<Vec<Event>>) -> (Vec<Intent>, WizardCanvas) {
        let ctx = Context::default();
        let (view, mut canvas) = (View::default(), WizardCanvas::default());
        let mut intents = Vec::new();
        for (k, events) in frames.into_iter().enumerate() {
            let input = RawInput {
                screen_rect: Some(egui::Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(800.0, 600.0))),
                time: Some((k + 1) as f64 / 60.0),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let response = ui.allocate_response(egui::vec2(800.0, 600.0), Sense::click_and_drag());
                intents.extend(canvas.interact(ui, &response, &view, Pos2::ZERO, wizard));
            });
            output.textures_delta.clear();
        }
        (intents, canvas)
    }

    fn click(button: PointerButton, pos: Pos2) -> Vec<Vec<Event>> {
        let event = |pressed| Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE };
        vec![vec![Event::PointerMoved(pos)], vec![event(true)], vec![event(false)], vec![]]
    }

    #[test]
    fn a_click_on_a_line_gives_it_to_the_selected_tick_and_a_right_click_keeps_it_for_the_menu() {
        let mut wizard = plotextract_usecases::wizard::fixtures::plot_wizard();
        wizard.apply(WizardIntent::GoTo(Step::PlotArea));
        plotextract_usecases::wizard::fixtures::settle(&mut wizard);
        // the first x tick of the sample plot is two pixels wide at x = 70 and 71, below the frame
        let on_tick = pos2(71.0, 116.0);
        let (intents, canvas) = plot_area_frames(&wizard, click(PointerButton::Primary, on_tick));
        let [Intent::Wizard(WizardIntent::SetTick(tick, Some(line)))] = intents.as_slice() else {
            panic!("one tick chosen: {intents:?}")
        };
        assert_eq!(*tick, Slot::XMin, "the selected tick");
        assert!((line.across - 71.0).abs() < 0.1 && line.orientation == Orientation::Vertical, "{line:?}");
        assert!(canvas.menu_line.is_none(), "a left click is not a menu");

        let (intents, canvas) = plot_area_frames(&wizard, click(PointerButton::Secondary, on_tick));
        assert!(intents.is_empty(), "a right click chooses nothing");
        assert_eq!(canvas.menu_line.map(|l| l.across.round()), Some(71.0), "but the line is kept for the menu");

        // away from every line, a click does nothing and a right click leaves the menu without a line
        let (intents, canvas) = plot_area_frames(&wizard, click(PointerButton::Primary, pos2(300.0, 300.0)));
        assert!(intents.is_empty() && canvas.menu_line.is_none());
        let (_, canvas) = plot_area_frames(&wizard, click(PointerButton::Secondary, pos2(300.0, 300.0)));
        assert!(canvas.menu_line.is_none());
    }

    #[test]
    fn the_edge_of_a_mask_is_black_with_white_just_outside_it() {
        // a 3 x 3 block in the middle of a 5 x 5 picture
        let mask: Vec<u8> =
            (0..25).map(|i| if (1..4).contains(&(i % 5)) && (1..4).contains(&(i / 5)) { 255 } else { 0 }).collect();
        let pixel = |x, y| exclude_pixel(&mask, 5, 5, x, y);
        assert_eq!(pixel(2, 2), MASK_FILL, "inside, away from the edge: just the fill");
        assert_eq!((pixel(1, 2), pixel(1, 1), pixel(3, 3)), (EDGE, EDGE, EDGE), "the pixels of the block on its edge");
        assert_eq!((pixel(0, 2), pixel(2, 0), pixel(4, 2)), (EDGE_HALO, EDGE_HALO, EDGE_HALO), "and just outside it");
        assert_eq!(pixel(0, 0), Color32::TRANSPARENT, "a corner is not beside the block");

        // a mask that reaches the border of the picture has no edge there
        let all = vec![255u8; 9];
        assert_eq!(exclude_pixel(&all, 3, 3, 0, 0), MASK_FILL);
        assert_eq!(exclude_pixel(&all, 3, 3, 1, 1), MASK_FILL);
    }
}
