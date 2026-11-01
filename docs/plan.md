# Architecture plan

**Status: done** (all stages below, in one change; the numbers are in [testing.md](testing.md)). Where the code was
going, and in what order. It keeps what works (the layers in [architecture.md](architecture.md), the
headless `Model` driven by intents and effects, the algorithms in UI-free crates) and fixes what grew too big while the
tracing wizard went from one step to five. Each stage below is a refactor on its own: the behaviour does not change, the
tests move with the code and pass after every stage.

## What hurts now

Measured on the code as it is (lines before the tests):

| File | Code | Holds |
|---|---|---|
| `app/src/wizard.rs` | 1450 | the state of all five steps, their jobs, the metadata of every mask, the series proposals and the point edits |
| `app/src/wizard_canvas.rs` | 1590 | every step's pointer handling, right-click menus, cached pictures and painting |
| `app/src/model.rs` | 940 | files, pages, passwords, projects, export, the wizard's jobs and the arXiv window |
| `app/src/app.rs` | 470 | the frame, dialogs, and one channel and one spawn per kind of job (six, written out each time) |

- **One reason to change, many files to change.** Adding the shape masks touched about ten `match`es on `MaskId` in five
  files: the name (`text.rs`), the swatch, the info, the delete tip and the section (`masks.rs`), and the metadata, the eye,
  the delete and the key (`wizard.rs`). A mask kind is a concept; its knowledge is spread out.
- **The wizard knows everything.** A change to the curves step means reading past the crop, the colours and the ticks;
  the tests for each step are in one 700 line module.
- **The canvas mixes deciding with drawing.** What is drawn on which step was decided in a dozen places inside the
  painting code. (Done: see stage 0.)
- **The shell repeats itself.** Each new background job (lines, pieces, arXiv, thumbnails) added a channel, a receive loop
  and a spawn, the same few lines each time.

## Principles, as they apply here

SOLID is the yardstick, straightforwardness the tie-breaker: an abstraction has to remove more code, or more places to
touch, than it adds.

- **Single responsibility.** A module owns one step's state, the intents that change it and the masks it makes. The
  canvas separates what is shown (`Scene`), the pictures it keeps (textures and their caches) and what the pointer does
  (one tool per step).
- **Open/closed, the Rust way.** The steps and the kinds of mask are closed sets, so they stay `enum`s: an exhaustive
  `match` makes the compiler list every place a new variant needs, which is better than a trait object that compiles with
  a case forgotten. What changes is that each concept is matched in **one** place: one function maps a mask to everything
  the UI shows about it, one function maps a step to its scene, one to its pointer tool.
- **Liskov.** Operations that do not apply return nothing rather than panic (deleting a mask that cannot be deleted is a
  no-op, its view says so with `delete: None`), so every mask and every step can be handled the same way by the panels.
- **Interface segregation.** A panel or a tool gets the step it shows (`&PlotArea`, `&ShapesStep`), not the whole
  wizard; a step gets what it reads from the steps before it as arguments (`Colours::curve_inks()`, `PlotArea::frame()`),
  not by reaching into the wizard.
- **Dependency inversion.** Slow work is a `Job` the model hands out and the shell runs, through one trait; the network is
  a `Downloader` (already), the file system the `Workspace` (already), the algorithms in `plotextract-raster` with no UI.
  The model never knows about threads, egui or the clock.

Not planned: a plugin system for steps, an ECS, an async runtime, generic "stage" traits with one implementation each.

## The shape it goes to

```
crates/app/src/
  app.rs            the frame, dialogs, the clipboard; runs jobs through jobs.rs
  jobs.rs           Job (run on a thread, answer back) and the runner: one channel, a few workers
  model/
    mod.rs          Model: Intent -> Effect, the open document
    opening.rs      load jobs, pages, passwords, projects
    arxiv.rs        the Import from arXiv window's state and its jobs
  wizard/
    mod.rs          Wizard: the step, the selected mask, the five steps; routes each intent to the step it belongs to
    masks.rs        MaskId and MaskView (name, swatch, info, tags and properties, delete) - the one match on MaskId
    prepare.rs      step 1: crop, exclude mask, its undo
    colours.rs      step 2 (as now)
    plot_area.rs    step 3: line search, ticks, values, calibration, frame and its ticks
    shapes.rs       step 4: piece search, shapes, kinds said by hand
    curves.rs       step 5: series, proposals, points put in and taken out, export
  canvas/
    mod.rs          the view and the page picture (canvas.rs now)
    scene.rs        what is drawn on which step
    pictures.rs     the overlays and their caches (ink, exclude mask, pieces, outlines)
    tools/          one per step: the pointer, the hover, the right-click menu
  panels/           the side panels (panel.rs and wizard_panel/ now), one file per step as now
```

