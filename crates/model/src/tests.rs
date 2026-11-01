//! The model's tests: every user-level flow, driven by intents and effects, with the real formats and disk behind the
//! ports and arXiv faked.
use plotextract_arxiv::{ArxivPapers, Downloader};
use plotextract_domain::Axis;
use plotextract_usecases::Registry;

use super::arxiv_import::THUMBNAIL_SIDE;
use super::*;

/// A library of every format, on the local disk.
fn library() -> Workspace {
    let registry = Registry::new(vec![
        Arc::new(plotextract_pdf::PdfImporter),
        Arc::new(plotextract_svg::SvgImporter),
        Arc::new(plotextract_ps::PsImporter),
        Arc::new(plotextract_image::RasterImporter),
    ]);
    Workspace::new(registry, Arc::new(plotextract_fs::LocalFiles))
}

/// A model with papers from `downloader`, unpacked into `cache`.
fn with_papers(downloader: Arc<dyn Downloader>, cache: PathBuf) -> Model {
    Model::new(library(), Arc::new(ArxivPapers::new(downloader, cache)))
}

/// A model whose papers never come.
fn model() -> Model {
    with_papers(Arc::new(FakeArxiv(None)), std::env::temp_dir().join("plotextract-no-papers"))
}

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
    <polyline points="10,90 50,50 90,10" fill="none" stroke="#000" stroke-width="1"/></svg>"##;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("plotextract-app-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn svg(&self) -> PathBuf {
        let path = self.0.join("fig.svg");
        std::fs::write(&path, SVG).unwrap();
        path
    }

    /// A PDF with `count` pages of 100 x 100 points; page k draws a line of length 10 * (k + 1).
    fn pdf(&self, count: usize) -> PathBuf {
        let kids = (0..count).map(|k| format!("{} 0 R", 3 + 2 * k)).collect::<Vec<_>>().join(" ");
        let mut objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            format!("<< /Type /Pages /Kids [{kids}] /Count {count} >>"),
        ];
        for k in 0..count {
            objects
                .push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents {} 0 R >>", 4 + 2 * k));
            let content = format!("10 10 m {} 10 l S", 10 * (k + 1) + 10);
            objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        let mut out = String::from("%PDF-1.7\n");
        let mut offsets = Vec::new();
        for (n, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out += &format!("{} 0 obj\n{body}\nendobj\n", n + 1);
        }
        let xref = out.len();
        out += &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
        for o in offsets {
            out += &format!("{o:010} 00000 n \n");
        }
        out += &format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1);
        let path = self.0.join("book.pdf");
        std::fs::write(&path, out).unwrap();
        path
    }

    /// A PDF of 2 pages of 100 x 100 points: page 1 draws a line, page 2 a 32 x 16 picture, red on the left half and blue
    /// on the right.
    fn pdf_with_image(&self) -> PathBuf {
        let mut pixels = Vec::new();
        for _ in 0..16 {
            for x in 0..32 {
                pixels.extend(if x < 16 { [255, 0, 0] } else { [0, 0, 255] });
            }
        }
        let stream = |dict: &str, data: &[u8]| {
            let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
            out.extend_from_slice(data);
            out.extend_from_slice(b"\nendstream");
            out
        };
        let objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 5 0 R >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject << /P 7 0 R >> >> /Contents 6 0 R >>"
                .to_vec(),
            stream("", b"10 10 m 90 90 l S"),
            stream("", b"q 64 0 0 32 10 10 cm /P Do Q"),
            stream(
                "/Type /XObject /Subtype /Image /Width 32 /Height 16 /ColorSpace /DeviceRGB /BitsPerComponent 8",
                &pixels,
            ),
        ];
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (n, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", n + 1).into_bytes());
            out.extend(body);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).into_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").into_bytes());
        }
        out.extend(
            format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).into_bytes(),
        );
        let path = self.0.join("paper.pdf");
        std::fs::write(&path, out).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

/// Dispatch an intent and carry out the jobs it starts.
fn run(m: &mut Model, intent: Intent) -> Vec<Effect> {
    let fx = m.dispatch(intent);
    m.run_jobs(fx)
}

fn opened(dir: &Scratch) -> Model {
    let mut m = model();
    run(&mut m, Intent::Open(dir.svg()));
    m
}

fn pick(vertex: usize) -> Intent {
    Intent::Select(Some(Pick { primitive: 0, vertex: Some(vertex) }))
}

fn message(m: &Model) -> &str {
    &m.message().expect("a message").text
}

#[test]
fn opening_runs_as_a_job_and_the_answer_installs_the_figure() {
    let dir = Scratch::new("open");
    let mut m = model();
    let fx = m.dispatch(Intent::Open(dir.svg()));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Load(_))]), "dispatch only asks for the work");
    assert!(m.session().is_none());
    assert_eq!(m.loading(), Some("fig.svg"));

    let fx = m.run_jobs(fx);
    assert!(m.session().is_some() && m.loading().is_none());
    assert!(matches!(&fx[0], Effect::Opened { title, source: Some(_) } if title == "fig.svg \u{2013} plotextract"));
    assert!(fx.iter().any(|e| matches!(e, Effect::ValuesChanged)));
    assert_eq!(message(&m), "Opened fig.svg: 1 curves. Ctrl+click a curve to select it.");
    assert_eq!(m.message().unwrap().level, Level::Info);
}

