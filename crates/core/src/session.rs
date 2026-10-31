//! The document state machine: everything the user does with a figure, changed only by commands.

use kurbo::Point;
use serde::{Deserialize, Serialize};

use crate::{Axis, Calibration, ExportError, Figure, Hit, Primitive, PrimitiveKind, Series, Slot, SnapIndex};

/// A layer that can be shown or hidden as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Strokes,
    Fills,
    Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layers {
    pub strokes: bool,
    pub fills: bool,
    pub page: bool,
}

/// The selected primitive and, if a click snapped to one, its vertex.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub primitive: usize,
    pub vertex: Option<usize>,
}

#[derive(Clone, Debug)]
pub enum Command {
    Select(Option<Pick>),
    SetHidden(usize, bool),
    SetLayer(Layer, bool),
    /// Store the selected point's coordinate in a calibration slot.
    RecordSelected(Slot),
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

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Select a point first (Ctrl+click on a curve): the point it snaps to is what gets recorded.")]
    NoPointSelected,
    #[error("that curve is hidden")]
    Hidden,
    #[error("no such primitive or series")]
    OutOfRange,
    #[error(transparent)]
    Export(#[from] ExportError),
}

/// What undo restores.
#[derive(Clone)]
struct Snapshot {
    calibration: Calibration,
    series: Vec<Series>,
    active: Option<usize>,
    hidden: Vec<bool>,
}

/// The saved part of a session; the figure itself is reloaded from `source`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub source: Option<String>,
    pub calibration: Calibration,
    pub series: Vec<Series>,
    pub hidden: Vec<usize>,
}

pub struct Session {
    pub figure: Figure,
    pub source: Option<String>,
    calibration: Calibration,
    series: Vec<Series>,
    active: Option<usize>,
    hidden: Vec<bool>,
    layers: Layers,
    visible: Vec<bool>,
    selection: Option<Pick>,
    index: SnapIndex,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

const UNDO_LIMIT: usize = 200;

impl Session {
    pub fn new(figure: Figure, source: Option<String>) -> Self {
        let n = figure.primitives.len();
        let index = SnapIndex::new(&figure.primitives);
        let mut s = Self {
            figure,
            source,
            calibration: Calibration::default(),
            series: Vec::new(),
            active: None,
            hidden: vec![false; n],
            layers: Layers { strokes: true, fills: true, page: true },
            visible: vec![true; n],
            selection: None,
            index,
            undo: Vec::new(),
            redo: Vec::new(),
        };
        s.refresh_visible();
        s
    }

    // --- reading -------------------------------------------------------------------------------

    pub fn calibration(&self) -> &Calibration {
        &self.calibration
    }

    pub fn series(&self) -> &[Series] {
        &self.series
    }

    pub fn active_series(&self) -> Option<usize> {
        self.active
    }

    pub fn layers(&self) -> Layers {
        self.layers
    }

    pub fn is_hidden(&self, primitive: usize) -> bool {
        self.hidden[primitive]
    }

    /// Whether a primitive is drawn, snapped to and selectable.
    pub fn is_visible(&self, primitive: usize) -> bool {
        self.visible[primitive]
    }

    pub fn selection(&self) -> Option<Pick> {
        self.selection
    }

    pub fn selected_primitive(&self) -> Option<&Primitive> {
        self.selection.map(|p| &self.figure.primitives[p.primitive])
    }

    /// The selected point in figure coordinates.
    pub fn selected_point(&self) -> Option<Point> {
        let pick = self.selection?;
        Some(self.figure.primitives[pick.primitive].points[pick.vertex?])
    }

    /// Plot values of a figure point, once both axes are calibrated.
    pub fn plot_coords(&self, p: Point) -> Option<(f64, f64)> {
        let (xm, ym) = self.calibration.maps()?;
        Some((xm.apply(p.x), ym.apply(p.y)))
    }

    pub fn snap_vertex(&self, at: Point, radius: f64) -> Option<Hit> {
        self.index.nearest_vertex(&self.figure.primitives, &self.visible, at, radius)
    }

