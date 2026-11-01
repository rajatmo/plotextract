//! A series: the data the user is extracting, made of one or more primitives.

use serde::{Deserialize, Serialize};

use crate::{Calibration, Primitive, format_g};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Series {
    pub name: String,
    /// Indices of the primitives that make up the series, in the order they were added.
    pub members: Vec<usize>,
    /// Order the exported rows by x instead of by member.
    pub sort_by_x: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("no such series")]
    NoSuchSeries,
    #[error("both axes must be calibrated")]
    NotCalibrated,
    #[error("the series has no points")]
    Empty,
    /// Points that map outside the range of representable numbers, typically because an axis's two reference ticks
    /// are far too close together, especially on a log axis.
    #[error("{bad} of {total} points map outside the range of representable numbers")]
    NonFinite { bad: usize, total: usize },
}

impl Series {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), members: Vec::new(), sort_by_x: false }
    }

    /// The series' points in plot units.
    pub fn rows(&self, primitives: &[Primitive], calibration: &Calibration) -> Result<Vec<[f64; 2]>, ExportError> {
        let (xmap, ymap) = calibration.maps().ok_or(ExportError::NotCalibrated)?;
        let mut rows: Vec<[f64; 2]> = self
            .members
            .iter()
            .filter_map(|&m| primitives.get(m))
            .flat_map(|p| p.points.iter().map(|pt| [xmap.apply(pt.x), ymap.apply(pt.y)]))
            .collect();
        if rows.is_empty() {
            return Err(ExportError::Empty);
        }
        let bad = rows.iter().filter(|r| !(r[0].is_finite() && r[1].is_finite())).count();
        if bad > 0 {
            return Err(ExportError::NonFinite { bad, total: rows.len() });
        }
        if self.sort_by_x {
            rows.sort_by(|a, b| a[0].total_cmp(&b[0]));
        }
        Ok(rows)
    }

    /// `x,y` header plus one line per point.
    pub fn to_csv(&self, primitives: &[Primitive], calibration: &Calibration) -> Result<String, ExportError> {
        let rows = self.rows(primitives, calibration)?;
        let mut out = String::from("x,y\n");
        for [x, y] in rows {
            out.push_str(&format!("{},{}\n", format_g(x, 10), format_g(y, 10)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use kurbo::Point;

    use super::*;
    use crate::{AxisCal, PrimitiveKind};

    fn calibration() -> Calibration {
        let axis = |p: [f64; 2], v: [f64; 2]| AxisCal { pos: p.map(Some), value: v.map(Some), log: false };
        Calibration { x: axis([0.0, 100.0], [0.0, 10.0]), y: axis([100.0, 0.0], [0.0, 1.0]) }
    }

    fn line(points: &[(f64, f64)]) -> Primitive {
        Primitive::new(PrimitiveKind::Stroke, 0, points.iter().map(|&(x, y)| Point::new(x, y)).collect(), vec![])
    }

    #[test]
    fn csv_of_two_primitives() {
        let prims = vec![line(&[(0.0, 100.0), (50.0, 50.0)]), line(&[(100.0, 0.0)])];
        let mut s = Series::new("s");
        s.members = vec![1, 0];
        assert_eq!(s.to_csv(&prims, &calibration()).unwrap(), "x,y\n10,1\n0,0\n5,0.5\n");
        s.sort_by_x = true;
        assert_eq!(s.to_csv(&prims, &calibration()).unwrap(), "x,y\n0,0\n5,0.5\n10,1\n");
    }

    #[test]
    fn export_refuses_what_it_cannot_write() {
        let prims = vec![line(&[(0.0, 0.0)])];
        let mut s = Series::new("s");
        assert!(matches!(s.rows(&prims, &calibration()), Err(ExportError::Empty)));
        s.members = vec![0];
        assert!(matches!(s.rows(&prims, &Calibration::default()), Err(ExportError::NotCalibrated)));
        // two log-axis references so close that the map overflows far away
        let mut cal = calibration();
        cal.x = AxisCal { pos: [Some(0.0), Some(1e-9)], value: [Some(1.0), Some(10.0)], log: true };
        let far = vec![line(&[(1e9, 0.0)])];
        s.members = vec![0];
        assert!(matches!(s.rows(&far, &cal), Err(ExportError::NonFinite { bad: 1, total: 1 })));
    }
}
