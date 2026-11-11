# Known limits

FluutCAM is at version 0.1. The path from a STEP file to G-code works, but the window is a first version and much is **not yet** built. This page lists what you will run into. It is checked against the program, and it changes as the program does.

If something you need is not here and does not work, [tell us](https://github.com/martinlong1978/fluutcam/issues).

## The window

The window is a first version. The ribbon drives everything the window can do. These are not yet in the window:

- **More than a first job in the Getting started card.** The card adds a 3-axis Setup and one toolpath of a type you choose; rotary and indexed Setups, more toolpaths and picking faces for a 2-D type are done with the ribbon and the inspector. Its last step is ticked when G-code is written in this session; it does not look at files on disk.
- **A dial for the index angle.** The index angle is typed in the inspector; there is no ring to drag on the model.
- **A factor per axis.** The window sets one scale factor for every axis. A STEP solid or a drawing with arcs cannot take different factors per axis (the Setup says so).
- **A rotary body stock is round.** A body as the stock of a rotary Setup is its bounding cylinder about the axis, not its true shape, so a square bar gives round stock that holds it.
- **Snapping on meshes.** The work zero snaps to the vertices and arc centres of STEP and DXF/SVG models, not to the corners of STL triangles.
- **Picking whole bodies.** A click on the model toggles a whole body, but the pointer lights up only the face or curve under it, not the whole body.
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

- **Tool libraries.** Fusion tool-library `.json` files import from the command line (`fluut tools import`, [Import tools](Import-Tools)); there is no import in the window yet. Vectric `.vtdb` tool databases do not import yet: the layout of that file is not public. Fusion's JSON is not a published specification, so check imported tools. Tool types without a FluutCAM shape (dovetail, lollipop, thread mill, reamer and others) are left out; an import never changes an existing library.

- **STL files (meshes).** They import as bodies and can be machined, with these limits:
  - Faces are not grouped. Each triangle is its own face, so picking a face picks one triangle.
  - Drops on a mesh are about 100 times slower than on exact STEP faces, so 3-D toolpaths on a detailed mesh take much longer.
  - Very large meshes (about a million triangles) use a lot of memory.
  - Re-import keeps your picks only while the triangles of the file keep their order.
  - An open or non-manifold mesh imports with the warning `model.geometry.mesh.open`; results near the gaps may be wrong.
  - STL has no unit. FluutCAM uses the project's unit and warns, and the window has no control to change it yet.
- **Two files with the same body ids** (two DXF files, each with a `b-1`) cannot be in one project. The second import is refused.
- DXF and SVG import gives curves. You pick them for a 2-D toolpath in pick mode.
- Re-importing a changed model file is a script function (`model.reimport`), not a window command.
- Importing from other CAM programs' files (posts, tools, machines) is not available.

## Setups

- A new rotary or indexed Setup assumes the part lies along the X axis through the model origin. For any other axis, set the rotary axis in the Orientation group.
- **Stock.** A 3-axis Setup calculates with a block of stock, a rotary one with a cylinder, an indexed one with either. Stock taken from a body of the model works in 3-axis Setups only.
- **5-axis.** Not yet. FluutCAM supports one rotary axis (A): rotary Setups (simultaneous) and indexed Setups (the part turned to an angle, then 3-axis). Two-angle indexing (3+2) and simultaneous 5-axis are planned.
- **Fixtures** (a chuck, a tailstock, clamps on the rotary) are part of a rotary or indexed Setup, but only in the project file and in scripts: the window has no fields for them yet. They raise the height the tool rises to before a turn, and the simulation of an indexed Setup reports a toolpath that meets one (`sim.fixture`). That check is conservative: a clamp counts as its bounding box, the tool as a plain cylinder, and arcs are checked at their ends only. Rotary Setups are not simulated, so a fixture hit there is not found. A vice or fixture that does not turn with A is not modelled; import it as a body of the model.
- A tool with no `overall_length` is checked at its tip only when the Z travel is checked before a turn, and its holder cannot be placed, so the simulation does not check it. You get the warning `cam.tool.length_missing` once per tool; enter the overall length in the tool to remove it.

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