#[test]
fn only_the_newest_open_counts() {
    let dir = Scratch::new("stale");
    let mut m = model();
    let first = m.dispatch(Intent::Open(dir.svg()));
    let second = m.dispatch(Intent::Open(dir.pdf(1)));
    let fx = m.run_jobs(first);
    assert!(fx.is_empty() && m.session().is_none(), "the older answer is ignored");
    m.run_jobs(second);
    assert!(m.pdf().is_some_and(|p| p.document().unwrap().name == "book.pdf"), "the newer is listed");
}

#[test]
fn opening_what_cannot_be_opened_keeps_the_current_figure() {
    let dir = Scratch::new("badopen");
    let mut m = opened(&dir);
    let notes = dir.0.join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();
    let fx = run(&mut m, Intent::Open(notes));
    assert!(fx.is_empty() && m.session().is_some() && m.loading().is_none());
    assert_eq!(m.message().unwrap().level, Level::Error);
}

#[test]
fn recording_moves_through_the_rows_and_needs_a_selected_point() {
    let dir = Scratch::new("record");
    let mut m = opened(&dir);
    m.dispatch(Intent::Record);
    assert_eq!(m.message().unwrap().level, Level::Error, "no point is selected yet");
    assert_eq!(m.slot(), Slot::XMin);

    m.dispatch(pick(0));
    m.dispatch(Intent::Record);
    assert_eq!(m.slot(), Slot::XMax);
    assert_eq!(message(&m), "x min recorded. Type its plot value in the table.");
    assert!(m.session().unwrap().calibration().position(Slot::XMin).is_some());

    m.dispatch(pick(2));
    m.dispatch(Intent::RecordAt(Slot::YMax));
    assert_eq!(m.slot(), Slot::YMax);
    assert!(m.session().unwrap().calibration().position(Slot::YMax).is_some());
}

#[test]
fn undo_and_clearing_ask_for_the_value_cells_to_be_refilled() {
    let dir = Scratch::new("undo");
    let mut m = opened(&dir);
    m.dispatch(Intent::Edit(Edit::SetValue(Slot::XMin, Some(2.0))));
    let fx = m.dispatch(Intent::Edit(Edit::SetLog(Axis::X, true)));
    assert!(fx.is_empty(), "typing and toggling need no refill");
    assert!(matches!(m.dispatch(Intent::Undo).as_slice(), [Effect::ValuesChanged]));
    assert!(matches!(m.dispatch(Intent::Redo).as_slice(), [Effect::ValuesChanged]));
    assert!(m.dispatch(Intent::Redo).is_empty(), "nothing left to redo");
    assert!(matches!(m.dispatch(Intent::Edit(Edit::ClearCalibration)).as_slice(), [Effect::ValuesChanged]));
}

#[test]
fn exporting_explains_what_is_missing_before_asking_for_a_path() {
    let dir = Scratch::new("export");
    let mut m = opened(&dir);
    assert!(m.dispatch(Intent::RequestExportCsv).is_empty());
    assert!(message(&m).starts_with("Make a series first"));

    m.dispatch(Intent::Edit(Edit::ToggleMember(0)));
    assert!(m.dispatch(Intent::CopyNumbers).is_empty());
    assert!(message(&m).starts_with("Calibrate both axes"));

    for (slot, vertex, value) in
        [(Slot::XMin, 0, 0.0), (Slot::XMax, 2, 8.0), (Slot::YMin, 0, 0.0), (Slot::YMax, 2, 8.0)]
    {
        m.dispatch(pick(vertex));
        m.dispatch(Intent::RecordAt(slot));
        m.dispatch(Intent::Edit(Edit::SetValue(slot, Some(value))));
    }
    let fx = m.dispatch(Intent::RequestExportCsv);
    assert!(matches!(fx.as_slice(), [Effect::AskCsvPath { suggested }] if suggested == "Series 1.csv"));
    let fx = m.dispatch(Intent::CopyNumbers);
    assert!(matches!(fx.as_slice(), [Effect::CopyText(t)] if t == "x,y\n0,0\n4,4\n8,8\n"));

    let out = dir.0.join("out.csv");
    m.dispatch(Intent::WriteCsv(out.clone()));
    assert_eq!(std::fs::read_to_string(out).unwrap(), "x,y\n0,0\n4,4\n8,8\n");
}

