# User acceptance tests

These are manual test scripts for FluutCAM. Use them to check a build before you rely on it, or to see what has changed between two builds. Each test says what to start from, what to do, and what you should see. Each has a **Pass** box to tick.

## How to run them

1. **Get the test files.** The tests use the reference projects in the FluutCAM source repository, in `crates/fluut-cli/tests/projects/`. Each project is a folder with a `.fluut` file and its model:

   | Folder | What it is | Used for |
   |---|---|---|
   | `block` | A block with a pocket; three toolpaths (two pockets and a cut-out) | Most window tests |
   | `boss` | A part with a boss; one outside profile | Posting |
   | `holes` | A plate with holes; a bore and a drill toolpath | Drilling, Grbl expansion |
   | `plate` | A plate from a DXF file; a pocket with a hole, and a cut-out | DXF import |
   | `plate2d` | Facing, an optimised-load pocket, a score cut and a chamfer | 2-D spot checks |
   | `ridge` | A ridge with two parallel finishing toolpaths | 3-D finishing |
   | `rough3d` | The ridge with optimised-load and rest roughing | 3-D roughing |
   | `dome` | A dome with five 3-D finishing toolpaths | 3-D finishing |
   | `vcarve` | A DXF file with lettering, V-carved | V-carving |
   | `flipside` | Two indexed Setups, 0 and 180 degrees | Indexed Setups, based-on |
   | `shaft` | A rotary Setup with five toolpaths | Rotary Setups |

   Other files: `fuzz/seeds/stl/tetra.stl` (a small closed STL file) and `fuzz/seeds/stl/open_binary.stl` (an open mesh), and the expected G-code in `crates/fluut-cli/tests/golden/`.

2. **Work on copies.** Copy the folder of a reference project into a work folder before you open it, so a test cannot change the repository. The tests call that folder the *work folder*. Several tests save into it.
3. **Install the build** you are testing, and note its version (**About FluutCAM**, or `fluut --version`).
4. **Record the result.** Tick **Pass** when the expected result happened exactly. If it did not, leave the box empty and write down what you saw, with the screenshot and the exact message. Report failures as issues at <https://github.com/martinlong1978/fluutcam/issues>.

Copy the page into an issue or a document if you want to tick the boxes. In the wiki the boxes cannot be saved.

The tests use the ribbon: click the stage tab (Model, Setup, Toolpaths, Simulate, Post), then the button by its label. **Open**, **Save as**, **Import model** and **Post** open your system's file dialog; pick or type the path it asks for. There is no File menu. The Library buttons are greyed out (not built yet).

## The areas

| Page | Prefix | Covers |
|---|---|---|
| [Getting started card](UAT-Guide) | UAT-GDE | The first-project card: its seven steps, ticks that follow the project, type and tool choice, closing and reopening |
| [Project files](UAT-Project-Files) | UAT-PRJ | New (with the unit question), open, save, save as, unsaved changes, undo and redo, the unit badge, bad project files |
| [Import](UAT-Import) | UAT-IMP | STEP, DXF, SVG and STL files, copying, units, bad files, no project |
| [Setups](UAT-Setups) | UAT-SET | 3-axis, rotary, indexed, based on another, editing, inherited values, kind, bodies, body stock, orientation, scale and mirror, handles and snapping, fixtures, the index angle dial, failures |
| [Toolpaths](UAT-Toolpaths) | UAT-TP | Adding, editing, tool and heights, picking on the model, Browser actions, defaults, out-of-date marks, errors and warnings, cancel |
| [Simulation](UAT-Simulation) | UAT-SIM | Simulate, playback, colours, findings, failures |
| [Post](UAT-Post) | UAT-PST | The three posts, the output file, the G-code, failures |
| [Command line](UAT-CLI) | UAT-CLI | `fluut calc`, `sim`, `post`, `run`, exit codes, errors |
| [Settings and updates](UAT-Settings) | UAT-STG | The Settings window: release channel, update check, saved choices |
| [Import tools](UAT-Tools) | UAT-TLS | Import of a Fusion tool library, in the window (UAT-TLS-09 to 14) and with `fluut tools import` (UAT-TLS-01 to 08): result, overwrite, ids, bad files, bad arguments |
| [Install (Flatpak)](UAT-Install) | UAT-INS | Installing, running, file dialogs and uninstalling the Linux Flatpak |

## Record sheet

Fill this in for each run.

| | |
|---|---|
| Tester | |
| Date | |
| Build version | |
| Operating system | |
| Graphics device (viewport works?) | |
| Areas run | |
| Tests passed / run | |
| Issues raised | |

## Running order

The areas do not depend on each other, but within a page the tests go in order: a later test may start from the result of an earlier one, and says so under Preconditions.

## Keeping the tests current

A change that alters what a test expects changes the test in the same pull request. A new feature gets tests. A feature that is not built gets no test; it is listed in [Known limits](Known-Limits).
