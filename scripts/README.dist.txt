plotextract: pick curves from plots and export them as data in plot units.

Run it with a file (PDF, SVG, EPS or PostScript), or open one from File > Open, or drop it on the window. Files open
in the background, so the window stays usable while a big one loads:

    plotextract plot.pdf

Hand tool (default): drag pans, the mouse wheel zooms, Ctrl+0 fits the page.
Select tool (hold Ctrl; Cmd on macOS): the mouse snaps to curve points, a click selects, Ctrl+Shift+click adds a curve to
the series.
Right click: record the selected point as x min / x max / y min / y max, then type its plot value in the side panel.
Export CSV or Copy numbers write the active series as x,y.

A document with several pages opens at page 1. Move between pages with the Page control at the top of the side panel or
with Page Up / Page Down; work on a page is kept while you look at others. A password protected PDF asks for its password.

Ctrl+Z / Ctrl+Shift+Z undo and redo, Ctrl+S saves the project (calibration, series, hidden curves and page), F1 shows the
About window.

Very large files are limited: past 1,000,000 paths or 16,000,000 vertices plotextract keeps the first ones, says so in the
status bar and draws no picture. A PostScript program still running after 20 seconds is stopped.

Copyright (c) 2026 Rajat Mandal. Licensed under either of Apache-2.0 or MIT, at your option (LICENSE-APACHE, LICENSE-MIT).
