//! What is known about a mask besides its pixels: an ID, a set of tags that say what it is, and a dictionary of properties
//! for the rest. The tracing steps read masks by their tags (the curves are the masks tagged [`Tag::Curve`], the ticks
//! those tagged [`Tag::Tick`]) and find what they need in the properties (a tick's position and plot value).
//!
//! Nothing here is wording for a person: the app decides how a tag or a value is shown.

use std::collections::BTreeSet;

/// What a mask is. A mask has as many tags as apply: a tick of the x axis is [`Tag::Tick`], [`Tag::XAxis`] and
/// [`Tag::XMin`] or [`Tag::XMax`]. The order is the order they are shown in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tag {
    /// The part of the image that is the plot.
    Crop,
    /// What is left out.
    Exclude,
    /// Where an ink colour is.
    Colour,
    /// A colour that is a line or the markers of a series.
    Curve,
    /// A colour that is a filled region.
    Fill,
    /// A colour that is the frame, its ticks and text; a tick is part of the axes too.
    Axes,
    /// A colour that is the grid.
    Grid,
    /// One of the four reference ticks.
    Tick,
    XAxis,
    YAxis,
    XMin,
    XMax,
    YMin,
    YMax,
    /// Part of an axis whose two ticks and values make a calibration.
    Calibrated,
    /// A group of pieces that is a solid line: one long stroke, or several of one drawing.
    Line,
    /// A group of pieces of a curve colour: copies of one shape, or a line.
    Shape,
    /// Changed by hand.
    ByHand,
}

/// A property's value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A number of things: pixels, shapes.
    Count(u64),
    /// A length or position in image pixels.
    Pixels(f64),
    Percent(f64),
    /// A number in the plot's own units.
    Number(f64),
    Colour([u8; 3]),
    Text(String),
    Flag(bool),
}

/// Properties by name, in the order they were set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Properties(Vec<(String, Value)>);

impl Properties {
    /// Set `key` to `value`, in its old place if it had one.
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        match self.0.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => *v = value,
            None => self.0.push((key, value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A mask's ID, tags and properties.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskMeta {
    /// Says which mask it is, and stays the same while the mask does: `crop`, `colour-3`, `tick-xmin`.
    pub id: String,
    pub tags: BTreeSet<Tag>,
    pub properties: Properties,
}

impl MaskMeta {
    pub fn new(id: impl Into<String>) -> Self {
        Self { id: id.into(), tags: BTreeSet::new(), properties: Properties::default() }
    }

    /// The same, with `tag` added.
    pub fn tag(mut self, tag: Tag) -> Self {
        self.tags.insert(tag);
        self
    }

    /// The same, with `tags` added.
    pub fn tags(mut self, tags: impl IntoIterator<Item = Tag>) -> Self {
        self.tags.extend(tags);
        self
    }

    /// The same, with property `key` set to `value`.
    pub fn with(mut self, key: impl Into<String>, value: Value) -> Self {
        self.properties.set(key, value);
        self
    }

    pub fn has(&self, tag: Tag) -> bool {
        self.tags.contains(&tag)
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.properties.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mask_is_known_by_its_id_tags_and_properties() {
        let tick = MaskMeta::new("tick-xmin")
            .tags([Tag::Tick, Tag::XMin, Tag::XAxis])
            .tag(Tag::Axes)
            .with("position", Value::Pixels(120.5))
            .with("value", Value::Number(0.0));
        assert_eq!(tick.id, "tick-xmin");
        assert!(tick.has(Tag::Tick) && tick.has(Tag::XMin) && !tick.has(Tag::YMin));
        // tags come in a fixed order, whatever order they were given in
        assert_eq!(tick.tags.iter().copied().collect::<Vec<_>>(), [Tag::Axes, Tag::Tick, Tag::XAxis, Tag::XMin]);
        assert_eq!(tick.get("position"), Some(&Value::Pixels(120.5)));
        assert_eq!(tick.get("nothing"), None);
    }

    #[test]
    fn properties_keep_the_order_they_were_set_in_and_a_new_value_replaces_the_old() {
        let mut p = Properties::default();
        p.set("width", Value::Count(300));
        p.set("height", Value::Count(200));
        p.set("width", Value::Count(310));
        assert_eq!(p.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["width", "height"]);
        assert_eq!(p.get("width"), Some(&Value::Count(310)));
        assert_eq!(p.len(), 2);
        assert!(!p.is_empty() && Properties::default().is_empty());
    }
}
