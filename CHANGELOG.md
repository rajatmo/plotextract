# Changelog

## Unreleased

### Added
- **Suggest ticks, on step 3.** Two small networks trained for it (in the plotML project, on synthetic matplotlib
  plots) find an image's ticks and read their labels: a tick finder (0.86 M parameters) marks where each tick meets its
  axis and where its label is, and a reader (0.16 M) reads the label (`-0.25`, `10^-24`). On each axis the values must
  lie on a line against position, or their logarithms must: the line through the most ticks decides the scale and drops
  misreadings, and the two outermost ticks on it, placed by a least-squares fit, are set with their values. An axis
  whose labels do not agree is left as it was. It runs offline in pure Rust (tract), on the crop with the painted areas
  as paper, as a job: about 0.1 s an image. On held-out synthetic plots 98% are calibrated within 0.5% of the span; on
  nine real arXiv figures every axis it set was right, and the axes it left were those with no numbers, or the x axis of
  a heatmap. The networks are behind a port (`TickModels`), in the `plotextract-ticks` adapter, with the ONNX files.
- **`corpus/tests/ticks.rs`** holds the Rust pipeline to plotML's on 60 plots (same axes, ticks within 0.15 px), and to
  their truth; the `suggest` example runs it on any images and draws what it sets.

### Changed
- **Step 3's choices can be undone.** The ticks, their values and the log scales are kept in a `History` like the other
  steps' edits: Ctrl+Z takes back a tick chosen or deleted, a value typed (all its keystrokes at once), a scale, or a
  suggestion.
- A debug build optimizes the tract crates, so the networks run at a usable speed there too.

## 1.1.0

The tracing wizard's last two steps are done by hand: the pieces are grouped by how they look and the groups fixed, then
a curve's pieces are linked in order and its points follow its strokes. Projects keep an image's tracing, every PDF opens
through a list of its images and pages, and a vector figure is laid out as an image is.

### Added
- **Projects keep the tracing of an image.** *Save project* while an image is traced saves what was done in the wizard:
  the crop and the exclude shapes, what each colour is, the ticks and their values, the settings, the changes to the
  groups and the links, and the step shown. *Load project* opens the image afresh and brings it all back on that step:
  the colours, lines and pieces are found again and the changes and links made again on them. A project from before
  loads as it did.
- **Every PDF opened is listed first: a page or an image of it is chosen.** Opening a PDF (*File > Open*, a drop, the
  command line, a figure from arXiv) lists every raster image the pages draw (each once, the tiny ones left out), then
  every page, each with a picture, read and drawn on threads, in folding sections of the Import from PDF window. A
  protected PDF asks for its password first, and what it lists opens without asking again. A page opens with its paths
  to pick. An image is taken out at the resolution it is stored at (laid on white where it is transparent), kept in the
  cache folder and opened to trace. A project names its page and opens it directly. The importers have a port for it
  (`Importer::is_document`, `contents`, `image`), tested on protected (AES-256, RC4, owner-only) and damaged PDFs.
- **`real_eval` scores real figures**, with no hand work or from a saved project, against the paths of their PDFs;
  `testdata/arxiv_worst` keeps the eight hardest of 40 arXiv figures, with PNGs of them.

### Changed
- **Step 4, Shapes, groups the pieces by how they look, and the groups are fixed by hand.** The colours tagged curve are
  broken into pieces; for a colour that is the axes too, the sides of the frame are cut out first. The pieces are
  measured and grouped, and nothing is said of what a piece is: each long stroke is a group of its own (a line), pieces
  that look alike however they are turned, four or more, are one group (a repeated shape: dots, dashes, markers), and
  the rest are left in no group. A group is named by how it looks ("Dots", "Dashes 2", "Line"). Every piece is drawn in
  its group's colour over the darkened image with a dark rim, the leftovers grey, and a piece too small to see is ringed,
  so a one-pixel piece is found at any zoom. Hold H to see the image alone. The groups are fixed with a selection and two
  changes: a click selects a piece's group (Shift adds or takes away a group, Ctrl one piece), a drag around pieces
  selects them, a group's row selects it; *Make one group* (G) makes the selected pieces one group and nothing else, and
  *Remove* (Delete) takes them out of their groups. *Select similar* (S) and *Zoom to them* (Z) help. Each change is an
  edit naming its pieces by a pixel of each, made again when the pieces are found again, and undone with Ctrl+Z. The
  links, paths, moved pieces and oddest-piece review of 1.0.0's step 4 are gone.
