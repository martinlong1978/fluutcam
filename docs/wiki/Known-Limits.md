# Known limits

FluutCAM is at version 0.1. The path from a STEP file to G-code works, but the window is a first version and much is **not yet** built. This page lists what you will run into. It is checked against the program, and it changes as the program does.

If something you need is not here and does not work, [tell us](https://github.com/martinlong1978/fluutcam/issues).

## The window

The window is a first version. The ribbon drives everything the window can do. These are not yet in the window:

- **Editing a Setup fully.** The Setup kind, a stock taken from a body of the model, the orientation (the tool axis, the X axis, the rotary axis) and the bodies a Setup machines cannot be changed in the window. A body stock shows, read only. Change these in the project file.
- **Picking whole bodies.** A click picks a face or a curve, not a body. A field that takes a body cannot be filled from the window.
- **Handles on the model.** The stock and the work zero have no handles to drag in the viewport. Change them in the inspector.
- **Picking in a box.** You click one entity at a time. After an edge, the inspector offers its closed loop.
- **A post that is not built in.** The Post stage's list has the built-in posts (`fluut/linuxcnc`, `fluut/grbl`, `fluut/gcode`, `fluut/linuxcnc-4x`) and not those of your project's `posts/` folder. A rotary Setup does not choose `fluut/linuxcnc-4x` by itself: pick it in the list.
- **The Library commands** (Tools, Machines, Posts) are greyed out. Browsing and editing the libraries is not built.
- **Choosing one model file to re-import.** **Re-import** reads every model file of the project.
- **A folder choice when posting.** The Post dialog names the first file; a post that writes more files numbers them beside it (`name.2.ngc`).
- **Picking the unit when importing** a DXF, SVG or STL file. FluutCAM assumes the project's unit and warns.
- **Running scripts.** Scripts run from the command line ([Scripting](Scripting)).
- **Keeping the window's layout, theme and other choices.** The window starts with the default layout and the dark theme.
- **Asking about unsaved changes** when you close the window. New project and Open do ask.
- **Languages other than English.** All the text goes through a catalog ready for translation, but only English ships.

Small thing you may notice: some error messages, such as a bad STEP file, show internal wording.

## Importing

- **STL files give no bodies yet.** The file is read, but no toolpath type can machine a mesh, so the import fails with "gave no geometry".
- **Two files with the same body ids** (two DXF files, each with a `b-1`) cannot be in one project. The second import is refused.
- DXF and SVG import gives curves. You pick them for a 2-D toolpath in pick mode.
- Re-importing a changed model file is a script function (`model.reimport`), not a window command.
- Importing from other CAM programs' files (posts, tools, machines) is not available.

## Setups

- A new rotary or indexed Setup assumes the part lies along the X axis through the model origin. For any other axis, set the orientation in the project file.
- **Stock.** A 3-axis Setup calculates with a block of stock, a rotary one with a cylinder, an indexed one with either. Stock taken from a body of the model is read but not calculated yet.
- **5-axis.** Not yet. FluutCAM supports one rotary axis (A): rotary Setups (simultaneous) and indexed Setups (the part turned to an angle, then 3-axis). Two-angle indexing (3+2) and simultaneous 5-axis are planned.
- **Fixtures** (a chuck, a tailstock, clamps) are not part of the project yet. The simulation, and the check that the stock clears the tool when it turns, do not know about them.
- A tool with no `overall_length` is checked at its tip only when the Z travel is checked before a turn.

## Calculating

- **Finishing toolpaths do not remove material from the stock** that later toolpaths see. A second finishing toolpath sees the stock as the roughing left it.
- **Results are not saved.** FluutCAM does not keep calculated paths with the project yet. Open a project and calculate again.
- **Rare failures in thin or touching 2-D shapes.** Very thin features, or shapes that touch along an edge, can make a 2-D region calculation fail. It then reports `cam.region.offset_failed` or `cam.region.boolean_failed` instead of producing a wrong path. Try a slightly different stepover or tolerance, and report the project.
- Most of the 22 toolpath types are tested on small reference parts. Treat a toolpath type you have not used before with care, and simulate it.

## Simulating

- **Rotary Setups cannot be simulated yet.** 3-axis and indexed Setups can.
- The simulation is an approximation at its resolution (0.2 mm by default); it does not model clamps or machine limits, or what the controller does.

## Posting

- The built-in posts write **millimetres** whatever unit the project shows.
- The built-in posts are plain G-code, LinuxCNC, Grbl and LinuxCNC with a rotary axis. There is no Fanuc, Mach3 or similar post yet; you can write one that extends `fluut/gcode` ([Machines and posts](Machines-and-Posts)).
- A rotary or indexed Setup needs `fluut/linuxcnc-4x`. The list offers it, but the window does not choose it for you.
- A folder of machines, posts and tools shared by all projects is not wired in yet. Put copies in each project's folders.

## Saving and recovery

- **No autosave and no crash recovery yet.** The pieces exist but are not switched on. Save often. The previous version of the file is kept as `name.fluut.bak`.
- The undo history is not saved with the project.

## Platforms

- macOS: FluutCAM is not notarised, so the first start needs the steps in [Install](Install).
- Linux: the file dialogs use the desktop portal (`xdg-desktop-portal`), so a desktop without one shows no dialog.
- There is no automatic update, by design; see [Settings and updates](Settings-and-Updates). A build made without the signing key cannot even check; at the moment none has the key.

## How this page is kept

When a planned feature is built, its line moves out of this page and into the pages that describe it. When you find a limit that is not listed, add it here.
