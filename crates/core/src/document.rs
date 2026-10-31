//! What the user builds from a figure: the calibration, the series and which curves are hidden. This is everything that is
//! saved in a project and everything undo covers; it knows nothing about selection, layers or the screen.

use std::collections::BTreeSet;

use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::{Axis, Calibration, ExportError, Primitive, Series, Slot};

/// One change to a [`Document`].
#[derive(Clone, Debug)]
pub enum Edit {
    SetHidden(usize, bool),
    /// Store a point's coordinate in a calibration slot: its x for the x slots, its y for the y slots.
    RecordPoint(Slot, Point),
    SetValue(Slot, Option<f64>),
    SetLog(Axis, bool),
    ClearCalibration,
    NewSeries,
    DeleteSeries(usize),
    RenameSeries(usize, String),
    SetActiveSeries(usize),
    /// Add a primitive to the active series (creating one if there is none), or remove it if already a member.
    ToggleMember(usize),
    ClearSeries(usize),
    SetSortByX(usize, bool),
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EditError {
    #[error("no such curve or series")]
    OutOfRange,
}

/// The saved part of a session; the figure itself is reloaded from `source`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub source: Option<String>,
    pub calibration: Calibration,
    pub series: Vec<Series>,
    pub hidden: Vec<usize>,
    /// The page of the source file the project is about, from 0.
    #[serde(default)]
    pub page: usize,
}

impl Project {
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(text: &str) -> serde_json::Result<Self> {
        serde_json::from_str(text)
    }
}

#[derive(Clone, Debug)]
pub struct Document {
    calibration: Calibration,
    series: Vec<Series>,
    active: Option<usize>,
    /// The hidden curves. Few in practice, so undo snapshots of the document stay small however many curves there are.
    hidden: BTreeSet<usize>,
    /// How many curves the figure has.
    curves: usize,
}

impl Document {
    /// An empty document for a figure with `primitive_count` primitives.
    pub fn new(primitive_count: usize) -> Self {
        Self {
            calibration: Calibration::default(),
            series: Vec::new(),
            active: None,
            hidden: BTreeSet::new(),
            curves: primitive_count,
        }
    }

    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn active_series(&self) -> Option<usize> {
        self.active
    }

    pub fn is_hidden(&self, primitive: usize) -> bool {
        self.hidden.contains(&primitive)
    }

    /// Whether the active series has points and both axes are calibrated.
    pub fn can_export(&self) -> bool {
        self.calibration.is_calibrated() && self.active.is_some_and(|a| !self.series[a].members.is_empty())
    }

    /// The numbers of a series in plot units, as CSV.
    pub fn csv(&self, series: usize, primitives: &[Primitive]) -> Result<String, ExportError> {
        self.series.get(series).ok_or(ExportError::NoSuchSeries)?.to_csv(primitives, &self.calibration)
    }

    pub fn apply(&mut self, edit: Edit) -> Result<(), EditError> {
        self.check(&edit)?;
        match edit {
            Edit::SetHidden(p, hidden) => {
                if hidden {
                    self.hidden.insert(p);
                } else {
                    self.hidden.remove(&p);
                }
            }
            Edit::RecordPoint(slot, point) => {
                let axis = self.calibration.axis_mut(slot.axis());
                axis.pos[slot.which()] = Some(if slot.axis() == Axis::X { point.x } else { point.y });
            }
            Edit::SetValue(slot, v) => self.calibration.axis_mut(slot.axis()).value[slot.which()] = v,
            Edit::SetLog(axis, log) => self.calibration.axis_mut(axis).log = log,
            Edit::ClearCalibration => self.calibration = Calibration::default(),
            Edit::NewSeries => self.new_series(),
            Edit::DeleteSeries(i) => {
                self.series.remove(i);
                self.active = match self.active {
                    _ if self.series.is_empty() => None,
                    Some(a) if a > i => Some(a - 1),
                    Some(a) if a == i => Some(i.min(self.series.len() - 1)),
                    other => other,
                };
            }
            Edit::RenameSeries(i, name) => self.series[i].name = name,
            Edit::SetActiveSeries(i) => self.active = Some(i),
            Edit::ToggleMember(p) => {
                if self.active.is_none() {
                    self.new_series();
                }
                let members = &mut self.series[self.active.expect("a series exists")].members;
                match members.iter().position(|&m| m == p) {
                    Some(at) => {
                        members.remove(at);
                    }
                    None => members.push(p),
                }
            }
            Edit::ClearSeries(i) => self.series[i].members.clear(),
            Edit::SetSortByX(i, on) => self.series[i].sort_by_x = on,
        }
        Ok(())
    }

    fn new_series(&mut self) {
        self.series.push(Series::new(format!("Series {}", self.series.len() + 1)));
        self.active = Some(self.series.len() - 1);
    }

    fn check(&self, edit: &Edit) -> Result<(), EditError> {
        let (curves, series) = (self.curves, self.series.len());
        let in_range = match edit {
            Edit::SetHidden(p, _) | Edit::ToggleMember(p) => *p < curves,
            Edit::DeleteSeries(i)
            | Edit::RenameSeries(i, _)
            | Edit::SetActiveSeries(i)
            | Edit::ClearSeries(i)
            | Edit::SetSortByX(i, _) => *i < series,
            _ => true,
        };
        if in_range { Ok(()) } else { Err(EditError::OutOfRange) }
    }

    pub fn to_project(&self, source: Option<String>, page: usize) -> Project {
        Project {
            version: 1,
            source,
            calibration: self.calibration.clone(),
            series: self.series.clone(),
            hidden: self.hidden.iter().copied().collect(),
            page,
        }
    }

