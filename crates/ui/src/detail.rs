//! Pictures of the page, all drawn on a worker thread. The base picture covers the whole page. Sharp detail: when the
//! view has stopped changing and shows more than the base picture's resolution, the visible part of the page is
//! re-rendered at the screen's resolution and laid over the base picture.

use std::sync::mpsc::{Receiver, Sender, channel};

use kurbo::Rect;
use plotextract_domain::Bitmap;
use plotextract_usecases::Source;

/// Longest side of the base picture, in pixels.
const BASE_MAX_PX: f64 = 4096.0;
/// Most pixels per figure unit in the base picture.
const BASE_MAX_SCALE: f64 = 6.0;

/// Pixels per figure unit of the base picture of a page this size.
pub fn base_scale(width: f64, height: f64) -> f64 {
    (BASE_MAX_PX / width.max(height)).min(BASE_MAX_SCALE)
}

/// Only re-render when the screen shows more than this times the base picture's resolution.
pub const DETAIL_ABOVE: f64 = 1.15;
/// Cap on the size of one detail render, in pixels (48 MB of RGBA).
pub const MAX_DETAIL_PIXELS: f64 = 12e6;
/// Render this fraction of the view size beyond each edge, so small pans stay sharp.
pub const MARGIN: f64 = 0.25;

/// What to render for a view: the figure-space region and its scale in pixels per figure unit, or `None` when the
/// base picture is already sharp enough.
pub fn plan(visible: Rect, page: Rect, screen_scale: f64, base_scale: f64) -> Option<(Rect, f64)> {
    if screen_scale <= base_scale * DETAIL_ABOVE {
        return None;
    }
    let area = |r: Rect| r.width() * r.height();
    let margin = MARGIN * visible.width().max(visible.height());
    let mut region = visible.inflate(margin, margin).intersect(page);
    if area(region) * screen_scale * screen_scale > MAX_DETAIL_PIXELS {
        region = visible.intersect(page); // no margin
    }
    if region.width() <= 0.0 || region.height() <= 0.0 {
        return None;
    }
    let mut scale = screen_scale;
    if area(region) * scale * scale > MAX_DETAIL_PIXELS {
        // still too big: render slightly coarser
        scale = (MAX_DETAIL_PIXELS / area(region)).sqrt();
        if scale <= base_scale * DETAIL_ABOVE {
            return None;
        }
    }
    Some((region, scale))
}

/// Whether a render of `have` at `have_scale` already serves a view that needs `need` at `need_scale`.
pub fn covers(have: Rect, have_scale: f64, need: Rect, need_scale: f64) -> bool {
    have.x0 <= need.x0
        && have.y0 <= need.y0
        && have.x1 >= need.x1
        && have.y1 >= need.y1
        && have_scale >= need_scale * 0.9
}

/// What a render is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The whole page at a modest resolution: the picture that is always there.
    Base,
    /// The visible part at the screen's resolution, laid over the base.
    Detail,
}

pub struct Request {
    pub id: u64,
    pub kind: Kind,
    pub region: Rect,
    pub scale: f64,
}

pub struct Rendered {
    pub id: u64,
    pub kind: Kind,
    pub region: Rect,
    pub scale: f64,
    pub bitmap: Bitmap,
}

/// The newest of each kind, base first: what is stale is skipped.
fn newest_per_kind<T>(items: impl IntoIterator<Item = T>, kind: impl Fn(&T) -> Kind) -> Vec<T> {
    let mut latest: [Option<T>; 2] = [None, None];
    for item in items {
        let at = kind(&item) as usize;
        latest[at] = Some(item);
    }
    latest.into_iter().flatten().collect()
}

/// A thread that owns the file's picture source and answers requests. It ends when dropped.
pub struct Worker {
    requests: Sender<Request>,
    results: Receiver<Rendered>,
    next_id: u64,
}

impl Worker {
    pub fn spawn(ctx: egui::Context, source: Source) -> Self {
        let (requests, rx) = channel::<Request>();
        let (tx, results) = channel::<Rendered>();
        std::thread::spawn(move || {
            // created here: some renderers cannot move between threads
            let Some(mut pictures) = source.picture() else { return };
            while let Ok(first) = rx.recv() {
                let queued = std::iter::once(first).chain(std::iter::from_fn(|| rx.try_recv().ok()));
                for req in newest_per_kind(queued, |r| r.kind) {
                    match pictures.render(req.region, req.scale) {
                        Ok(bitmap) => {
                            let done =
                                Rendered { id: req.id, kind: req.kind, region: req.region, scale: req.scale, bitmap };
                            if tx.send(done).is_err() {
                                return;
                            }
                            ctx.request_repaint();
                        }
                        Err(e) => tracing::warn!("render failed: {e}"),
                    }
                }
            }
        });
        Self { requests, results, next_id: 0 }
    }

