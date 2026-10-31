//! Axis calibration: map figure coordinates on one axis to plot values.
//!
//! Each axis is set by two reference ticks: the figure coordinate of each and the plot value it stands for. The map is
//! linear in the value, or linear in log10(value) on a log axis. Both axes are independent and axis-aligned; a
//! registration step for rotated or skewed scans would sit in front of this.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    X,
    Y,
}

/// One of the four reference ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    XMin,
    XMax,
    YMin,
    YMax,
}

impl Slot {
    pub const ALL: [Slot; 4] = [Slot::XMin, Slot::XMax, Slot::YMin, Slot::YMax];

    pub fn axis(self) -> Axis {
        match self {
            Slot::XMin | Slot::XMax => Axis::X,
            Slot::YMin | Slot::YMax => Axis::Y,
        }
    }

    /// Which of the axis's two references this is.
    pub fn which(self) -> usize {
        matches!(self, Slot::XMax | Slot::YMax) as usize
    }

    /// The row the workflow moves to after this one: x min, x max, y min, y max, then stays on the last.
    pub fn next(self) -> Slot {
        match self {
            Slot::XMin => Slot::XMax,
            Slot::XMax => Slot::YMin,
            Slot::YMin => Slot::YMax,
            Slot::YMax => Slot::YMax,
        }
    }
}

/// The plot value typed for a tick: a plain number or one in scientific notation (`0.001`, `1e3`, `2.5E-4`), with
/// optional surrounding spaces. `None` for anything else, including `nan` and `inf`, which no axis can be set from.
pub fn parse_value(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CalError {
    #[error("incomplete")]
    Incomplete,
    #[error("the two reference positions are identical")]
    SamePosition,
    #[error("the two plot values are identical")]
    SameValue,
    #[error("log axis needs positive plot values")]
    NonPositiveLog,
}

/// The two references of one axis.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AxisCal {
    /// Figure coordinate of each reference tick.
    pub pos: [Option<f64>; 2],
    /// Plot value of each reference tick.
    pub value: [Option<f64>; 2],
    pub log: bool,
}

/// A figure coordinate to plot value map.
#[derive(Clone, Copy, Debug)]
pub struct AxisMap {
    p0: f64,
    u0: f64,
    slope: f64,
    log: bool,
}

impl AxisMap {
    pub fn apply(&self, p: f64) -> f64 {
        let u = self.u0 + self.slope * (p - self.p0);
        if self.log { 10f64.powf(u) } else { u }
    }
}

impl AxisCal {
    pub fn map(&self) -> Result<AxisMap, CalError> {
        let (Some(p0), Some(p1), Some(v0), Some(v1)) = (self.pos[0], self.pos[1], self.value[0], self.value[1]) else {
            return Err(CalError::Incomplete);
        };
        if p0 == p1 {
            return Err(CalError::SamePosition);
        }
        let (u0, u1) = if self.log {
            if v0 <= 0.0 || v1 <= 0.0 {
                return Err(CalError::NonPositiveLog);
            }
            (v0.log10(), v1.log10())
        } else {
            (v0, v1)
        };
        if u0 == u1 {
            return Err(CalError::SameValue);
        }
        Ok(AxisMap { p0, u0, slope: (u1 - u0) / (p1 - p0), log: self.log })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub x: AxisCal,
    pub y: AxisCal,
}

impl Calibration {
    pub fn axis(&self, axis: Axis) -> &AxisCal {
        match axis {
            Axis::X => &self.x,
            Axis::Y => &self.y,
        }
    }

    pub fn axis_mut(&mut self, axis: Axis) -> &mut AxisCal {
        match axis {
            Axis::X => &mut self.x,
            Axis::Y => &mut self.y,
        }
    }

    /// The maps of both axes, once both are fully and validly specified.
    pub fn maps(&self) -> Option<(AxisMap, AxisMap)> {
        Some((self.x.map().ok()?, self.y.map().ok()?))
    }

    pub fn is_calibrated(&self) -> bool {
        self.maps().is_some()
    }

    pub fn position(&self, slot: Slot) -> Option<f64> {
        self.axis(slot.axis()).pos[slot.which()]
    }

    pub fn value(&self, slot: Slot) -> Option<f64> {
        self.axis(slot.axis()).value[slot.which()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(p: [f64; 2], v: [f64; 2], log: bool) -> AxisCal {
        AxisCal { pos: p.map(Some), value: v.map(Some), log }
    }

    #[test]
    fn values_can_be_typed_in_scientific_notation() {
        for (text, want) in [
            ("1e3", 1000.0),
            ("1E3", 1000.0),
            ("2.5e-3", 0.0025),
            ("-1.5e+2", -150.0),
            (".5", 0.5),
            ("0.001", 0.001),
            ("  10  ", 10.0),
            ("1e-300", 1e-300),
        ] {
            assert_eq!(parse_value(text), Some(want), "{text:?}");
        }
    }

    #[test]
    fn values_that_are_not_finite_numbers_are_refused() {
        for text in ["", " ", "abc", "1e", "e3", "1,5", "1 000", "nan", "inf", "-inf", "infinity", "1e999", "--1"] {
            assert_eq!(parse_value(text), None, "{text:?}");
        }
    }

    #[test]
    fn linear_map_extrapolates_and_flips() {
        let m = axis([100.0, 200.0], [0.0, 10.0], false).map().unwrap();
        assert_eq!(m.apply(150.0), 5.0);
        assert_eq!(m.apply(300.0), 20.0);
        let flipped = axis([100.0, 200.0], [10.0, 0.0], false).map().unwrap(); // y down: larger coordinate, smaller value
        assert_eq!(flipped.apply(125.0), 7.5);
    }

    #[test]
    fn log_map_is_linear_in_the_exponent() {
        let m = axis([0.0, 100.0], [1.0, 1000.0], true).map().unwrap();
        assert!((m.apply(50.0) - 10f64.powf(1.5)).abs() < 1e-9);
        assert!((m.apply(100.0) - 1000.0).abs() < 1e-9);
    }

    #[test]
    fn invalid_axes_say_why() {
        assert_eq!(AxisCal::default().map().unwrap_err(), CalError::Incomplete);
        assert_eq!(axis([5.0, 5.0], [0.0, 1.0], false).map().unwrap_err(), CalError::SamePosition);
        assert_eq!(axis([0.0, 1.0], [2.0, 2.0], false).map().unwrap_err(), CalError::SameValue);
        assert_eq!(axis([0.0, 1.0], [0.0, 1.0], true).map().unwrap_err(), CalError::NonPositiveLog);
    }

    #[test]
    fn calibrated_needs_both_axes() {
        let mut c = Calibration { x: axis([0.0, 1.0], [0.0, 1.0], false), y: AxisCal::default() };
        assert!(!c.is_calibrated());
        c.y = axis([0.0, 1.0], [1.0, 0.0], false);
        assert!(c.is_calibrated());
    }
}
