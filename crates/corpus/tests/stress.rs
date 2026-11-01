//! Awkward inputs: big, many-paged, damaged, protected and hostile files (made by `make_stress.py`).
//!
//! Whatever the file, opening it must finish in reasonable time and either give a figure or refuse with a reason: never
//! hang, panic or keep every path of a file that holds millions. The heavier files are only worth opening in a release
//! build; in a debug build they are skipped.

use std::path::PathBuf;
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use plotextract_corpus::data_dir;
use plotextract_domain::{ImportError, OpenOptions};
use plotextract_usecases::{Opened, WorkspaceError};

/// How long any file may take to open.
const LIMIT: Duration = Duration::from_secs(90);

fn path(name: &str) -> Option<PathBuf> {
    let path = data_dir().join("stress").join(name);
    if !path.exists() {
        eprintln!("skipped: no test data at {} (see testdata/make_stress.py)", path.display());
        return None;
    }
    Some(path)
}

/// Open a file on its own thread, with a time limit and catching panics.
fn open(name: &str, options: OpenOptions) -> Option<(Result<Opened, WorkspaceError>, Duration)> {
    let path = path(name)?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let t = Instant::now();
        let result = std::panic::catch_unwind(|| plotextract_corpus::library().open(&path, options));
        let _ = tx.send((result, t.elapsed()));
    });
    match rx.recv_timeout(LIMIT) {
        Ok((Ok(result), took)) => Some((result, took)),
        Ok((Err(_), _)) => panic!("{name}: opening panicked"),
        Err(RecvTimeoutError::Timeout) => panic!("{name}: still opening after {LIMIT:?}"),
        Err(RecvTimeoutError::Disconnected) => panic!("{name}: the opening thread died"),
    }
}

fn opened(name: &str) -> Option<Opened> {
    let (result, _) = open(name, OpenOptions::default())?;
    Some(result.unwrap_or_else(|e| panic!("{name}: should open, but: {e}")))
}

fn refusal(name: &str) -> Option<WorkspaceError> {
    let (result, _) = open(name, OpenOptions::default())?;
    Some(result.err().unwrap_or_else(|| panic!("{name}: should be refused")))
}

/// Heavy files only run in release builds.
fn release_only(name: &str) -> bool {
    let skip = cfg!(debug_assertions);
    if skip {
        eprintln!("skipped {name}: too slow for a debug build");
    }
    skip
}

#[test]
fn a_good_pdf_opens() {
    let Some(o) = opened("good.pdf") else { return };
    assert_eq!(o.session.figure.page_count, 1);
    assert!(!o.session.figure.truncated);
}

#[test]
fn every_page_of_a_long_document_is_reachable() {
    let Some(o) = opened("pages_120.pdf") else { return };
    assert_eq!(o.session.figure.page_count, 120);
    let (last, _) = open("pages_120.pdf", OpenOptions { page: 119, ..Default::default() }).unwrap();
    assert_eq!(last.unwrap().session.figure.page, 119);
    let (past, _) = open("pages_120.pdf", OpenOptions { page: 120, ..Default::default() }).unwrap();
    assert!(matches!(past.err(), Some(WorkspaceError::Import(ImportError::NoSuchPage { page: 120, count: 120 }))));
}

#[test]
fn thousands_of_blank_pages_open_at_once() {
    let Some((result, took)) = open("pages_5000_blank.pdf", OpenOptions { page: 4999, ..Default::default() }) else {
        return;
    };
    let o = result.unwrap();
    assert_eq!((o.session.figure.page, o.session.figure.page_count), (4999, 5000));
    assert!(took < Duration::from_secs(10), "took {took:?}");
}

#[test]
fn a_password_protected_file_asks_for_its_password() {
    for name in ["encrypted_open_password.pdf", "encrypted_rc4.pdf"] {
        let Some(e) = refusal(name) else { return };
        assert!(matches!(e, WorkspaceError::Import(ImportError::PasswordRequired)), "{name}: {e}");

        let wrong = OpenOptions { password: Some("not it".into()), ..Default::default() };
        let (result, _) = open(name, wrong).unwrap();
        assert!(matches!(result.err(), Some(WorkspaceError::Import(ImportError::WrongPassword))), "{name}");

        let right = OpenOptions { password: Some("secret".into()), ..Default::default() };
        let (result, _) = open(name, right).unwrap();
        let o = result.unwrap_or_else(|e| panic!("{name} with its password: {e}"));
        assert!(!o.session.figure.primitives.is_empty(), "{name}");
    }
}

#[test]
fn a_file_with_only_an_owner_password_opens_without_one() {
    let Some(o) = opened("encrypted_owner_only.pdf") else { return };
    assert!(!o.session.figure.primitives.is_empty());
}

#[test]
fn files_that_are_not_pdfs_are_refused_with_a_reason() {
    for name in ["empty.pdf", "header_only.pdf", "random_after_header.pdf", "text_named_pdf.pdf", "bit_flips.pdf"] {
        let Some(e) = refusal(name) else { return };
        assert!(!e.to_string().is_empty(), "{name}");
    }
}

#[test]
fn files_with_nothing_to_show_are_refused() {
    for name in ["zero_pages.pdf", "pagetree_loop.pdf", "mediabox_huge.pdf"] {
        let Some(e) = refusal(name) else { return };
        assert!(!e.to_string().is_empty(), "{name}");
    }
}

