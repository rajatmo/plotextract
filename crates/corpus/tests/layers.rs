//! The dependency rule, checked: every crate depends only on the layers inside it. A crate's manifest that names a crate
//! of its own layer or of an outer one fails here, so the architecture cannot drift without a test saying so.

use std::collections::BTreeMap;
use std::path::Path;

/// Each crate and the internal crates it may depend on (not counting dev-dependencies, which tests may take from
/// anywhere).
fn allowed() -> BTreeMap<&'static str, &'static [&'static str]> {
    const ADAPTER: &[&str] = &["plotextract-domain", "plotextract-ports"];
    const VECTOR: &[&str] = &["plotextract-domain", "plotextract-ports", "plotextract-import"];
    const IMAGE: &[&str] = &["plotextract-domain", "plotextract-ports", "plotextract-import", "plotextract-algo"];
    BTreeMap::from([
        ("domain", &[][..]),
        ("algo", &["plotextract-domain"][..]),
        ("ports", &["plotextract-domain"][..]),
        ("usecases", &["plotextract-domain", "plotextract-algo", "plotextract-ports"][..]),
        ("model", &["plotextract-domain", "plotextract-algo", "plotextract-ports", "plotextract-usecases"][..]),
        (
            "ui",
            &[
                "plotextract-domain",
                "plotextract-algo",
                "plotextract-ports",
                "plotextract-usecases",
                "plotextract-model",
            ][..],
        ),
        // the adapters implement ports; the vector formats share the path flattening, the images the decoding
        ("adapters/import", &["plotextract-domain"][..]),
        ("adapters/pdf", VECTOR),
        ("adapters/ps", VECTOR),
        ("adapters/svg", VECTOR),
        ("adapters/image", IMAGE),
        ("adapters/arxiv", ADAPTER),
        ("adapters/fs", &["plotextract-ports"][..]),
    ])
}

/// The internal crates a manifest depends on, outside its dev-dependencies.
fn internal_dependencies(manifest: &str) -> Vec<String> {
    let mut section = "";
    let mut out = Vec::new();
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if section == "[dependencies]" && line.starts_with("plotextract-") {
            out.push(line.split(['.', ' ', '=']).next().unwrap_or_default().to_string());
        }
    }
    out
}

#[test]
fn every_crate_depends_only_on_the_layers_inside_it() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the crates folder");
    for (name, may) in allowed() {
        let manifest = std::fs::read_to_string(crates.join(name).join("Cargo.toml"))
            .unwrap_or_else(|e| panic!("{name}: no manifest: {e}"));
        for dep in internal_dependencies(&manifest) {
            assert!(may.contains(&dep.as_str()), "{name} depends on {dep}, which is not inside its layer");
        }
    }
}

#[test]
fn the_core_knows_no_window_disk_network_or_thread_pool() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the crates folder");
    for name in ["domain", "algo", "ports", "usecases", "model"] {
        let manifest = std::fs::read_to_string(crates.join(name).join("Cargo.toml")).unwrap();
        let deps = manifest.split("[dev-dependencies]").next().unwrap_or_default();
        for outer in ["egui", "eframe", "rfd", "ureq", "hayro", "usvg", "resvg", "stet"] {
            assert!(!deps.lines().any(|l| l.trim().starts_with(outer)), "{name} depends on {outer}");
        }
    }
}
