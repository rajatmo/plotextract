# Architecture

plotextract is a Rust workspace of small crates. The rule that shapes it: a layer knows the layer below, never the one
above, and nothing below the shell knows about a window.

```
shell        plotextract            egui panels, canvas, dialogs, threads; the Model that decides what happens
service      plotextract-workspace  format registry; open, projects, export
importers    plotextract-pdf / -svg / -ps  (+ plotextract-import: what they share)
domain       plotextract-core       figure, calibration, session, snap index, ports
```

## Domain (`plotextract-core`)

- **`Figure`**: one page of a file. Width and height in the file's own `Unit` (points for PDF and PostScript, CSS pixels
  for SVG), origin top left, y down; its `Primitive`s; which `page` it is of how many (`page_count`); and `truncated`
  when the importer stopped at its limits.
- **`Primitive`**: one stroked or filled path as vertices plus the places where it breaks into pieces. Strokes come
  first, each kind numbered from 0. Importers apply the same flattening rules (PyMuPDF compatible: a rectangle `re` and a
  quad differ in corner order), so the numbers are reproducible.
- **`Session`**: works on a figure. It composes three things that change for different reasons:
  - `Document`: calibration, series, active series, hidden curves. Everything saved in a project and everything undo
    covers. Changed only by an `Edit`; hidden curves are a set, so a snapshot stays small for a figure of a million
    curves.
  - `History<Document>`: snapshots, 200 deep.
  - `ViewState`: layers and the selection. Not saved, not undone.
  and a `SnapIndex`: vertices sorted by x and a bounding box per primitive, both in single precision (about 12 bytes per
  vertex) and used only to choose what to look at; distances come from the real coordinates.
- **Ports**: `Importer` (`sniff`, `extensions`, `import`, `picture`) and `PictureSource` (`render(region, scale)`).
  `OpenOptions` carries the page and password; `ImportError` says `PasswordRequired`, `WrongPassword`, `NoSuchPage` or a
  message. No wording for a user lives here.

## Importers

Each walks its format's drawing model and feeds every painted path to `plotextract_import::PrimitiveBuilder`:

- `flatten_path` turns a path into vertices with the shared Bezier sampling and break handling.
- `PrimitiveBuilder` numbers the paths and **enforces the limits**: 1,000,000 paths, 16,000,000 vertices. Past them it
  drops the rest, remembers it (`truncated`), and `has_room()` lets importers skip flattening paths that would be
  dropped.

Format specifics:

| | |
|---|---|
| PDF (hayro) | implements `hayro_interpret::Device`; clipping is ignored on purpose; `Pdf::new_with_password`; a wrong or missing password is reported as such; the file's bytes are shared (`Arc`), not copied |
| SVG (usvg, resvg) | the resolved tree's paths; a document with several pages is not a thing, so page 0 only |
| PostScript (stet) | the display list of the page; the interpreter runs under a 20 second limit enforced by setting stet's interrupt flag from a watchdog thread |

A `PictureSource` is created on the thread that uses it (hayro's and stet's renderers are not `Send`) by
`Importer::picture`, from the same bytes and options as the figure.

## Application service (`plotextract-workspace`)

- `Registry`: finds the importer by content (the first kilobyte), then by extension.
- `Source`: the bytes, the importer and the `OpenOptions` of a file. Cheap to clone, `Send`; `with_page` and
  `with_password` give variants; `picture()` makes a `PictureSource`.
- `Workspace`: `read` (file to `Source`), `load` (interpret a `Source` into an `Opened`: a `Session` plus its `Source`),
  `open` (both), projects (`save_project`, `read_project`) and CSV export. It refuses a page without a finite positive
  size.
- `Opened::picture_source()` is `None` for a truncated figure: a file that heavy is not drawn.

## Shell (`plotextract`)

```
 input ──► Intent ──► Model::dispatch ──► Effect ──► shell performs it
                          │                              │
                          ▼                              ▼
                       Session                dialogs, clipboard, title,
                                              background threads
```

- `Model` owns the `Session` (and the `Source`, the sessions of other pages that have work on them, the status message,
  the password prompt). It has no egui in it, so the tests drive it with intents. `text.rs` is the one place that turns
  facts (errors, units, calibration status) into sentences.
- **Opening is asynchronous.** `Intent::Open` makes a `LoadJob`; `dispatch` returns it as `Effect::Load`. The shell runs
  `job.run()` on a thread and later gives the `Loaded` answer to `Model::finish_load`. Jobs are numbered; the answer to
  any job but the newest is dropped, so quickly opening two files shows the second. A password error does not fail: it
  becomes a `PasswordPrompt` the shell shows as a window, and the typed password starts a new job. Project loading reads
  the (small) project file at once and starts a job only if its figure is not the open one.
- **Pages.** `Intent::GoToPage` reopens the same `Source` at another page without touching the file. A page the user
  worked on (`Session::has_work`) is parked and comes back instantly; opening a different file forgets the parked pages.
- **`CanvasState`** (view, hover, pictures, worker) and **`PanelState`** (value cells, table scroll) are the widgets'
  own memory. `canvas::show` and `panel::show` read the model and return `Vec<Intent>`.
- **Pictures** are drawn by one worker thread per open figure (`detail.rs`). Requests are `Base` (the whole page at up to
  6 pixels per unit and 4096 pixels) or `Detail` (the visible part at screen resolution, only when the view has been still
  for 250 ms and shows more than 1.15 times the base resolution). The worker keeps the newest request of each kind and
  skips what is stale. Bitmaps are cut into GPU-sized tiles.

## Resource limits

| Limit | Value | Where |
|---|---|---|
| Paths kept from a file | 1,000,000 | `plotextract_import::MAX_PRIMITIVES` |
| Vertices kept from a file | 16,000,000 | `MAX_POINTS` |
| Longest side of a picture | 16,384 px | `MAX_PICTURE_PIXELS` |
| Base picture | 4,096 px, 6 px per unit | `detail.rs` |
| Detail render | 12 million pixels | `detail.rs` |
| PostScript run time | 20 s | `plotextract-ps` |
| Undo depth | 200 | `session.rs` |
| Curves outlined at once | 10,000 | `canvas.rs` |

## Frame cost

The canvas does not draw what cannot be seen. A curve's screen points are produced by `screen_runs`: the stretches
outside the view are dropped and points within a pixel of the last one are skipped, so a two-million-vertex curve is a few
thousand points at any zoom. Curves smaller than a pixel are not outlined, and neither is anything when more than 10,000
are in view. Visibility uses the index's bounding boxes, not a pass over every vertex. `docs/testing.md` shows how to
measure it.
