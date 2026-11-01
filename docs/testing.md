# Testing

```
cargo test --workspace                       # everything, in a debug build
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
```

Most tests need nothing but the source: the domain (calibration, session, history, snap index), the importers' builders
and limits, the workspace (open, pages, projects, export, damaged files, written out by hand) and the app's `Model`
(every user-level flow, driven by intents and effects) run anywhere.

## Test data

The accuracy and stress tests use files that are not in the repository. They are looked for in `testdata/` next to the
repository, or in the folder named by `PLOTEXTRACT_TESTDATA`; when they are missing the tests print `skipped` and pass.

| Script | Needs | Makes |
|---|---|---|
| `make_matplotlib.py` | matplotlib, numpy | five figures as PDF, SVG and EPS, each with a JSON file of ground truth: where every tick landed on the page, and the data every series was drawn from |
| `make_mathematica.wls` | Mathematica | four figures in the same three formats, with their truth |
| `make_stress.py` | PyMuPDF, matplotlib, numpy | the awkward files below, in `stress/` |

### Accuracy (`crates/corpus/tests/corpus.rs`)

For each of 27 figure and format combinations the test does what a person does: snaps to the tick end points, records and
types the four calibration values, finds the series' curves or markers by colour, adds them to a series, exports CSV, and
compares with the data that drew the plot. The tolerance is 0.4% of the series' extent; matplotlib's results are exact to
the printed precision, Mathematica's within 0.1%.

`pdf.rs`, `ps.rs` and `svg.rs` check each importer separately: counts, the y-down frame, that vertices sit on ink in the
rendered page, and that a zoomed region is the same picture in more pixels.

### Stress (`crates/corpus/tests/stress.rs`)

45 files made by `make_stress.py`; each test opens them under a time limit, catching panics, and says what must happen.

| Group | Files |
|---|---|
| Big | a curve of 2,000,000 vertices; 100,000 markers; 5,000 curves; a path of 1,000,000 segments; an SVG with 100,000 paths |
| Many pages | 120 pages; 5,000 blank pages |
| Page geometry | 14,400 x 14,400 pt; 1 x 1 pt; 14,400 x 20 pt; rotated 90 degrees; cropped; zero, inverted and huge media boxes |
| Protected | AES-256 and RC4 with an open password (asks; wrong password; right password); owner password only |
| Damaged | empty; header only; truncated in half and at the tail; no xref; 400 flipped bytes; 4,000 zeroed bytes; random bytes after a header; a text file named `.pdf`; garbage xref offsets; a PDF after 20 KB of junk |
| Hostile | a form that draws itself; a page tree that contains itself; no pages; 1e38 and 10^23 coordinates; a million nested `q`; a 200 MB flate bomb of 15,000,000 paths |
| Other formats | malformed, empty, huge-viewBox and entity-bomb SVG; PostScript that loops forever, recurses, allocates without end, has no `showpage`, or is garbage |

The heavy ones are skipped in a debug build; run them with

```
cargo test --release -p plotextract-corpus --test stress
```

### The probe (`crates/corpus/examples/probe.rs`)

Opens one file the way the app does and prints how long each step took and what came out:

```
cargo run --release -p plotextract-corpus --example probe -- file.pdf [page]
```

`testdata/run_stress.py` runs it on every stress file in its own process with a 60 second limit and a 6 GB memory limit
and prints one line per file (outcome, time, peak memory), so a hang, a crash or a blow-up cannot hide. Results on a
16-thread machine with 32 GB (release build; "picture" is the base page picture drawn on the worker thread):

