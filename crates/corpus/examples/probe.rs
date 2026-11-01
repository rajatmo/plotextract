//! Open one file the way the app does and report what happened and how long each step took.
//!
//!     cargo run --release -p plotextract-corpus --example probe -- file.pdf
//!
//! The last line is `OK`, `REFUSED <why>` or `PANIC <why>`; `scripts`-style runners watch it, and the exit status, to
//! tell a clean refusal from a crash or a hang.

use std::path::PathBuf;
use std::time::Instant;

use kurbo::Rect;
use plotextract_domain::OpenOptions;

fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("usage: probe <file> [page]"));
    let page = std::env::args().nth(2).and_then(|p| p.parse().ok()).unwrap_or(0);
    let outcome = std::panic::catch_unwind(|| run(&path, page));
    match outcome {
        Ok(Ok(())) => println!("OK"),
        Ok(Err(why)) => println!("REFUSED {}", why.replace('\n', " ")),
        Err(panic) => {
            let why = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_default();
            println!("PANIC {why}");
            std::process::exit(101);
        }
    }
}

fn run(path: &std::path::Path, page: usize) -> Result<(), String> {
    let t = Instant::now();
    let opened = plotextract_corpus::library()
        .open(path, OpenOptions { page, ..Default::default() })
        .map_err(|e| e.to_string())?;
    let (n, points) = {
        let f = &opened.session.figure;
        (f.primitives.len(), f.primitives.iter().map(|p| p.points.len()).sum::<usize>())
    };
    println!(
        "open {:>8.0} ms  {:>7} primitives  {:>9} points{}  page {}  {:.0} x {:.0} {}",
        t.elapsed().as_secs_f64() * 1e3,
        n,
        points,
        if opened.session.figure.truncated { " (cut short)" } else { "" },
        opened.session.figure.page + 1,
        opened.session.figure.width,
        opened.session.figure.height,
        opened.session.figure.unit.symbol()
    );

    let figure = &opened.session.figure;
    let scale = std::env::var("PROBE_SCALE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or((4096.0 / figure.width.max(figure.height)).min(6.0));
    let t = Instant::now();
    let Some(mut picture) = opened.picture_source().and_then(|s| s.picture()) else {
        println!("picture: none");
        return Ok(());
    };
    let created = t.elapsed();
    let t = Instant::now();
    let bitmap = picture
        .render(Rect::new(0.0, 0.0, figure.width, figure.height), scale)
        .map_err(|e| format!("cannot draw the page: {e}"))?;
    println!(
        "picture {:>5.0} ms to create, {:>6.0} ms to draw {} x {}",
        created.as_secs_f64() * 1e3,
        t.elapsed().as_secs_f64() * 1e3,
        bitmap.width,
        bitmap.height
    );
    Ok(())
}
