//! The wizard's colour step, with no window in it: the ink colours found in the prepared image, and what the user says
//! each one is (any of the [`Role`]s, or none). The analysis itself is in `plotextract-raster`; it runs on a thread (see `AnalysisJob` in the model) and its
//! answer is kept here.

use std::collections::BTreeSet;
use std::sync::Arc;

use plotextract_raster::{ColourMap, ColourParams, Mask};

/// What a colour is in the figure. A colour can be several things at once: black is often the axes and a curve too, and
/// the later steps tell those apart by shape. A colour that is nothing is ignored: it has no mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// A line or the markers of a series: traced.
    Curve,
    /// A filled region: its outline is traced.
    Fill,
    /// The frame, its ticks and the text: the lines of the axes are looked for in it.
    Axes,
    /// The grid: told apart from the curves.
    Grid,
}

/// What a colour is: any of the roles, or none.
pub type Roles = BTreeSet<Role>;

impl Role {
    pub const ALL: [Role; 4] = [Role::Curve, Role::Fill, Role::Axes, Role::Grid];

    pub fn label(self) -> &'static str {
        match self {
            Role::Curve => "Curve",
            Role::Fill => "Fill",
            Role::Axes => "Axes",
            Role::Grid => "Grid",
        }
    }

    pub fn tip(self) -> &'static str {
        match self {
            Role::Curve => "A line or the markers of a series of data: it will be traced",
            Role::Fill => "A filled region, such as a bar or a shaded band: its outline will be traced",
            Role::Axes => "The frame, the ticks and the text, which are usually black or dark grey",
            Role::Grid => "Grid lines, usually a pale grey",
        }
    }

    /// A first guess from the colour alone, to be corrected: colours are curves; greys are the axes and text if dark, the
    /// grid if light.
    pub fn suggest(rgb: [u8; 3]) -> Roles {
        let (max, min) = (rgb.iter().max().copied().unwrap_or(0), rgb.iter().min().copied().unwrap_or(0));
        let role = if max - min >= 40 {
            Role::Curve
        } else if (u16::from(rgb[0]) + u16::from(rgb[1]) + u16::from(rgb[2])) / 3 < 110 {
            Role::Axes
        } else {
            Role::Grid
        };
        Roles::from([role])
    }
}

/// How much of the pixel `(x, y)` ink `ink` covers.
fn coverage_in(map: &ColourMap, ink: usize, x: u32, y: u32) -> u8 {
    match map.at(x, y) {
        (label, coverage) if usize::from(label) == ink + 1 => coverage,
        _ => 0,
    }
}

/// The inks that are the axes. It owns what it needs, so it can go to another thread.
#[derive(Clone)]
pub struct AxesSource {
    map: Arc<ColourMap>,
    inks: Vec<usize>,
}

impl AxesSource {
    /// The axes mask: over the crop, the most any of the inks covers each pixel.
    pub fn mask(&self) -> Mask {
        let crop = self.map.crop;
        let mut data = vec![0u8; crop.width as usize * crop.height as usize];
        for (j, row) in data.chunks_exact_mut(crop.width as usize).enumerate() {
            for (i, cell) in row.iter_mut().enumerate() {
                let (x, y) = (crop.x + i as u32, crop.y + j as u32);
                *cell = self.inks.iter().map(|&ink| coverage_in(&self.map, ink, x, y)).max().unwrap_or(0);
            }
        }
        Mask::from_data(crop, data)
    }
}

enum Analysis {
    /// Not looked at yet.
    Idle,
    /// The analysis with this job number is being made.
    Running(u64),
    Ready(Arc<ColourMap>),
}

/// The colour step's state.
pub struct Colours {
    /// How different two colours must be to be told apart (ΔE in Lab).
    pub tolerance: f32,
    analysis: Analysis,
    /// What each ink is.
    roles: Vec<Roles>,
    selected: Option<usize>,
}

impl Default for Colours {
    fn default() -> Self {
        Self {
            tolerance: ColourParams::default().tolerance,
            analysis: Analysis::Idle,
            roles: Vec::new(),
            selected: None,
        }
    }
}

impl Colours {
    /// The parameters the analysis runs with.
    pub fn params(&self) -> ColourParams {
        ColourParams { tolerance: self.tolerance, ..ColourParams::default() }
    }

    /// The colours found, once they are.
    pub fn map(&self) -> Option<&Arc<ColourMap>> {
        match &self.analysis {
            Analysis::Ready(map) => Some(map),
            _ => None,
        }
    }

    /// An analysis was started as job `id`: what was found before no longer matches.
    pub fn begin(&mut self, id: u64) {
        self.analysis = Analysis::Running(id);
        self.roles.clear();
        self.selected = None;
    }

    /// The answer of job `id`. Whether it was the one waited for.
    pub fn finish(&mut self, id: u64, map: Arc<ColourMap>) -> bool {
        if !matches!(self.analysis, Analysis::Running(running) if running == id) {
            return false;
        }
        self.roles = map.inks.iter().map(|ink| Role::suggest(ink.rgb)).collect();
        self.analysis = Analysis::Ready(map);
        true
    }

    /// Job `id` failed.
    pub fn fail(&mut self, id: u64) {
        if matches!(self.analysis, Analysis::Running(running) if running == id) {
            self.analysis = Analysis::Idle;
        }
    }

    /// How much of the pixel at image position `(x, y)` ink `ink` covers.
    pub fn coverage(&self, ink: usize, x: u32, y: u32) -> u8 {
        self.map().map_or(0, |map| coverage_in(map, ink, x, y))
    }

