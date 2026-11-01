//! Listing what a PDF holds (its pages and images), their pictures, and opening each, on protected and corrupted files
//! from `testdata/stress` (made by `make_stress.py`): the right password lists and opens everything, a missing or wrong
//! one is asked for again, and a damaged file is listed as far as it can be or refused, never taking the program down.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use plotextract_domain::{ImportError, OpenOptions};
use plotextract_model::jobs::{Answer, Job};
use plotextract_model::{Effect, Intent, Level, Model, Part};
use plotextract_usecases::{Source, Workspace, WorkspaceError};

/// The path of a stress file, or `None` (after saying so) when the test data is not there.
fn stress(name: &str) -> Option<PathBuf> {
    let path = plotextract_corpus::data_dir().join("stress").join(name);
    if path.exists() {
        Some(path)
    } else {
        eprintln!("skipped: no {}; run testdata/make_stress.py", path.display());
        None
    }
}

/// A workspace that keeps the images it takes out in a folder of its own.
fn library(name: &str) -> Workspace {
    let cache = std::env::temp_dir().join(format!("plotextract-pdf-parts-{}-{name}", std::process::id()));
    plotextract_corpus::library().with_cache(cache)
}

fn read(ws: &Workspace, path: &std::path::Path, password: Option<&str>) -> Result<Source, WorkspaceError> {
    ws.read(path, OpenOptions { password: password.map(str::to_string), ..Default::default() })
}

/// The pixels of each image the document lists, decoded from the PNG it gives.
fn images_of(ws: &Workspace, source: &Source) -> Vec<image::RgbImage> {
    let contents = source.contents().unwrap();
    contents
        .images
        .iter()
        .map(|e| {
            let png = ws.image_of(source, e).unwrap().bytes().clone();
            let picture = image::load_from_memory(&png).unwrap().to_rgb8();
            assert_eq!(picture.dimensions(), (e.width, e.height));
            picture
        })
        .collect()
}

#[test]
fn a_plain_document_lists_its_images_and_pages() {
    let Some(path) = stress("images.pdf") else { return };
    let ws = library("plain");
    let source = read(&ws, &path, None).unwrap();
    let contents = source.contents().unwrap();
    assert_eq!(contents.pages, 2);
    let listed: Vec<(usize, usize)> = contents.images.iter().map(|e| (e.page, e.index)).collect();
    assert_eq!(listed, vec![(1, 0), (1, 1)], "both images are on page 2");
    for picture in images_of(&ws, &source) {
        // a plot: mostly white paper, with ink on it
        let white = picture.pixels().filter(|p| p.0.iter().all(|&c| c > 240)).count();
        let share = white as f64 / f64::from(picture.width() * picture.height());
        assert!((0.5..0.99).contains(&share), "{share}");
    }
}

#[test]
fn a_protected_document_lists_and_gives_the_same_images_with_its_password() {
    let Some(plain) = stress("images.pdf") else { return };
    let ws = library("protected");
    let expected = images_of(&ws, &read(&ws, &plain, None).unwrap());
    for name in ["images_aes.pdf", "images_rc4.pdf"] {
        let Some(path) = stress(name) else { return };
        // the listing needs the password, as opening does
        let missing = read(&ws, &path, None).and_then(|s| Ok(s.contents()?)).err();
        assert!(matches!(missing, Some(WorkspaceError::Import(ImportError::PasswordRequired))), "{name}: {missing:?}");
        let wrong = read(&ws, &path, Some("guess")).and_then(|s| Ok(s.contents()?)).err();
        assert!(matches!(wrong, Some(WorkspaceError::Import(ImportError::WrongPassword))), "{name}: {wrong:?}");

        let source = read(&ws, &path, Some("secret")).unwrap();
        assert_eq!(source.contents().unwrap().pages, 2, "{name}");
        assert!(images_of(&ws, &source) == expected, "{name}: the images decrypted are the images");
        // the pages are drawn and opened with the password the source was read with
        for page in 0..2 {
            assert!(ws.thumbnail_of(source.with_page(page), 64).is_ok(), "{name} page {page}");
        }
        let opened = ws.load(source.with_page(0), None).unwrap();
        assert!(!opened.session.figure.primitives.is_empty(), "{name}: the vector page has its paths");
    }
}

#[test]
fn a_document_protected_only_from_changes_lists_without_a_password() {
    let Some(path) = stress("images_owner_only.pdf") else { return };
    let ws = library("owner");
    let source = read(&ws, &path, None).unwrap();
    assert_eq!(images_of(&ws, &source).len(), 2);
}

