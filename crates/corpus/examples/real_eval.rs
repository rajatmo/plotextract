//! Trace real figures through the wizard's raster steps and, where the figure is a vector file, compare what was traced
//! with the paths the file draws.
//!
//!     cargo run --release -p plotextract-corpus --example real_eval -- file.pdf file.png ...
//!
//! A vector figure (PDF, SVG, EPS) is drawn so its longer side is 1600 pixels and traced as an image; its paths, at the
//! same scale, are the truth. An image is traced and counted only.
//!
//! With no hand work: every ink is called a curve, and the darkest the axes too, and the pieces are found. Precision is
//! the share of the points traced (along each long piece, at the centre of each spot) within 2 pixels of a path; recall
//! the share of the paths (sampled every pixel) within 2 pixels of a point traced.
//!
//! By hand: if a project saved from the app is found for a vector figure (`<name>.plotextract.json` next to it, or in
//! `png/` beside it), its tracing is brought back on the image it names, as the app does, and what the links export is
//! scored. Precision is the share of the points of the links within 2 pixels of a path; coverage, of the paths the links
//! follow (those a quarter or more of whose samples are within 2 pixels of a point), the share within 2 pixels of one.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use image::RgbaImage;
use plotextract_algo::analyse_colours;
use plotextract_domain::{Figure, OpenOptions};
use plotextract_usecases::Workspace;
use plotextract_usecases::wizard::{Role, Step, Wizard, WizardIntent, Work};

/// Run the jobs the wizard asks for, here and now, until it asks for no more.
fn settle(w: &mut Wizard, image: &RgbaImage) {
    let mut id = 100;
    loop {
        id += 1;
        if let Some(params) = w.take_analysis_request() {
            w.begin_analysis(id);
            w.finish_analysis(id, Ok(Arc::new(analyse_colours(image, w.prepare(), &params))));
        } else if let Some(search) = w.take_lines_request() {
            w.begin_lines(id);
            w.finish_lines(id, search.run());
        } else if let Some(search) = w.take_pieces_request() {
            w.begin_pieces(id);
            w.finish_pieces(id, search.run());
        } else {
            break;
        }
    }
}