    /// Restore a saved project. Out-of-range references are dropped.
    pub fn load_project(&mut self, project: Project) {
        let n = self.curves;
        self.hidden = project.hidden.into_iter().filter(|&i| i < n).collect();
        self.calibration = project.calibration;
        self.series = project.series;
        for s in &mut self.series {
            s.members.retain(|&m| m < n);
        }
        self.active = if self.series.is_empty() { None } else { Some(0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrimitiveKind;

    fn line(points: &[(f64, f64)]) -> Primitive {
        Primitive::new(PrimitiveKind::Stroke, 0, points.iter().map(|&(x, y)| Point::new(x, y)).collect(), vec![])
    }

    #[test]
    fn recording_stores_the_axis_coordinate_of_the_point() {
        let mut d = Document::new(1);
        d.apply(Edit::RecordPoint(Slot::XMin, Point::new(12.0, 99.0))).unwrap();
        d.apply(Edit::RecordPoint(Slot::YMax, Point::new(12.0, 99.0))).unwrap();
        assert_eq!(d.calibration().position(Slot::XMin), Some(12.0));
        assert_eq!(d.calibration().position(Slot::YMax), Some(99.0));
        assert_eq!(d.calibration().position(Slot::XMax), None);
    }

    #[test]
    fn indices_out_of_range_are_refused_and_change_nothing() {
        let mut d = Document::new(2);
        assert_eq!(d.apply(Edit::SetHidden(2, true)), Err(EditError::OutOfRange));
        assert_eq!(d.apply(Edit::ToggleMember(5)), Err(EditError::OutOfRange));
        assert_eq!(d.apply(Edit::DeleteSeries(0)), Err(EditError::OutOfRange));
        assert!(d.series().is_empty() && !d.is_hidden(0) && !d.is_hidden(1));
    }

    #[test]
    fn toggling_a_member_creates_the_first_series_and_removes_on_the_second_toggle() {
        let mut d = Document::new(3);
        d.apply(Edit::ToggleMember(1)).unwrap();
        d.apply(Edit::ToggleMember(2)).unwrap();
        assert_eq!(d.series()[0].name, "Series 1");
        assert_eq!(d.series()[0].members, vec![1, 2]);
        d.apply(Edit::ToggleMember(1)).unwrap();
        assert_eq!(d.series()[0].members, vec![2]);
    }

    #[test]
    fn deleting_a_series_keeps_a_sensible_active_one() {
        let mut d = Document::new(1);
        for _ in 0..3 {
            d.apply(Edit::NewSeries).unwrap();
        }
        assert_eq!(d.active_series(), Some(2));
        d.apply(Edit::DeleteSeries(0)).unwrap();
        assert_eq!(d.active_series(), Some(1), "the active series moved down with the list");
        d.apply(Edit::DeleteSeries(1)).unwrap();
        assert_eq!(d.active_series(), Some(0));
        d.apply(Edit::DeleteSeries(0)).unwrap();
        assert_eq!(d.active_series(), None);
    }

    #[test]
    fn csv_needs_calibration_and_points() {
        let prims = vec![line(&[(0.0, 100.0), (50.0, 50.0)])];
        let mut d = Document::new(1);
        d.apply(Edit::ToggleMember(0)).unwrap();
        assert!(!d.can_export());
        assert!(matches!(d.csv(0, &prims), Err(ExportError::NotCalibrated)));
        assert!(matches!(d.csv(3, &prims), Err(ExportError::NoSuchSeries)));
        for (slot, pos, value) in
            [(Slot::XMin, 0.0, 0.0), (Slot::XMax, 100.0, 10.0), (Slot::YMin, 100.0, 0.0), (Slot::YMax, 0.0, 1.0)]
        {
            let point = if slot.axis() == Axis::X { Point::new(pos, 0.0) } else { Point::new(0.0, pos) };
            d.apply(Edit::RecordPoint(slot, point)).unwrap();
            d.apply(Edit::SetValue(slot, Some(value))).unwrap();
        }
        assert!(d.can_export());
        assert_eq!(d.csv(0, &prims).unwrap(), "x,y\n0,0\n5,0.5\n");
    }

    #[test]
    fn projects_round_trip_through_json() {
        let mut d = Document::new(3);
        d.apply(Edit::RecordPoint(Slot::XMin, Point::new(4.0, 0.0))).unwrap();
        d.apply(Edit::SetValue(Slot::XMin, Some(1.5))).unwrap();
        d.apply(Edit::SetLog(Axis::Y, true)).unwrap();
        d.apply(Edit::ToggleMember(1)).unwrap();
        d.apply(Edit::SetHidden(2, true)).unwrap();
        let json = d.to_project(Some("fig.pdf".into()), 0).to_json().unwrap();
        assert!(json.contains("fig.pdf"));

        let mut fresh = Document::new(3);
        fresh.load_project(Project::from_json(&json).unwrap());
        assert_eq!(fresh.calibration(), d.calibration());
        assert_eq!(fresh.series(), d.series());
        assert!(fresh.is_hidden(2) && !fresh.is_hidden(0));
    }

    #[test]
    fn a_project_for_a_bigger_figure_loses_what_no_longer_exists() {
        let mut big = Document::new(5);
        big.apply(Edit::ToggleMember(4)).unwrap();
        big.apply(Edit::ToggleMember(1)).unwrap();
        big.apply(Edit::SetHidden(4, true)).unwrap();
        let project = big.to_project(None, 0);
        let mut small = Document::new(2);
        small.load_project(project);
        assert_eq!(small.series()[0].members, vec![1]);
        assert!(!small.is_hidden(0) && !small.is_hidden(1));
    }
}
