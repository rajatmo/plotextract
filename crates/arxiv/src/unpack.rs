//! A paper's source as arXiv serves it, unpacked: a gzipped tar of the TeX files and figures (most papers), a single
//! gzipped TeX file (a paper with no figure files), or the PDF alone (a paper submitted as a PDF).
//!
//! The source is someone else's file: only figures are written, each under the folder given, with names that cannot leave
//! it, and no more is read than [`MAX_UNPACKED`] whatever the archive claims.

use std::cell::Cell;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use flate2::read::GzDecoder;

use crate::tex::{self, Document};
use crate::{ArxivError, Figure, Kind};

/// The most bytes read out of a compressed source.
pub const MAX_UNPACKED: u64 = 4 << 30;
/// The most files an archive may hold.
const MAX_ENTRIES: usize = 100_000;
/// The largest TeX file read for the figures it draws.
const MAX_TEX: u64 = 16 << 20;

/// What a source held.
pub struct Unpacked {
    pub kind: Kind,
    pub figures: Vec<Figure>,
}

/// Unpack `bytes`, the source of paper `name`, into folder `into`: the files `is_figure` accepts (it is given the name
/// within the source, with `/` between names), in the order the TeX files draw them, then the rest by name. A PDF
/// alone is written as `name.pdf`.
pub fn unpack(bytes: &[u8], into: &Path, name: &str, is_figure: &dyn Fn(&str) -> bool) -> Result<Unpacked, ArxivError> {
    let over = Cell::new(false);
    let result = if bytes.starts_with(b"%PDF") {
        pdf(bytes, into, name)
    } else if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut reader = Limited { inner: GzDecoder::new(bytes), left: MAX_UNPACKED, over: &over };
        let head = read_head(&mut reader).map_err(|e| damaged(&e, &over))?;
        let all = io::Cursor::new(head.clone()).chain(reader);
        if is_tar(&head) {
            archive(all, into, is_figure, &over)
        } else if head.starts_with(b"%PDF") {
            pdf(all, into, name)
        } else {
            Ok(Unpacked { kind: Kind::Tex, figures: Vec::new() })
        }
    } else if is_tar(bytes) {
        archive(bytes, into, is_figure, &over)
    } else {
        return Err(ArxivError::NotAnArchive);
    };
    result.map_err(|e| if over.get() { ArxivError::TooLarge(MAX_UNPACKED) } else { e })
}

/// A reader that gives no more than `left` bytes, and says so when there were more.
struct Limited<'a, R> {
    inner: R,
    left: u64,
    over: &'a Cell<bool>,
}