/// Points along each path, every pixel, in pixels at `(sx, sy)` to a unit.
fn path_samples(figure: &Figure, (sx, sy): (f64, f64)) -> Vec<Vec<(f64, f64)>> {
    let mut out = Vec::new();
    for p in &figure.primitives {
        let mut samples = Vec::new();
        for (k, w) in p.points.windows(2).enumerate() {
            if p.breaks.contains(&(k + 1)) {
                continue;
            }
            let (a, b) = ((w[0].x * sx, w[0].y * sy), (w[1].x * sx, w[1].y * sy));
            let n = ((b.0 - a.0).hypot(b.1 - a.1)).ceil().max(1.0) as usize;
            for i in 0..=n {
                let t = i as f64 / n as f64;
                samples.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        out.push(samples);
    }
    out
}

/// A grid of the points, by cells of `cell` pixels, to find what is near quickly.
struct Grid {
    cell: f64,
    cells: std::collections::HashMap<(i64, i64), Vec<(f64, f64)>>,
}

impl Grid {
    fn new(points: &[(f64, f64)], cell: f64) -> Self {
        let mut cells: std::collections::HashMap<(i64, i64), Vec<(f64, f64)>> = Default::default();
        for &p in points {
            cells.entry(((p.0 / cell) as i64, (p.1 / cell) as i64)).or_default().push(p);
        }
        Self { cell, cells }
    }

    fn near(&self, p: (f64, f64), r: f64) -> bool {
        let (cx, cy) = ((p.0 / self.cell) as i64, (p.1 / self.cell) as i64);
        (-1..=1).any(|dy| {
            (-1..=1).any(|dx| {
                self.cells.get(&(cx + dx, cy + dy)).is_some_and(|v| v.iter().any(|q| (q.0 - p.0).hypot(q.1 - p.1) <= r))
            })
        })
    }
}

/// The share of `points` within 2 pixels of one of `of`.
fn share_near(points: &[(f64, f64)], of: &Grid) -> f64 {
    points.iter().filter(|&&q| of.near(q, 2.0)).count() as f64 / points.len().max(1) as f64
}

/// The figure in a file, and its picture drawn at `scale` (or so its longer side is `longest` pixels).
fn draw(library: &Workspace, path: &Path, longest: Option<f64>) -> Result<(Figure, bool, RgbaImage), String> {
    let opened = library.open(path, OpenOptions::default()).map_err(|e| format!("cannot open: {e}"))?;
    let figure = opened.session.figure.clone();
    let vector = !opened.needs_tracing();
    let scale = longest.map_or(1.0, |l| l / figure.width.max(figure.height));
    let mut picture = opened.picture_source().and_then(|s| s.picture()).ok_or("draws no picture")?;
    let bitmap = picture
        .render(kurbo::Rect::new(0.0, 0.0, figure.width, figure.height), scale)
        .map_err(|e| format!("cannot draw: {e}"))?;
    let image = RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba).ok_or("a picture of the wrong size")?;
    Ok((figure, vector, image))
}

/// The project saved for figure `path`, if there is one, with the image it traced.
fn project_for(path: &Path) -> Option<(PathBuf, Work, PathBuf)> {
    let dir = path.parent()?;
    let file = format!("{}.plotextract.json", path.file_stem()?.to_string_lossy());
    let project = [dir.join(&file), dir.join("png").join(&file)].into_iter().find(|p| p.exists())?;
    let text = std::fs::read_to_string(&project).ok()?;
    let saved = plotextract_domain::Project::from_json(&text).ok()?;
    let work = Work::from_value(saved.tracing?).ok()?;
    // the image where the project says, else where the project is
    let source = saved.source.map(PathBuf::from).unwrap_or_default();
    let image = [source.clone(), project.parent()?.join(source.file_name()?)].into_iter().find(|p| p.exists())?;
    Some((project, work, image))
}

fn main() {
    let library = plotextract_corpus::library();
    println!(
        "{:<44} {:>5} {:>9} {:>4} {:>6} {:>5} {:>5} {:>5} {:>6} {:>6} {:>6}",
        "figure", "kind", "pixels", "ink", "pieces", "grps", "left", "one", "prec%", "rec%", "ms"
    );
    let mut by_hand = Vec::new();
    for path in std::env::args().skip(1).map(PathBuf::from) {
        let name: String = path.file_name().unwrap().to_string_lossy().chars().take(44).collect();
        // an image as it is; a vector figure drawn so its longer side is 1600 pixels
        let vector = matches!(path.extension().and_then(|e| e.to_str()), Some("pdf" | "svg" | "eps" | "ps"));
        let (figure, vector, image) = match draw(&library, &path, vector.then_some(1600.0)) {
            Ok(drawn) => drawn,
            Err(e) => {
                println!("{name:<44} {e}");
                continue;
            }
        };
        let t = Instant::now();
        let mut w = Wizard::new(image.width(), image.height());
        w.apply(WizardIntent::GoTo(Step::Colours));
        settle(&mut w, &image);
        let rgbs: Vec<[u8; 3]> = w.colours().map().map_or(Vec::new(), |m| m.inks.iter().map(|i| i.rgb).collect());
        let inks = rgbs.len();
        for i in 0..inks {
            w.apply(WizardIntent::SetRole(i, Role::Curve, true));
        }
        // the darkest ink is also the axes, as one marks it by hand, so the axis lines found are cut out of it
        if let Some(dark) = (0..inks).min_by_key(|&i| rgbs[i].iter().map(|&c| u32::from(c)).sum::<u32>()) {
            w.apply(WizardIntent::SetRole(dark, Role::Axes, true));
        }
        w.apply(WizardIntent::GoTo(Step::PlotArea));
        settle(&mut w, &image);
        w.apply(WizardIntent::GoTo(Step::Shapes));
        settle(&mut w, &image);
        let ms = t.elapsed().as_secs_f64() * 1e3;
        let (Some(pieces), Some(shapes)) = (w.pieces(), w.shapes()) else {
            println!("{name:<44} no pieces");
            continue;
        };
        let n = pieces.pieces().len();
        let groups = shapes.groups().count();
        let one = shapes.groups().filter(|(_, g)| g.members.len() == 1).count();
        let left = shapes.leftovers().len();
        let (prec, rec) = if vector {
            let scale = f64::from(image.width()) / figure.width;
            let traced: Vec<(f64, f64)> = (0..n).flat_map(|p| w.piece_points(p)).map(|q| (q.x, q.y)).collect();
            let truth: Vec<(f64, f64)> = path_samples(&figure, (scale, scale)).concat();
            let (prec, rec) =
                (share_near(&traced, &Grid::new(&truth, 4.0)), share_near(&truth, &Grid::new(&traced, 4.0)));
            (format!("{:.0}", prec * 100.0), format!("{:.0}", rec * 100.0))
        } else {
            ("-".into(), "-".into())
        };
        println!(
            "{name:<44} {:>5} {:>9} {inks:>4} {n:>6} {groups:>5} {left:>5} {one:>5} {prec:>6} {rec:>6} {ms:>6.0}",
            if vector { "vec" } else { "img" },
            format!("{}x{}", image.width(), image.height())
        );
        if vector && let Some(found) = project_for(&path) {
            by_hand.push((name, figure, found));
        }
    }
    if by_hand.is_empty() {
        return;
    }
    println!("\nby hand, from the projects saved:");
    println!("{:<44} {:>5} {:>6} {:>6} {:>6}  project", "figure", "links", "points", "prec%", "cov%");
    for (name, figure, (project, work, source)) in by_hand {
        let image = match draw(&library, &source, None) {
            Ok((_, _, image)) => image,
            Err(e) => {
                println!("{name:<44} {}: {e}", source.display());
                continue;
            }
        };
        let mut w = Wizard::new(image.width(), image.height());
        w.restore(work);
        settle(&mut w, &image);
        let points: Vec<(f64, f64)> = w.link_points().into_iter().map(|q| (q.x, q.y)).collect();
        // the image traced may be drawn at any size: the paths are scaled to it
        let scale = (f64::from(image.width()) / figure.width, f64::from(image.height()) / figure.height);
        let paths = path_samples(&figure, scale);
        let prec = share_near(&points, &Grid::new(&paths.concat(), 4.0));
        let near = Grid::new(&points, 4.0);
        let followed: Vec<&Vec<(f64, f64)>> =
            paths.iter().filter(|p| !p.is_empty() && share_near(p, &near) >= 0.25).collect();
        let (hit, all) =
            followed.iter().fold((0.0, 0usize), |(h, a), p| (h + share_near(p, &near) * p.len() as f64, a + p.len()));
        println!(
            "{name:<44} {:>5} {:>6} {:>6.0} {:>6.0}  {}",
            w.links().len(),
            points.len(),
            prec * 100.0,
            100.0 * hit / all.max(1) as f64,
            project.display()
        );
    }
}
