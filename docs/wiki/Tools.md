# Tools

A **tool library** is a text file that lists cutting tools: their shape, their size, and cutting data for each material. A toolpath names one tool as `library#tool`, for example `fluut/starter#flat-6` (the tool `flat-6` of the library `fluut/starter`).

## The starter tools

FluutCAM has one built-in library, **`fluut/starter`**, so that a new project can calculate at once. It holds five common bits, each with cutting data for **MDF**:

| Tool | Name | Shape | Diameter | Used first by |
|---|---|---|---|---|
| `flat-6` | 6 mm flat end mill | flat | 6 mm | most 2-D types, optimised load, pocket, horizontal, rotary rough |
| `flat-12` | 12 mm flat end mill | flat | 12 mm | Face |
| `ball-6` | 6 mm ball end mill | ball | 6 mm | Parallel, Contour, Spiral, Scallop, Steep and shallow, rotary finishing |
| `vbit-60` | 60 degree V bit | v | 12 mm | 2D chamfer, Engrave |
| `drill-5` | 5 mm drill | drill | 5 mm | Drill |

The cutting data is cautious data for MDF on a small router. It is a starting point, not a recommendation for your machine. Set your own feeds and speeds before you cut.

## What a tool says

A tool has an `id`, a `name`, a tool `number` (the T number on the machine) and a `shape`:

| Shape | Use |
|---|---|
| `flat` | Flat end mill |
| `ball` | Ball end mill |
| `bull` | Bull-nose end mill (a flat end with rounded corners, `corner_radius`) |
| `v` | V bit (`half_angle_deg`, and `tip_diameter` for a flat tip) |
| `taper` | Tapered tool (`half_angle_deg`, and `tip_diameter` for the tip) |
| `drill` | Drill (`point_angle_deg`, 118 degrees if left out) |

Other settings, all in millimetres: `diameter`, `corner_radius`, `flute_length`, `overall_length`, `shank_diameter`, `pitch` (taps), `holder = { diameter = 20, length = 40 }`. `centre_cutting = false` says the tool cannot plunge straight down (FluutCAM then warns about plunge ramps).

**Cutting data** is a `[[tool.preset]]` table per material:

| Key | Unit | Meaning |
|---|---|---|
| `material` | | The name that a Setup's `material` selects (for example `MDF`) |
| `rpm` | rev/min | Spindle speed |
| `feed` | mm/min | Cutting feed |
| `plunge` | mm/min | Plunge feed |
| `stepover` | mm | Sideways step between passes |
| `step_down` | mm | Depth of each level |
| `optimal_load` | mm | Sideways width of cut for the optimised-load types (optional) |

A toolpath takes these as the defaults for its feeds and passes. If the tool has no preset for the Setup's material, the toolpath cannot find its feeds and says so (`param.required`) until you type the values.

## Add your own tools

Make a file `tools/shop.toml` in the project folder (beside the project file). This is the library used by the `block` reference project:

```toml
format = "fluut.tools/1"
id     = "shop"
name   = "My shop"

[[tool]]
id            = "flat-6"
name          = "6 mm flat"
number        = 1
shape         = "flat"
diameter      = 6.0
flute_length  = 25

  [[tool.preset]]
  material  = "MDF"
  rpm       = 18000
  feed      = 2000
  plunge    = 600
  stepover  = 2.4
  step_down = 4.0
```

Then open the project. The tool appears in the **Tool** list of a toolpath's Tool tab (see [Add a toolpath](Add-a-Toolpath)), as "shop: Flat 6 mm, ...". In the project file it is written `tool = "shop#flat-6"`. The full list of tool settings is in the [data format specification](https://github.com/martinlong1978/fluutcam/blob/main/docs/spec/02-data-formats.md).

The command-line programs also take extra tool files with `--tools <file>`, loaded after the project's `tools/` folder and after the libraries the project lists. See [Command line](Command-Line).

## Use a library from another folder

A library does not have to live in the project's `tools/` folder. A shop-wide library, such as one imported from Fusion, can stay where it is, and each project that uses it can **refer to it**.

1. Open the project, then on the **Library** tab click **Add tool library…**. Your system's file dialog opens; choose the library's `.toml` file. (You can also use **Add library…** under **Tool libraries** in the browser on the left.)
2. The library is loaded at once, with no need to reopen the project, and its tools appear in the Tool list. The browser's **Tool libraries** list now shows the file. The project is marked as changed, and **Undo** takes the library out again.
3. To stop using a library, click **Remove** beside it in that list. That is also one undo step.

The project file keeps the reference, not a copy: a line `tool_libraries = ["../shared/fusion-mill.toml"]`. The path is relative to the project file when the library and the project share a folder (so the two can move together), and a full path otherwise. When you **Save As** into another folder, the relative paths are rewritten so that they still point to the same files. See [Save, undo and recover](Save-Undo-Recover) for how projects are saved.

**Which library wins.** FluutCAM loads the project's `tools/` folder first, then the libraries in the list in the order shown, then any `--tools` files from the command line. If two libraries have the same id and version, the one loaded later replaces the earlier one, and a warning (`project.tools.replaces`) says so. A library with another version number is a different library.

**If a file is missing.** The project still opens. The browser marks the library, a warning (`project.tools.missing`) names the file, and a toolpath that used one of its tools shows a missing tool until the file is back. The reference stays in the project, so putting the file back is enough.

**Only files.** A reference is a path to a file on your computer or network drive that you can open. Web addresses (`https:`, `file:`) and network paths that start with `\` or `//` are not used (`project.tools.rejected`): opening a project never fetches anything from the network, and the library file is only read as data, never run.

A project that lists libraries is saved as format `fluut.project/1.1`. A project that lists none is saved exactly as before. An older FluutCAM that opens a `1.1` project warns that newer fields will not be saved, because it does not know the list.

## Import from Fusion

A Fusion tool library can be converted with **Import tools…** on the Library tab, or with `fluut tools import`. See [Import tools](Import-Tools).

## Not yet

- Editing the tool library in the window, and picking a tool for a toolpath there.
- Importing tool libraries from other CAM programs.
- A library of your own tools shared by all your projects (copy the file into each project's `tools/` folder).

## Next

[Command line](Command-Line)
