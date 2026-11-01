# plotextract

Get the numbers out of a plot. Open a figure from a paper (PDF, SVG, EPS or PostScript, or an image) or import it
straight from arXiv. Tell plotextract what the axes are worth and pick the curves. You get `x,y` in the plot's own units.

![plotextract, first with a vector figure imported from arXiv: the grid lines and minor ticks hidden, the four ticks snapped to and given their values, a curve picked and exported as CSV. Then with a raster image: its colours named, its dotted curve found among black axes and text, its shapes fixed by hand, and the curve traced into a series](docs/demo.gif)

The same demo at full size: [docs/demo.mp4](docs/demo.mp4) (1480 × 1090, 1 minute).

## Download

Get the package for your system from the [releases page](https://github.com/rajatmo/plotextract/releases), unpack it and
run `plotextract`. There is nothing to install. On Linux you need X11 or Wayland and a graphics driver. On macOS the app
is unsigned: right-click it and choose Open the first time.

## Vector figures: exact

PDF, SVG, EPS and PostScript figures keep the paths they were drawn with. plotextract reads those paths, so every point
it exports is a vertex of the original curve. On real matplotlib and Mathematica figures the result is within 0.1% of
the plot's extent.

1. **Open** the file: *File > Open*, drop it on the window, or run `plotextract plot.pdf`. *Page Up* and *Page Down* move
   between pages. A protected PDF asks for its password.
2. **Clear the way** (if you need to). Grid lines and minor ticks can sit right on the points you want. Ctrl+click
   one, right-click it and choose *Hide*: it is no longer drawn or snapped to.
3. **Calibrate.** Zoom in on a tick, hold Ctrl and click its end (the pointer snaps to it). Right-click and record it as
   *x min*, *x max*, *y min* or *y max*, then type the value that tick stands for. Two ticks per axis; tick *log* for a
   logarithmic axis.
4. **Pick.** Hover a curve with Ctrl held to read it in plot units; Ctrl+Shift+click adds it to the series, or takes it
   out. A series can be any number of curves, such as a dashed line drawn in pieces or a set of markers.
5. **Export.** *Export CSV* or *Copy numbers*.

## Images: traced

A PNG, JPEG, GIF, BMP, TIFF or WebP image opens in a five-step wizard. Each step makes **masks**, listed on the left
with an eye to show or hide each one; the right panel is the step you are on.

1. **Crop and mask.** Drag out the plot, and paint over anything that is not part of it, such as a legend or a colour bar.
2. **Colours.** The colours of the image are found. Say what each one is: *curve*, *fill*, *axes* or *grid*, or several
   (black is often both the axes and a curve). Nothing is guessed; a colour you say nothing about is ignored. Hover the
   image to see where a colour is used.
3. **Plot area.** Straight lines are found in the axes colour. Click a tick's line for each of the four ticks and type
   its value.
4. **Shapes.** The curve colours are broken into pieces, and pieces are grouped by how they look, by measure alone: each
   long stroke is a **line**, and many pieces alike (dots, dashes, markers) are a **repeated shape**; the few like no
   other are left in **no group**. Every piece is drawn in its group's colour over the darkened image and ringed when it
   is small, so even a one-pixel piece is seen; hold H to see the image alone. A measure can see one drawing as two
   groups, or put in a group pieces that do not belong. Fix it with a selection and two buttons:
   - **Select**: click a piece to select its group, Shift+click to add another group, Ctrl+click for one piece; or drag
     around pieces (Shift adds, Alt takes away). A group's row in the list selects it too.
   - **Make one group** (G): the selected pieces become one group, and nothing else is in it.
   - **Remove** (Delete): the selected pieces leave their groups.
   - *Select similar* (S) adds the pieces that look like those selected; Ctrl+Z undoes a change.
5. **Link.** Choose the group of a curve, then link its pieces in the order the curve runs, as with a pen drawing a
   polyline: click a piece to start a link, click the next ones (or drag through them), double-click or press Esc to
   end it. The next click starts another link; a click on an end of a link goes on with it. Pieces still to link are
   ringed. In the panel the links can be put in order, turned round or deleted. *Copy* or *Save CSV* writes the centre
   of every linked piece in plot values, link after link.

Your changes are kept when you go back a step and the image is looked at again.

## Figures from arXiv

*File > Import from arXiv* takes a paper's number (`2203.15668`, `hep-th/9901001`) or a link to it. It downloads the
source the authors submitted and shows every figure in it, in the order the paper draws them. Pick one and *Import*.
When the authors submitted a PDF, EPS or SVG figure, you get its exact vector paths.

## Keys

| | |
|---|---|
| Drag, mouse wheel | move and zoom the view (on the Shapes step a drag selects pieces; the middle button moves the view) |
| Ctrl+0 | fit the page |
| Ctrl+Z, Ctrl+Shift+Z | undo and redo |
| Ctrl+S | save the project: for a vector figure its calibration, series and page; for an image all the wizard's work (crop, masks, colours, ticks, groups, links). *File > Open project* reopens it with its figure |
| G, Delete, S, Z, Esc, H | on the Shapes step: make the selection one group, remove it from its groups, add the pieces like it, zoom to it, let go of it; hold H to see the image alone |
| Esc, Backspace | on the Link step: end the link being made, take its last piece off |
| F1 | About |

## Good to know

- Text, embedded images and gradients in a vector figure are shown but cannot be picked. Clipping is ignored on purpose,
  so ticks a PDF clips away can still be picked.
- The axes must be upright: rotated or skewed scans are not supported yet.
- Big, broken and hostile files are handled: plotextract opens them, says why it cannot, or keeps the first million paths
  and warns you. It does not hang or crash. A PostScript program still running after 20 seconds is stopped.

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