#[test]
fn a_saved_project_comes_back_with_its_figure() {
    let dir = Scratch::new("project");
    let mut m = opened(&dir);
    m.dispatch(Intent::Edit(Edit::ToggleMember(0)));
    m.dispatch(Intent::Edit(Edit::SetValue(Slot::YMax, Some(7.0))));
    let fx = m.dispatch(Intent::RequestSaveProject);
    assert!(matches!(fx.as_slice(), [Effect::AskProjectSavePath { suggested }] if suggested == "fig.plotextract.json"));
    let project = dir.0.join("fig.plotextract.json");
    m.dispatch(Intent::SaveProject(project.clone()));

    let mut fresh = model();
    let fx = run(&mut fresh, Intent::LoadProject(project));
    assert!(matches!(&fx[0], Effect::Opened { .. }), "the figure the project names is opened");
    assert!(matches!(fx.last(), Some(Effect::ValuesChanged)));
    let s = fresh.session().unwrap();
    assert_eq!(s.series()[0].members, vec![0]);
    assert_eq!(s.calibration().value(Slot::YMax), Some(7.0));
}

#[test]
fn selecting_a_hidden_curve_is_refused_with_a_message() {
    let dir = Scratch::new("hidden");
    let mut m = opened(&dir);
    m.dispatch(Intent::Edit(Edit::SetHidden(0, true)));
    m.dispatch(pick(0));
    assert_eq!(m.session().unwrap().selection(), None);
    assert_eq!(message(&m), "That curve is hidden.");
    m.dispatch(Intent::SetLayer(Layer::Fills, false));
    assert!(!m.session().unwrap().layers().fills);
}

#[test]
fn requests_that_need_a_dialog_become_effects() {
    let mut m = model();
    assert!(matches!(m.dispatch(Intent::RequestOpen).as_slice(), [Effect::AskOpenPath]));
    assert!(matches!(m.dispatch(Intent::RequestLoadProject).as_slice(), [Effect::AskProjectToLoad]));
    assert!(m.dispatch(Intent::RequestSaveProject).is_empty(), "nothing to save without a figure");
    assert!(matches!(m.dispatch(Intent::Quit).as_slice(), [Effect::Quit]));
    assert!(m.extensions().contains(&"pdf"));
}

#[test]
fn opening_a_pdf_lists_its_pages_and_opens_the_one_chosen() {
    let dir = Scratch::new("pages");
    let mut m = model();
    let fx = run(&mut m, Intent::Open(dir.pdf(3)));
    assert!(m.session().is_none(), "nothing is opened before a page is chosen");
    assert!(matches!(fx.as_slice(), [Effect::RunBatch(jobs)] if jobs.len() == 3), "a picture of each page: {fx:?}");
    let document = m.pdf().expect("the window is open").document().unwrap();
    assert_eq!((document.name.as_str(), document.contents.pages), ("book.pdf", 3));
    assert_eq!(message(&m), "Choose a page or an image of book.pdf to open.");

    let fx = run(&mut m, Intent::ImportPart(Part::Page(2)));
    assert!(matches!(&fx[0], Effect::Opened { title, .. } if title == "book.pdf \u{2013} plotextract"));
    assert!(m.pdf().is_none(), "the window closes");
    let f = &m.session().unwrap().figure;
    assert_eq!(f.page, 2);
    assert_eq!(message(&m), "Opened book.pdf, page 3: 1 curves. Ctrl+click a curve to select it.");
}

#[test]
fn closing_the_list_of_a_pdf_keeps_what_was_open() {
    let dir = Scratch::new("pdf-cancel");
    let mut m = opened(&dir);
    run(&mut m, Intent::Open(dir.pdf(2)));
    assert!(m.pdf().is_some());
    run(&mut m, Intent::ClosePdf);
    assert!(m.pdf().is_none());
    assert_eq!(m.session().unwrap().source.as_deref(), Some(dir.svg().display().to_string().as_str()));
}

#[test]
fn a_figure_from_arxiv_that_is_a_pdf_is_listed_too() {
    let dir = Scratch::new("arxiv-pdf");
    let mut m = model();
    m.dispatch(Intent::RequestArxiv);
    run(&mut m, Intent::ImportFigure(dir.pdf(2)));
    assert!(m.arxiv().is_none() && m.pdf().is_some_and(|p| p.document().unwrap().contents.pages == 2));
}

#[test]
fn a_project_names_its_page() {
    let dir = Scratch::new("projectpage");
    let mut m = model();
    run(&mut m, Intent::Open(dir.pdf(3)));
    run(&mut m, Intent::ImportPart(Part::Page(2)));
    m.dispatch(Intent::Edit(Edit::SetValue(Slot::YMin, Some(5.0))));
    let saved = dir.0.join("p.plotextract.json");
    m.dispatch(Intent::SaveProject(saved.clone()));

    // the project names its page: nothing is chosen
    let mut fresh = model();
    run(&mut fresh, Intent::LoadProject(saved));
    assert!(fresh.pdf().is_none());
    assert_eq!(fresh.session().unwrap().figure.page, 2);
    assert_eq!(fresh.session().unwrap().calibration().value(Slot::YMin), Some(5.0));
}