- **Step 5 is Link: a curve's pieces are linked in order by hand**, in place of the series 1.0.0's Curves step
  proposed. As with a pen drawing a polyline: a click on a piece starts a link, each next click (or the pieces a drag
  passes through) adds to it; a double-click or Esc ends it, Backspace takes its last piece off, and a click on an end of
  a link goes on with it. A group of one piece links itself. The panel lists the links in the order their points are
  written: each can be moved up or down, turned round or deleted, and every change undone. The points follow the
  strokes: along each line or dash a point every other pixel, in the direction the link runs, and one point for each dot
  or marker. Each link is drawn along its points, with its number and an arrow. *Copy* and *Save CSV* write them in plot
  values, through the same export as the vector figures'.
- **A vector figure is laid out as an image is.** The panels are the wizard's two, at the same widths. On the left are
  the layers (picture, strokes, fills), each with an eye and a count, and every curve as a row like a mask's: an eye
  that hides it, a picture of its kind (ringed green in the active series), its name and points. A click selects the
  curve and the pointer on a row outlines it on the page. The list draws only the rows in view, so a million curves
  scroll smoothly. On the right are a card saying what the figure is, then calibration, series and the selection. The
  side panel's layer buttons and curve table are gone, and so is the `egui_extras` dependency.
- **No more moving between the pages of an open file.** The page is chosen when a PDF is opened, so the Page control,
  Page Up / Page Down, View > Previous page / Next page and the pages kept with their work are gone, and `Figure` no
  longer has `page_count` (`Importer::contents` lists the pages). A PostScript program opens at its first page.
- **The workspace is in layers, ports and adapters style, and every dependency points inward.** The domain
  (`plotextract-domain`) depends on no crate of ours; the algorithms (`plotextract-algo`) and the ports
  (`plotextract-ports`: `Importer`, `PictureSource`, `PaperSource`, `Files`) on the domain; the use cases
  (`plotextract-usecases`: the vector session, the library of files, the tracing wizard) on those; the model
  (`plotextract-model`) on the use cases; the window (`plotextract-ui`) on the model. The file formats, arXiv and the
  disk are adapters under `crates/adapters`, and only the composition root (`plotextract`) knows them all. A test checks
  the rule on every manifest. One `History` type keeps every undo, and the wizard routes Ctrl+Z through a `WizardStep`
  trait. `docs/plan.md` is gone: `docs/architecture.md` describes what is.
- **The README and its demo are redone** for this version: opening a figure (the PDF list, arXiv), both workflows,
  projects; the demo shows a PDF figure from arXiv calibrated and picked, then an image through the five steps.

## 1.0.0

Images are traced into data: a five-step wizard (crop and mask, colours, plot area, shapes, curves) that finds what the
curves are drawn with, lets every mistake be fixed by hand, and writes the series in plot values; and figures are
imported straight from arXiv.

### Added
- **Step 4 is where the shapes found are checked and fixed by hand.** Each shape is listed with a picture of its most
  typical piece, all at one scale, dots and dashes first and the most pieces first, lone pieces folded into one row, under
  a summary of what the next step will propose. A click on the image selects a shape, moves pieces (a click puts a piece
  in the selected shape or takes it out, a drag around pieces moves them all, Alt takes them out), or, with Links, cuts the
  paths of dots and dashes where a link is clicked and joins two ends; the links are drawn and the ends ringed. The
  selected shape shows its two oddest pieces and is looked through from the oddest, each ringed and brought into view;
  it can be zoomed to, shown alone and merged into another; *New shape* makes an empty one. Moving one piece offers the
  pieces that look more like it than like the rest. Hovering a piece says what it is and why (linked on a path of so many,
  one of so many copies, close to other shapes, moved by hand). How alike copies must be is a setting under Advanced.
  Every change is kept as an edit naming its pieces by a pixel, and made again when the pieces are found again. Keys: Up
  and Down, 1 to 6, S, Z, N and Shift+N.
