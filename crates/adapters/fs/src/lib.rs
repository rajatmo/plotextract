//! The local file system behind the [`Files`] port.

use std::path::Path;

use plotextract_ports::Files;

/// The files of this computer.
pub struct LocalFiles;

impl Files for LocalFiles {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        std::fs::write(path, bytes)
    }
}
