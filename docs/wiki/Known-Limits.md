# Known limits

FluutCAM is at version 0.1. The path from a STEP file to G-code works, but the window is a first version and much is **not yet** built. This page lists what you will run into. It is checked against the program, and it changes as the program does.

If something you need is not here and does not work, [tell us](https://github.com/martinlong1978/fluutcam/issues).

## The window

The window has a temporary **File** menu and a first ribbon. A new ribbon is being built and will replace both. These are not yet in the window:

- **Picking curves and faces for a toolpath.** The Geometry tab shows the selection but cannot change it. So 2-D toolpaths (pocket, contour, chamfer, bore, drill and so on) need their curves or faces set in the project file; a 2-D toolpath added in the window reports "select drive curves or drive faces". Clicking a face in the viewport shows its id (for example `f-494.10`), which you can copy into the file. 3-D toolpaths need no picking.
- **Editing heights, selections and points.** They show, read only. Numbers, choices and yes-or-no settings of a toolpath can be edited.
- **Choosing a toolpath's tool.** A new toolpath gets a starter tool; to use another, edit the project file.
- **Editing a Setup.** The inspector shows its kind, material, machine, work offset, stock, work zero and bodies, read only. Change them in the project file. The same goes for the index angle, the orientation and the clearance.
- **Deleting, renaming, reordering, duplicating or disabling** Setups and toolpaths. (The calculation skips a toolpath with `enabled = false` in the file.)
- **Choosing a machine, or a post that is not in the list.** The window's post list has `fluut/linuxcnc`, `fluut/grbl` and `fluut/gcode`.
- **Picking the unit when importing** a DXF, SVG or STL file. FluutCAM assumes the project's unit and warns.
- **Running scripts.** Scripts run from the command line ([Scripting](Scripting)).
- **Keeping the window's layout, theme and other choices.** The window starts with the default layout and the dark theme.
- **Asking about unsaved changes** when you close the window, or when you open a project by typing its path in the ribbon. The File menu's Open and New do ask.
- **Languages other than English.** All the text goes through a catalog ready for translation, but only English ships.

Small things you may notice: the arrow of a submenu in the File menu shows as a question mark; the ribbon's file and output boxes can still show the first project's names after you open another; some error messages, such as a bad STEP file, show internal wording.

## Importing

- **STL files give no bodies yet.** The file is read, but no toolpath type can machine a mesh, so the import fails with "gave no geometry".
- **Two files with the same body ids** (two DXF files, each with a `b-1`) cannot be in one project. The second import is refused.
- DXF and SVG import gives curves; using them needs the curve picking above.
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
- The window cannot post a rotary or indexed Setup well: its list has no 4-axis post. Use `fluut post --post fluut/linuxcnc-4x`.
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