- **The Shapes step of the tracing wizard (step 4): what the curves are drawn with.** The colours tagged curve are broken
  into pieces, the connected strokes of each colour (`plotextract_raster::curves`, with the connected components of
  `imageproc`); for a colour that is the axes too, the sides of the frame are cut out first, so a curve that starts on an
  axis is not one piece with the frame. A side of the frame is a long straight line found on step 3 that meets another at
  a corner, so a curve as long and straight as an axis is not cut. Then `plotextract_raster::shapes` finds, with nothing
  learned beforehand, what the pieces are: markers glued onto a line are taken off it (a morphological opening); pieces
  that look alike however they are turned (area, moments, fill, holes, stroke) are grouped; copies a few strokes apart
  are linked into paths, keeping at each piece the straightest pair of links so the dot of an *i* beside a dotted curve
  stays out, and paths that nearly meet are joined; so a type is dots, dashes, markers, lines, text (rows of different
  shapes close together) or other. A dotted black curve drawn among black axes and black text, as in the first figure of
  arXiv 2203.15668, comes out as one curve. Each type is a mask (`shape-N`, tagged shape and its kind, with its size,
  stroke and spacing); its kind can be said by hand, and is kept when the shapes are found again. A new example,
  `shapes`, runs it on an image from the command line.
- **The Curves step of the tracing wizard (step 5): images are traced into data.** A series is proposed for each curve
  of dots and dashes, for the lines of each colour and for each kind of marker, of the pieces inside the frame and not on
  its ticks; they follow the shapes until a series of the colour is changed by hand. Select a series and click pieces to
  add them or take them out; the right-click menu puts a piece in any series, in none, or in a new one. With Points, click
  a stray point to take it out (a cross, clicked again to put it back) and click where one was missed to put one in, at
  the middle of the ink there. A series remembers its pieces by a pixel of each, so it keeps them, and the points put in
  and taken out, when the pieces are found again. A series is traced into points at the centre of the coverage, to a
  fraction of a pixel: a point for every column of each piece (Line), one per piece (Markers), or one per dot and one
  every stroke along each dash, in the order of the path (Pattern; written in that order, so a curve that doubles back is
  written as drawn). The points are drawn over the image, and Copy and Save CSV write the selected series in plot values
  with the calibration of the ticks, through the same export as the vector figures'. Each series is a mask: ID
  `series-N`, tags curve, series, line, markers or pattern and calibrated, properties its colours, pieces, points and plot
  ranges. The search for pieces and shapes runs on a thread like the others, after the lines it cuts out.
- **The workspace writes CSV made elsewhere** (`Workspace::write_csv`): the wizard's series are saved with it.
- **File > Import from arXiv.** Type a paper's number or paste a link to it: its source is downloaded from arXiv
  (`plotextract-arxiv`, with ureq over rustls, flate2 and tar), the figures in it are unpacked into the user's cache
  folder and shown as pictures, the ones the TeX files draw first in the order they draw them. Choose one and Import (or
  double-click) to open it. The download runs on a thread with a progress line; the pictures are drawn by four threads
  through the new `Workspace::thumbnail`. Only figure files are written, under names that cannot leave the paper's
  folder; a paper submitted as a PDF alone is offered whole. The welcome screen has the same button.
- **Hide a curve from the right-click menu of a vector figure.** Ctrl+click a curve in the way (grid lines over the data,
  minor ticks beside the major ones), right-click and *Hide* it: it is no longer drawn, snapped to or picked. Before, a
  curve was hidden only from its row in the Curves table.

