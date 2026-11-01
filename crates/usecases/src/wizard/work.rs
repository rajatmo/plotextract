//! What the user did in the wizard, to save with a project and bring back: the crop and the exclude shapes, the tolerance
//! and what each colour is, the ticks with their values, the settings of the searches, the changes made to the groups and
//! the links. What is found from them (the colours, the lines, the pieces and their groups) is not kept: it is found
//! again, and the edits and the links, which name their pieces by a pixel of each, are made again on it.

use kurbo::Rect;
use plotextract_algo::curves;
use plotextract_algo::lines::{self, Algorithm, Line};
use plotextract_algo::shapes::{self, Edit, Seed};
use plotextract_domain::MaskOp;
use serde::{Deserialize, Serialize};

use super::{Step, Wizard};
use crate::colours::Roles;

/// What a colour was said to be, by its colour: the colours are found again, and each takes the roles of the saved one
/// nearest it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InkWork {
    pub rgb: [u8; 3],
    pub roles: Roles,
}

/// The wizard's work, as it is saved. A field a file lacks is what a new wizard has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Work {
    /// The step that was shown.
    pub step: Step,
    pub crop: Option<Rect>,
    pub exclude: Vec<MaskOp>,
    pub tolerance: f32,
    /// What each colour found was said to be; empty if the colours were not looked at.
    pub inks: Vec<InkWork>,
    pub algorithm: Algorithm,
    pub line_params: lines::Params,
    /// The line of each tick and the value typed for it, in the order of `Slot::ALL`.
    pub ticks: [Option<Line>; 4],
    pub values: [Option<f64>; 4],
    /// Whether the x and the y axis are logarithmic.
    pub log: [bool; 2],
    pub curve_params: curves::Params,
    pub shape_params: shapes::Params,
    /// The changes made to the groups by hand, in order.
    pub edits: Vec<Edit>,
    /// The group whose pieces are linked, by its number.
    pub link_group: Option<usize>,
    /// The links, in order, each its pieces (by a pixel of each) in order.
    pub links: Vec<Vec<Seed>>,
    /// The pieces whose link of that one piece runs from right to left.
    pub turned: Vec<Seed>,
}

impl Default for Work {
    fn default() -> Self {
        Wizard::new(1, 1).work()
    }
}

impl Work {
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("the work is plain data")
    }

    pub fn from_value(value: serde_json::Value) -> serde_json::Result<Self> {
        serde_json::from_value(value)
    }
}

/// How far apart two colours are, channel by channel.
pub(crate) fn rgb_distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.iter().zip(b).map(|(&x, y)| u32::from(x.abs_diff(y))).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_lacks_fields_gets_what_a_new_wizard_has() {
        let work = Work::from_value(serde_json::json!({ "step": "Shapes" })).unwrap();
        assert_eq!(work.step, Step::Shapes);
        assert_eq!(work.tolerance, Work::default().tolerance);
        assert!(work.tolerance > 0.0 && work.exclude.is_empty() && work.links.is_empty());
    }
}