#[test]
fn a_figure_the_importer_cut_short_warns_and_draws_no_picture() {
    let dir = Scratch::new("truncated");
    let mut m = model();
    run(&mut m, Intent::Open(dir.svg()));
    // pretend the file was too big: what matters is how the model reacts
    let mut opened = library().open(&dir.svg(), OpenOptions::default()).unwrap();
    opened.session.figure.truncated = true;
    let mut fx = Vec::new();
    m.install(opened, &mut fx);
    m.announce();
    assert!(matches!(&fx[0], Effect::Opened { source: None, .. }));
    assert_eq!(m.message().unwrap().level, Level::Warning);
    assert!(message(&m).contains("only the first 1"));
}

impl Scratch {
    fn png(&self, w: u32, h: u32) -> PathBuf {
        let path = self.0.join("shot.png");
        image::RgbaImage::from_pixel(w, h, image::Rgba([250, 250, 250, 255])).save(&path).unwrap();
        path
    }
}

#[test]
fn opening_an_image_starts_the_wizard_on_an_empty_figure() {
    use plotextract_usecases::wizard::WizardIntent;
    let dir = Scratch::new("image");
    let mut m = model();
    let fx = run(&mut m, Intent::Open(dir.png(300, 200)));
    let s = m.session().unwrap();
    assert_eq!((s.figure.width, s.figure.height, s.figure.primitives.len()), (300.0, 200.0, 0));
    let w = m.wizard().expect("an image is prepared before it is traced");
    assert_eq!((w.size(), w.tool), ((300, 200), None));
    assert!(matches!(&fx[0], Effect::Opened { source: Some(_), .. }), "the image is the picture");
    assert_eq!(message(&m), "Opened shot.png: 300 x 200 pixels.");

    // the wizard takes the tool and mask intents, and Undo and Redo go to it (they undo drawing the exclude mask), not
    // to the empty session
    m.dispatch(Intent::Wizard(WizardIntent::SetCrop(Some(kurbo::Rect::new(20.0, 10.0, 220.0, 150.0)))));
    assert_eq!(m.wizard().unwrap().crop_pixels().width, 200);
    let block = plotextract_domain::MaskOp {
        shape: plotextract_domain::Shape::Rect(kurbo::Rect::new(30.0, 20.0, 60.0, 40.0)),
        erase: false,
    };
    m.dispatch(Intent::Wizard(WizardIntent::Exclude(block)));
    assert!(m.dispatch(Intent::Undo).is_empty(), "no refill of calibration cells is asked for");
    assert!(m.wizard().unwrap().prepare().mask.is_empty());
    m.dispatch(Intent::Redo);
    assert_eq!(m.wizard().unwrap().prepare().mask.len(), 1);
    assert_eq!(m.wizard().unwrap().crop_pixels().height, 140, "the crop is not undone");
}

#[test]
fn opening_anything_else_leaves_the_wizard() {
    let dir = Scratch::new("leave");
    let mut m = model();
    run(&mut m, Intent::Open(dir.png(80, 60)));
    assert!(m.wizard().is_some());
    run(&mut m, Intent::Open(dir.svg()));
    assert!(m.wizard().is_none(), "a vector figure needs no tracing");
    run(&mut m, Intent::Open(dir.png(80, 60)));
    assert!(m.wizard().is_some());
    // a file that cannot be opened keeps what was open
    let notes = dir.0.join("notes.txt");
    std::fs::write(&notes, "hello").unwrap();
    run(&mut m, Intent::Open(notes));
    assert!(m.wizard().is_some());
}

impl Scratch {
    /// A 120 x 80 plot on white: a blue line, a black one and an orange block.
    fn coloured_png(&self) -> PathBuf {
        let path = self.0.join("plot.png");
        let mut img = image::RgbaImage::from_pixel(120, 80, image::Rgba([255, 255, 255, 255]));
        for x in 10..110 {
            img.put_pixel(x, 20, image::Rgba([31, 119, 180, 255]));
            img.put_pixel(x, 21, image::Rgba([31, 119, 180, 255]));
            img.put_pixel(x, 60, image::Rgba([0, 0, 0, 255]));
        }
        for y in 30..50 {
            for x in 50..70 {
                img.put_pixel(x, y, image::Rgba([255, 127, 14, 255]));
            }
        }
        img.save(&path).unwrap();
        path
    }
}

fn colours_of(m: &Model) -> usize {
    m.wizard().unwrap().colours().map().map_or(0, |map| map.inks.len())
}

/// Say the colour `rgb` is each of `roles`.
fn say(m: &mut Model, rgb: [u8; 3], roles: &[plotextract_usecases::wizard::Role]) {
    let ink = m.wizard().unwrap().colours().map().unwrap().inks.iter().position(|i| i.rgb == rgb).expect("the ink");
    for &role in roles {
        m.dispatch(Intent::Wizard(plotextract_usecases::wizard::WizardIntent::SetRole(ink, role, true)));
    }
}

