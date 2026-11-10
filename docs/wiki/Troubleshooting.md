# Troubleshooting

Find your problem in the list. [Diagnostics](Diagnostics) has the full list of messages, and [Known limits](Known-Limits) lists what is not built yet.

## The window

**The viewport is off and says "No GPU device: the viewport is off".** FluutCAM could not start the 3-D drawing, usually because the graphics driver is missing or too old. Everything else works, including calculating and posting. Update the graphics driver, or try another computer for the 3-D view.

**No file dialog opens** (Linux). The dialogs use the desktop portal, `xdg-desktop-portal`, with a backend for your desktop. Install it and start FluutCAM again. Meanwhile you can start the window with the project as the first argument: `fluutcam my-part.fluut`.

**A ribbon button is greyed out.** It does not apply now. Save, Save as, Import model and Post need an open project. New Setup based on selected needs a selected Setup. The toolpath buttons need a Setup. Undo and Redo need something to undo or redo. Re-import needs a model. While a job runs, most buttons wait; the status bar shows the job. The Library buttons are always greyed out: they are not built yet.

**The window says "Failed: ..." in red.** The last job failed and changed nothing. The text says why. Import problems are in [Import a model](Import-a-Model).

## Importing

**"not a valid STEP file".** The file is not STEP or is damaged. Export it again from your CAD program as STEP.

**"gave no geometry" for an STL file.** STL files cannot be used yet. Use STEP.

**The model is tiny or huge.** A DXF, SVG or STL file has no unit, so FluutCAM assumed the project's unit. Undo the import, start a project in the other unit, and import again.

## Toolpaths

**A toolpath shows a red triangle.** Select it and read the message under the setting it names, or in the Messages tab. The most common ones:

| Message | What to do |
|---|---|
| "select drive curves or drive faces" | A 2-D toolpath needs curves or faces. On the Geometry tab click **Pick** and click them on the model (see [Add a toolpath](Add-a-Toolpath)), or use a 3-D toolpath |
| "... runs in rotary Setups only" or "... runs in three_axis and indexed Setups only" | The toolpath type does not suit the Setup's kind. Add it to a Setup of the right kind |
| "... has no value and the tool has no preset for the Setup's material" | The tool has no cutting data for the Setup's material. Type the value, or add a preset to the tool ([Tools](Tools)) |
| "the toolpath has no tool" | Name a tool in the project file |
| "heights must satisfy clearance ≥ retract ≥ feed ≥ top > bottom ..." | Fix the order of the heights on the Heights tab |
| "a height refers to the selection, but the drive selection is empty" | A height uses the selection and nothing is selected. Give the height another reference, or select something |

**A toolpath shows an amber triangle.** It calculated, with warnings. Read them; some, such as "the tool cuts the fixture" below the stock, may be what you meant.

**A toolpath shows an amber ring with a dot.** Its inputs changed after it was calculated. Click **Calculate all**.

**I opened my project and the toolpaths are not calculated.** FluutCAM does not keep results with the project yet. Calculate again.

**The path has gaps or leaves material in narrow places.** A tool larger than a narrow feature cannot cut it. The warning `... narrower than the tool` says so. Use a smaller tool for those places, in a second toolpath.

**The calculation seems stuck.** A big 3-D model or a very small stepover takes a while; the progress bar shows the toolpath it is on. Click **Cancel** to stop after the current toolpath.

## Simulating and posting

**"rotary Setups are not simulated yet".** Right: only 3-axis and indexed Setups can be simulated.

**Simulate says "calculate first".** Calculate the Setup's toolpaths, then simulate.

**"Cannot post the project".** A toolpath has an error. Fix it and post again.

**Posting my rotary part fails with "rotary moves need a post with rotary.enabled".** The post does not know the rotary axis. Choose `fluut/linuxcnc-4x` in the Post stage's list, or use it from the command line ([Post G-code](Post-G-code)).

**The G-code has `M0 (Change to T1: ...)` lines.** That is the Grbl post: it has no tool changer, so it stops and asks you to fit the tool. This is by design.

**My controller does not understand the program.** Check the post. `fluut/gcode` is plain RS-274; LinuxCNC and Grbl add their own words. For another controller, extend `fluut/gcode` with your own post ([Machines and posts](Machines-and-Posts)). The built-in posts write millimetres only.

## Installing

**Windows warns when I run the installer.** The installer and programs are signed (SignPath Foundation). If Windows still warns because the program is new, check the file as described in [Install](Install) and choose to run it.

**macOS says the app cannot be opened.** The app is not notarised. Follow the steps in [Install](Install): right-click, Open, Open.

**The AppImage will not start.** It needs FUSE 2 (`libfuse2`) and glibc 2.35 or later. Without FUSE, run it with `--appimage-extract-and-run` as the first argument.

## Reporting a problem

Open an issue at <https://github.com/martinlong1978/fluutcam/issues>. Please include:

- the version: **About FluutCAM** in the window, or `fluut --version`;
- your system (Windows, macOS or Linux, and its version);
- what you did, what you expected and what happened;
- the messages, copied from the Messages tab or from the command line;
- the project, if you can share it, or a small model that shows the problem.

If FluutCAM crashed, there is a report in its data folder (see [Settings and updates](Settings-and-Updates)). It stays on your computer unless you send it, and holds the error, the version and your operating system, not your project.
