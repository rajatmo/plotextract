plotextract: pick curves from plots and export them as data in plot units.

Run it with a file (PDF, SVG, EPS or PostScript), or open one from File > Open, or drop it on the window. Files open
in the background, so the window stays usable while a big one loads:

    plotextract plot.pdf

Hand tool (default): drag pans, the mouse wheel zooms, Ctrl+0 fits the page.
Select tool (hold Ctrl; Cmd on macOS): the mouse snaps to curve points, a click selects, Ctrl+Shift+click adds a curve to
the series.
Right click: record the selected point as x min / x max / y min / y max, then type its plot value in the side panel; or
hide the selected curve (grid lines, minor ticks) so it is no longer drawn or snapped to.
Export CSV or Copy numbers write the active series as x,y.

A raster image (PNG, JPEG, GIF, BMP, TIFF, WebP) opens in the tracing wizard: crop and mask, the colours, the four ticks
with the plot value of each, the shapes the curves are drawn with (dots, dashes, markers, lines, text; fix any the app
got wrong by moving pieces between shapes, or cutting and joining the links of a dotted curve), and the curves:
a series is proposed for each, you take stray points out and put missed ones in, and it is copied or saved as CSV in plot
values. The masks you make appear on the left, the step and its choices on the right.

File > Import from arXiv takes a paper's number (2301.01234) or a link to it, downloads its source from arXiv and shows
every figure in it; choose one and Import to open it. The figures are kept in your cache folder (plotextract\arxiv).

A document with several pages opens at page 1. Move between pages with the Page control at the top of the side panel or
with Page Up / Page Down; work on a page is kept while you look at others. A password protected PDF asks for its password.

Ctrl+Z / Ctrl+Shift+Z undo and redo, Ctrl+S saves the project (calibration, series, hidden curves and page), F1 shows the
About window.

Very large files are limited: past 1,000,000 paths or 16,000,000 vertices plotextract keeps the first ones, says so in the
status bar and draws no picture. A PostScript program still running after 20 seconds is stopped.

Copyright (c) 2026 Rajat Mandal. Licensed under either of Apache-2.0 or MIT, at your option (LICENSE-APACHE, LICENSE-MIT).