#[test]
fn the_colour_step_looks_at_the_image_on_a_thread_and_keeps_what_it_finds() {
    use plotextract_usecases::wizard::{Step, WizardIntent};
    let dir = Scratch::new("colours");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    assert_eq!(m.wizard().unwrap().step(), Step::Prepare);
    assert_eq!(colours_of(&m), 0);

    // asking for the step starts the analysis; its answer, taken later, holds the inks
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Analyse(_))]), "{fx:?}");
    assert_eq!(colours_of(&m), 0, "not yet");
    m.run_jobs(fx);
    let w = m.wizard().unwrap();
    assert_eq!(w.step(), Step::Colours);
    let map = w.colours().map().expect("analysed");
    let found: Vec<[u8; 3]> = map.inks.iter().map(|i| i.rgb).collect();
    assert_eq!(found.len(), 3, "{found:?}");
    for want in [[31, 119, 180], [255, 127, 14], [0, 0, 0]] {
        assert!(found.contains(&want), "{want:?} in {found:?}");
    }
    assert!((0..found.len()).all(|i| w.colours().is_ignored(i)), "nothing is guessed: each colour is said by the user");

    // the user's word is kept, and is not asked for again
    let ink = 0;
    assert!(m.dispatch(Intent::Wizard(WizardIntent::IgnoreColour(ink))).is_empty());
    assert!(m.dispatch(Intent::Wizard(WizardIntent::Select(Some(1)))).is_empty());
    assert!(m.wizard().unwrap().colours().is_ignored(ink));
    assert_eq!(m.wizard().unwrap().colours().selected(), Some(1));
}

#[test]
fn a_new_tolerance_looks_again_and_going_back_does_not() {
    use plotextract_usecases::wizard::{Step, WizardIntent};
    let dir = Scratch::new("tolerance");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    m.run_jobs(fx);
    assert!(m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Prepare))).is_empty(), "back needs no analysis");
    let fx = m.dispatch(Intent::Wizard(WizardIntent::SetTolerance(30.0)));
    assert!(fx.is_empty(), "the tolerance is not looked at on the first step");
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Analyse(_))]), "coming back looks again, with the new tolerance");
    m.run_jobs(fx);
    let fx = m.dispatch(Intent::Wizard(WizardIntent::SetTolerance(5.0)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Analyse(_))]));
    assert_eq!(colours_of(&m), 0, "what was found no longer matches until the new answer arrives");
    m.run_jobs(fx);
    assert_eq!(colours_of(&m), 3);
}

#[test]
fn an_older_analysis_does_not_replace_a_newer_one() {
    use plotextract_usecases::wizard::{Step, WizardIntent};
    let dir = Scratch::new("stale-analysis");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    m.dispatch(Intent::Wizard(WizardIntent::SetCrop(Some(kurbo::Rect::new(0.0, 0.0, 60.0, 80.0)))));
    let first = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    let second = m.dispatch(Intent::Wizard(WizardIntent::SetTolerance(12.0)));
    m.run_jobs(first);
    assert_eq!(colours_of(&m), 0, "the first answer is for a job that is no longer waited for");
    m.run_jobs(second);
    assert!(colours_of(&m) > 0);
    assert_eq!(m.wizard().unwrap().colours().map().unwrap().crop.width, 60, "the crop is what is read");
}

#[test]
fn changing_the_crop_or_the_exclude_mask_on_the_colours_step_looks_at_the_colours_again() {
    use plotextract_domain::{MaskOp, Shape};

    use plotextract_usecases::wizard::{MaskId, Step, WizardIntent};
    let dir = Scratch::new("recolour");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    m.run_jobs(fx);
    assert_eq!(colours_of(&m), 3);

    // exclude the orange block: its colour is gone from the answer
    let block = MaskOp { shape: Shape::Rect(kurbo::Rect::new(45.0, 25.0, 75.0, 55.0)), erase: false };
    let fx = m.dispatch(Intent::Wizard(WizardIntent::Exclude(block)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Analyse(_))]), "{fx:?}");
    m.run_jobs(fx);
    assert_eq!(colours_of(&m), 2);

    // deleting the exclude mask brings it back, with another look
    let fx = m.dispatch(Intent::Wizard(WizardIntent::Delete(MaskId::Exclude)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Analyse(_))]));
    m.run_jobs(fx);
    assert_eq!(colours_of(&m), 3);
    assert!(m.wizard().unwrap().prepare().mask.is_empty());
}

#[test]
fn opening_another_image_forgets_the_colours() {
    use plotextract_usecases::wizard::{Step, WizardIntent};
    let dir = Scratch::new("forget-colours");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    m.run_jobs(fx);
    run(&mut m, Intent::Open(dir.png(40, 30)));
    let w = m.wizard().unwrap();
    assert_eq!((w.step(), colours_of(&m)), (Step::Prepare, 0));
}

