# The tick networks

Trained in the plotML project (beside this repository) on synthetic matplotlib plots, with no outside data. Both run on
the CPU in tract; what goes in and comes out is laid down in `plotextract_algo::ticks`.

| File | Network | plotML run | Parameters | Input | Output |
|---|---|---|---|---|---|
| `ticks.onnx` | tick finder, width 16 | `runs/det_w16_large` (6,000 plots, 20 epochs) | 0.86 M | 1 x 1 x 512 x 512, grey, paper 1 | 1 x 8 x 128 x 128: heat of x ticks, y ticks, x labels, y labels; offsets (2); label size (2) |
| `reader.onnx` | label reader, width 16 | `runs/rd_w16_large` (labels of 6,000 plots, 10 epochs) | 0.16 M | n x 1 x 32 x 192, grey, paper 1 | n x 48 x 14: CTC scores, class 0 the blank, then `0123456789-.^` |

Exported with plotML's `export.py` (opset 17). On plotML's 293 held-out plots the pair calibrates 97.6% of plots within
0.5% of each axis' span. To change them: train in plotML, export, copy both files here, write `testdata/ticks/` again
with plotML's `eval/parity.py`, and run `cargo test -p plotextract-corpus --test ticks`.
