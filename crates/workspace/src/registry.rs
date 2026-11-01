//! The file formats the application can read.

use std::sync::Arc;

use plotextract_core::Importer;
use plotextract_pdf::PdfImporter;
use plotextract_ps::PsImporter;
use plotextract_raster::RasterImporter;
use plotextract_svg::SvgImporter;

/// How many leading bytes a format gets to recognize its file.
const SNIFF_BYTES: usize = 1024;

/// The importers, in the order they are asked.
pub struct Registry {
    importers: Vec<Arc<dyn Importer>>,
}

impl Registry {
    pub fn new(importers: Vec<Arc<dyn Importer>>) -> Self {
        Self { importers }
    }

    /// Every format the application ships with.
    pub fn standard() -> Self {
        Self::new(vec![Arc::new(PdfImporter), Arc::new(SvgImporter), Arc::new(PsImporter), Arc::new(RasterImporter)])
    }

    /// The importer for a file: by its content, and failing that by its extension.
    pub fn detect(&self, bytes: &[u8], extension: Option<&str>) -> Option<Arc<dyn Importer>> {
        let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
        self.importers
            .iter()
            .find(|i| i.sniff(head))
            .or_else(|| {
                let ext = extension?.to_ascii_lowercase();
                self.importers.iter().find(|i| i.extensions().contains(&ext.as_str()))
            })
            .cloned()
    }

    /// All file extensions that can be opened, for a file dialog.
    pub fn extensions(&self) -> Vec<&'static str> {
        self.importers.iter().flat_map(|i| i.extensions().iter().copied()).collect()
    }

    /// Whether a file of this name has an extension that can be opened.
    pub fn opens_name(&self, name: &str) -> bool {
        let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
        let Some((_, ext)) = file.rsplit_once('.') else { return false };
        let ext = ext.to_ascii_lowercase();
        self.importers.iter().any(|i| i.extensions().contains(&ext.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(r: &Registry, bytes: &[u8], ext: Option<&str>) -> Option<&'static str> {
        r.detect(bytes, ext).map(|i| i.name())
    }

    #[test]
    fn content_decides_before_the_extension() {
        let r = Registry::standard();
        assert_eq!(name(&r, b"%PDF-1.7\n", Some("svg")), Some("PDF"));
        assert_eq!(name(&r, b"<?xml version='1.0'?><svg>", None), Some("SVG"));
        assert_eq!(name(&r, b"%!PS-Adobe-3.0 EPSF-3.0", Some("txt")), Some("PostScript"));
        assert_eq!(name(&r, b"\x89PNG\r\n\x1a\n", Some("svg")), Some("Image"));
        assert_eq!(name(&r, &[0xFF, 0xD8, 0xFF, 0xE0], None), Some("Image"));
    }

    #[test]
    fn the_extension_is_the_fallback_for_files_whose_start_is_unrecognizable() {
        let r = Registry::standard();
        let long_prolog = format!("<!--{}--><svg>", "x".repeat(2000));
        assert_eq!(name(&r, long_prolog.as_bytes(), Some("SVG")), Some("SVG"));
        assert_eq!(name(&r, b"plain text", Some("txt")), None);
        assert_eq!(name(&r, b"plain text", None), None);
    }

    #[test]
    fn lists_every_extension() {
        let all = Registry::standard().extensions();
        for ext in ["pdf", "svg", "ps", "eps", "png", "jpg", "jpeg", "tif", "bmp", "gif", "webp"] {
            assert!(all.contains(&ext), "{ext}");
        }
    }

    #[test]
    fn names_are_known_by_their_extension() {
        let r = Registry::standard();
        assert!(r.opens_name("figs/Loss.PDF") && r.opens_name("a.b/c.eps") && r.opens_name("x.png"));
        assert!(!r.opens_name("main.tex") && !r.opens_name("figs.png/README") && !r.opens_name("png"));
    }
}