#[test]
fn the_lines_are_looked_for_on_a_thread_one_search_at_a_time() {
    use plotextract_algo::lines::Params;
    use plotextract_usecases::wizard::{Role, Step, WizardIntent};
    let dir = Scratch::new("lines-job");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    m.run_jobs(fx);
    say(&mut m, [0, 0, 0], &[Role::Axes]);

    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::PlotArea)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Lines(_))]), "entering the step starts the search: {fx:?}");
    assert!(m.wizard().unwrap().lines_busy(), "the search is running");

    // changes made meanwhile ask again, but start nothing: one search runs at a time
    let mut params = Params::default();
    for shortest in [30.0, 40.0] {
        params.min_length = shortest;
        let fx = m.dispatch(Intent::Wizard(WizardIntent::SetLineParams(params.clone())));
        assert!(fx.is_empty(), "{fx:?}");
    }
    assert!(m.wizard().unwrap().lines_busy());

    // the first answer starts exactly one more search, with the newest settings
    let Some(Effect::Run(Job::Lines(first))) = fx.into_iter().next() else { panic!("a job") };
    let next = m.finish_lines(first.run());
    let [Effect::Run(Job::Lines(_))] = next.as_slice() else { panic!("one more search: {next:?}") };
    assert!(m.wizard().unwrap().lines_busy());

    // and its answer ends it: the black line of the plot is 100 pixels long
    let Some(Effect::Run(Job::Lines(second))) = next.into_iter().next() else { panic!("a job") };
    assert!(m.finish_lines(second.run()).is_empty());
    let w = m.wizard().unwrap();
    assert!(!w.lines_busy());
    assert_eq!(w.lines().len(), 1, "{:?}", w.lines());
    assert_eq!(w.line_params().min_length, 40.0);
}

#[test]
fn the_pieces_of_the_curves_are_found_on_a_thread_and_the_points_need_the_calibration() {
    use plotextract_usecases::wizard::{Role, Step, WizardIntent};
    let dir = Scratch::new("pieces-job");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    m.run_jobs(fx);
    say(&mut m, [0, 0, 0], &[Role::Axes]);
    say(&mut m, [31, 119, 180], &[Role::Curve]);
    say(&mut m, [255, 127, 14], &[Role::Curve]);
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::PlotArea)));
    m.run_jobs(fx);
    let fx = m.dispatch(Intent::Wizard(WizardIntent::GoTo(Step::Shapes)));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Pieces(_))]), "entering the step starts the search: {fx:?}");
    assert!(m.wizard().unwrap().pieces_busy());
    m.run_jobs(fx);
    let w = m.wizard().unwrap();
    let shapes = w.shapes().expect("the groups are found with the pieces");
    // the blue line a group, the orange block like no other: in none
    let line = w.pieces().unwrap().at(30, 20).expect("the blue line");
    assert!(shapes.type_of[line].is_some_and(|k| shapes.types[k].solid()), "{:?}", shapes.types);
    assert_eq!((shapes.groups().count(), shapes.leftovers().len()), (1, 1));

    // the changes to the groups are undone with the app's undo
    m.dispatch(Intent::Wizard(WizardIntent::SelectPieces(vec![line], plotextract_usecases::wizard::Pick::Replace)));
    m.dispatch(Intent::Wizard(WizardIntent::RemoveSelected));
    assert_eq!(m.wizard().unwrap().shapes().unwrap().leftovers().len(), 2);
    m.dispatch(Intent::Undo);
    assert_eq!(m.wizard().unwrap().shapes().unwrap().leftovers().len(), 1);

    assert!(m.dispatch(Intent::CopyNumbers).is_empty());
    assert!(message(&m).starts_with("Calibrate the axes"), "{}", message(&m));
    assert!(m.dispatch(Intent::RequestExportCsv).is_empty());
}

#[test]
fn a_saved_project_brings_back_the_tracing_of_an_image_on_its_step() {
    use plotextract_domain::{MaskOp, Shape};
    use plotextract_usecases::wizard::{Pick, Role, Step, WizardIntent};
    let dir = Scratch::new("tracing-project");
    let mut m = model();
    let image = dir.coloured_png();
    run(&mut m, Intent::Open(image.clone()));
    let corner = MaskOp { shape: Shape::Rect(kurbo::Rect::new(0.0, 0.0, 5.0, 5.0)), erase: false };
    run(&mut m, Intent::Wizard(WizardIntent::Exclude(corner)));
    run(&mut m, Intent::Wizard(WizardIntent::GoTo(Step::Colours)));
    say(&mut m, [0, 0, 0], &[Role::Axes]);
    say(&mut m, [31, 119, 180], &[Role::Curve]);
    say(&mut m, [255, 127, 14], &[Role::Curve]);
    run(&mut m, Intent::Wizard(WizardIntent::GoTo(Step::PlotArea)));
    run(&mut m, Intent::Wizard(WizardIntent::GoTo(Step::Shapes)));
    let line = m.wizard().unwrap().pieces().unwrap().at(30, 20).expect("the blue line");
    run(&mut m, Intent::Wizard(WizardIntent::SelectPieces(vec![line], Pick::Replace)));
    run(&mut m, Intent::Wizard(WizardIntent::RemoveSelected));
    run(&mut m, Intent::Wizard(WizardIntent::GoTo(Step::Links)));
    run(&mut m, Intent::Wizard(WizardIntent::LinkPiece(line)));
    run(&mut m, Intent::Wizard(WizardIntent::EndLink));
    let saved = dir.0.join("traced.plotextract.json");
    m.dispatch(Intent::SaveProject(saved.clone()));
    let before = m.wizard().unwrap();
    let (work, links, points) = (before.work(), before.links(), before.link_points());
    assert_eq!(links.len(), 1);

    // in a fresh window, and again over the same image already open: the image is opened afresh, the colours
    // looked at, and the lines and pieces found, with the edit and the link made again
    let mut fresh = model();
    for m in [&mut fresh, &mut m] {
        run(m, Intent::LoadProject(saved.clone()));
        let w = m.wizard().expect("the image is traced again");
        assert_eq!(w.step(), Step::Links);
        assert!(!w.pieces_busy() && !w.lines_busy());
        assert_eq!(w.work(), work);
        assert_eq!((w.links(), w.link_points()), (links.clone(), points.clone()));
        assert!(w.shapes().unwrap().leftovers().contains(&line), "the line taken out of its group is out again");
    }
}

