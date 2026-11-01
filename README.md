# plotextract

Get the numbers out of a plot. Open a figure from a paper (PDF, SVG, EPS or PostScript, or an image) or import it
straight from arXiv. Tell plotextract what the axes are worth and pick the curves. You get `x,y` in the plot's own units.

![plotextract, first with a vector figure imported from arXiv: its PDF listed and its page opened, the grid lines and minor ticks hidden, the four ticks snapped to and given their values, a curve picked and exported as CSV. Then with a raster image, in five steps: the legend painted over, its colours named, the frame's sides made the four ticks, the two parts of the blue curve joined into one group, linked in order and saved as CSV](docs/demo.gif)

The same demo at full size: [docs/demo.mp4](docs/demo.mp4) (1480 × 1090, 1 minute).

## Download

Get the package for your system from the [releases page](https://github.com/rajatmo/plotextract/releases), unpack it and
run `plotextract`. There is nothing to install. On Linux you need X11 or Wayland and a graphics driver. On macOS the app
is unsigned: right-click it and choose Open the first time.

## Opening a figure

Use *File > Open*, drop a file on the window, or run `plotextract plot.pdf`.

- **SVG, EPS and PostScript** open at once with their paths. A PostScript program opens at its first page.
- **Images** (PNG, JPEG, GIF, BMP, TIFF, WebP) open in the tracing wizard.
- **A PDF** first lists every raster image its pages draw, then every page, each with a picture, in sections you can
  fold away. Open a page to pick its paths, or an image to trace it. An image is opened at the resolution it is stored
  at, so a plot pasted into a paper as a picture is often sharper there than on the page. The image is kept in your
  cache folder, so a project made from it finds it again. A protected PDF asks for its password before the list.
- **From arXiv:** *File > Import from arXiv* takes a paper's number (`2203.15668`, `hep-th/9901001`) or a link to it. It
  downloads the source the authors submitted and shows every figure in it, in the order the paper draws them. Pick one
  and *Import*. A PDF figure, or a paper arXiv has only as a PDF, is listed like any PDF.

## Vector figures: exact

PDF, SVG, EPS and PostScript figures keep the paths they were drawn with. plotextract reads those paths, so every point
it exports is a vertex of the original curve. On real matplotlib and Mathematica figures the result is within 0.1% of
the plot's extent.

1. **Clear the way** (if you need to). Grid lines and minor ticks can sit right on the points you want. Ctrl+click
   one, right-click it and choose *Hide*: it is no longer drawn or snapped to.
2. **Calibrate.** Choose a row (*x min*, *x max*, *y min*, *y max*), zoom in on its tick, hold Ctrl and click the tick's
   end (the pointer snaps to it), press *Record*, and type the value the tick stands for. Two ticks per axis; tick *log*
   for a logarithmic axis. The card turns green when both axes are done.
3. **Pick.** Hover a curve with Ctrl held to read it in plot units; Ctrl+Shift+click adds it to the series, or takes it
   out. A series can be any number of curves, such as a dashed line drawn in pieces or a set of markers.
4. **Export.** *Export CSV* or *Copy numbers*.

## Images: traced

An image is traced in five steps. Each step makes **masks**, listed on the left with an eye to show or hide each one;
the right panel is the step you are on. Nothing is guessed: the steps measure and group, and you say what things are.

1. **Crop and mask.** Drag out the plot, and paint over what is in the way (a legend, a colour bar, a label crossing a
   curve) with a brush, a rectangle or a polygon. What is painted over is ignored, not read as paper.
2. **Colours.** The colours of the image are found. Say what each one is: *curve*, *fill*, *axes* or *grid*, or several
   (black is often both the axes and a curve). A colour you say nothing about is ignored. Hover the image to see where a
   colour is used.
3. **Plot area.** Straight lines are found in the axes colour. Choose a tick, click its line, and type its value, for
   each of the four; tick *log* for a logarithmic axis. The frame's sides found here are cut out of a colour that is
   both the axes and a curve, so a curve touching the frame is not one piece with it.
4. **Shapes.** The curve colours are broken into pieces, and pieces are grouped by how they look: each long stroke is a
   **line**, and many pieces alike (dots, dashes, markers) are a **repeated shape**; the few like no other are left in
   **no group**. Every piece is drawn in its group's colour over the darkened image, and ringed when it is small, so even
   a one-pixel piece is seen; hold H to see the image alone. Fix the groups with a selection and two buttons:
   - **Select**: click a piece to select its group, Shift+click to add another group, Ctrl+click for one piece; or drag
     around pieces (Shift adds, Alt takes away). A group's row in the list selects it too.
   - **Make one group** (G): the selected pieces become one group, and nothing else is in it. Use it for a curve that
     came out in parts.
   - **Remove** (Delete): the selected pieces leave their groups.
   - *Select similar* (S) adds the pieces that look like those selected; Ctrl+Z undoes a change.
5. **Link.** Choose the group of a curve, then link its pieces in the order the curve runs, as with a pen drawing a
   polyline: click a piece to start a link, click the next ones (or drag through them), double-click or press Esc to
   end it. A group of one piece links itself. The next click starts another link; a click on an end of a link goes on
   with it. Each link is drawn with its number and an arrow showing which way it runs. In the panel the links can be
   put in order, turned round or deleted. *Copy* or *Save CSV* writes the points in plot values, link after link: along
   each line or dash, a point every other pixel; one point for each dot or marker.

Your changes are kept when you go back a step and the image is looked at again.

## Projects

*File > Save project* (Ctrl+S) saves what you did. For a vector figure that is its calibration, series, hidden curves
and page. For an image it is all the wizard's work: the crop and the painted areas, what each colour is, the ticks and
their values, the changes to the groups and the links, and the step you were on. *File > Open project* opens the figure
again and brings it all back on that step.

## Keys

| | |
|---|---|
| Drag, mouse wheel | move and zoom the view (on the Shapes step a drag selects pieces; the middle button moves the view) |
| Ctrl+O, Ctrl+S | open a figure, save the project |
| Ctrl+0 | fit the page |
| Ctrl+Z, Ctrl+Shift+Z | undo and redo |
| G, Delete, S, Z, Esc, H | on the Shapes step: make the selection one group, remove it from its groups, add the pieces like it, zoom to it, let go of it; hold H to see the image alone |
| Esc, Backspace | on the Link step: end the link being made, take its last piece off |
| F1 | About |

## Good to know

- Text, embedded images and gradients in a vector figure are shown but cannot be picked (open an embedded image from
  the PDF's list to trace it). Clipping is ignored on purpose, so ticks a PDF clips away can still be picked.
- The axes must be upright: rotated or skewed scans are not supported yet.
- Big, broken, protected and hostile files are handled: plotextract opens them, says why it cannot, or keeps the first
  million paths and warns you. It does not hang or crash. A PostScript program still running after 20 seconds is
  stopped.

## Build from source

You need Rust 1.95 or newer and your platform's usual C toolchain.

```
cargo run --release -p plotextract -- plot.pdf
cargo test --workspace
```

`scripts/dist.ps1` (Windows) and `scripts/dist.sh` (Linux, macOS) build the release packages. How the program is put
together is in [docs/architecture.md](docs/architecture.md), and how it is tested in [docs/testing.md](docs/testing.md).
What changed in each version is in [CHANGELOG.md](CHANGELOG.md).

## Licence

Copyright (c) 2026 Rajat Mandal. Licensed under either of [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your
option. All dependencies are permissively licensed; the details are in [docs/architecture.md](docs/architecture.md).
