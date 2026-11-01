//! The local file system behind the [`Files`] port.

use std::path::Path;

use plotextract_ports::Files;

/// The files of this computer.
pub struct LocalFiles;

impl Files for LocalFiles {
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    /// Write the file, making the folder it goes in if there is none.
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if let Some(folder) = path.parent().filter(|f| !f.as_os_str().is_empty()) {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(path, bytes)
    }
}