| File | Open | Picture | Peak memory |
|---|---|---|---|
| good plot | 0.0 s | 4 ms | 2 MB |
| curve of 2,000,000 vertices | 0.4 s | 1.4 s | 1.35 GB (the renderer's) |
| 100,000 markers (13,000,000 vertices) | 1.5 s | 10 s | 0.5 GB |
| 5,000 curves | 0.03 s | 2.3 s | 1.05 GB |
| path of 1,000,000 segments | 0.2 s | 1.5 s | 0.76 GB |
| decompression bomb (15,000,000 paths) | 2.6 s, cut short | none | 0.37 GB (was 6.2 GB before the limits) |
| 5,000 pages, last page | 0.0 s | 1 ms | 2 MB |
| infinite PostScript loop | stopped after 20 s | none | 8 MB (was: never returned) |
| SVG, 100,000 paths | 0.2 s | 55 s (resvg) | 0.13 GB |

Opening and drawing both run on other threads, so none of these waits freezes the window; the pictures arrive when they
are done.

### Colours of an image (`crates/algo`, `crates/corpus/examples/colours.rs`)

`cargo run --release -p plotextract-corpus --example colours -- image.png` prints the colours the wizard finds and the time
it took (a few milliseconds for a 1050 x 525 image). The unit tests in `plotextract-algo` draw plots with antialiased
lines, noise, dark backgrounds, masks and crops and check which inks are found and what each pixel is; the app's tests
drive the step through intents (the analysis runs as a job, like opening a file). `testdata/make_raster.py` makes sample
images: a clean plot, one with a legend, inset and colourbar, a JPEG of it, a dark one resized by 125%, and a screenshot with
window chrome.

### Straight lines of an image (`crates/algo`, `crates/corpus/examples/lines.rs`)

`cargo run --release -p plotextract-corpus --example lines -- image.png [outdir] [--list]` finds the colours, takes the dark
greys as the axes, and runs each algorithm on that mask: it prints how many lines were found and how long it took (about
5 to 15 ms for the sample images), lists them with `--list`, and with `outdir` writes a picture of each algorithm's lines.
The unit tests draw frames with ticks on generated masks and check that both algorithms find the frame to a fraction of a
pixel, that the Hough transform finds 6 pixel ticks and the line segment detector does not, and that blocks, tilted strokes
and an empty mask give no lines. The app's tests drive the step with intents: choosing and clearing ticks, the algorithm,
and coming back to the colours.

### Pieces of an image (`crates/algo/src/curves.rs`)

The unit tests draw strokes, dashes, specks and markers on generated masks and check the pieces found (with the axes cut
out), and a piece's centre to a fraction of a pixel (a pixel counts as much as it is covered), with the inks kept.
The app's tests draw a framed plot with a blue line, a black curve that starts on the frame (black being the axes too) and
a legend dash outside the frame, and check through intents that each curve colour gets a series without the frame or the
legend, that the series is written in plot values, that pieces are moved between series by hand and kept when the pieces
are found again, and that the search runs as a job after the line search.

### Shapes of an image (`crates/algo/src/shapes.rs`, `crates/corpus/examples/shapes.rs`)

The unit tests draw the things a curve mask holds and check how the pieces are grouped: alike pieces are one group however
they are turned (the dots of a dotted line, rings far apart, and a lone square as big as the dots, which only a person can
tell is not one), each named by how it looks; pieces like too few others are left over, and each long line is a group of
its own however like another it is; markers glued onto their line are taken off it; a group knows its typical piece and
puts a ring a little bigger than the rest first among its odd ones; and a stricter tolerance splits rings of two sizes a
looser one groups. They check the edits too: two whole groups made one keep the larger's number, a few pieces made one are
a new group, pieces taken out are left over and put back by joining, edits made again on the pieces found again give the
same groups, and the pieces similar to a selection are those like one of it. The app's tests draw framed plots and check
through intents that each line is a group without the frame and a legend dash is left over, that the dots of a dotted
curve are one group listed first, that pieces are selected, added, toggled and taken away, that a removal and a join stay
when the pieces are found again and are undone and redone on their step only, that a deleted group is emptied into the
leftovers, and that the pieces' picture is made again when the groups, the hidden ones or the selection change. For the
link step they check that a link is made click by click, a double-click (the piece again, then the end) ends it and the
next click starts another; that a link goes on from either end, loses its last piece or any piece, and takes a piece
from another link; that links are moved, turned round, deleted, undone and redone on their step only; that only the
chosen group's pieces are linked; and that the points are written link after link in plot values, refused before the
calibration and with nothing linked, and the same after the pieces are found again.

`cargo run --release -p plotextract-corpus --example shapes -- image.png [out.png]` does what the wizard does with the dark
colour of an image (frame cut out, pieces, groups) and prints the groups and how many pieces are left over, with a picture
of the pieces coloured by group.

### Saving and bringing back the tracing

The wizard's tests trace a dotted plot through every step (an exclude shape, the roles, a tick with its value, a log
axis, a dot taken out of its group, two links), save the work as JSON text and bring it back on a new wizard. The colours,
lines and pieces found again give the same groups, links and points, and the work saved again is the same. Work saved
before the colours were looked at waits for them. The model's tests save a project while an image is traced and load it
in a fresh window and over the same image: it comes back on its step, with the jobs run. A project whose tracing cannot
be read opens the image afresh with a warning.

### Tracing real figures (`crates/corpus/examples/real_eval.rs`)