    /// Ask for a render; returns its id (a newer request of the same kind makes an older one stale).
    pub fn request(&mut self, kind: Kind, region: Rect, scale: f64) -> u64 {
        self.next_id += 1;
        let _ = self.requests.send(Request { id: self.next_id, kind, region, scale });
        self.next_id
    }

    /// The finished renders, the newest of each kind.
    pub fn poll_all(&self) -> Vec<Rendered> {
        newest_per_kind(std::iter::from_fn(|| self.results.try_recv().ok()), |r| r.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAGE: Rect = Rect::new(0.0, 0.0, 300.0, 200.0);

    #[test]
    fn no_detail_while_the_backdrop_is_sharp_enough() {
        assert!(plan(PAGE, PAGE, 3.0, 6.0).is_none(), "screen coarser than the backdrop");
        assert!(plan(PAGE, PAGE, 6.0 * 1.15, 6.0).is_none(), "at the threshold");
        assert!(plan(PAGE, PAGE, 6.0 * 1.2, 6.0).is_some());
    }

    #[test]
    fn the_region_is_the_view_plus_a_margin_cut_to_the_page() {
        let visible = Rect::new(100.0, 80.0, 140.0, 110.0);
        let (region, scale) = plan(visible, PAGE, 40.0, 6.0).unwrap();
        assert_eq!(scale, 40.0);
        assert_eq!(region, Rect::new(90.0, 70.0, 150.0, 120.0)); // 25% of the longer side (40) = 10 on every edge

        let at_edge = Rect::new(-20.0, -10.0, 20.0, 20.0); // the view hangs over the page corner
        let (region, _) = plan(at_edge, PAGE, 40.0, 6.0).unwrap();
        assert_eq!((region.x0, region.y0), (0.0, 0.0));
    }

    #[test]
    fn a_view_off_the_page_needs_nothing() {
        assert!(plan(Rect::new(400.0, 400.0, 440.0, 430.0), PAGE, 40.0, 6.0).is_none());
    }

    #[test]
    fn huge_renders_drop_the_margin_then_the_resolution() {
        // 100 x 70 pt at 300 px/pt = 30000 x 21000 px: far past the cap even without a margin
        let visible = Rect::new(50.0, 50.0, 150.0, 120.0);
        let (region, scale) = plan(visible, PAGE, 300.0, 6.0).unwrap();
        assert_eq!(region, visible);
        assert!(scale < 300.0);
        assert!(region.width() * region.height() * scale * scale <= MAX_DETAIL_PIXELS * 1.0001);

        // a margin that alone breaks the cap is dropped while the resolution stays
        let visible = Rect::new(100.0, 80.0, 130.0, 100.0); // 30 x 20 pt
        let (region, scale) = plan(visible, PAGE, 100.0, 6.0).unwrap(); // with margin 45 x 35 pt = 15.8 Mpx at 100
        assert_eq!((region, scale), (visible, 100.0)); // without: 6 Mpx
    }

    #[test]
    fn a_render_serves_the_view_it_covers() {
        let have = Rect::new(90.0, 70.0, 150.0, 120.0);
        assert!(covers(have, 40.0, Rect::new(100.0, 80.0, 140.0, 110.0), 40.0));
        assert!(covers(have, 40.0, Rect::new(100.0, 80.0, 140.0, 110.0), 43.0), "a little coarser is fine");
        assert!(!covers(have, 40.0, Rect::new(100.0, 80.0, 140.0, 110.0), 60.0), "zoomed in further");
        assert!(!covers(have, 40.0, Rect::new(80.0, 80.0, 140.0, 110.0), 40.0), "panned past its edge");
    }

    #[test]
    fn the_base_picture_is_capped_by_size_and_by_scale() {
        assert_eq!(base_scale(100.0, 50.0), 6.0, "small pages stop at the scale cap");
        assert_eq!(base_scale(2000.0, 500.0), 2.048, "big pages are limited by the longest side");
        assert_eq!(base_scale(500.0, 2000.0), 2.048);
    }

    #[test]
    fn only_the_newest_request_of_each_kind_is_kept() {
        let r = |id, kind| Request { id, kind, region: PAGE, scale: 1.0 };
        let kept =
            newest_per_kind([r(1, Kind::Detail), r(2, Kind::Base), r(3, Kind::Detail), r(4, Kind::Detail)], |r| r.kind);
        assert_eq!(kept.iter().map(|r| r.id).collect::<Vec<_>>(), [2, 4], "base first, the last detail");
        assert!(newest_per_kind(Vec::<Request>::new(), |r| r.kind).is_empty());
    }
}