### Changed
- **The README is shorter and written for people using the app**, with a new demo GIF that shows both kinds of figure: a
  vector figure from arXiv (the grid and minor ticks hidden, the ticks snapped to and valued, a curve picked and exported)
  and a raster image (its colours named, its dotted curve found among black axes and text, fixed by hand and traced), and
  the same demo at full size as an MP4 (`docs/demo.mp4`). The crates, the dependencies and their licences moved to
  `docs/architecture.md`.
- **The app is split along its steps, as `docs/plan.md` planned, and the tracing wizard answers faster.** Slow work is
  one enum of jobs (`jobs.rs`) run by one runner, in place of a channel and a thread spawn per kind; the model is split into
  opening, the wizard's jobs and the arXiv window (`model/`); the wizard into a module per step that owns its state, its
  changes and its masks' metadata (`wizard/`), with what the UI shows of a mask in one place (`Wizard::view`); the
  wizard's canvas into what is drawn when, the cached pictures, and a tool per step (`wizard_canvas/`). On the way, the
  search for pieces and shapes on a 2652 x 2144 figure went from 298 ms to 88 ms (the curve colours labelled in parallel
  within their own bounds, every piece's look gathered in one pass, a quick test before looking for markers glued to a
  line, which also stops letters being split), entering the curves step no longer searches again when nothing changed,
  a change to one series traces only it, and a point put in or taken out no longer makes the picture of the pieces again.
  A shape type hidden from its row now disappears from the image at once. `bench.rs` measures it all
  (`cargo test --release -p plotextract how_responsive -- --ignored --nocapture`).
- **The masks panel is in sections, and the image shows what the step needs.** The masks are grouped by the step that
  makes them (Plot, Colours, Ticks, Shapes, Series), each section with a count and one eye for all of its masks; the
  current step's section is open and the others folded until opened by hand. Hidden masks, and shapes that are text or
  other, are named faintly; point counts are short (2.4k pt). What the image shows is decided in one place
  (`wizard_canvas::scene`): a mask is drawn on its own step and after it, never before; the lines to choose ticks from are
  drawn lightly on their step only (hundreds of them no longer hide the plot); the ticks chosen stay on the later steps;
  the crop's size is shown while it is chosen; the points of the selected series are drawn full size and on top, the
  other series' small and faint. `docs/plan.md` is the plan for the structure from here.
- **A colour can be several things at once.** What a colour is, is now a set of roles (curve, fill, axes, grid) instead
  of one class: black is often both the axes and a curve, and it can now say so. Its mask is tagged with every role it has,
  so the line search (which takes the masks tagged axes) and the curve step (the masks tagged curve) both find it.
  A colour with no role is ignored and has no mask. On step 2 each colour has a toggle for each role, in place of the
  drop-down; the right-click menu has a box for each and an Ignore button; the bin on a mask's row ignores the colour.
