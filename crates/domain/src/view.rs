//! What the user is looking at, as opposed to what they are building: which layers are shown and what is selected.

use crate::PrimitiveKind;

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

impl Default for Layers {
    fn default() -> Self {
        Self { strokes: true, fills: true, page: true }
    }
}

impl Layers {
    /// Whether primitives of this kind are drawn.
    pub fn shows(&self, kind: PrimitiveKind) -> bool {
        match kind {
            PrimitiveKind::Stroke => self.strokes,
            PrimitiveKind::Fill => self.fills,
        }
    }

    pub fn set(&mut self, layer: Layer, on: bool) {
        match layer {
            Layer::Strokes => self.strokes = on,
            Layer::Fills => self.fills = on,
            Layer::Page => self.page = on,
        }
    }
}

/// The selected primitive and, if a click snapped to one, its vertex.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    pub primitive: usize,
    pub vertex: Option<usize>,
}

/// The view's state: not saved with a project and not part of undo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewState {
    pub layers: Layers,
    pub selection: Option<Pick>,
}
