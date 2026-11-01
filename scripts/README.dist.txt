plotextract: pick curves from plots and export them as data in plot units.

Run it with a file (PDF, SVG, EPS, PostScript or an image), or open one from File > Open, or drop it on the window.
Files open in the background, so the window stays usable while a big one loads:

    plotextract plot.pdf

A vector figure (PDF page, SVG, EPS, PostScript) is laid out as an image is: on the left its layers and every curve,
each with an eye that hides it (a click on a row selects the curve), on the right calibration, series and the selection.
Hand tool (default): drag pans, the mouse wheel zooms, Ctrl+0 fits the page.
Select tool (hold Ctrl; Cmd on macOS): the mouse snaps to curve points, a click selects, Ctrl+Shift+click adds a curve to
the series.
Calibrate: choose a row (x min, x max, y min, y max), Ctrl+click its tick, Record, and type its plot value. The right
click menu records the selected point too, and hides the selected curve (grid lines, minor ticks).
Export CSV or Copy numbers write the active series as x,y.

A raster image (PNG, JPEG, GIF, BMP, TIFF, WebP) opens in the tracing wizard: crop and mask, the colours (say what each
is: nothing is guessed), the four ticks with the plot value of each, and the shapes: the pieces of the curve colours
grouped by how they look, each drawn in its group's colour and ringed when small. Fix the groups: select pieces (click a
piece for its group, Shift+click to add a group, drag around pieces) and Make one group (G) or Remove (Delete). Then
link the pieces of a curve in order, as with a pen: click piece after piece (or drag through them), double-click to end
a link; order the links in the panel, and copy or save the points as CSV in plot values. The masks you make appear on
the left, the step and its choices on the right.

Opening a PDF lists the images drawn on its pages, then its pages; open an image to trace it at the resolution it is
stored at, or a page to pick its paths. The images are kept in your cache folder (plotextract\pdf). A password protected PDF asks for
its password first.

File > Import from arXiv takes a paper's number (2301.01234) or a link to it, downloads its source from arXiv and shows
every figure in it; choose one and Import to open it. The figures are kept in your cache folder (plotextract\arxiv).

Ctrl+Z / Ctrl+Shift+Z undo and redo, Ctrl+S saves the project (calibration, series, hidden curves and page; for an image,
everything done in the wizard, brought back on the same step when the project is opened), F1 shows the About window.

Very large files are limited: past 1,000,000 paths or 16,000,000 vertices plotextract keeps the first ones, says so in the
status bar and draws no picture. A PostScript program still running after 20 seconds is stopped.

Copyright (c) 2026 Rajat Mandal. Licensed under either of Apache-2.0 or MIT, at your option (LICENSE-APACHE, LICENSE-MIT).