- **Two tools, a delete icon, and undo for what is drawn by hand.** Step 1 has a Crop tool and an Exclude tool (a brush,
  a rectangle or a polygon; Alt erases). With neither chosen the pointer moves the image, so there is no pan tool and no
  "erase instead" box. Only drawing the exclude mask is undone, with Ctrl+Z and Ctrl+Shift+Z on step 1; the crop, the
  colours and the ticks are chosen again instead. Every row of the masks panel has a bin that deletes the mask (removes the
  crop, empties the exclude mask, ignores a colour, takes a tick's line away), in place of the Clear, Undo and Redo
  buttons. Hand edits of a colour's mask are gone, and with them the code that kept them across analyses.
- **Every mask has an ID, tags and properties.** `plotextract_raster::meta` describes a mask besides its pixels: an ID
  (`crop`, `exclude`, `colour-2`, `tick-xmin`), a set of tags for what it is (crop, exclude, colour, curve, fill, axes,
  grid, edited, tick, x axis, x min, calibrated, ...) and a dictionary of properties for the rest (the crop's size, a
  colour's RGB, pixel count and share, a tick's position, length, plot value and log scale). The wizard describes each mask
  (`Wizard::meta`, `Wizard::masks`) and finds masks by their tags (`Wizard::find_masks`); the steps to come read masks that
  way. The masks panel is a grid of aligned columns with an eye to show or hide each mask, and each row opens to show its
  ID, tags and properties. How a tag or a value reads is decided in `text.rs`.
- **The masks appear as they are made, and the choosing is on the right.** The left panel lists only the masks that exist:
  the crop and the exclude mask once they are made, a mask for every colour that is not ignored, and each tick once
  it has a line. The right panel is the step. Step 1 has the tools, which no later step has; step 2 has the list of colours
  with what each is (it was in the mask rows); step 3 has the lines, and the four ticks to select, click a line for, and
  type a value for. The panel code is a module with a file for each step.
- **Masks are outlined black with white beside the edge** when they are selected or the pointer is on them (a colour, a
  tick, the exclude mask, the crop, the line under the pointer), so they can be told on a black or a white background.
- **Unused dependencies are gone** (`tiff`, `ndarray`), and the README says what the raster crate is now. The four ticks are
  named as the calibration's slots are (`XMin`, `XMax`, `YMin`, `YMax`).
- **The masks are on the left and the step and its tools on the right**, and the exclude hint shows on the first two steps only.
- **The tracing wizard has two panels.** The masks are in their own panel on the left; on the right are the name of the
  step and what it is for, the tools of that step, its settings and the way to the next step. The steps list is gone. The
  tools follow the page: the crop page has pan, crop, brush, rectangle and polygon; the colour page the same without crop;
  the plot area page only picks lines, so it has none to choose from.
- **Cursors over the canvas are pictures the app supplies** (a black core with a white halo: crosshair, four-arrow move and
  the resize arrows, at 100%, 150%, 200% and 300% display scaling), handed to the window system as real cursors. They show on
  any image, however the user's cursor scheme is set, and follow the pointer without lag. They are drawn in TikZ and built
  with `build_cursors.py` (source and script live with the icon sources, outside this repository); where the window system
  does not take them, egui falls back to the system cursor. The wizard's brush ring and polygon lines have the same pale
  halo.

### Fixed
- **Import from arXiv lists a paper's figures in the paper's order when it is split into files.** The files a main file
  brings in with `\input`, `\include` or `\subfile` are read in their places, as TeX reads them; before, they were read in
  the order of the archive, so a paper split into sections (arXiv 2501.11723) listed its first four figures last.
- **The Hough search could lose a horizontal line.** `imageproc` rounds a line's distance in single precision, which at 90
  degrees put the votes of a horizontal line far from the left edge into the bucket of the row above, so a high enough
  vote threshold missed it. Horizontal lines are now found as the vertical lines of the transposed mask.
- **The mask tools no longer set the cursor over the whole window.** With the rectangle, polygon or brush selected, the
  crosshair replaced the menu's, the panel's and the buttons' cursors everywhere; it is now asked for only over the canvas.
- **Adding a shape to the mask no longer redraws the whole mask.** The picture of the mask is kept and the new shape is
  drawn into it, sending only the part it touches to the window system: about 2.6 ms whatever the number of shapes, where
  it was 48 ms with 20 brush strokes and 83 ms with 100 (undo and redo still make the picture again).
- **The wizard's tools start where the mouse button goes down.** egui calls a press a drag only after the pointer has
  moved about 6 pixels, and the crop, rectangle and brush started there: the first stretch of every stroke was lost and
  nothing showed until the pointer had travelled that far. Gestures now start on the press (a polygon corner is placed by the
  press too, so a slightly moving click no longer drops it).

### Added
- **The four ticks of the Plot area step are tagged with their plot values.** A Calibration card (the same grid as the one
  for vector figures: the tick, its position in pixels, a box for its value, green when its axis is complete, and the log
  axis boxes) turns the chosen ticks and the typed values into a `Calibration` the later steps read. Values accept `1e-3`
  and the like, and half-typed text is kept. The ticks are the calibration's own `Slot`s now, not a type of their own.
- **The Plot area step of the tracing wizard.** The third page looks for straight lines in the axes colour (the inks
  called *Axes and text*) and lights them on the image. The user chooses how they are looked for: the Hough transform
  (`imageproc`; upright lines, short ticks as well as long lines) or the line segment detector (`lsdetect`; long lines, not
  ticks shorter than about 15 pixels). Clicking a line gives it to the selected one of four new masks, **X min**, **X max**,
  **Y min** and **Y max**: each is the axes colour inside its line, and its position across the line is the centre of its
  coverage, to a fraction of a pixel. Choosing and clearing a tick are undoable. The step runs on the axes mask, not on the
  whole image. A right click on a line opens a menu that says which tick it is. The search runs on a background thread, one at a time (the lines found before stay on the image
  until the new ones arrive), so the window stays responsive. Three sliders tune the search live: the shortest
  line, the widest line, and (for the line segment detector) how many times the mask is enlarged first, which lets it see
  short ticks. Blocks wider than a line are taken out first, and the two edges of a stroke (or neighbouring Hough buckets)
  are merged into one line.
- **Named masks.** The tracing wizard works on masks: for every pixel of the crop, how much of it belongs to something. The
  side panel lists them: **Crop** (what is kept), **Exclude** (what is left out), then one mask for every ink colour,
  **Colour 1**, **Colour 2**, ... in order of how much of the image they cover. Each has an eye that shows it on the image
  (the exclude mask in red, an ink's mask tinted in its own colour, the crop as the dimming outside it), its size or share, and
  for an ink what it is. Clicking a row makes it the mask the Brush, Rectangle and Polygon tools draw on (the Crop tool is the
  crop's), so an ink's mask can be corrected by hand: paint pixels into it, erase pixels from it. Those edits are kept with the
  colour, so looking at the colours again (another tolerance, a new crop or exclude shape) keeps them, and undo and redo cover
  every mask. Changing the crop or the exclude mask on the colour step looks at the colours again.
- **The wizard's second step, Colours.** *Next: Colours* looks at the cropped, unmasked image on a thread and finds the colours it
  is drawn in: the background (the commonest colour on the border), then the inks, densest first. A colour is a new ink
  unless it is within a tolerance of one found or lies on the line between two (the antialiased blend of them), so edges
  and JPEG noise do not make colours, and each pixel gets its ink and how much of the pixel the ink covers. Moving the
  pointer over the image lights every pixel of the colour under it and dims the rest; the same happens when the pointer is
  on a colour's row in the list. Click a colour (or its swatch) to select it; right-click, or the list's menu, says what
  it is: *Curve*, *Fill*, *Axes and text*, *Grid* or *Ignore* (greys are guessed to be axes and text, or grid; the rest
  curves). *Merge closer than* sets how different two colours must be to be two inks. *Back* returns to the crop and mask;
  coming back to the colours looks again. Undo and redo belong to the first step.
- **Raster images open** (PNG, JPEG, GIF, BMP, TIFF, WebP; up to 50 million pixels). An image opens in the tracing wizard,
  whose first step is ready: crop the image to the plot (drag it out, then drag its edges and corners), and paint over what
  must be ignored with a rectangle, a polygon or a brush (Alt, or the Erase box, takes mask away). Everything is undoable.
  Masked pixels are kept as *unknown*, not as paper. The next steps (colours, plot area, curves, check) are not built yet.
  Zoomed in, the image is drawn as crisp pixels.
- `plotextract-raster` now decodes and draws images and holds the crop and mask (shapes in image pixels, rasterized on
  demand); `testdata/make_raster.py` makes sample images (a clean plot, one with legend, inset and colourbar, a JPEG, a
  dark screenshot, a screenshot with window chrome).

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
