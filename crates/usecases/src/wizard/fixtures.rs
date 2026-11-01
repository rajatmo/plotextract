//! Wizards set up on generated plots, for the tests of the wizard and of what draws it (the UI's tests turn this module
//! on with the `fixtures` feature).

use std::sync::Arc;

use super::*;

/// Run the searches for lines and for pieces that the wizard asks for, here and now, until it asks for no more.
pub fn settle(w: &mut Wizard) {
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

/// A wizard that has looked at the colours of a black frame with ticks on white, black said to be the axes: 200 x 150,
/// the frame's left side at x = 40 and its bottom at y = 110, both 2 pixels thick; x ticks 8 long below it at x = 70 and
/// 130, y ticks 8 long left of it at y = 40 and 80.
pub fn plot_wizard() -> Wizard {
    let mut w = analysed(&plot_image());
    w.apply(WizardIntent::SetRole(0, Role::Axes, true));
    w
}

/// The image of [`plot_wizard`].
pub fn plot_image() -> image::RgbaImage {
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

/// A wizard that has looked at the colours of `image`: no colour is anything yet.
pub fn analysed(image: &image::RgbaImage) -> Wizard {
    let mut w = Wizard::new(image.width(), image.height());
    w.apply(WizardIntent::GoTo(Step::Colours));
    let params = w.take_analysis_request().expect("the colours are to be looked at");
    w.begin_analysis(1);
    w.finish_analysis(1, Ok(Arc::new(plotextract_algo::analyse_colours(image, w.prepare(), &params))));
    w
}