#[test]
fn damaged_documents_are_listed_as_far_as_they_can_be_or_refused_and_never_take_the_program_down() {
    let names = [
        "images_garbled_stream.pdf",
        "images_truncated.pdf",
        "images_no_xref.pdf",
        "images_bit_flips.pdf",
        "empty.pdf",
        "header_only.pdf",
        "truncated_half.pdf",
        "truncated_tail.pdf",
        "no_xref.pdf",
        "bit_flips.pdf",
        "zeroed_block.pdf",
        "random_after_header.pdf",
        "garbage_xref_offsets.pdf",
        "pdf_in_the_middle.pdf",
        "zero_pages.pdf",
        "pagetree_loop.pdf",
        "form_recursion.pdf",
        "mediabox_zero.pdf",
        "absurd_numbers.pdf",
    ];
    let ws = library("damaged");
    for name in names {
        let Some(path) = stress(name) else { return };
        let start = Instant::now();
        let listed = read(&ws, &path, None).and_then(|s| Ok((s.contents()?, s)));
        let mut report = String::new();
        match listed {
            Err(e) => {
                assert!(!e.to_string().is_empty(), "{name}");
                report = format!("refused: {e}");
            }
            Ok((contents, source)) => {
                // every part is drawn small and opened, or refused with a reason
                let mut shown = 0;
                for image in &contents.images {
                    let picture = ws.image_of(&source, image).and_then(|s| ws.thumbnail_of(s, 64));
                    let opened = ws.open_image_of(&source, &path, image);
                    shown += usize::from(picture.is_ok() && opened.is_ok());
                }
                for page in 0..contents.pages {
                    let picture = ws.thumbnail_of(source.with_page(page), 64);
                    let opened = ws.load(source.with_page(page), None);
                    shown += usize::from(picture.is_ok() && opened.is_ok());
                }
                report += &format!(
                    "{} images, {} pages listed; {shown} drawn and opened",
                    contents.images.len(),
                    contents.pages
                );
            }
        }
        let took = start.elapsed();
        eprintln!("{name:<28} {report} ({took:.0?})");
        assert!(took < Duration::from_secs(20), "{name} took {took:?}");
    }
}

/// A model on the local disk whose papers never come.
fn model(name: &str) -> Model {
    let papers = plotextract_arxiv::ArxivPapers::new(
        Arc::new(NoPapers),
        std::env::temp_dir().join(format!("plotextract-no-papers-{name}")),
    );
    Model::new(library(name), Arc::new(papers))
}

struct NoPapers;

impl plotextract_arxiv::Downloader for NoPapers {
    fn download(
        &self,
        id: &plotextract_domain::ArxivId,
        _: &plotextract_ports::papers::Progress,
    ) -> Result<plotextract_arxiv::Download, plotextract_ports::papers::ArxivError> {
        Err(plotextract_ports::papers::ArxivError::NotFound(id.to_string()))
    }
}

/// Carry out an intent and the jobs it starts, here, as the window would on its threads; the pictures of a batch too.
fn run(m: &mut Model, intent: Intent) -> Vec<Answer> {
    let mut pictures = Vec::new();
    let mut todo = m.dispatch(intent);
    while !todo.is_empty() {
        let mut next = Vec::new();
        for effect in todo {
            match effect {
                Effect::Run(job) => next.extend(m.finish(job.run())),
                Effect::RunBatch(jobs) => pictures.extend(jobs.into_iter().map(Job::run)),
                _ => {}
            }
        }
        todo = next;
    }
    pictures
}

#[test]
fn opening_a_protected_document_asks_for_its_password_then_lists_and_opens_its_parts() {
    let Some(path) = stress("images_aes.pdf") else { return };
    let mut m = model("password");
    run(&mut m, Intent::Open(path.clone()));
    let prompt = m.password_prompt().expect("its password is asked for");
    assert_eq!((prompt.file.as_str(), prompt.wrong), ("images_aes.pdf", false));
    assert!(m.pdf().is_none());

    run(&mut m, Intent::SubmitPassword("guess".into()));
    assert!(m.password_prompt().is_some_and(|p| p.wrong), "a wrong password is asked for again");

    let pictures = run(&mut m, Intent::SubmitPassword("secret".into()));
    assert!(m.password_prompt().is_none());
    let document = m.pdf().expect("listed").document().unwrap();
    assert_eq!((document.contents.images.len(), document.contents.pages), (2, 2));
    let image = document.contents.images[0];
    assert_eq!(pictures.len(), 4, "a picture of each image and page");
    for answer in pictures {
        let Answer::PartPicture(p) = answer else { panic!("a picture") };
        assert!(p.picture.is_ok(), "{:?}: {:?}", p.part, p.picture.err());
    }

    // the image and the pages open without the password being asked for again
    run(&mut m, Intent::ImportPart(Part::Image(image)));
    assert!(m.password_prompt().is_none());
    let w = m.wizard().expect("the image, to trace");
    assert_eq!(w.size(), (image.width, image.height));
    run(&mut m, Intent::Open(path));
    run(&mut m, Intent::SubmitPassword("secret".into()));
    run(&mut m, Intent::ImportPart(Part::Page(0)));
    assert!(m.wizard().is_none());
    assert!(!m.session().unwrap().figure.primitives.is_empty());
    assert_eq!(m.message().unwrap().level, Level::Info, "{}", m.message().unwrap().text);
}

#[test]
fn opening_a_damaged_document_lists_what_it_can_or_says_why_not() {
    for name in ["images_garbled_stream.pdf", "images_truncated.pdf", "truncated_half.pdf", "random_after_header.pdf"] {
        let Some(path) = stress(name) else { return };
        let mut m = model(name);
        let pictures = run(&mut m, Intent::Open(path));
        assert!(m.loading().is_none(), "{name}: the load ended");
        match m.pdf().and_then(|p| p.document()) {
            Some(document) => {
                let parts = document.contents.images.len() + document.contents.pages;
                assert_eq!(pictures.len(), parts, "{name}: every part is drawn, or says why not");
                // each part opens or is refused with a message; the program goes on
                let first = document.contents.images.first().map(|&i| Part::Image(i)).unwrap_or(Part::Page(0));
                run(&mut m, Intent::ImportPart(first));
                assert!(m.session().is_some() || m.message().is_some_and(|msg| msg.level == Level::Error), "{name}");
            }
            None => assert_eq!(m.message().map(|msg| msg.level), Some(Level::Error), "{name}: refused with a reason"),
        }
    }
}