    /// What a click at `at` would select: the vertex within `vertex_radius`, else a line within `line_radius`.
    pub fn pick_at(&self, at: Point, vertex_radius: f64, line_radius: f64) -> Option<Pick> {
        if let Some(h) = self.snap_vertex(at, vertex_radius) {
            return Some(Pick { primitive: h.primitive, vertex: Some(h.vertex) });
        }
        self.index.nearest_primitive(&self.visible, at, line_radius).map(|primitive| Pick { primitive, vertex: None })
    }

    pub fn can_export(&self) -> bool {
        self.calibration.is_calibrated() && self.active.is_some_and(|a| !self.series[a].members.is_empty())
    }

    pub fn csv(&self, series: usize) -> Result<String, SessionError> {
        let s = self.series.get(series).ok_or(SessionError::OutOfRange)?;
        Ok(s.to_csv(&self.figure.primitives, &self.calibration)?)
    }

    // --- changing ------------------------------------------------------------------------------

    pub fn apply(&mut self, command: Command) -> Result<(), SessionError> {
        let n = self.figure.primitives.len();
        match &command {
            Command::Select(Some(pick)) => {
                if pick.primitive >= n {
                    return Err(SessionError::OutOfRange);
                }
                if !self.visible[pick.primitive] {
                    return Err(SessionError::Hidden);
                }
            }
            Command::SetHidden(p, _) | Command::ToggleMember(p) if *p >= n => return Err(SessionError::OutOfRange),
            Command::DeleteSeries(i)
            | Command::RenameSeries(i, _)
            | Command::SetActiveSeries(i)
            | Command::ClearSeries(i)
            | Command::SetSortByX(i, _)
                if *i >= self.series.len() =>
            {
                return Err(SessionError::OutOfRange);
            }
            Command::RecordSelected(_) if self.selected_point().is_none() => return Err(SessionError::NoPointSelected),
            _ => {}
        }
        let undoable = !matches!(command, Command::Select(_) | Command::SetLayer(..));
        let before = undoable.then(|| self.snapshot());
        self.execute(command);
        if let Some(before) = before {
            self.undo.push(before);
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        Ok(())
    }

    fn execute(&mut self, command: Command) {
        match command {
            Command::Select(pick) => self.selection = pick,
            Command::SetHidden(p, hidden) => {
                self.hidden[p] = hidden;
                self.refresh_visible();
            }
            Command::SetLayer(layer, on) => {
                match layer {
                    Layer::Strokes => self.layers.strokes = on,
                    Layer::Fills => self.layers.fills = on,
                    Layer::Page => self.layers.page = on,
                }
                self.refresh_visible();
            }
            Command::RecordSelected(slot) => {
                let p = self.selected_point().expect("checked in apply");
                let axis = self.calibration.axis_mut(slot.axis());
                axis.pos[slot.which()] = Some(if slot.axis() == Axis::X { p.x } else { p.y });
            }
            Command::SetValue(slot, v) => self.calibration.axis_mut(slot.axis()).value[slot.which()] = v,
            Command::SetLog(axis, log) => self.calibration.axis_mut(axis).log = log,
            Command::ClearCalibration => self.calibration = Calibration::default(),
            Command::NewSeries => {
                self.series.push(Series::new(format!("Series {}", self.series.len() + 1)));
                self.active = Some(self.series.len() - 1);
            }
            Command::DeleteSeries(i) => {
                self.series.remove(i);
                self.active = match self.active {
                    _ if self.series.is_empty() => None,
                    Some(a) if a > i => Some(a - 1),
                    Some(a) if a == i => Some(i.min(self.series.len() - 1)),
                    other => other,
                };
            }
            Command::RenameSeries(i, name) => self.series[i].name = name,
            Command::SetActiveSeries(i) => self.active = Some(i),
            Command::ToggleMember(p) => {
                if self.active.is_none() {
                    self.execute(Command::NewSeries);
                }
                let members = &mut self.series[self.active.expect("a series exists")].members;
                match members.iter().position(|&m| m == p) {
                    Some(at) => {
                        members.remove(at);
                    }
                    None => members.push(p),
                }
            }
            Command::ClearSeries(i) => self.series[i].members.clear(),
            Command::SetSortByX(i, on) => self.series[i].sort_by_x = on,
        }
        // A hidden primitive cannot stay selected.
        if self.selection.is_some_and(|p| !self.visible[p.primitive]) {
            self.selection = None;
        }
    }

    fn refresh_visible(&mut self) {
        let layers = self.layers;
        self.visible = self
            .figure
            .primitives
            .iter()
            .zip(&self.hidden)
            .map(|(p, &hidden)| {
                !hidden
                    && match p.kind {
                        PrimitiveKind::Stroke => layers.strokes,
                        PrimitiveKind::Fill => layers.fills,
                    }
            })
            .collect();
    }

    // --- undo ----------------------------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            calibration: self.calibration.clone(),
            series: self.series.clone(),
            active: self.active,
            hidden: self.hidden.clone(),
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.calibration = s.calibration;
        self.series = s.series;
        self.active = s.active;
        self.hidden = s.hidden;
        self.refresh_visible();
        if self.selection.is_some_and(|p| !self.visible[p.primitive]) {
            self.selection = None;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        self.restore(previous);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        self.restore(next);
        true
    }

    // --- projects ------------------------------------------------------------------------------

    pub fn project(&self) -> Project {
        Project {
            version: 1,
            source: self.source.clone(),
            calibration: self.calibration.clone(),
            series: self.series.clone(),
            hidden: self.hidden.iter().enumerate().filter(|&(_, &h)| h).map(|(i, _)| i).collect(),
        }
    }

    /// Restore a saved project onto this session's figure. Out-of-range references are dropped.
    pub fn load_project(&mut self, project: Project) {
        let n = self.figure.primitives.len();
        self.hidden = vec![false; n];
        for i in project.hidden.into_iter().filter(|&i| i < n) {
            self.hidden[i] = true;
        }
        self.calibration = project.calibration;
        self.series = project.series;
        for s in &mut self.series {
            s.members.retain(|&m| m < n);
        }
        self.active = if self.series.is_empty() { None } else { Some(0) };
        self.selection = None;
        self.undo.clear();
        self.redo.clear();
        self.refresh_visible();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn figure() -> Figure {
        let line = |kind, index, pts: &[(f64, f64)]| {
            Primitive::new(kind, index, pts.iter().map(|&(x, y)| Point::new(x, y)).collect(), vec![])
        };
        Figure {
            width: 100.0,
            height: 100.0,
            backdrop: None,
            primitives: vec![
                line(PrimitiveKind::Stroke, 0, &[(10.0, 90.0), (60.0, 40.0), (90.0, 10.0)]),
                line(PrimitiveKind::Stroke, 1, &[(10.0, 10.0), (90.0, 90.0)]),
                line(PrimitiveKind::Fill, 0, &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)]),
            ],
        }
    }

