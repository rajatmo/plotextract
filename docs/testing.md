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
