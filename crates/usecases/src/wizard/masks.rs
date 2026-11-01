//! The masks of the wizard, and what the UI shows of each: the one place that goes through every kind of mask for the
//! panels. A new kind of mask is a variant of [`MaskId`] here, an arm in [`Wizard::view`] and its step's module.

use plotextract_domain::Slot;

use super::{Step, Wizard};

/// A mask the wizard works with: the crop, the exclude mask, the inks of the image (numbered from 0 in the order they cover
/// it; the panel calls them Colour 1, 2, ...), the four ticks, and the groups of pieces.
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
    /// A group of pieces of the curve colours, by its number: copies of one shape, or a solid line.
    Shape(usize),
}

/// The ID a mask goes by in its metadata: `crop`, `exclude`, `colour-3`, `tick-xmin`, `shape-2`.
pub fn mask_key(mask: MaskId) -> String {
    match mask {
        MaskId::Crop => "crop".into(),
        MaskId::Exclude => "exclude".into(),
        MaskId::Ink(i) => format!("colour-{}", i + 1),
        MaskId::Tick(slot) => format!("tick-{}", ["xmin", "xmax", "ymin", "ymax"][slot as usize]),
        MaskId::Shape(k) => format!("shape-{}", k + 1),
    }
}

/// The masks grouped by the step that makes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Plot,
    Colours,
    Ticks,
    Shapes,
}

impl Section {
    pub const ALL: [Section; 4] = [Section::Plot, Section::Colours, Section::Ticks, Section::Shapes];

    pub fn title(self) -> &'static str {
        match self {
            Section::Plot => "Plot",
            Section::Colours => "Colours",
            Section::Ticks => "Ticks",
            Section::Shapes => "Shapes",
        }
    }

    /// The step that makes its masks.
    pub fn step(self) -> Step {
        match self {
            Section::Plot => Step::Prepare,
            Section::Colours => Step::Colours,
            Section::Ticks => Step::PlotArea,
            Section::Shapes => Step::Shapes,
        }
    }
}

/// The small picture of a mask, by what it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    Crop,
    Exclude,
    Ink {
        ink: usize,
        rgb: [u8; 3],
    },
    Tick(Slot),
    /// A group: a series of shapes, or a solid line.
    Shape {
        k: usize,
        solid: bool,
    },
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
}

impl Wizard {
    /// What the UI shows of mask `mask`.
    pub fn view(&self, mask: MaskId) -> MaskView {
        let plural = |n: usize, one: &str| format!("{n} {one}{}", if n == 1 { "" } else { "s" });
        let (section, swatch, info, delete) = match mask {
            MaskId::Crop => {
                let c = self.crop_pixels();
                (Section::Plot, Swatch::Crop, format!("{}\u{d7}{}", c.width, c.height), "Remove the crop")
            }
            MaskId::Exclude => (
                Section::Plot,
                Swatch::Exclude,
                plural(self.prepare().mask.len(), "shape"),
                "Clear everything excluded (Ctrl+Z brings it back)",
            ),
            MaskId::Ink(i) => {
                let map = self.colours().map();
                let rgb = map.and_then(|m| m.inks.get(i)).map_or([0; 3], |ink| ink.rgb);
                let share = map.and_then(|m| Some((m, m.inks.get(i)?))).map_or(0.0, |(m, ink)| {
                    100.0 * ink.pixels as f64 / (f64::from(m.crop.width) * f64::from(m.crop.height)).max(1.0)
                });
                (Section::Colours, Swatch::Ink { ink: i, rgb }, format!("{share:.1}%"), "Ignore this colour")
            }
            MaskId::Tick(t) => {
                let axis = if super::on_x_axis(t) { "x" } else { "y" };
                let info = self.tick_position(t).map_or_else(|| "none".into(), |at| format!("{axis} {at:.1}"));
                (Section::Ticks, Swatch::Tick(t), info, "Take this tick's line away")
            }
            MaskId::Shape(k) => {
                let t = self.shapes().and_then(|s| s.types.get(k));
                let solid = t.is_some_and(|t| t.solid());
                let n = t.map_or(0, |t| t.members.len());
                let delete = "Take its pieces out of it: they are left over";
                (Section::Shapes, Swatch::Shape { k, solid }, format!("\u{d7}{n}"), delete)
            }
        };
        let name = if let MaskId::Shape(k) = mask { self.group_name(k) } else { mask_name(mask) };
        MaskView { name, section, swatch, info, delete }
    }
}

/// A tick as the name of a mask: "X min".
pub fn tick_name(slot: Slot) -> &'static str {
    match slot {
        Slot::XMin => "X min",
        Slot::XMax => "X max",
        Slot::YMin => "Y min",
        Slot::YMax => "Y max",
    }
}

/// The name a mask of the tracing wizard goes by.
pub fn mask_name(mask: MaskId) -> String {
    match mask {
        MaskId::Crop => "Crop".into(),
        MaskId::Exclude => "Exclude".into(),
        MaskId::Ink(i) => format!("Colour {}", i + 1),
        MaskId::Tick(t) => tick_name(t).into(),
        MaskId::Shape(k) => format!("Shape {}", k + 1),
    }
}