    fn select_vertex(s: &mut Session, primitive: usize, vertex: usize) {
        s.apply(Command::Select(Some(Pick { primitive, vertex: Some(vertex) }))).unwrap();
    }

    #[test]
    fn calibrate_select_and_export() {
        let mut s = Session::new(figure(), None);
        // the x axis: figure x 10 is value 0, x 90 is value 8; the y axis: figure y 90 is 0, y 10 is 8
        for (primitive, vertex, slot, value) in
            [(0, 0, Slot::XMin, 0.0), (0, 2, Slot::XMax, 8.0), (0, 0, Slot::YMin, 0.0), (0, 2, Slot::YMax, 8.0)]
        {
            select_vertex(&mut s, primitive, vertex);
            s.apply(Command::RecordSelected(slot)).unwrap();
            s.apply(Command::SetValue(slot, Some(value))).unwrap();
        }
        assert!(s.calibration().is_calibrated());
        assert_eq!(s.plot_coords(Point::new(60.0, 40.0)), Some((5.0, 5.0)));

        assert!(!s.can_export());
        s.apply(Command::ToggleMember(0)).unwrap(); // creates "Series 1"
        assert_eq!(s.series()[0].name, "Series 1");
        assert!(s.can_export());
        assert_eq!(s.csv(0).unwrap(), "x,y\n0,0\n5,5\n8,8\n");
    }