    /// The axes, to look for lines in; `None` while the colours are not known or no ink is the axes.
    pub fn axes_source(&self) -> Option<AxesSource> {
        let map = self.map()?;
        let inks: Vec<usize> = (0..map.inks.len()).filter(|&i| self.has_role(i, Role::Axes)).collect();
        if inks.is_empty() {
            return None;
        }
        Some(AxesSource { map: map.clone(), inks })
    }

    /// What the user says ink `ink` is (or the guess); nothing for an ink that is ignored or not there.
    pub fn roles(&self, ink: usize) -> Roles {
        self.roles.get(ink).cloned().unwrap_or_default()
    }

    pub fn has_role(&self, ink: usize, role: Role) -> bool {
        self.roles.get(ink).is_some_and(|roles| roles.contains(&role))
    }

    /// Whether ink `ink` is nothing, so it has no mask.
    pub fn is_ignored(&self, ink: usize) -> bool {
        self.roles.get(ink).is_none_or(BTreeSet::is_empty)
    }

    /// Say whether ink `ink` is `role`, leaving its other roles as they are.
    pub fn set_role(&mut self, ink: usize, role: Role, on: bool) {
        if let Some(roles) = self.roles.get_mut(ink) {
            if on {
                roles.insert(role);
            } else {
                roles.remove(&role);
            }
        }
    }

    /// Say ink `ink` is nothing.
    pub fn ignore(&mut self, ink: usize) {
        if let Some(roles) = self.roles.get_mut(ink) {
            roles.clear();
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn select(&mut self, ink: Option<usize>) {
        self.selected = ink.filter(|&i| i < self.roles.len());
    }
}

#[cfg(test)]
mod tests {
    use image::{Rgba, RgbaImage};
    use plotextract_raster::{Prepare, analyse_colours};

    use super::*;

    fn map() -> Arc<ColourMap> {
        let mut image = RgbaImage::from_pixel(60, 40, Rgba([255, 255, 255, 255]));
        for x in 5..55 {
            image.put_pixel(x, 10, Rgba([31, 119, 180, 255]));
            image.put_pixel(x, 11, Rgba([31, 119, 180, 255]));
            image.put_pixel(x, 30, Rgba([0, 0, 0, 255]));
            image.put_pixel(x, 31, Rgba([0, 0, 0, 255]));
            image.put_pixel(x, 20, Rgba([255, 127, 14, 255]));
        }
        Arc::new(analyse_colours(&image, &Prepare::default(), &ColourParams::default()))
    }

    #[test]
    fn colours_are_curves_dark_greys_are_the_axes_and_light_greys_the_grid() {
        assert_eq!(Role::suggest([31, 119, 180]), Roles::from([Role::Curve]));
        assert_eq!(Role::suggest([255, 127, 14]), Roles::from([Role::Curve]));
        assert_eq!(Role::suggest([0, 0, 0]), Roles::from([Role::Axes]));
        assert_eq!(Role::suggest([60, 60, 60]), Roles::from([Role::Axes]));
        assert_eq!(Role::suggest([200, 200, 200]), Roles::from([Role::Grid]));
    }

    #[test]
    fn an_answer_is_taken_only_for_the_job_waited_for() {
        let mut c = Colours::default();
        assert!(c.map().is_none());
        c.begin(3);
        assert!(!c.finish(2, map()), "an older job's answer is ignored");
        assert!(c.map().is_none());
        assert!(c.finish(3, map()));
        assert!(c.map().is_some());
        assert!(!c.finish(3, map()), "and only once");
    }

    #[test]
    fn every_ink_starts_with_a_guess_that_can_be_changed() {
        let mut c = Colours::default();
        c.begin(1);
        c.finish(1, map());
        let m = c.map().unwrap().clone();
        assert_eq!(m.inks.len(), 3);
        for (i, ink) in m.inks.iter().enumerate() {
            assert_eq!(c.roles(i), Role::suggest(ink.rgb));
        }
        // a colour can be several things at once, and is ignored once it is none
        let black = m.inks.iter().position(|i| i.rgb == [0, 0, 0]).unwrap();
        c.set_role(black, Role::Curve, true);
        assert_eq!(c.roles(black), Roles::from([Role::Curve, Role::Axes]));
        assert!(c.has_role(black, Role::Axes) && c.has_role(black, Role::Curve) && !c.is_ignored(black));
        c.set_role(black, Role::Axes, false);
        c.set_role(black, Role::Curve, false);
        assert!(c.is_ignored(black) && c.roles(black).is_empty());
        let blue = m.inks.iter().position(|i| i.rgb == [31, 119, 180]).unwrap();
        c.ignore(blue);
        assert!(c.is_ignored(blue));
        assert!(c.is_ignored(99) && !c.has_role(99, Role::Curve), "an ink that is not there is nothing");
        c.set_role(99, Role::Curve, true);
        assert!(c.is_ignored(99));
    }

    #[test]
    fn selecting_takes_only_inks_that_exist_and_a_new_analysis_forgets_it() {
        let mut c = Colours::default();
        c.begin(1);
        c.finish(1, map());
        c.select(Some(1));
        assert_eq!(c.selected(), Some(1));
        c.select(Some(9));
        assert_eq!(c.selected(), None);
        c.select(Some(0));
        c.begin(2);
        assert_eq!(c.selected(), None);
        assert!(c.map().is_none(), "the old colours do not match what is being analysed");
        c.fail(2);
        assert!(c.map().is_none());
    }
}