#[test]
fn damaged_files_open_if_they_can_be_recovered_and_are_refused_if_not() {
    for name in [
        "truncated_half.pdf",
        "truncated_tail.pdf",
        "no_xref.pdf",
        "zeroed_block.pdf",
        "garbage_xref_offsets.pdf",
        "pdf_in_the_middle.pdf",
    ] {
        let Some((result, took)) = open(name, OpenOptions::default()) else { return };
        assert!(took < Duration::from_secs(10), "{name} took {took:?}");
        if let Err(e) = result {
            assert!(!e.to_string().is_empty(), "{name}");
        }
    }
}

#[test]
fn odd_page_geometry_does_not_break_the_figure() {
    for name in [
        "page_max_14400.pdf",
        "page_tiny_1pt.pdf",
        "page_wide_strip.pdf",
        "rotated_90.pdf",
        "cropped.pdf",
        "mediabox_zero.pdf",
        "mediabox_inverted.pdf",
        "absurd_numbers.pdf",
        "form_recursion.pdf",
    ] {
        let Some((result, _)) = open(name, OpenOptions::default()) else { return };
        if let Ok(o) = result {
            let f = &o.session.figure;
            assert!(f.width.is_finite() && f.width > 0.0 && f.height.is_finite() && f.height > 0.0, "{name}");
            for p in &f.primitives {
                assert!(
                    p.points.iter().all(|q| q.x.is_finite() && q.y.is_finite()) || name == "absurd_numbers.pdf",
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn a_rotated_page_swaps_its_sides() {
    let (Some(plain), Some(rotated)) = (opened("good.pdf"), opened("rotated_90.pdf")) else { return };
    let (a, b) = (&plain.session.figure, &rotated.session.figure);
    assert!((a.width - b.height).abs() < 0.5 && (a.height - b.width).abs() < 0.5);
}

#[test]
fn a_curve_of_two_million_points_opens_whole() {
    if release_only("curve_2m_points.pdf") {
        return;
    }
    let Some((result, took)) = open("curve_2m_points.pdf", OpenOptions::default()) else { return };
    let o = result.unwrap();
    assert!(!o.session.figure.truncated);
    assert!(o.session.figure.primitives.iter().map(|p| p.points.len()).sum::<usize>() >= 2_000_000);
    assert!(took < Duration::from_secs(20), "took {took:?}");
}

#[test]
fn a_hundred_thousand_markers_open_whole() {
    if release_only("scatter_100k_paths.pdf") {
        return;
    }
    let Some(o) = opened("scatter_100k_paths.pdf") else { return };
    assert!(o.session.figure.primitives.len() >= 100_000);
    assert!(o.picture_source().is_some() == !o.session.figure.truncated);
}

#[test]
fn a_decompression_bomb_is_cut_short_instead_of_exhausting_memory() {
    if release_only("flate_bomb.pdf") {
        return;
    }
    let Some(o) = opened("flate_bomb.pdf") else { return };
    let f = &o.session.figure;
    assert!(f.truncated, "fifteen million paths cannot all be kept");
    assert!(f.primitives.len() <= plotextract_import::MAX_PRIMITIVES);
    assert!(f.primitives.iter().map(|p| p.points.len()).sum::<usize>() <= plotextract_import::MAX_POINTS);
    assert!(o.picture_source().is_none(), "a file that heavy is not drawn");
}

#[test]
fn a_content_stream_of_a_million_nested_states_is_survived() {
    if release_only("q_nesting_1m.pdf") {
        return;
    }
    let Some((result, took)) = open("q_nesting_1m.pdf", OpenOptions::default()) else { return };
    let _ = result;
    assert!(took < Duration::from_secs(30), "took {took:?}");
}

#[test]
fn a_postscript_program_that_never_ends_is_stopped() {
    if release_only("ps_infinite_loop.ps") {
        return;
    }
    for name in ["ps_infinite_loop.ps", "ps_deep_recursion.ps"] {
        let Some((result, took)) = open(name, OpenOptions::default()) else { return };
        let e = result.err().unwrap_or_else(|| panic!("{name} cannot have a page"));
        assert!(e.to_string().contains("stopped"), "{name}: {e}");
        assert!(took < Duration::from_secs(40), "{name} took {took:?}");
    }
}

#[test]
fn broken_postscript_is_refused_with_a_reason() {
    for name in ["ps_garbage.ps", "ps_memory_hog.ps", "ps_no_showpage.ps"] {
        let Some(e) = refusal(name) else { return };
        assert!(!e.to_string().is_empty(), "{name}");
    }
}

#[test]
fn broken_and_hostile_svgs_are_refused_or_tamed() {
    for name in ["svg_malformed.svg", "svg_xml_bomb.svg"] {
        let Some(e) = refusal(name) else { return };
        assert!(!e.to_string().is_empty(), "{name}");
    }
    for name in ["svg_empty.svg", "svg_huge_viewbox.svg"] {
        let Some((result, _)) = open(name, OpenOptions::default()) else { return };
        let _ = result; // opening or refusing are both fine; hanging or crashing is not
    }
    if release_only("svg_100k_paths.svg") {
        return;
    }
    let Some(o) = opened("svg_100k_paths.svg") else { return };
    assert!(o.session.figure.primitives.len() >= 100_000, "each path is stroked and, by default, filled");
}
