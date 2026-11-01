//! The ink colours of a plot, and which pixel belongs to which.
//!
//! A plot is drawn in a handful of colours on a background, but an image of it holds many more: every antialiased edge is
//! a blend of an ink and what lies under it, and a JPEG adds noise. The analysis finds the real colours and says, for each
//! pixel, which ink it is (or that it is background) and how much of the pixel the ink covers.
//!
//! - The **background** is the commonest colour along the border of the crop.
//! - **Inks** are found where the pixels are densest: the cells of the colour cube are taken in order of how many pixels lie
//!   within `tolerance` of them (a thin line's colour is spread over many slightly different values, none of them common,
//!   but together they are). A cell is a new ink unless it is near one already found or on the straight line between two
//!   (an antialiased blend of them). The ink's colour is the commonest exact colour in its cell.
//! - A pixel's **coverage** is how far along the line from the background to its ink its colour is, so a pixel halfway
//!   between white paper and red ink is red with coverage one half. This is what later steps use to place a curve's centre
//!   to a fraction of a pixel instead of cutting it at a threshold.
//!
//! Colours are compared in CIE Lab, where distance is close to how different two colours look. Pixels under the mask, or
//! outside the crop, are *unknown*: they do not count towards any colour.

use std::collections::HashMap;

use image::RgbaImage;
use rayon::prelude::*;

use crate::mask::Mask;
use crate::prepare::{PixelRect, Prepare};

/// The label of a pixel that is background.
pub const BACKGROUND: u8 = 0;
/// The label of a pixel that is masked or outside the crop. Inks are labelled 1, 2, ... in order of how much of the image
/// they cover.
pub const UNKNOWN: u8 = 255;

/// Past this many distinct colours (a photograph) the histogram is made coarser first.
const MANY_COLOURS: usize = 400_000;
/// Inks are looked for among cells of the colour cube this coarse (5 bits a channel): there are at most 32,768.
const CELL_MASK: u32 = 0x00F8_F8F8;
const CELL_CENTRE: u32 = 0x0004_0404;

#[derive(Clone, Debug)]
pub struct Params {
    /// Colours closer than this in Lab (the ΔE of CIE76) are the same ink.
    pub tolerance: f32,
    /// How far, in RGB, from the line between background and ink a colour may be and still be a blend of them. (The
    /// line is straight in RGB, where renderers mix; it is curved in Lab.)
    pub blend: f32,
    /// An ink must cover at least this share of the known pixels.
    pub min_share: f32,
    /// The most inks to find.
    pub max_inks: usize,
}

impl Default for Params {
    fn default() -> Self {
        Self { tolerance: 10.0, blend: 8.0, min_share: 0.0002, max_inks: 16 }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ink {
    pub rgb: [u8; 3],
    /// How many pixels are this ink (blends included, whatever their coverage).
    pub pixels: u64,
}

/// For every pixel of the crop: background, unknown, or which ink and how much of it.
pub struct ColourMap {
    pub crop: PixelRect,
    pub background: [u8; 3],
    /// The inks, the one covering most of the image first. Ink `i` is label `i + 1`.
    pub inks: Vec<Ink>,
    labels: Vec<u8>,
    coverage: Vec<u8>,
}

impl ColourMap {
    /// The label and the coverage (0 to 255) of the pixel at image position `(x, y)`; unknown outside the crop.
    pub fn at(&self, x: u32, y: u32) -> (u8, u8) {
        let c = self.crop;
        if x < c.x || y < c.y || x >= c.x + c.width || y >= c.y + c.height {
            return (UNKNOWN, 0);
        }
        let i = ((y - c.y) * c.width + (x - c.x)) as usize;
        (self.labels[i], self.coverage[i])
    }

    /// The ink of the pixel at `(x, y)`, if it is one.
    pub fn ink_at(&self, x: u32, y: u32) -> Option<usize> {
        match self.at(x, y).0 {
            BACKGROUND | UNKNOWN => None,
            label => Some(usize::from(label) - 1),
        }
    }

    /// The ink at the pixel nearest to `(x, y)` (image pixels, fractions allowed) within `radius`, if any pixel there is
    /// mostly ink. A pointer over the edge of a thin line is rarely exactly on it.
    pub fn nearest_ink(&self, x: f64, y: f64, radius: f64) -> Option<usize> {
        let r = radius.max(0.5).ceil() as i64;
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let mut best: Option<(f64, usize)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                let (px, py) = (cx + dx, cy + dy);
                if px < 0 || py < 0 {
                    continue;
                }
                let (label, coverage) = self.at(px as u32, py as u32);
                if label == BACKGROUND || label == UNKNOWN || coverage < 100 {
                    continue;
                }
                let d = ((px as f64 + 0.5 - x).powi(2) + (py as f64 + 0.5 - y).powi(2)).sqrt();
                if d <= radius.max(0.5) && best.is_none_or(|(b, _)| d < b) {
                    best = Some((d, usize::from(label) - 1));
                }
            }
        }
        best.map(|(_, i)| i)
    }

