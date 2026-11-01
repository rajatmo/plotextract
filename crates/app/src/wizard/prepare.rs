//! Step 1, crop and mask: the part of the image that is the plot, and the shapes painted over what is not part of it. Only
//! drawing the exclude mask can be undone: it is the one thing done with a hand that slips.

use kurbo::Rect;
use plotextract_core::History;
use plotextract_raster::{MaskMeta, MaskOp, PixelRect, Prepare, Tag, Value, clamp_crop};

/// How many shapes of the exclude mask can be undone.
const UNDO_LIMIT: usize = 200;

pub struct PrepareStep {
    size: (u32, u32),
    /// The crop and the exclude mask.
    prepare: Prepare,
    /// Earlier states of the exclude mask, for undo.
    history: History<Vec<MaskOp>>,
    /// Whether the crop is shown (what is outside it dimmed) and the exclude mask is shown (tinted red).
    pub(super) show_crop: bool,
    pub(super) show_exclude: bool,
    /// Counts the changes that do not just add a shape to the end of the exclude mask (undo, redo, deleting): a picture of
    /// the mask made before one of them cannot be brought up to date by adding the new shapes.
    mask_epoch: u64,
}

impl PrepareStep {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            size: (width, height),
            prepare: Prepare::default(),
            history: History::new(UNDO_LIMIT),
            show_crop: true,
            show_exclude: true,
            mask_epoch: 0,
        }
    }

    /// The crop and the exclude mask.
    pub fn prepare(&self) -> &Prepare {
        &self.prepare
    }

    /// The crop in whole pixels: the whole image until one is set.
    pub fn crop_pixels(&self) -> PixelRect {
        self.prepare.crop_pixels(self.size.0, self.size.1)
    }

    /// See the field: while it stays the same, the exclude mask only grows at its end.
    pub fn mask_epoch(&self) -> u64 {
        self.mask_epoch
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Take back the last change to the exclude mask. Whether there was one.
    pub(super) fn undo(&mut self) -> bool {
        let Some(previous) = self.history.undo(&self.prepare.mask) else { return false };
        self.restore(previous);
        true
    }

    pub(super) fn redo(&mut self) -> bool {
        let Some(next) = self.history.redo(&self.prepare.mask) else { return false };
        self.restore(next);
        true
    }

    fn restore(&mut self, mask: Vec<MaskOp>) {
        self.prepare.mask = mask;
        self.mask_epoch += 1;
    }

    /// Set the crop (made valid first), or `None` for the whole image. Whether it changed: a rectangle that cannot be a
    /// crop (too small, off the image) is ignored, and one covering the whole image is no crop.
    pub(super) fn set_crop(&mut self, rect: Option<Rect>) -> bool {
        let crop = match rect {
            None => None,
            Some(r) => match clamp_crop(r, self.size.0, self.size.1) {
                Some(c) => Some(c),
                None => return false,
            },
        };
        let whole = Rect::new(0.0, 0.0, f64::from(self.size.0), f64::from(self.size.1));
        let crop = crop.filter(|c| *c != whole);
        let changed = crop != self.prepare.crop;
        self.prepare.crop = crop;
        changed
    }

    /// Add a shape to the exclude mask, as one undoable step. Whether it changed.
    pub(super) fn exclude(&mut self, op: MaskOp) -> bool {
        self.change(|mask| mask.push(op))
    }

    /// Empty the exclude mask, as one undoable step. Whether it changed.
    pub(super) fn clear(&mut self) -> bool {
        if !self.prepare.mask.is_empty() {
            self.mask_epoch += 1;
        }
        self.change(Vec::clear)
    }

    /// Change the exclude mask as one undoable step; nothing happens (and nothing is recorded) if it changes nothing.
    fn change(&mut self, change: impl FnOnce(&mut Vec<MaskOp>)) -> bool {
        let before = self.prepare.mask.clone();
        change(&mut self.prepare.mask);
        let changed = self.prepare.mask != before;
        if changed {
            self.history.record(before);
        }
        changed
    }

    /// What the crop is.
    pub fn crop_meta(&self, meta: MaskMeta) -> MaskMeta {
        let c = self.crop_pixels();
        meta.tag(Tag::Crop)
            .with("left", Value::Count(u64::from(c.x)))
            .with("top", Value::Count(u64::from(c.y)))
            .with("width", Value::Count(u64::from(c.width)))
            .with("height", Value::Count(u64::from(c.height)))
    }

    /// What the exclude mask is.
    pub fn exclude_meta(&self, meta: MaskMeta) -> MaskMeta {
        let ops = &self.prepare.mask;
        let erased = ops.iter().filter(|op| op.erase).count();
        meta.tag(Tag::Exclude)
            .with("shapes", Value::Count((ops.len() - erased) as u64))
            .with("erased shapes", Value::Count(erased as u64))
    }
}
