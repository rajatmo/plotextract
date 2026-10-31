//! The mapping between figure space and the canvas on screen, with zoom and pan.

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use kurbo::Point;

/// Most zoomed-in view, in screen pixels per figure unit.
pub const MAX_ZOOM: f32 = 400.0;
/// Most zoomed-out view, relative to the figure fitted in the canvas.
pub const MIN_FIT_FRACTION: f32 = 0.5;
const FIT_MARGIN: f32 = 8.0;

pub struct View {
    /// Screen pixels per figure unit.
    pub zoom: f32,
    /// Screen position of the figure origin, relative to the canvas's top left.
    pub offset: Vec2,
    /// Keep the figure fitted on resize until the user zooms or pans.
    pub auto_fit: bool,
    canvas_size: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, offset: Vec2::ZERO, auto_fit: true, canvas_size: Vec2::ZERO }
    }
}

impl View {
    pub fn fit_zoom(canvas: Vec2, figure: (f64, f64)) -> f32 {
        let (w, h) = (figure.0 as f32, figure.1 as f32);
        ((canvas.x - 2.0 * FIT_MARGIN) / w).min((canvas.y - 2.0 * FIT_MARGIN) / h).max(1e-3)
    }

    pub fn fit(&mut self, canvas: Vec2, figure: (f64, f64)) {
        self.zoom = Self::fit_zoom(canvas, figure);
        self.offset = (canvas - vec2(figure.0 as f32, figure.1 as f32) * self.zoom) / 2.0;
        self.auto_fit = true;
        self.canvas_size = canvas;
    }

    /// Fit the page at the next frame, whatever the canvas size, and keep it fitted on resize.
    pub fn request_fit(&mut self) {
        self.auto_fit = true;
        self.canvas_size = Vec2::ZERO; // differs from any real canvas: `resized` refits
    }

    /// Called every frame: refit while auto-fit is on and the canvas changed size.
    pub fn resized(&mut self, canvas: Vec2, figure: (f64, f64)) {
        if canvas != self.canvas_size {
            if self.auto_fit {
                self.fit(canvas, figure);
            } else {
                self.canvas_size = canvas;
                self.zoom = self.zoom.clamp(Self::fit_zoom(canvas, figure) * MIN_FIT_FRACTION, MAX_ZOOM);
            }
        }
    }

    pub fn to_screen(&self, origin: Pos2, p: Point) -> Pos2 {
        origin + self.offset + vec2(p.x as f32, p.y as f32) * self.zoom
    }

    pub fn to_figure(&self, origin: Pos2, s: Pos2) -> Point {
        let v = (s - origin - self.offset) / self.zoom;
        Point::new(f64::from(v.x), f64::from(v.y))
    }

    /// Screen rectangle of a figure rectangle.
    pub fn screen_rect(&self, origin: Pos2, r: kurbo::Rect) -> Rect {
        Rect::from_two_pos(
            self.to_screen(origin, Point::new(r.x0, r.y0)),
            self.to_screen(origin, Point::new(r.x1, r.y1)),
        )
    }

    /// Zoom by `factor` about the screen position `at`, within the allowed range.
    pub fn zoom_about(&mut self, origin: Pos2, at: Pos2, factor: f32, figure: (f64, f64)) {
        let min = Self::fit_zoom(self.canvas_size, figure) * MIN_FIT_FRACTION;
        let target = (self.zoom * factor).clamp(min, MAX_ZOOM);
        if (target / self.zoom - 1.0).abs() < 1e-6 {
            return;
        }
        let k = target / self.zoom;
        let rel = at - origin;
        self.offset = rel - (rel - self.offset) * k;
        self.zoom = target;
        self.auto_fit = false;
    }

    pub fn pan(&mut self, delta: Vec2) {
        self.offset += delta;
        self.auto_fit = false;
    }
}

/// A point on the screen as an egui position, for the markers drawn at constant pixel size.
pub fn marker_pos(p: Pos2) -> Pos2 {
    pos2(p.x.round(), p.y.round())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_and_figure_round_trip() {
        let mut v = View::default();
        v.fit(vec2(800.0, 600.0), (260.0, 166.0));
        let origin = pos2(100.0, 50.0);
        let p = Point::new(123.4, 56.7);
        let back = v.to_figure(origin, v.to_screen(origin, p));
        assert!((back.x - p.x).abs() < 1e-3 && (back.y - p.y).abs() < 1e-3);
    }

    #[test]
    fn fit_centres_and_fills_the_limiting_side() {
        let mut v = View::default();
        v.fit(vec2(800.0, 600.0), (200.0, 100.0));
        assert!((v.zoom - (800.0 - 16.0) / 200.0).abs() < 1e-4, "width limits");
        assert!((v.offset.x - FIT_MARGIN).abs() < 1e-3);
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let mut v = View::default();
        v.fit(vec2(800.0, 600.0), (200.0, 100.0));
        let origin = pos2(10.0, 20.0);
        let cursor = pos2(300.0, 250.0);
        let before = v.to_figure(origin, cursor);
        v.zoom_about(origin, cursor, 3.0, (200.0, 100.0));
        let after = v.to_figure(origin, cursor);
        assert!((before.x - after.x).abs() < 1e-3 && (before.y - after.y).abs() < 1e-3);
        assert!(!v.auto_fit);
    }

    #[test]
    fn requested_fit_brings_the_page_back_without_a_resize() {
        let (canvas, figure) = (vec2(800.0, 600.0), (200.0, 100.0));
        let mut v = View::default();
        v.fit(canvas, figure);
        let fitted = (v.zoom, v.offset);
        v.zoom_about(pos2(0.0, 0.0), pos2(100.0, 100.0), 5.0, figure);
        v.pan(vec2(30.0, -20.0));
        v.resized(canvas, figure);
        assert!(v.zoom != fitted.0, "an unchanged canvas leaves a zoomed view alone");
        v.request_fit();
        v.resized(canvas, figure);
        assert_eq!((v.zoom, v.offset), fitted);
        assert!(v.auto_fit);
    }

    #[test]
    fn zoom_is_clamped() {
        let mut v = View::default();
        v.fit(vec2(800.0, 600.0), (200.0, 100.0));
        v.zoom_about(pos2(0.0, 0.0), pos2(0.0, 0.0), 1e9, (200.0, 100.0));
        assert_eq!(v.zoom, MAX_ZOOM);
        v.zoom_about(pos2(0.0, 0.0), pos2(0.0, 0.0), 1e-9, (200.0, 100.0));
        assert!((v.zoom - View::fit_zoom(vec2(800.0, 600.0), (200.0, 100.0)) * MIN_FIT_FRACTION).abs() < 1e-4);
    }
}