impl<R: Read> Read for Limited<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n as u64 > self.left {
            self.over.set(true);
            return Err(io::Error::other("the source unpacks to too much"));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// The first bytes of `reader`: enough to tell a tar from the rest.
fn read_head(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(512);
    reader.take(512).read_to_end(&mut head)?;
    Ok(head)
}

fn is_tar(head: &[u8]) -> bool {
    head.get(257..262) == Some(b"ustar")
}

fn damaged(e: &io::Error, over: &Cell<bool>) -> ArxivError {
    if over.get() { ArxivError::TooLarge(MAX_UNPACKED) } else { ArxivError::Damaged(e.to_string()) }
}

fn pdf(reader: impl Read, into: &Path, name: &str) -> Result<Unpacked, ArxivError> {
    let file = format!("{name}.pdf");
    let path = into.join(&file);
    let bytes = write(&path, reader)?;
    Ok(Unpacked { kind: Kind::Pdf, figures: vec![Figure { path, name: file, bytes, cited: false }] })
}

fn archive(
    reader: impl Read,
    into: &Path,
    is_figure: &dyn Fn(&str) -> bool,
    over: &Cell<bool>,
) -> Result<Unpacked, ArxivError> {
    let mut tar = tar::Archive::new(reader);
    let mut figures = Vec::new();
    let mut documents = Vec::new();
    for (n, entry) in tar.entries().map_err(|e| damaged(&e, over))?.enumerate() {
        if n >= MAX_ENTRIES {
            return Err(ArxivError::Damaged(format!("it holds more than {MAX_ENTRIES} files")));
        }
        let mut entry = entry.map_err(|e| damaged(&e, over))?;
        // links and folders are not followed or made: only the files' contents are taken
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let Some(name) = entry.path().ok().and_then(|p| clean(&p)) else { continue };
        if is_figure(&name) {
            let path = name.split('/').fold(into.to_path_buf(), |p, part| p.join(part));
            let bytes = write(&path, &mut entry).map_err(|e| match e {
                ArxivError::Damaged(_) if over.get() => ArxivError::TooLarge(MAX_UNPACKED),
                e => e,
            })?;
            figures.push(Figure { path, name, bytes, cited: false });
        } else if extension(&name).eq_ignore_ascii_case("tex") && entry.size() <= MAX_TEX {
            let mut text = Vec::new();
            entry.read_to_end(&mut text).map_err(|e| damaged(&e, over))?;
            documents.push(Document { path: name.to_string(), text: String::from_utf8_lossy(&text).into_owned() });
        }
    }
    order(&mut figures, &tex::references(&documents));
    Ok(Unpacked { kind: Kind::Archive, figures })
}

/// Put the figures the TeX files draw first, in the order they are drawn, and mark them.
fn order(figures: &mut [Figure], references: &[Vec<String>]) {
    let rank = |f: &Figure| {
        let stem = without_extension(&f.name);
        references.iter().position(|places| places.iter().any(|p| *p == f.name || p == stem))
    };
    for f in figures.iter_mut() {
        f.cited = rank(f).is_some();
    }
    figures.sort_by_cached_key(|f| (rank(f).unwrap_or(usize::MAX), f.name.clone()));
}

/// The name of an archive's file as a path inside the folder it is unpacked in, with `/` between names; `None` for a
/// name that would leave the folder or that Windows cannot hold.
fn clean(path: &Path) -> Option<String> {
    const RESERVED: [&str; 22] = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1",
        "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
    ];
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str()?;
                let stem = part.split('.').next().unwrap_or_default().to_ascii_lowercase();
                if part.contains([':', '*', '?', '"', '<', '>', '|', '\\'])
                    || part.ends_with(['.', ' '])
                    || part.chars().any(char::is_control)
                    || RESERVED.contains(&stem.as_str())
                {
                    return None;
                }
                parts.push(part);
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn extension(name: &str) -> &str {
    let file = name.rsplit('/').next().unwrap_or(name);
    file.rsplit_once('.').map_or("", |(_, ext)| ext)
}

fn without_extension(name: &str) -> &str {
    let ext = extension(name);
    if ext.is_empty() { name } else { &name[..name.len() - ext.len() - 1] }
}

/// Write what `reader` gives to `path`, making its folders. How many bytes were written.
fn write(path: &PathBuf, mut reader: impl Read) -> Result<u64, ArxivError> {
    let cannot = |source| ArxivError::Write { path: path.clone(), source };
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).map_err(cannot)?;
    }
    let mut file = fs::File::create(path).map_err(cannot)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ArxivError::Damaged(e.to_string())),
        };
        file.write_all(&buf[..n]).map_err(cannot)?;
        total += n as u64;
    }
    file.flush().map_err(cannot)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use flate2::Compression;
    use flate2::write::GzEncoder;

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("plotextract-arxiv-{}-{name}", std::process::id()));
            fs::remove_dir_all(&dir).ok();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).ok();
        }
    }

    fn figure_file(name: &str) -> bool {
        ["png", "pdf", "eps", "jpg"].contains(&extension(name).to_ascii_lowercase().as_str())
    }

    /// A tar of `files`, gzipped. A name is written into the header as it is, so names a well-behaved tool refuses can
    /// be tried too.
    fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = tar::Header::new_ustar();
            let bytes = name.as_bytes();
            header.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_entry_type(tar::EntryType::Regular);
            header.set_cksum();
            builder.append(&header, *data).unwrap();
        }
        gzip(&builder.into_inner().unwrap())
    }

    fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(bytes).unwrap();
        gz.finish().unwrap()
    }

    #[test]
    fn the_figures_are_unpacked_in_the_order_the_paper_draws_them() {
        let dir = Scratch::new("order");
        let tex = br"\documentclass{article}\graphicspath{{figs/}}
            \includegraphics{zeta}\includegraphics[width=3cm]{./figs/alpha.pdf}";
        let source = tar_gz(&[
            ("figs/alpha.pdf", b"%PDF-1.4 alpha"),
            ("figs/beta.png", b"png"),
            ("main.tex", tex),
            ("./figs/zeta.eps", b"%!PS"),
            ("notes.txt", b"not a figure"),
        ]);
        let out = unpack(&source, &dir.0, "1234.56789v1", &figure_file).unwrap();
        assert_eq!(out.kind, Kind::Archive);
        let names: Vec<(&str, bool)> = out.figures.iter().map(|f| (f.name.as_str(), f.cited)).collect();
        assert_eq!(names, [("figs/zeta.eps", true), ("figs/alpha.pdf", true), ("figs/beta.png", false)]);
        let alpha = &out.figures[1];
        assert_eq!(alpha.path, dir.0.join("figs").join("alpha.pdf"));
        assert_eq!((fs::read(&alpha.path).unwrap(), alpha.bytes), (b"%PDF-1.4 alpha".to_vec(), 14));
        assert!(!dir.0.join("notes.txt").exists() && !dir.0.join("main.tex").exists(), "only figures are written");
    }

    #[test]
    fn names_that_would_leave_the_folder_are_skipped() {
        let dir = Scratch::new("escape");
        let source = tar_gz(&[
            ("../evil.png", b"x"),
            ("/abs.png", b"x"),
            ("figs/../../evil2.png", b"x"),
            ("c:evil3.png", b"x"),
            ("figs/fig.png:stream", b"x"),
            ("con.png", b"x"),
            ("ok.png", b"fine"),
        ]);
        let out = unpack(&source, &dir.0, "x", &figure_file).unwrap();
        let names: Vec<&str> = out.figures.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["ok.png"]);
        let parent = dir.0.parent().unwrap();
        assert!(!parent.join("evil.png").exists() && !parent.join("evil2.png").exists());
    }

    #[test]
    fn a_single_tex_file_has_no_figures_and_a_pdf_is_the_paper() {
        let dir = Scratch::new("single");
        let tex = unpack(&gzip(br"\documentclass{article}"), &dir.0, "x", &figure_file).unwrap();
        assert!(tex.kind == Kind::Tex && tex.figures.is_empty());
        let pdf = unpack(b"%PDF-1.7 paper", &dir.0, "1706.03762v7", &figure_file).unwrap();
        assert_eq!(pdf.kind, Kind::Pdf);
        assert_eq!(pdf.figures[0].name, "1706.03762v7.pdf");
        assert_eq!(fs::read(&pdf.figures[0].path).unwrap(), b"%PDF-1.7 paper");
        assert!(matches!(
            unpack(b"<html>rate limited</html>", &dir.0, "x", &figure_file),
            Err(ArxivError::NotAnArchive)
        ));
        assert!(matches!(unpack(&[0x1f, 0x8b, 8, 0, 1, 2, 3], &dir.0, "x", &figure_file), Err(ArxivError::Damaged(_))));
    }

    #[test]
    fn reading_stops_at_the_limit() {
        let over = Cell::new(false);
        let mut reader = Limited { inner: io::repeat(0).take(100), left: 64, over: &over };
        let mut sink = Vec::new();
        assert!(reader.read_to_end(&mut sink).is_err());
        assert!(over.get() && sink.len() <= 64);
    }
}
