# Add a toolpath

A **toolpath** is one cutting job with one tool: a pocket, a contour, a finishing pass over a 3-D surface. A Setup holds a list of toolpaths, cut in the order they are listed.

## Add one

1. Select a Setup, or a toolpath in it, in the Browser. The new toolpath goes into the Setup you selected. If nothing is selected it goes into the first Setup. The toolpath buttons are greyed out until the project has a Setup. See [Add a Setup](Add-a-Setup).
2. Open the **Toolpaths** stage of the ribbon and click the type in its gallery.

![The Toolpaths stage of the ribbon](images/ribbon-toolpaths.png)

The galleries show only the types that suit the selected Setup's kind: a rotary Setup shows the Rotary gallery and not the 2D, Holes and 3D ones; the other kinds show those and not Rotary. Manual NC is always there.

The new toolpath appears at the end of the Setup's list, named after its type ("Parallel 1"), selected, and not calculated. Adding it is one undo step.

## Choose the type

There are 22 types. [Toolpath types](Toolpath-Types) lists them with a page for each, with every setting. In short:

| Group | Types | Use for |
|---|---|---|
| 2-D | 2D contour, 2D pocket, Face, Trace, 2D chamfer, Bore, 2D optimised load, Engrave | Cut-outs, pockets, flat tops, grooves, chamfers, round holes, V-carving |
| Holes | Drill | Drilled holes, with drilling cycles |
| 3-D roughing | 3D optimised load, 3D pocket | Removing the bulk of the material above a 3-D shape |
| 3-D finishing | Parallel, Contour, Horizontal, Spiral, Scallop, Steep and shallow | Cutting a 3-D surface to its final shape |
| Rotary | Rotary rough, Rotary along X, Rotary around A, Rotary spiral | Parts on a rotary axis (rotary Setups only) |
| Utility | Manual NC | A stop, a comment or your own line in the program |

Two rules to know:

- **Rotary types run in rotary Setups only**, and all the other types run in 3-axis and indexed Setups. The ribbon shows only the types that suit the selected Setup. If a project file puts a type in the wrong kind of Setup, the calculation reports `param.type.setup_kind`.
- **3-D types need no curves picked.** With nothing selected they machine every face of the Setup's bodies. **2-D types need curves or faces selected**, and the window cannot select them yet, so a 2-D toolpath added in the window reports "select drive curves or drive faces". Set its geometry in the project file for now. See [Known limits](Known-Limits).

## What a new toolpath starts with

FluutCAM writes no settings for a new toolpath: every setting takes its default. The defaults come from the Setup (heights, material) and from the tool's cutting data for the Setup's material (feeds, speeds, stepover, step-down). The inspector shows each default under the setting.

The tool comes from the built-in **starter tools** (see [Tools](Tools)):

| Type | Starting tool |
|---|---|
| Face | 12 mm flat end mill |
| 2D chamfer, Engrave | 60 degree V bit |
| Drill | 5 mm drill |
| Parallel, Contour, Spiral, Scallop, Steep and shallow, Rotary along X, Rotary around A, Rotary spiral | 6 mm ball end mill |
| Manual NC | none |
| Everything else | 6 mm flat end mill |

The window cannot change a toolpath's tool yet. To use another tool, edit the `tool` line of the toolpath in the project file, for example `tool = "shop#flat-6"` for the tool `flat-6` of the library file `tools/shop.toml` beside the project.

## Change a setting

Select the toolpath. The inspector has five tabs: **Tool**, **Geometry**, **Heights**, **Passes** and **Linking**.

![The inspector](images/calculated.png)

- Click a number box, type a value or an expression (`6/2`, `1/4in`, `tool.diameter*0.4`) and press Enter. A unit you type wins over the project unit: `0.25in` is a quarter inch even in a millimetre project.
- Choices are buttons or a list. Yes-or-no settings are check boxes.
- A setting you changed shows **Reset to default**. One you did not change shows its default in grey.
- Settings that do not apply to your other choices are hidden. For example the offset order only shows for the offset pattern.
- Each change is one undo step.

Heights, face and curve selections and points are shown but cannot be edited in the window yet.

**Which changes make a toolpath out of date.** A change that alters the path (most settings on the Geometry, Heights, Passes and Linking tabs) marks the toolpath, and often the later toolpaths in the Setup, as out of date: its Browser icon becomes an amber ring with a dot. Feeds, spindle speed and coolant only change the G-code, so they do not. Click **Calculate all** to bring everything up to date.

## Problems

A toolpath that cannot be calculated shows a red triangle in the Browser. Select it: the message sits under the setting it is about, and the **Messages** tab lists it. [Diagnostics](Diagnostics) explains each message and what to do.

## Next

[Calculate and simulate](Calculate-and-Simulate)
