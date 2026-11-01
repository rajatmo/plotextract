//! A session: one figure being worked on. It keeps the three things a session consists of apart and coordinates them:
//! the [`Document`] (what the user builds, saved in a project), the [`History`] that undoes it, and the [`ViewState`]
//! (layers and selection, neither saved nor undone), plus the spatial index over the figure.

use kurbo::Point;

use plotextract_domain::{
    Calibration, Document, Edit, EditError, ExportError, Figure, History, Hit, Layer, Layers, Pick, Primitive, Project,
    Series, Slot, SnapIndex, ViewState,
};

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("no point is selected")]
    NoPointSelected,
    #[error("the curve is hidden")]
    Hidden,
    #[error("no such curve")]
    OutOfRange,
    #[error(transparent)]
    Edit(#[from] EditError),
    #[error(transparent)]
    Export(#[from] ExportError),
}

const UNDO_LIMIT: usize = 200;

pub struct Session {
    pub figure: Figure,
    /// Where the figure was loaded from, as the user gave it.
    pub source: Option<String>,
    document: Document,
    history: History<Document>,
    view: ViewState,
    index: SnapIndex,
}

impl Session {
    pub fn new(figure: Figure, source: Option<String>) -> Self {
        let index = SnapIndex::new(&figure.primitives);
        let document = Document::new(figure.primitives.len());
        Self { figure, source, document, history: History::new(UNDO_LIMIT), view: ViewState::default(), index }
    }

    // --- the document --------------------------------------------------------------------------

    pub fn calibration(&self) -> &Calibration {
        self.document.calibration()
    }

    pub fn series(&self) -> &[Series] {
        self.document.series()
    }

    pub fn active_series(&self) -> Option<usize> {
        self.document.active_series()
    }

    pub fn is_hidden(&self, primitive: usize) -> bool {
        self.document.is_hidden(primitive)
    }

    pub fn can_export(&self) -> bool {
        self.document.can_export()
    }

    pub fn csv(&self, series: usize) -> Result<String, SessionError> {
        Ok(self.document.csv(series, &self.figure.primitives)?)
    }

    /// Change the document. The change can be undone; a curve it hides stops being selected.
    pub fn edit(&mut self, edit: Edit) -> Result<(), SessionError> {
        let before = self.document.clone();
        self.document.apply(edit)?;
        self.history.record(before);
        self.drop_hidden_selection();
        Ok(())
    }

    /// Store the selected point's coordinate in a calibration slot.
    pub fn record_selected(&mut self, slot: Slot) -> Result<(), SessionError> {
        let point = self.selected_point().ok_or(SessionError::NoPointSelected)?;
        self.edit(Edit::RecordPoint(slot, point))
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.history.undo(&self.document) else { return false };
        self.document = previous;
        self.drop_hidden_selection();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.history.redo(&self.document) else { return false };
        self.document = next;
        self.drop_hidden_selection();
        true
    }

    pub fn project(&self) -> Project {
        self.document.to_project(self.source.clone(), self.figure.page)
    }

    /// Restore a saved project onto this session's figure. Undo history and selection start afresh.
    pub fn load_project(&mut self, project: Project) {
        self.document.load_project(project);
        self.history.clear();
        self.view.selection = None;
    }

    // --- the view ------------------------------------------------------------------------------

    pub fn layers(&self) -> Layers {
        self.view.layers
    }

    pub fn set_layer(&mut self, layer: Layer, on: bool) {
        self.view.layers.set(layer, on);
        self.drop_hidden_selection();
    }

    pub fn selection(&self) -> Option<Pick> {
        self.view.selection
    }

    /// Select a curve (and maybe one of its points), or nothing. A hidden curve cannot be selected.
    pub fn select(&mut self, pick: Option<Pick>) -> Result<(), SessionError> {
        if let Some(pick) = pick {
            if pick.primitive >= self.figure.primitives.len() {
                return Err(SessionError::OutOfRange);
            }
            if !self.is_visible(pick.primitive) {
                return Err(SessionError::Hidden);
            }
        }
        self.view.selection = pick;
        Ok(())
    }

    /// Whether a primitive is drawn, snapped to and selectable.
    pub fn is_visible(&self, primitive: usize) -> bool {
        !self.document.is_hidden(primitive) && self.view.layers.shows(self.figure.primitives[primitive].kind)
    }

    pub fn selected_primitive(&self) -> Option<&Primitive> {
        self.view.selection.map(|p| &self.figure.primitives[p.primitive])
    }

    /// The selected point in figure coordinates.
    pub fn selected_point(&self) -> Option<Point> {
        let pick = self.view.selection?;
        Some(self.figure.primitives[pick.primitive].points[pick.vertex?])
    }

    fn drop_hidden_selection(&mut self) {
        if self.view.selection.is_some_and(|p| !self.is_visible(p.primitive)) {
            self.view.selection = None;
        }
    }

    // --- looking things up ---------------------------------------------------------------------

    /// Plot values of a figure point, once both axes are calibrated.
    pub fn plot_coords(&self, p: Point) -> Option<(f64, f64)> {
        let (xm, ym) = self.calibration().maps()?;
        Some((xm.apply(p.x), ym.apply(p.y)))
    }

    /// A box that holds all of a primitive's vertices (a little larger than needed). Cheap, unlike measuring it.
    pub fn bounds(&self, primitive: usize) -> kurbo::Rect {
        self.index.bounds(primitive)
    }

    pub fn snap_vertex(&self, at: Point, radius: f64) -> Option<Hit> {
        self.index.nearest_vertex(&self.figure.primitives, |k| self.is_visible(k), at, radius)
    }

    /// What a click at `at` would select: the vertex within `vertex_radius`, else a line within `line_radius`.
    pub fn pick_at(&self, at: Point, vertex_radius: f64, line_radius: f64) -> Option<Pick> {
        if let Some(h) = self.snap_vertex(at, vertex_radius) {
            return Some(Pick { primitive: h.primitive, vertex: Some(h.vertex) });
        }
        self.index
            .nearest_primitive(&self.figure.primitives, |k| self.is_visible(k), at, line_radius)
            .map(|primitive| Pick { primitive, vertex: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plotextract_domain::{Axis, PrimitiveKind, Unit};

    fn figure() -> Figure {
        let line = |kind, index, pts: &[(f64, f64)]| {
            Primitive::new(kind, index, pts.iter().map(|&(x, y)| Point::new(x, y)).collect(), vec![])
        };
        Figure {
            width: 100.0,
            height: 100.0,
            unit: Unit::Point,
            page: 0,
            truncated: false,
            primitives: vec![
                line(PrimitiveKind::Stroke, 0, &[(10.0, 90.0), (60.0, 40.0), (90.0, 10.0)]),
                line(PrimitiveKind::Stroke, 1, &[(10.0, 10.0), (90.0, 90.0)]),
                line(PrimitiveKind::Fill, 0, &[(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)]),
            ],
        }
    }

    fn select_vertex(s: &mut Session, primitive: usize, vertex: usize) {
        s.select(Some(Pick { primitive, vertex: Some(vertex) })).unwrap();
    }

    #[test]
    fn calibrate_select_and_export() {
        let mut s = Session::new(figure(), None);
        // the x axis: figure x 10 is value 0, x 90 is value 8; the y axis: figure y 90 is 0, y 10 is 8
        for (primitive, vertex, slot, value) in
            [(0, 0, Slot::XMin, 0.0), (0, 2, Slot::XMax, 8.0), (0, 0, Slot::YMin, 0.0), (0, 2, Slot::YMax, 8.0)]
        {
            select_vertex(&mut s, primitive, vertex);
            s.record_selected(slot).unwrap();
            s.edit(Edit::SetValue(slot, Some(value))).unwrap();
        }
        assert!(s.calibration().is_calibrated());
        assert_eq!(s.plot_coords(Point::new(60.0, 40.0)), Some((5.0, 5.0)));

        assert!(!s.can_export());
        s.edit(Edit::ToggleMember(0)).unwrap(); // creates "Series 1"
        assert_eq!(s.series()[0].name, "Series 1");
        assert!(s.can_export());
        assert_eq!(s.csv(0).unwrap(), "x,y\n0,0\n5,5\n8,8\n");
    }

    #[test]
    fn recording_needs_a_selected_point() {
        let mut s = Session::new(figure(), None);
        assert!(matches!(s.record_selected(Slot::XMin), Err(SessionError::NoPointSelected)));
        s.select(Some(Pick { primitive: 0, vertex: None })).unwrap(); // a curve, not a point
        assert!(matches!(s.record_selected(Slot::XMin), Err(SessionError::NoPointSelected)));
        assert_eq!(s.calibration().position(Slot::XMin), None);
    }

    #[test]
    fn hidden_curves_cannot_be_selected_or_picked() {
        let mut s = Session::new(figure(), None);
        select_vertex(&mut s, 0, 1);
        s.edit(Edit::SetHidden(0, true)).unwrap();
        assert_eq!(s.selection(), None, "hiding the selected curve deselects it");
        assert!(matches!(s.select(Some(Pick { primitive: 0, vertex: None })), Err(SessionError::Hidden)));
        assert_eq!(s.pick_at(Point::new(60.0, 40.0), 3.0, 2.0), None);
        s.edit(Edit::SetHidden(0, false)).unwrap();
        assert_eq!(s.pick_at(Point::new(60.5, 40.0), 3.0, 2.0), Some(Pick { primitive: 0, vertex: Some(1) }));
        assert!(matches!(s.select(Some(Pick { primitive: 9, vertex: None })), Err(SessionError::OutOfRange)));
    }

    #[test]
    fn layers_hide_by_kind_and_are_not_undoable() {
        let mut s = Session::new(figure(), None);
        select_vertex(&mut s, 2, 0);
        s.set_layer(Layer::Fills, false);
        assert!(s.is_visible(0) && !s.is_visible(2));
        assert_eq!(s.selection(), None, "a layer that hides the selected curve deselects it");
        assert!(!s.can_undo(), "layer toggles are view state, not edits");
        s.set_layer(Layer::Fills, true);
        assert!(s.is_visible(2));
    }

    #[test]
    fn pick_falls_back_to_a_line_away_from_vertices() {
        let s = Session::new(figure(), None);
        // midway along the first segment of primitive 0, far from its vertices
        assert_eq!(s.pick_at(Point::new(35.0, 65.0), 3.0, 2.0), Some(Pick { primitive: 0, vertex: None }));
    }

    #[test]
    fn undo_and_redo_restore_edits_but_not_the_selection() {
        let mut s = Session::new(figure(), None);
        s.edit(Edit::ToggleMember(0)).unwrap();
        s.edit(Edit::ToggleMember(1)).unwrap();
        select_vertex(&mut s, 1, 0);
        assert_eq!(s.series()[0].members, vec![0, 1]);
        assert!(s.undo());
        assert_eq!(s.series()[0].members, vec![0]);
        assert_eq!(s.selection().map(|p| p.primitive), Some(1), "selecting is not an edit, so undo leaves it");
        assert!(s.undo());
        assert!(s.series().is_empty());
        assert!(!s.undo());
        assert!(s.redo() && s.redo());
        assert_eq!(s.series()[0].members, vec![0, 1]);
        s.edit(Edit::ToggleMember(1)).unwrap(); // toggling again removes it
        assert_eq!(s.series()[0].members, vec![0]);
        assert!(!s.can_redo());
    }

    #[test]
    fn undoing_a_hide_after_selecting_elsewhere_keeps_the_selection_valid() {
        let mut s = Session::new(figure(), None);
        s.edit(Edit::SetHidden(1, true)).unwrap();
        select_vertex(&mut s, 0, 0);
        s.undo(); // the hide goes away: curve 1 is visible again, curve 0 was never hidden
        assert_eq!(s.selection().map(|p| p.primitive), Some(0));
        s.redo(); // hidden again: whatever is selected must still be visible
        assert!(s.selection().is_none_or(|p| s.is_visible(p.primitive)));
    }

    #[test]
    fn projects_load_onto_a_session_and_reset_history_and_selection() {
        let mut s = Session::new(figure(), Some("fig.pdf".into()));
        select_vertex(&mut s, 0, 0);
        s.record_selected(Slot::XMin).unwrap();
        s.edit(Edit::SetValue(Slot::XMin, Some(1.5))).unwrap();
        s.edit(Edit::SetLog(Axis::Y, true)).unwrap();
        s.edit(Edit::ToggleMember(1)).unwrap();
        s.edit(Edit::SetHidden(2, true)).unwrap();
        let project = s.project();
        assert_eq!(project.source.as_deref(), Some("fig.pdf"));

        let mut fresh = Session::new(figure(), None);
        select_vertex(&mut fresh, 1, 0);
        fresh.edit(Edit::NewSeries).unwrap();
        fresh.load_project(project);
        assert_eq!(fresh.calibration(), s.calibration());
        assert_eq!(fresh.series(), s.series());
        assert!(fresh.is_hidden(2) && !fresh.is_hidden(0));
        assert_eq!(fresh.selection(), None);
        assert!(!fresh.can_undo());
    }
}