`cargo run --release -p plotextract-corpus --example real_eval -- figure.pdf ...` draws each vector figure so its longer
side is 1600 pixels and traces it with no hand work: every ink a curve, the darkest the axes too. It prints the pieces
and groups, and the precision and recall of the points traced against the figure's own paths (within 2 pixels). These
scores say how much hand work a figure needs, not what a user ends up with.

For that, trace the figure's image in the app and save the project as `<name>.plotextract.json`, next to the PDF or in
`png/` beside it. `real_eval` then brings the project back as the app does and scores what the links export: the
precision of their points, and how much of the paths they follow they cover. `../testdata/arxiv_worst` holds the
eight worst of 40 arXiv figures with their PNGs.

### Figures from arXiv (`crates/adapters/arxiv`)

The unit tests read paper numbers written every way (and refuse paths, other sites and near misses), find the figures a
set of TeX files draws in order (`\graphicspath`, options with brackets, comments, `\epsfig`), and unpack tars made in
memory: only figures are written, in the paper's order, and names that would leave the folder (`..`, a root, a drive, a
colon, `con.png`) are skipped. The app's tests drive the window through intents with a fake downloader: the paper is
fetched as a job, listed, a figure's picture drawn, and the figure opened from the cache; a missing paper says so, and a
fetch replaced by another is dropped. One test downloads a real paper and is ignored unless asked for:

```
cargo test -p plotextract-arxiv -- --ignored
```

### Responsiveness of the tracing wizard (`crates/ui/src/bench.rs`)

```
cargo test --release -p plotextract-ui how_responsive -- --ignored --nocapture
```

takes an image through the wizard as a person would and times each background job, the work done on the window's
thread in answer to each click, and canvas frames on each step (fitted and zoomed in; the first frame, which makes the
pictures, and the next ones). The image is drawn by the test (3000 x 2400: a framed plot with ticks, a dotted curve, a
dashed one, markers, a solid line and rows of text-like marks), or read from `PLOTEXTRACT_BENCH_IMAGE`. On the first
figure of arXiv 2203.15668 (2652 x 2144, six curve colours; release build, 16 threads), before and after the refactor
that split the wizard into its steps:

| Work | Before | After |
|---|---|---|
| job: colours | 11 ms | 11 ms |
| job: straight lines (Hough, 1260 found) | 115 ms | 113 ms |
| job: pieces and shapes | 298 ms | 88 ms |
| of which the pieces (each colour labelled within its own bounds, the colours in parallel) | 228 ms | 59 ms |
| of which the shapes (every piece's look in one pass; a quick test before looking for markers glued to a line) | 73 ms | 26 ms |
| entering the curves step: the search run again with nothing changed | 298 ms | not run |
| window: entering the curves step (tracing; the proposals since removed) | 11 ms | 11 ms |
| window: taking a point out of the biggest series (one series traced again, not all) | 7.1 ms | 4.0 ms |
| frame after a point is put in (the pieces' picture is kept) | | 0.2 ms |
| frame after a piece is taken out (the pieces' picture made again) | | 3.2 ms |
| first frame of a step, fitted, eight colours shown | 31 ms | 31 ms |
| any other frame | | 0.2 ms or less |

The same figure with the groups and links steps, before and after the pictures of the pieces were made on every core
and each piece's centre found once, on the search's thread (release build, 16 threads):

| Work | Before | After |
|---|---|---|
| first frame of the shapes step, fitted | 21 ms | 12 ms |
| frame after the biggest group is selected | 35 ms | 15 ms |
| frame after a piece is taken out of it | 17 ms | 7 ms |
| window: the points of the links, each frame | 4.2 ms | 0.0 ms |
| window: taking the pieces of a search | 0.0 ms | 0.0 ms (the centres found on the thread) |
| first frame of the links step, fitted | | 12 ms |
| window: linking 500 pieces, a click each | | 0.6 ms |

### Frame cost

```
cargo test --release -p plotextract-ui how_long_a_frame -- --ignored --nocapture
```

records and tessellates one canvas frame for three large synthetic figures, fitted and zoomed in. Before and after the
level-of-detail work (ms per frame):

| Figure | Fitted | Zoomed in |
|---|---|---|
| one curve of 2,000,000 points | 112 -> 23 | 111 -> 3 |
| 100,000 markers | 183 -> 0.1 (not outlined) | 6.6 -> 0.9 |
| 5,000 curves | 11.6 -> 13 | 11 -> 7 |

## Adding a stress case

Add the file to `make_stress.py`, then a test to `stress.rs` that names the outcome it must have (opens, is refused, is
cut short, is stopped). Prefer a hand-written file in a unit test when the case is small: `crates/usecases` builds
multi-page and damaged PDFs in memory, so those run everywhere.
