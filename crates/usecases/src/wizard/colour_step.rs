//! Step 2, colours: the ink colours of the prepared image, what each one is, and which are shown. The analysis runs as a
//! job; the colours found are kept until the crop or the exclude mask changes.

use std::collections::BTreeSet;
use std::sync::Arc;

use plotextract_algo::{ColourMap, ColourParams};
use plotextract_domain::{MaskMeta, Tag, Value};

use crate::colours::{Colours, Role};

#[derive(Default)]
pub struct ColourStep {
    pub(super) colours: Colours,
    /// The colours must be analysed afresh: the step was entered or its tolerance changed.
    wanted: bool,
    /// The crop or the exclude mask changed away from the colour step, so what it found is out of date.
    stale: bool,
    /// The inks whose masks are tinted on the image.
    pub(super) shown: BTreeSet<usize>,
}

/// The tag of what a colour is.
fn role_tag(role: Role) -> Tag {
    match role {
        Role::Curve => Tag::Curve,
        Role::Fill => Tag::Fill,
        Role::Axes => Tag::Axes,
        Role::Grid => Tag::Grid,
    }
}

impl ColourStep {
    pub fn colours(&self) -> &Colours {
        &self.colours
    }

    /// Whether the colours found match the crop and the exclude mask now (they do not while those have changed since).
    pub fn current(&self) -> bool {
        self.colours.map().is_some() && !self.stale
    }

    /// If the colours are to be analysed, the parameters to do it with (once: asking again gives nothing until the next
    /// change).
    pub(super) fn take_request(&mut self) -> Option<ColourParams> {
        let wanted = std::mem::take(&mut self.wanted);
        if wanted {
            self.stale = false;
        }
        wanted.then(|| self.colours.params())
    }

    /// The crop, the exclude mask or the tolerance changed: the colours are looked at again now if they are in view
    /// (`here`), else on return.
    pub(super) fn outdated(&mut self, here: bool) {
        if here {
            self.wanted = true;
        } else {
            self.stale = true;
        }
    }

    /// The step is entered: the colours are looked at if they never were, or the crop or the mask changed since.
    pub(super) fn enter(&mut self) {
        self.wanted = self.colours.map().is_none() || self.stale;
    }

    /// The step is left: nothing is looked at.
    pub(super) fn leave(&mut self) {
        self.wanted = false;
    }

    /// The analysis was started as job `id`.
    pub(super) fn begin(&mut self, id: u64) {
        self.colours.begin(id);
        self.shown.clear();
    }

    /// A project was brought back on a step after this one: the colours are looked at now.
    pub(super) fn restore(&mut self) {
        self.wanted = true;
    }

    /// The answer of job `id`. Whether the colours are found now.
    pub(super) fn finish(&mut self, id: u64, result: Result<Arc<ColourMap>, ()>) -> bool {
        match result {
            Ok(map) => self.colours.finish(id, map),
            Err(()) => {
                self.colours.fail(id);
                false
            }
        }
    }

    /// A colour that is nothing has no mask: it is not shown. Whether it is nothing.
    pub(super) fn forget_if_ignored(&mut self, ink: usize) -> bool {
        let ignored = self.colours.is_ignored(ink);
        if ignored {
            self.shown.remove(&ink);
        }
        ignored
    }

    pub(super) fn show(&mut self, ink: usize, on: bool) {
        if on {
            self.shown.insert(ink);
        } else {
            self.shown.remove(&ink);
        }
    }

    /// What colour `i` is, and how much of the image it covers.
    pub fn meta(&self, i: usize, meta: MaskMeta) -> MaskMeta {
        let meta = meta.tag(Tag::Colour).tags(self.colours.roles(i).into_iter().map(role_tag));
        match self.colours.map().and_then(|map| Some((map, map.inks.get(i)?))) {
            Some((map, ink)) => {
                let area = (f64::from(map.crop.width) * f64::from(map.crop.height)).max(1.0);
                meta.with("colour", Value::Colour(ink.rgb))
                    .with("pixels", Value::Count(ink.pixels))
                    .with("share", Value::Percent(100.0 * ink.pixels as f64 / area))
            }
            None => meta,
        }
    }
}
