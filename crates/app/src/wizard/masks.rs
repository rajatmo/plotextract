//! The masks of the wizard, and what the UI shows of each: the one place that goes through every kind of mask for the
//! panels. A new kind of mask is a variant of [`MaskId`] here, an arm in [`Wizard::view`] and its step's module.

use plotextract_core::Slot;
use plotextract_raster::shapes::Kind;

use super::{Step, Wizard};
use crate::text::mask_name;

/// A mask the wizard works with: the crop, the exclude mask, the inks of the image (numbered from 0 in the order they cover
/// it; the panel calls them Colour 1, 2, ...), the four ticks, the shape types, and the series.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MaskId {
    /// What is kept: the part of the image that is the plot.
    Crop,
    /// What is left out: the shapes painted over what is not part of the plot.
    Exclude,
    /// Where an ink colour is.
    Ink(usize),
    /// A line chosen as one of the four ticks, cut out of the axes colour.
    Tick(Slot),
    /// A shape type of the curve colours, by its place in the list (Shape 1, 2, ...): copies of one shape, or the lines,
    /// text or other pieces of a colour.
    Shape(usize),
    /// A series of data, by its number: pieces of the curve colours.
    Series(usize),
}

/// The ID a mask goes by in its metadata: `crop`, `exclude`, `colour-3`, `tick-xmin`, `shape-2`, `series-1`.
pub fn mask_key(mask: MaskId) -> String {
    match mask {
        MaskId::Crop => "crop".into(),
        MaskId::Exclude => "exclude".into(),
        MaskId::Ink(i) => format!("colour-{}", i + 1),
        MaskId::Tick(slot) => format!("tick-{}", ["xmin", "xmax", "ymin", "ymax"][slot as usize]),
        MaskId::Shape(k) => format!("shape-{}", k + 1),
        MaskId::Series(number) => format!("series-{number}"),
    }
}

/// The masks grouped by the step that makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Plot,
    Colours,
    Ticks,
    Shapes,
    Series,
}

impl Section {
    pub const ALL: [Section; 5] = [Section::Plot, Section::Colours, Section::Ticks, Section::Shapes, Section::Series];

    pub fn title(self) -> &'static str {
        match self {
            Section::Plot => "Plot",
            Section::Colours => "Colours",
            Section::Ticks => "Ticks",
            Section::Shapes => "Shapes",
            Section::Series => "Series",
        }
    }

    /// The step that makes its masks.
    pub fn step(self) -> Step {
        match self {
            Section::Plot => Step::Prepare,
            Section::Colours => Step::Colours,
            Section::Ticks => Step::PlotArea,
            Section::Shapes => Step::Shapes,
            Section::Series => Step::Curves,
        }
    }
}

/// The small picture of a mask, by what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Crop,
    Exclude,
    Ink { ink: usize, rgb: [u8; 3] },
    Tick(Slot),
    Shape { k: usize, kind: Kind },
    Series(usize),
}

/// What the UI shows of a mask, besides its metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskView {
    pub name: String,
    pub section: Section,
    pub swatch: Swatch,
    /// A few characters about it: how big, how much of the image, where, how many.
    pub info: String,
    /// What deleting it does.
    pub delete: &'static str,
    /// Whether it is named faintly: a shape that is no data.
    pub faint: bool,
}

impl Wizard {
    /// What the UI shows of mask `mask`.
    pub fn view(&self, mask: MaskId) -> MaskView {
        let plural = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
        let (section, swatch, info, delete, faint) = match mask {
            MaskId::Crop => {
                let c = self.crop_pixels();
                (Section::Plot, Swatch::Crop, format!("{}\u{d7}{}", c.width, c.height), "Remove the crop", false)
            }
            MaskId::Exclude => (
                Section::Plot,
                Swatch::Exclude,
                plural(self.prepare().mask.len(), "shape"),
                "Clear everything excluded (Ctrl+Z brings it back)",
                false,
            ),
            MaskId::Ink(i) => {
                let map = self.colours().map();
                let rgb = map.and_then(|m| m.inks.get(i)).map_or([0; 3], |ink| ink.rgb);
                let share = map.and_then(|m| Some((m, m.inks.get(i)?))).map_or(0.0, |(m, ink)| {
                    100.0 * ink.pixels as f64 / (f64::from(m.crop.width) * f64::from(m.crop.height)).max(1.0)
                });
                (Section::Colours, Swatch::Ink { ink: i, rgb }, format!("{share:.1}%"), "Ignore this colour", false)
            }
            MaskId::Tick(t) => {
                let axis = if super::on_x_axis(t) { "x" } else { "y" };
                let info = self.tick_position(t).map_or_else(|| "none".into(), |at| format!("{axis} {at:.1}"));
                (Section::Ticks, Swatch::Tick(t), info, "Take this tick's line away", false)
            }
            MaskId::Shape(k) => {
                let t = self.shapes().and_then(|s| s.types.get(k));
                let kind = t.map_or(Kind::Other, |t| t.kind);
                let n = t.map_or(0, |t| t.members.len());
                let delete = "Say this shape is nothing: it is in no series";
                (Section::Shapes, Swatch::Shape { k, kind }, format!("\u{d7}{n}"), delete, !kind.is_data())
            }
            MaskId::Series(number) => {
                let n = self.curve_step().find(number).map_or(0, |s| s.points().len());
                // short enough for the column at any count
                let info = if n >= 1000 { format!("{:.1}k pt", n as f64 / 1000.0) } else { format!("{n} pt") };
                (
                    Section::Series,
                    Swatch::Series(number),
                    info,
                    "Remove this series (its pieces stay, in no series)",
                    false,
                )
            }
        };
        MaskView { name: mask_name(mask), section, swatch, info, delete, faint }
    }
}
