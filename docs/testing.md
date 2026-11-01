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

### Colours of an image (`crates/raster`, `crates/corpus/examples/colours.rs`)

`cargo run --release -p plotextract-corpus --example colours -- image.png` prints the colours the wizard finds and the time
it took (a few milliseconds for a 1050 x 525 image). The unit tests in `plotextract-raster` draw plots with antialiased
lines, noise, dark backgrounds, masks and crops and check which inks are found and what each pixel is; the app's tests
drive the step through intents (the analysis runs as a job, like opening a file). `testdata/make_raster.py` makes sample
images: a clean plot, one with a legend, inset and colourbar, a JPEG of it, a dark one resized by 125%, and a screenshot with
window chrome.

### Straight lines of an image (`crates/raster`, `crates/corpus/examples/lines.rs`)

`cargo run --release -p plotextract-corpus --example lines -- image.png [outdir] [--list]` finds the colours, takes the dark
greys as the axes, and runs each algorithm on that mask: it prints how many lines were found and how long it took (about
5 to 15 ms for the sample images), lists them with `--list`, and with `outdir` writes a picture of each algorithm's lines.
The unit tests draw frames with ticks on generated masks and check that both algorithms find the frame to a fraction of a
pixel, that the Hough transform finds 6 pixel ticks and the line segment detector does not, and that blocks, tilted strokes
and an empty mask give no lines. The app's tests drive the step with intents: choosing and clearing ticks, the algorithm,
and coming back to the colours.

### Curves of an image (`crates/raster/src/curves.rs`)

The unit tests draw strokes, dashes, specks and markers on generated masks and check the pieces found (with the axes cut
out), the centre of a stroke to a fraction of a pixel (the faint edge pixels count), and markers traced to their centres.
The app's tests draw a framed plot with a blue line, a black curve that starts on the frame (black being the axes too) and
a legend dash outside the frame, and check through intents that each curve colour gets a series without the frame or the
legend, that the series is written in plot values, that pieces are moved between series by hand and kept when the pieces
are found again, and that the search runs as a job after the line search.

### Shapes of an image (`crates/raster/src/shapes.rs`, `crates/corpus/examples/shapes.rs`)

The unit tests draw the things a curve mask holds and check what each piece is found to be: the dots of a dotted line
turned with it chain in order whatever their turn, rings far apart are markers with one hole, a lone square is a stray;
a dot just off the line (the dot of an *i*) stays out of it; dashes and dots alternating are one path, traced to a point
every stroke along each dash and one per dot; markers glued onto their line are taken off it; and a word of different
shapes close together is text. They check the edits too: a type knows its typical piece and puts a ring a little bigger
than the rest first among its odd ones; a stricter tolerance splits rings of two sizes a looser one groups; pieces are
moved between types, out to the other pieces of their colour and into a type made by hand, and types merged; paths are
cut and joined (only at their ends, or to a piece on none); the look-alikes of a piece are those more like it than like
the rest; and edits made again on the pieces found again give the same shapes. The app's tests draw a framed plot with a
dotted curve of the axes colour and check through intents that the shapes step finds its dots, that the curves step
proposes one pattern series of them in order, that a kind said by hand changes the proposal until a series is changed by
hand, that a piece moved, a shape made and merged, and a path cut stay so when the pieces are found again, that a few
taller dots are offered together when one is moved, that an emptied shape is not listed and makes no series, and that
points are taken out, put back and put in, and kept when the pieces are found again.

`cargo run --release -p plotextract-corpus --example shapes -- image.png [out.png]` does what the wizard does with the dark
colour of an image (frame cut out, pieces, shapes) and prints the types and paths found, with a picture of the pieces
coloured by kind. On the first figure of arXiv 2203.15668 (2652 x 2144, a dotted black curve with rotated text along it)
the classification takes about 30 ms: the curve's 247 dots and double dots are one curve, its text is text.

### Figures from arXiv (`crates/arxiv`)

The unit tests read paper numbers written every way (and refuse paths, other sites and near misses), find the figures a
set of TeX files draws in order (`\graphicspath`, options with brackets, comments, `\epsfig`), and unpack tars made in
memory: only figures are written, in the paper's order, and names that would leave the folder (`..`, a root, a drive, a
colon, `con.png`) are skipped. The app's tests drive the window through intents with a fake downloader: the paper is
fetched as a job, listed, a figure's picture drawn, and the figure opened from the cache; a missing paper says so, and a
fetch replaced by another is dropped. One test downloads a real paper and is ignored unless asked for:

```
cargo test -p plotextract-arxiv -- --ignored
```

### Responsiveness of the tracing wizard (`crates/app/src/bench.rs`)

```
cargo test --release -p plotextract how_responsive -- --ignored --nocapture
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
| window: entering the curves step (proposals, tracing) | 11 ms | 11 ms |
| window: taking a point out of the biggest series (one series traced again, not all) | 7.1 ms | 4.0 ms |
| frame after a point is put in (the pieces' picture is kept) | | 0.2 ms |
| frame after a piece is taken out (the pieces' picture made again) | | 3.2 ms |
| first frame of a step, fitted, eight colours shown | 31 ms | 31 ms |
| any other frame | | 0.2 ms or less |

### Frame cost

```
cargo test --release -p plotextract how_long_a_frame -- --ignored --nocapture
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
cut short, is stopped). Prefer a hand-written file in a unit test when the case is small: `crates/workspace` builds
multi-page and damaged PDFs in memory, so those run everywhere.