    #[test]
    fn recording_needs_a_selected_point() {
        let mut s = Session::new(figure(), None);
        assert!(matches!(s.apply(Command::RecordSelected(Slot::XMin)), Err(SessionError::NoPointSelected)));
        s.apply(Command::Select(Some(Pick { primitive: 0, vertex: None }))).unwrap(); // a curve, not a point
        assert!(matches!(s.apply(Command::RecordSelected(Slot::XMin)), Err(SessionError::NoPointSelected)));
    }

    #[test]
    fn hidden_primitives_cannot_be_selected_or_picked() {
        let mut s = Session::new(figure(), None);
        select_vertex(&mut s, 0, 1);
        s.apply(Command::SetHidden(0, true)).unwrap();
        assert_eq!(s.selection(), None);
        assert!(matches!(
            s.apply(Command::Select(Some(Pick { primitive: 0, vertex: None }))),
            Err(SessionError::Hidden)
        ));
        assert_eq!(s.pick_at(Point::new(60.0, 40.0), 3.0, 2.0), None);
        s.apply(Command::SetHidden(0, false)).unwrap();
        assert_eq!(s.pick_at(Point::new(60.5, 40.0), 3.0, 2.0), Some(Pick { primitive: 0, vertex: Some(1) }));
    }

    #[test]
    fn layers_hide_by_kind() {
        let mut s = Session::new(figure(), None);
        s.apply(Command::SetLayer(Layer::Fills, false)).unwrap();
        assert!(s.is_visible(0) && !s.is_visible(2));
        assert!(!s.can_undo(), "layer toggles are view state, not undoable edits");
    }

    #[test]
    fn pick_falls_back_to_a_line_away_from_vertices() {
        let s = Session::new(figure(), None);
        // midway along the first segment of primitive 0, far from its vertices
        assert_eq!(s.pick_at(Point::new(35.0, 65.0), 3.0, 2.0), Some(Pick { primitive: 0, vertex: None }));
    }

    #[test]
    fn undo_and_redo_restore_edits() {
        let mut s = Session::new(figure(), None);
        s.apply(Command::ToggleMember(0)).unwrap();
        s.apply(Command::ToggleMember(1)).unwrap();
        assert_eq!(s.series()[0].members, vec![0, 1]);
        assert!(s.undo());
        assert_eq!(s.series()[0].members, vec![0]);
        assert!(s.undo());
        assert!(s.series().is_empty());
        assert!(!s.undo());
        assert!(s.redo() && s.redo());
        assert_eq!(s.series()[0].members, vec![0, 1]);
        s.apply(Command::ToggleMember(1)).unwrap(); // toggling again removes it
        assert_eq!(s.series()[0].members, vec![0]);
    }

    #[test]
    fn projects_round_trip_through_json() {
        let mut s = Session::new(figure(), Some("fig.pdf".into()));
        select_vertex(&mut s, 0, 0);
        s.apply(Command::RecordSelected(Slot::XMin)).unwrap();
        s.apply(Command::SetValue(Slot::XMin, Some(1.5))).unwrap();
        s.apply(Command::SetLog(Axis::Y, true)).unwrap();
        s.apply(Command::ToggleMember(1)).unwrap();
        s.apply(Command::SetHidden(2, true)).unwrap();
        let json = serde_json::to_string(&s.project()).unwrap();

        let mut fresh = Session::new(figure(), None);
        fresh.load_project(serde_json::from_str(&json).unwrap());
        assert_eq!(fresh.calibration(), s.calibration());
        assert_eq!(fresh.series(), s.series());
        assert!(fresh.is_hidden(2) && !fresh.is_hidden(0));
        assert_eq!(s.project().source, Some("fig.pdf".into()));
        assert!(json.contains("fig.pdf"));
    }
}