#[test]
fn a_project_whose_tracing_cannot_be_read_opens_the_image_afresh_with_a_warning() {
    use plotextract_usecases::wizard::Step;
    let dir = Scratch::new("bad-tracing");
    let saved = dir.0.join("bad.plotextract.json");
    let mut m = model();
    run(&mut m, Intent::Open(dir.coloured_png()));
    m.dispatch(Intent::SaveProject(saved.clone()));
    let mut text: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
    text["tracing"]["step"] = "NoSuchStep".into();
    std::fs::write(&saved, text.to_string()).unwrap();
    let mut m = model();
    run(&mut m, Intent::LoadProject(saved));
    assert_eq!(m.wizard().expect("the image is open").step(), Step::Prepare);
    assert_eq!(m.message().unwrap().level, Level::Warning, "{}", message(&m));
}

/// A model that keeps the images it takes out of documents in `dir`'s cache folder.
fn caching(dir: &Scratch) -> Model {
    let papers = ArxivPapers::new(Arc::new(FakeArxiv(None)), dir.0.join("papers"));
    Model::new(library().with_cache(dir.0.join("cache")), Arc::new(papers))
}

/// Run the jobs of a batch among `effects`, and give back their answers.
fn run_batch(effects: Vec<Effect>) -> Vec<Answer> {
    effects
        .into_iter()
        .flat_map(|e| match e {
            Effect::RunBatch(jobs) => jobs,
            other => panic!("not a batch: {other:?}"),
        })
        .map(Job::run)
        .collect()
}

#[test]
fn a_pdf_lists_its_pages_and_images_with_their_pictures_and_an_image_opens_to_be_traced() {
    use plotextract_usecases::wizard::Step;
    let dir = Scratch::new("pdf-import");
    let mut m = caching(&dir);

    // opening it is the load job, whose answer lists the parts and asks for their pictures, a batch
    let fx = m.dispatch(Intent::Open(dir.pdf_with_image()));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Load(_))]), "{fx:?}");
    assert_eq!(m.loading(), Some("paper.pdf"));
    let pictures = m.run_jobs(fx);
    let pdf = m.pdf().expect("the window is open");
    let document = pdf.document().expect("read");
    assert_eq!(document.contents.pages, 2);
    let image = *document.contents.images.first().expect("the picture on page 2");
    assert_eq!((image.page, image.index, image.width, image.height), (1, 0, 32, 16));

    let answers = run_batch(pictures);
    assert_eq!(answers.len(), 3, "both pages and the image");
    for answer in answers {
        let Answer::PartPicture(p) = answer else { panic!("a picture") };
        assert_eq!(Some(p.id), m.pdf().unwrap().document_job());
        let picture = p.picture.unwrap_or_else(|e| panic!("{:?}: {e}", p.part));
        assert_eq!(picture.width.max(picture.height), super::arxiv_import::THUMBNAIL_SIDE, "{:?}", p.part);
    }

    // the image is taken out into the cache and opened, to trace
    run(&mut m, Intent::ImportPart(Part::Image(image)));
    assert!(m.pdf().is_none(), "the window closes");
    let w = m.wizard().expect("an image to trace");
    assert_eq!((w.size(), w.step()), ((32, 16), Step::Prepare));
    let source = PathBuf::from(m.session().unwrap().source.clone().unwrap());
    assert!(source.starts_with(dir.0.join("cache").join("pdf")), "{}", source.display());
    assert!(source.file_name().unwrap().to_string_lossy().ends_with("-page2-image1.png"));
    assert!(message(&m).contains("32 x 16"), "{}", message(&m));

    // a page opens as any page, its paths to pick
    run(&mut m, Intent::Open(dir.pdf_with_image()));
    run(&mut m, Intent::ImportPart(Part::Page(0)));
    assert!(m.wizard().is_none());
    let figure = &m.session().unwrap().figure;
    assert_eq!((figure.page, figure.primitives.len()), (0, 1));
}