    /// The mask of ink `ink`: every pixel of the crop with the share of it that ink covers.
    pub fn mask_of(&self, ink: usize) -> Mask {
        let label = (ink + 1) as u8;
        let data = self.labels.iter().zip(&self.coverage).map(|(&l, &c)| if l == label { c } else { 0 }).collect();
        Mask::from_data(self.crop, data)
    }

    /// How many pixels of the crop are known (not masked).
    pub fn known_pixels(&self) -> u64 {
        self.labels.iter().filter(|&&l| l != UNKNOWN).count() as u64
    }
}

// --- colour ---------------------------------------------------------------------------------------------------------

fn pack(rgb: [u8; 3]) -> u32 {
    u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2])
}

fn unpack(v: u32) -> [u8; 3] {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// sRGB to CIE Lab (D65).
pub fn lab(rgb: [u8; 3]) -> [f32; 3] {
    let linear = |v: u8| {
        let c = f32::from(v) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (r, g, b) = (linear(rgb[0]), linear(rgb[1]), linear(rgb[2]));
    let x = (0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47;
    let y = 0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b;
    let z = (0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83;
    let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
    [116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z))]
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// Where the point `c` falls on the line from `a` to `b` (0 at `a`, 1 at `b`), and how far it is from that line.
fn along(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> (f32, f32) {
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len2 = v[0] * v[0] + v[1] * v[1] + v[2] * v[2];
    if len2 <= f32::EPSILON {
        return (0.0, distance(a, c));
    }
    let t = ((c[0] - a[0]) * v[0] + (c[1] - a[1]) * v[1] + (c[2] - a[2]) * v[2]) / len2;
    let on = [a[0] + t * v[0], a[1] + t * v[1], a[2] + t * v[2]];
    (t, distance(on, c))
}

struct Seed {
    rgb: [u8; 3],
    lab: [f32; 3],
}

/// A cell of the colour cube that holds pixels.
struct Cell {
    key: u32,
    /// The commonest exact colour in the cell.
    mode: u32,
    mode_lab: [f32; 3],
    pixels: u64,
    centre: [f32; 3],
}

/// What a colour is: a blend of the background and an ink (the ink, and how much of the pixel it covers), or else the
/// nearest of the seeds (an ink covering it fully, or the background).
fn classify(seeds: &[Seed], background: [u8; 3], blend_tolerance: f32, k: u32, c: [f32; 3]) -> (usize, f32) {
    let rgb = floats(unpack(k));
    let mut blend: Option<(f32, usize, f32)> = None;
    for (i, s) in seeds.iter().enumerate().skip(1) {
        let (t, d) = along(floats(background), floats(s.rgb), rgb);
        if (0.12..=1.25).contains(&t) && d <= blend_tolerance && blend.is_none_or(|(bd, _, _)| d < bd) {
            blend = Some((d, i, t.clamp(0.0, 1.0)));
        }
    }
    match blend {
        Some((_, i, t)) => (i, t),
        None => {
            let nearest =
                (0..seeds.len()).min_by(|&a, &b| distance(seeds[a].lab, c).total_cmp(&distance(seeds[b].lab, c)));
            match nearest {
                Some(i) if i > 0 => (i, 1.0),
                _ => (0, 0.0),
            }
        }
    }
}

fn floats(rgb: [u8; 3]) -> [f32; 3] {
    rgb.map(f32::from)
}

// --- the analysis ---------------------------------------------------------------------------------------------------

/// The colours of the crop of `image`, leaving out what the mask of `prepare` covers.
pub fn analyse(image: &RgbaImage, prepare: &Prepare, params: &Params) -> ColourMap {
    let (width, height) = image.dimensions();
    let (crop, unknown) = prepare.unknown_in_crop(width, height);
    let (cw, ch) = (crop.width as usize, crop.height as usize);
    let colour = |x: usize, y: usize| {
        let p = image.get_pixel(crop.x + x as u32, crop.y + y as u32).0;
        [p[0], p[1], p[2]]
    };

    // the histogram of exact colours of the known pixels
    let mut histogram: HashMap<u32, u64> = (0..ch)
        .into_par_iter()
        .fold(HashMap::new, |mut counts, y| {
            for x in 0..cw {
                if unknown[y * cw + x] == 0 {
                    *counts.entry(pack(colour(x, y))).or_default() += 1;
                }
            }
            counts
        })
        .reduce(HashMap::new, |mut a, mut b| {
            if a.len() < b.len() {
                std::mem::swap(&mut a, &mut b);
            }
            for (k, v) in b {
                *a.entry(k).or_default() += v;
            }
            a
        });
    // a noisy picture has too many colours to tell apart: look at them coarsely
    let coarse = histogram.len() > MANY_COLOURS;
    if coarse {
        let mut merged: HashMap<u32, u64> = HashMap::new();
        for (k, v) in histogram.drain() {
            *merged.entry(k & 0x00F8_F8F8 | 0x0004_0404).or_default() += v;
        }
        histogram = merged;
    }
    let known: u64 = histogram.values().sum();

    // the background: the commonest known colour along the border
    let mut border: HashMap<u32, u64> = HashMap::new();
    let mut note = |x: usize, y: usize| {
        if unknown[y * cw + x] == 0 {
            let k = pack(colour(x, y));
            *border.entry(if coarse { k & 0x00F8_F8F8 | 0x0004_0404 } else { k }).or_default() += 1;
        }
    };
    for x in 0..cw {
        note(x, 0);
        note(x, ch - 1);
    }
    for y in 0..ch {
        note(0, y);
        note(cw - 1, y);
    }
    let mode =
        |counts: &HashMap<u32, u64>| counts.iter().max_by_key(|&(k, v)| (*v, std::cmp::Reverse(*k))).map(|(k, _)| *k);
    let background = unpack(mode(&border).or_else(|| mode(&histogram)).unwrap_or(0x00FF_FFFF));
    let background_lab = lab(background);

    // the distinct colours, commonest first (ties by value, so the result does not depend on hashing)
    let mut colours: Vec<(u32, u64, [f32; 3])> = histogram.iter().map(|(&k, &n)| (k, n, lab(unpack(k)))).collect();
    colours.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    // the cells of the colour cube: how many pixels fall in each, its commonest exact colour (the first met, as the
    // colours are in order), and how many pixels are near it
    let mut by_cell: HashMap<u32, Cell> = HashMap::new();
    for &(k, n, l) in &colours {
        by_cell.entry(k & CELL_MASK).and_modify(|c| c.pixels += n).or_insert_with(|| Cell {
            key: k & CELL_MASK,
            mode: k,
            mode_lab: l,
            pixels: n,
            centre: lab(unpack(k & CELL_MASK | CELL_CENTRE)),
        });
    }
    let mut cells: Vec<Cell> = by_cell.into_values().collect();
    cells.sort_by_key(|c| c.key);
    let density: Vec<u64> = cells
        .par_iter()
        .map(|a| cells.iter().filter(|b| distance(a.centre, b.centre) <= params.tolerance).map(|b| b.pixels).sum())
        .collect();
    let mut by_density: Vec<usize> = (0..cells.len()).collect();
    by_density.sort_by(|&a, &b| density[b].cmp(&density[a]).then(cells[a].key.cmp(&cells[b].key)));

    // the inks, densest first
    let min_count = ((known as f64 * f64::from(params.min_share)) as u64).max(4);
    let mut seeds = vec![Seed { rgb: background, lab: background_lab }];
    'cells: for &i in &by_density {
        let cell = &cells[i];
        if density[i] < min_count || seeds.len() > params.max_inks {
            break;
        }
        if seeds.iter().any(|s| distance(s.lab, cell.mode_lab) <= params.tolerance) {
            continue; // the same colour as one found
        }
        // The densest cell may be beside the colour that was drawn (the blends around an ink all count towards a cell
        // next to it): climb to the commonest colour near it, which is the ink itself.
        let (mut mode, mut mode_lab) = (cell.mode, cell.mode_lab);
        for _ in 0..8 {
            let Some(&(better, _, better_lab)) = colours.iter().find(|c| distance(c.2, mode_lab) <= params.tolerance)
            else {
                break;
            };
            if better == mode {
                break;
            }
            (mode, mode_lab) = (better, better_lab);
        }
        if seeds.iter().any(|s| distance(s.lab, mode_lab) <= params.tolerance) {
            continue;
        }
        let rgb = floats(unpack(mode));
        for a in 0..seeds.len() {
            for b in a + 1..seeds.len() {
                let (t, d) = along(floats(seeds[a].rgb), floats(seeds[b].rgb), rgb);
                if (0.05..0.95).contains(&t) && d <= params.blend {
                    continue 'cells; // an antialiased blend of two that were found
                }
            }
        }
        seeds.push(Seed { rgb: unpack(mode), lab: mode_lab });
    }

    // what each distinct colour is, and how much of the image each ink covers; an ink nothing ended up in is no ink
    let (assigned, pixels) = loop {
        let assigned: HashMap<u32, (usize, f32)> =
            colours.iter().map(|&(k, _, c)| (k, classify(&seeds, background, params.blend, k, c))).collect();
        let mut pixels = vec![0u64; seeds.len()];
        for &(k, n, _) in &colours {
            pixels[assigned[&k].0] += n;
        }
        match (1..seeds.len()).rev().find(|&i| pixels[i] < min_count) {
            Some(empty) => {
                seeds.remove(empty);
            }
            None => break (assigned, pixels),
        }
    };

    // number the inks by how much of the image they cover
    let mut order: Vec<usize> = (1..seeds.len()).collect();
    order.sort_by(|&a, &b| pixels[b].cmp(&pixels[a]).then(a.cmp(&b)));
    let mut label_of = vec![BACKGROUND; seeds.len()];
    for (rank, &seed) in order.iter().enumerate() {
        label_of[seed] = (rank + 1) as u8;
    }
    let inks = order.iter().map(|&s| Ink { rgb: seeds[s].rgb, pixels: pixels[s] }).collect();

    // label every pixel
    let lut: HashMap<u32, (u8, u8)> =
        assigned.iter().map(|(&k, &(seed, t))| (k, (label_of[seed], (t * 255.0).round() as u8))).collect();
    let mut labels = vec![UNKNOWN; cw * ch];
    let mut coverage = vec![0u8; cw * ch];
    labels.par_chunks_mut(cw).zip(coverage.par_chunks_mut(cw)).enumerate().for_each(|(y, (label_row, cover_row))| {
        let mut last: Option<(u32, (u8, u8))> = None;
        for x in 0..cw {
            if unknown[y * cw + x] != 0 {
                continue;
            }
            let mut k = pack(colour(x, y));
            if coarse {
                k = k & 0x00F8_F8F8 | 0x0004_0404;
            }
            let (label, cover) = match last {
                Some((lk, v)) if lk == k => v,
                _ => {
                    let v = lut.get(&k).copied().unwrap_or((BACKGROUND, 0));
                    last = Some((k, v));
                    v
                }
            };
            label_row[x] = label;
            cover_row[x] = cover;
        }
    });

    ColourMap { crop, background, inks, labels, coverage }
}

#[cfg(test)]
mod tests {
    use image::Rgba;
    use kurbo::Rect;

    use super::*;
    use crate::prepare::{MaskOp, Shape};

    const WHITE: [u8; 3] = [255, 255, 255];
    const RED: [u8; 3] = [214, 39, 40];
    const BLUE: [u8; 3] = [31, 119, 180];
    const BLACK: [u8; 3] = [0, 0, 0];

    fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
        [0, 1, 2].map(|i| (f32::from(a[i]) * (1.0 - t) + f32::from(b[i]) * t).round() as u8)
    }

    /// 200 x 100 on white: a black frame, a red line 3 pixels thick with antialiased edges (a quarter and then three
    /// quarters ink on each side), and a solid blue block.
    fn plot() -> RgbaImage {
        let mut img = RgbaImage::from_pixel(200, 100, Rgba([255, 255, 255, 255]));
        let mut put = |x: u32, y: u32, c: [u8; 3]| img.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
        for x in 10..190 {
            put(x, 10, BLACK);
            put(x, 90, BLACK);
        }
        for y in 10..91 {
            put(10, y, BLACK);
            put(189, y, BLACK);
        }
        for x in 20..180 {
            put(x, 40, mix(WHITE, RED, 0.25));
            put(x, 41, RED);
            put(x, 42, RED);
            put(x, 43, mix(WHITE, RED, 0.75));
        }
        for y in 60..80 {
            for x in 140..170 {
                put(x, y, BLUE);
            }
        }
        img
    }

    fn analyse_all(image: &RgbaImage) -> ColourMap {
        analyse(image, &Prepare::default(), &Params::default())
    }

    #[test]
    fn the_inks_are_found_and_antialiasing_is_not_one_of_them() {
        let map = analyse_all(&plot());
        assert_eq!(map.background, WHITE);
        let found: Vec<[u8; 3]> = map.inks.iter().map(|i| i.rgb).collect();
        assert_eq!(found.len(), 3, "red, blue and black, not their blends: {found:?}");
        for want in [RED, BLUE, BLACK] {
            assert!(found.contains(&want), "{want:?} in {found:?}");
        }
        // the commonest first: the red line (4 rows of 160, blends included), the block (600) and the frame (522)
        assert!(map.inks.windows(2).all(|w| w[0].pixels >= w[1].pixels));
        let order: Vec<[u8; 3]> = map.inks.iter().map(|i| i.rgb).collect();
        assert_eq!(order, [RED, BLUE, BLACK]);
        assert_eq!(map.inks[0].pixels, 640);
    }

    #[test]
    fn the_mask_of_an_ink_is_where_it_is_with_how_much_of_each_pixel_it_covers() {
        let map = analyse_all(&plot());
        let red = map.inks.iter().position(|i| i.rgb == RED).unwrap();
        let mask = map.mask_of(red);
        assert_eq!(mask.crop(), map.crop);
        assert_eq!((mask.at(100, 41), mask.at(100, 42)), (255, 255), "the solid rows");
        assert!((i32::from(mask.at(100, 40)) - 64).abs() <= 12, "a quarter covered");
        assert_eq!(mask.at(100, 20), 0, "paper");
        assert_eq!(mask.at(150, 70), 0, "another ink's");
        assert_eq!(mask.pixels(), 160 * 3, "three rows of the line are mostly ink");
    }

    #[test]
    fn a_pixel_between_paper_and_ink_is_that_ink_with_the_share_it_covers() {
        let map = analyse_all(&plot());
        let red = map.inks.iter().position(|i| i.rgb == RED).unwrap();
        let (label, cover) = map.at(100, 40);
        assert_eq!(usize::from(label) - 1, red);
        assert!((i32::from(cover) - 64).abs() <= 12, "a quarter ink: {cover}");
        let (_, cover) = map.at(100, 43);
        assert!((i32::from(cover) - 191).abs() <= 12, "three quarters ink: {cover}");
        let (label, cover) = map.at(100, 41);
        assert_eq!((usize::from(label) - 1, cover), (red, 255), "solid ink covers fully");
        assert_eq!(map.at(100, 20), (BACKGROUND, 0));
        assert_eq!(map.ink_at(100, 20), None);
    }

    #[test]
    fn noise_does_not_make_new_colours() {
        let mut img = plot();
        let mut seed = 12345u32;
        let mut noise = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((seed >> 24) % 7) as i32 - 3
        };
        for px in img.pixels_mut() {
            for c in &mut px.0[..3] {
                *c = (i32::from(*c) + noise()).clamp(0, 255) as u8;
            }
        }
        let map = analyse_all(&img);
        assert_eq!(map.inks.len(), 3, "{:?}", map.inks);
        let blue = map.inks.iter().position(|i| distance(lab(i.rgb), lab(BLUE)) < 5.0).unwrap();
        assert_eq!(map.ink_at(150, 70), Some(blue));
        assert_eq!(map.ink_at(50, 20), None, "noisy paper is still paper");
    }

    #[test]
    fn a_dark_background_works_the_same() {
        let mut img = RgbaImage::from_pixel(100, 60, Rgba([20, 20, 28, 255]));
        for x in 10..90 {
            img.put_pixel(x, 30, Rgba([0, 255, 255, 255]));
            img.put_pixel(x, 29, Rgba([10, 138, 142, 255])); // about half cyan over the background
        }
        let map = analyse_all(&img);
        assert_eq!(map.background, [20, 20, 28]);
        assert_eq!(map.inks.len(), 1);
        assert_eq!(map.inks[0].rgb, [0, 255, 255]);
        let (label, cover) = map.at(50, 29);
        assert_eq!(label, 1);
        assert!((90..=170).contains(&cover), "{cover}");
    }

    #[test]
    fn what_is_masked_is_unknown_and_adds_no_colour() {
        let mask = MaskOp { shape: Shape::Rect(Rect::new(135.0, 55.0, 175.0, 85.0)), erase: false }; // over the block
        let prepare = Prepare { crop: None, mask: vec![mask] };
        let map = analyse(&plot(), &prepare, &Params::default());
        assert!(map.inks.iter().all(|i| i.rgb != BLUE), "the masked block is not a colour: {:?}", map.inks);
        assert_eq!(map.at(150, 70), (UNKNOWN, 0));
        assert_eq!(map.known_pixels(), 200 * 100 - 40 * 30);
    }

    #[test]
    fn only_the_crop_is_read_and_positions_stay_those_of_the_image() {
        let prepare = Prepare { crop: Some(Rect::new(0.0, 0.0, 130.0, 100.0)), mask: vec![] }; // leaves out the block
        let map = analyse(&plot(), &prepare, &Params::default());
        assert!(map.inks.iter().all(|i| i.rgb != BLUE));
        assert_eq!((map.crop.x, map.crop.width), (0, 130));
        assert_eq!(map.at(150, 70), (UNKNOWN, 0), "outside the crop");
        let cropped = Prepare { crop: Some(Rect::new(30.0, 30.0, 200.0, 100.0)), mask: vec![] };
        let map = analyse(&plot(), &cropped, &Params::default());
        let red = map.inks.iter().position(|i| i.rgb == RED).unwrap();
        assert_eq!(map.ink_at(100, 41), Some(red), "image coordinates, not crop coordinates");
        assert_eq!(map.at(5, 5).0, UNKNOWN);
    }

    #[test]
    fn the_nearest_ink_is_found_for_a_pointer_beside_a_thin_line() {
        let map = analyse_all(&plot());
        let red = map.inks.iter().position(|i| i.rgb == RED).unwrap();
        assert_eq!(map.nearest_ink(100.5, 39.0, 3.0), Some(red), "above the line, within reach of its solid rows");
        assert_eq!(map.nearest_ink(100.5, 30.0, 3.0), None, "nothing within reach");
        assert_eq!(map.nearest_ink(100.5, 41.5, 0.5), Some(red));
    }

    #[test]
    fn a_tighter_tolerance_tells_similar_colours_apart() {
        let mut img = RgbaImage::from_pixel(100, 40, Rgba([255, 255, 255, 255]));
        for x in 10..50 {
            img.put_pixel(x, 10, Rgba([200, 30, 30, 255]));
            img.put_pixel(x, 20, Rgba([215, 45, 40, 255]));
        }
        let loose = analyse(&img, &Prepare::default(), &Params { tolerance: 20.0, ..Params::default() });
        let tight = analyse(&img, &Prepare::default(), &Params { tolerance: 3.0, ..Params::default() });
        assert_eq!(loose.inks.len(), 1);
        assert_eq!(tight.inks.len(), 2);
    }

    #[test]
    fn the_number_of_inks_is_limited_and_the_result_does_not_vary() {
        let mut img = RgbaImage::from_pixel(300, 40, Rgba([255, 255, 255, 255]));
        for k in 0..30u32 {
            let hue = [(k * 8) as u8, (255 - k * 8) as u8, ((k * 37) % 256) as u8];
            for x in 0..8 {
                for y in 10..30 {
                    img.put_pixel(k * 9 + x, y, Rgba([hue[0], hue[1], hue[2], 255]));
                }
            }
        }
        let params = Params { max_inks: 6, ..Params::default() };
        let a = analyse(&img, &Prepare::default(), &params);
        let b = analyse(&img, &Prepare::default(), &params);
        assert!(a.inks.len() <= 6, "{}", a.inks.len());
        assert_eq!(a.inks, b.inks);
        assert!(a.labels == b.labels && a.coverage == b.coverage);
    }

    #[test]
    fn a_plain_image_has_no_ink_and_a_big_noisy_one_is_still_analysed() {
        let blank = RgbaImage::from_pixel(50, 50, Rgba([255, 255, 255, 255]));
        let map = analyse_all(&blank);
        assert!(map.inks.is_empty());
        assert_eq!(map.at(10, 10), (BACKGROUND, 0));

        // more than a hundred thousand distinct colours: looked at coarsely
        let mut noisy = RgbaImage::new(500, 400);
        let mut seed = 99u32;
        for (x, y, px) in noisy.enumerate_pixels_mut() {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = ((seed >> 24) % 5) as u8;
            *px = if (150..250).contains(&y) && (100..400).contains(&x) {
                Rgba([30 + n, 100 + n, 200 + n, 255])
            } else {
                Rgba([250 + n, 250 + n, 250 + n, 255])
            };
        }
        let map = analyse_all(&noisy);
        assert_eq!(map.inks.len(), 1, "{:?}", map.inks);
        assert_eq!(map.ink_at(200, 200), Some(0));
        assert_eq!(map.ink_at(10, 10), None);
    }
}