Data still flows forward by tags: a step finds what it works on in the masks of the steps before it.

## Stages

| | Stage | Size | What it removes |
|---|---|---|---|
| 0 | **Scene: what is drawn when** (done). `wizard_canvas::scene` says, for the wizard as it is, which masks and choices the image shows; the painting reads it. Rule: a mask is drawn when its eye is open on its own step and after it, never before; what a step chooses from is drawn on that step only. Tested on its own. | small | a dozen step checks in the painting code |
| 1 | **One job runner.** `trait Job: Send { fn run(self: Box<Self>) -> Answer }`, `enum Answer` for what comes back; `Effect::Run(Box<dyn Job>)`; the shell has one channel and one spawn (and a worker pool for the thumbnails); `Model::finish(Answer)` routes it. | small | five channels, five spawns, five receive loops |
| 2 | **MaskView.** `Wizard::view(mask) -> MaskView { name, swatch, info, meta, delete: Option<tip> }`; the masks panel, the sections and `text::mask_name` read it. | small | eight `match`es on `MaskId` outside the wizard |
| 3 | **Split the wizard.** Move each step's fields, intents and tests to its module (`prepare`, `plot_area`, `shapes`, `curves`); `Wizard` keeps the step, the selection and the routing. `WizardIntent` splits into one enum per step inside one wrapper, so a step's `apply` matches only its own. | medium, mechanical | the 1450 line file; every step's code in one place |
| 4 | **Split the canvas.** `scene.rs`, `pictures.rs` (the cached textures), and a tool per step behind one `match` on the step for pointer, hover and menu. | medium | the 1590 line file |
| 5 | **Split the model.** Opening (load jobs, pages, passwords, projects) and the arXiv window to their own modules. | small | the 940 line file |

Each stage is one change that can be reviewed alone; 1, 2 and 5 do not depend on the others, 4 is easier after 3.

## How it went

All five stages are done, the behaviour unchanged and every test passing after each. Where the result differs from the
plan above, the simpler choice won:

- **Jobs are an enum, not a trait.** `jobs::Job` lists the kinds of job and `Job::run` gives an `Answer`; the shell has one
  `Runner` (a thread per job, four workers for a batch, one channel). The kinds are a closed set, and an enum lets the
  tests say which job was asked for; a trait object would have hidden that for nothing.
- **`WizardIntent` stays one enum** (in `wizard/intent.rs`), routed to the steps in one `match`. Splitting it per step
  would have changed every place an intent is made, panels and tests alike, for no gain in what a step can see.
- **`Wizard` keeps its methods** as one-line delegates to the step that owns the state, so the panels, the canvas and the
  model did not change; a step's own type (`CurveStep`, `PlotArea`, ...) is there for code that works on that step only.
- **Responsiveness**, measured on the way (`bench.rs`), led to four fixes the plan did not foresee: the curve colours are
  labelled in parallel within their own bounds, every piece's look is gathered in one pass, a search with unchanged
  inputs is not run again, and the picture of the pieces is not made again for a point put in or taken out. The search
  for pieces and shapes on a 2652 x 2144 figure went from 298 ms to 88 ms.

| | Stage | Now |
|---|---|---|
| 0 | Scene | `wizard_canvas/scene.rs` |
| 1 | One job runner | `jobs.rs`; `Effect::Run`, `Effect::RunBatch`, `Model::finish` |
| 2 | MaskView | `wizard/masks.rs`: `MaskId`, `Section`, `Swatch`, `MaskView`, `Wizard::view` |
| 3 | Split the wizard | `wizard/`: `prepare`, `colour_step`, `plot_area`, `shape_step`, `curve_step`, `intent`, `masks`, `tests` |
| 4 | Split the canvas | `wizard_canvas/`: `mod`, `scene`, `pictures`, `tools/{prepare, colours, ticks, shapes, curves}`, `tests` |
| 5 | Split the model | `model/`: `mod`, `opening`, `tracing`, `arxiv_import`, `tests` |

## How to tell it worked

- No file in `app/src` over about 600 lines of code. (Met: the largest are `canvas.rs`, 530 lines of code, and
  `wizard/mod.rs`, 633 lines with its doc comments, under 600 without; the tests are in files of their own.)
- A new kind of mask touches its step's module and `wizard/masks.rs`, and nothing else. (Met for the panels and the
  metadata; the canvas draws a new kind in its step's tool.)
- A new background job is a variant of `Job` and an arm in `Model::finish`; the shell does not change. (Met.)
- Every step's behaviour is tested through its own API or the wizard's. (Met: the wizard's tests drive each step through
  its intents.)