#[test]
fn a_damaged_pdf_is_refused_without_a_list() {
    let dir = Scratch::new("pdf-import-bad");
    let mut m = caching(&dir);
    let bad = dir.0.join("notes.pdf");
    std::fs::write(&bad, "%PDF-1.7 not really").unwrap();
    run(&mut m, Intent::Open(bad));
    assert!(m.pdf().is_none() && m.session().is_none());
    assert_eq!(m.message().unwrap().level, Level::Error, "{}", message(&m));
}

/// Serves one source for every paper, as arXiv would with the version `v2`; or nothing.
struct FakeArxiv(Option<Vec<u8>>);

impl Downloader for FakeArxiv {
    fn download(&self, id: &ArxivId, _: &Progress) -> Result<plotextract_arxiv::Download, ArxivError> {
        match &self.0 {
            Some(bytes) => {
                Ok(plotextract_arxiv::Download { version: ArxivId::parse(&format!("{id}v2")), bytes: bytes.clone() })
            }
            None => Err(ArxivError::NotFound(id.to_string())),
        }
    }
}

/// A gzipped tar of `files`.
fn source(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for (name, data) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, *data).unwrap();
    }
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    std::io::Write::write_all(&mut gz, &builder.into_inner().unwrap()).unwrap();
    gz.finish().unwrap()
}

#[test]
fn a_figure_of_an_arxiv_paper_is_downloaded_listed_drawn_and_opened() {
    let dir = Scratch::new("arxiv");
    let tex = br"\documentclass{article}\includegraphics{figs/plot}";
    let files: [(&str, &[u8]); 3] =
        [("main.tex", tex), ("figs/other.svg", SVG.as_bytes()), ("figs/plot.svg", SVG.as_bytes())];
    let fake = Arc::new(FakeArxiv(Some(source(&files))));
    let mut m = with_papers(fake, dir.0.clone());
    assert!(m.arxiv().is_none());
    m.dispatch(Intent::RequestArxiv);

    // a number is needed
    assert!(m.dispatch(Intent::FetchArxiv("plot.svg".into())).is_empty());
    assert!(m.arxiv().unwrap().error().unwrap().contains("not an arXiv number"));

    let fx = m.dispatch(Intent::FetchArxiv(" arXiv:2301.01234 ".into()));
    assert!(matches!(fx.as_slice(), [Effect::Run(Job::Arxiv(_))]), "{fx:?}");
    assert_eq!(m.arxiv().unwrap().fetching().map(|(id, _)| id.to_string()).as_deref(), Some("2301.01234"));
    let fx = m.run_jobs(fx);
    let a = m.arxiv().unwrap();
    let paper = a.paper().unwrap();
    assert!(a.fetching().is_none() && a.error().is_none());
    assert_eq!(paper.id, "2301.01234v2");
    let names: Vec<&str> = paper.figures.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["figs/plot.svg", "figs/other.svg"], "the figure the paper draws comes first");

    // the pictures are drawn by the shell's threads, one figure at a time
    let paper_job = a.paper_job().unwrap();
    let first = paper.figures[0].path.clone();
    let Ok([Effect::RunBatch(jobs)]) = <[Effect; 1]>::try_from(fx) else { panic!("a batch of pictures") };
    assert_eq!(jobs.len(), 2);
    let Some(Answer::Thumbnail(thumbnail)) = jobs.into_iter().next().map(Job::run) else { panic!("a picture") };
    assert_eq!((thumbnail.id, &thumbnail.path), (paper_job, &first));
    assert!(thumbnail.picture.is_ok_and(|p| p.width.max(p.height) == THUMBNAIL_SIDE));
    let a = m.arxiv().unwrap();
    let paper = a.paper().unwrap();

    // importing opens the figure from where it was unpacked, and closes the window; the paper stays for next time
    let path = paper.figures[0].path.clone();
    assert!(path.starts_with(dir.0.join("2301.01234v2")));
    run(&mut m, Intent::ImportFigure(path.clone()));
    assert!(m.arxiv().is_none());
    assert_eq!(m.session().unwrap().source.as_deref(), Some(path.display().to_string().as_str()));
    m.dispatch(Intent::RequestArxiv);
    assert!(m.arxiv().unwrap().paper().is_some());
}

#[test]
fn a_paper_that_is_not_there_says_so_and_a_stale_answer_is_dropped() {
    let dir = Scratch::new("arxiv-missing");
    let mut m = with_papers(Arc::new(FakeArxiv(None)), dir.0.clone());
    m.dispatch(Intent::RequestArxiv);
    let first = m.dispatch(Intent::FetchArxiv("2301.01234".into()));
    let second = m.dispatch(Intent::FetchArxiv("2301.99999".into()));
    assert!(m.run_jobs(first).is_empty(), "the first fetch was replaced");
    assert!(m.arxiv().unwrap().fetching().is_some() && m.arxiv().unwrap().error().is_none());
    m.run_jobs(second);
    assert_eq!(m.arxiv().unwrap().error(), Some("arXiv has no paper 2301.99999. Check the number."));
}
