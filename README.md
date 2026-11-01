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
   (black is often both the axes and a curve). Hover the image to see where a colour is used.
3. **Plot area.** Straight lines are found in the axes colour. Click a tick's line for each of the four ticks and type
   its value.
4. **Shapes.** The curve colours are broken into pieces, and alike pieces are grouped into shapes: the **dots** or
   **dashes** of a dotted or dashed curve, **markers**, solid **lines**, **text**, and **other**. So a dotted curve drawn
   in the same black as the axes and the labels comes out on its own. Each shape is listed with a picture of a typical
   piece; hover a piece to see what it is and why. Fix what the grouping got wrong:
   - **Select**: say what a shape is (right-click, or keys 1 to 6). Step through its pieces from the oddest one (N), where
     mistakes collect. You can also zoom to a shape, show it alone, or merge it into another.
   - **Move**: click a piece to put it in the selected shape or take it out, or drag around many. Moving one piece offers
     to move the pieces that look like it too.
   - **Links**: dots and dashes are linked along their curve. Click a wrong link to cut it, or click two ends to join them.
5. **Curves.** A series is proposed for each dotted or dashed curve, each set of markers and the solid lines. Click
   pieces to add or remove them; with *Points*, click a stray point to remove it, or click where one is missing to add it.
   Then *Copy* or *Save CSV*.

Your changes are kept when you go back a step and the image is looked at again.

## Figures from arXiv

*File > Import from arXiv* takes a paper's number (`2203.15668`, `hep-th/9901001`) or a link to it. It downloads the
source the authors submitted and shows every figure in it, in the order the paper draws them. Pick one and *Import*.
When the authors submitted a PDF, EPS or SVG figure, you get its exact vector paths.

## Keys

| | |
|---|---|
| Drag, mouse wheel | move and zoom the view (the middle button moves it while pieces are being moved) |
| Ctrl+0 | fit the page |
| Ctrl+Z, Ctrl+Shift+Z | undo and redo |
| Ctrl+S | for a vector figure, save the project: calibration, series and page (*File > Open project* reopens it with its figure) |
| Up, Down, 1 to 6, N, S, Z | on the Shapes step: select a shape, say what it is, go to its next odd piece, show it alone, zoom to it |
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
