# Changelog

## 0.2.0

Robustness, speed and multi-page documents.

### Added
- **Pages.** Any page of a PDF (or PostScript program) can be opened: a Page control at the top of the side panel, Page Up
  and Page Down, and View > Previous page / Next page. Work on a page is kept while other pages are shown. Projects
  remember their page (projects from 0.1.0 still load, as page 1).
- **Passwords.** A password protected PDF asks for its password in a window; a wrong password is said to be wrong. A file
  with only an owner password opens as before.
- **Background opening.** Reading and interpreting a file no longer freezes the window; the status bar shows a spinner. When
  two files are opened in quick succession the second wins.
- **Limits.** At most 1,000,000 paths and 16,000,000 vertices are kept from a file. Past that the rest is dropped, the
  status bar and the Curves card say so, and no picture is drawn (a 200 MB decompression bomb used 6.2 GB of memory before;
  it now uses 0.4 GB and opens in 2.6 s).
- **Time limit for PostScript.** A program still running after 20 seconds is stopped (an endless loop used to hang the
  importer forever).
- **Clear messages** for damaged files ("it is damaged or not a PDF"), files without pages, missing pages and pages without
  size.
- A stress corpus (`testdata/make_stress.py`, 45 files), `crates/corpus/tests/stress.rs`, the `probe` example and
  `testdata/run_stress.py`; documentation in `docs/testing.md` and `docs/architecture.md`.

### Changed
- **Level of detail on the canvas.** Curve outlines are thinned to one point per pixel and cut to the view, curves smaller
  than a pixel are not outlined, and with more than 10,000 curves in view none is (the picture still shows them). One
  frame with a two-million-vertex curve went from 112 ms to 23 ms fitted and from 111 ms to 3 ms zoomed in; with 100,000
  markers from 183 ms to 0.1 ms.
- **Memory.** The snap index holds about 12 bytes per vertex instead of about 64 (single precision keys and bounding
  boxes, no copy of the segments); hidden curves are a set, so each of the 200 undo snapshots no longer carries a flag per
  curve; the file's bytes are shared between the importer and the picture worker instead of copied.
- The curve table and the canvas no longer build a flag per curve every frame.
- `Importer::import` and `picture` take the bytes as `Arc<Vec<u8>>` and `OpenOptions` (page, password); `ImportError` is an
  enum; `Figure` has `page`, `page_count` and `truncated`; `plotextract-workspace` has `Source` and `Workspace::{read, load,
  open}`.

### Fixed
- A PostScript file that never ends no longer hangs opening it.
- Opening a file with a very large number of paths no longer allocates without bound.

## 0.1.0

First release: PDF, SVG and EPS/PostScript import, two-tick calibration (linear and log), series of curves, CSV export,
projects, undo, sharp zoom, About window; Windows, Linux and macOS packages.
